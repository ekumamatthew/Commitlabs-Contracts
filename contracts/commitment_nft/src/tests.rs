#![cfg(test)]

use super::*;
use shared_utils::TimeUtils;
use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{Address as _, Events, Ledger},
    token::StellarAssetClient,
    vec, Address, Env, IntoVal, String,
};

#[contract]
struct MockNftContract;

#[contractimpl]
impl MockNftContract {
    pub fn mint(
        _e: Env,
        _owner: Address,
        _commitment_id: String,
        _duration_days: u32,
        _max_loss_percent: u32,
        _commitment_type: String,
        _initial_amount: i128,
        _asset_address: Address,
        _early_exit_penalty: u32,
    ) -> u32 {
        1
    }
    pub fn settle(_e: Env, _caller: Address, _token_id: u32) {}
    pub fn mark_inactive(_e: Env, _caller: Address, _token_id: u32) {}
}

fn test_rules(e: &Env) -> CommitmentRules {
    CommitmentRules {
        duration_days: 30,
        max_loss_percent: 10,
        commitment_type: String::from_str(e, "balanced"),
        early_exit_penalty: 5,
        min_fee_threshold: 100,
        grace_period_days: 0,
    }
}

// ... [KEEP ALL YOUR HELPER FUNCTIONS: create_test_commitment, store_commitment, setup_token_contract] ...

// ============================================
// create_commitment Validation Tests
// ============================================

#[test]
#[should_panic(expected = "Duration would cause expiration timestamp overflow")]
fn test_create_commitment_expiration_overflow() {
    let e = Env::default();
    e.mock_all_auths();

    let contract_id = e.register_contract(None, CommitmentCoreContract);
    let admin = Address::generate(&e);
    let nft_contract = Address::generate(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    e.as_contract(&contract_id, || {
        CommitmentCoreContract::initialize(e.clone(), admin.clone(), nft_contract.clone());
    });

    // Set ledger timestamp so created_at + duration_days * 86400 overflows u64
    e.ledger().with_mut(|l| {
        l.timestamp = u64::MAX - 50_000;
    });

    let rules = CommitmentRules {
        duration_days: 1,
        max_loss_percent: 10,
        commitment_type: String::from_str(&e, "safe"),
        early_exit_penalty: 5,
        min_fee_threshold: 100,
        grace_period_days: 0,
    };

    e.as_contract(&contract_id, || {
        CommitmentCoreContract::create_commitment(e.clone(), owner, 1000, asset_address, rules);
    });
}

// ... [KEEP ALL YOUR 2,000 LINES OF EXISTING TESTS] ...

#[test]
fn test_update_value_no_violation() {
    let e = Env::default();
    e.mock_all_auths();
    
    let contract_id = e.register_contract(None, CommitmentCoreContract);
    let admin = Address::generate(&e);
    let nft_contract = Address::generate(&e);
    let owner = Address::generate(&e);

    e.as_contract(&contract_id, || {
        CommitmentCoreContract::initialize(e.clone(), admin.clone(), nft_contract.clone());
        let commitment = create_test_commitment(&e, "test_id", &owner, 1000, 1000, 10, 30, e.ledger().timestamp());
        set_commitment(&e, &commitment);
        e.storage()
            .instance()
            .set(&DataKey::TotalValueLocked, &1000i128);
    });

    // MERGED: Pass admin.clone() as the caller argument
    e.as_contract(&contract_id, || {
        CommitmentCoreContract::update_value(e.clone(), admin.clone(), String::from_str(&e, "test_id"), 950);
    });

    let client = CommitmentCoreContractClient::new(&e, &contract_id);
    let updated = client.get_commitment(&String::from_str(&e, "test_id"));
    assert_eq!(updated.current_value, 950);
    assert_eq!(updated.status, String::from_str(&e, "active"));
}

#[test]
fn test_check_violations_after_update_value() {
    let e = Env::default();
    e.mock_all_auths();
    let contract_id = e.register_contract(None, CommitmentCoreContract);
    let admin = Address::generate(&e);
    let nft_contract = Address::generate(&e);
    let owner = Address::generate(&e);
    
    e.as_contract(&contract_id, || {
        CommitmentCoreContract::initialize(e.clone(), admin.clone(), nft_contract.clone());
        let commitment = create_test_commitment(&e, "test_id", &owner, 1000, 1000, 10, 30, 1000);
        set_commitment(&e, &commitment);
    });

    let client = CommitmentCoreContractClient::new(&e, &contract_id);
    
    e.as_contract(&contract_id, || {
        let mut commitment = read_commitment(&e, &String::from_str(&e, "test_id")).unwrap();
        commitment.current_value = 850; // 15% loss > 10% max
        set_commitment(&e, &commitment);
    });
    
    assert!(client.check_violations(&String::from_str(&e, "test_id")));
}

// ============================================
// Issue #140: Zero Address Validation
// ============================================

#[test]
#[should_panic(expected = "Zero address is not allowed")]
fn test_create_commitment_zero_address_fails() {
    let e = Env::default();
    e.mock_all_auths();

    let contract_id = e.register_contract(None, CommitmentCoreContract);
    let client = CommitmentCoreContractClient::new(&e, &contract_id);

    let admin = Address::generate(&e);
    let nft_contract = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin, &nft_contract);

    let zero_str = String::from_str(&e, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
    let zero_address = Address::from_string(&zero_str);

    let rules = CommitmentRules {
        duration_days: 30,
        max_loss_percent: 10,
        commitment_type: String::from_str(&e, "safe"),
        early_exit_penalty: 5,
        min_fee_threshold: 100,
        grace_period_days: 0,
    };

    client.create_commitment(&zero_address, &1000i128, &asset_address, &rules);
}