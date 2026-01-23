#![cfg(test)]

use super::*;
 feat/compliance-verification
use soroban_sdk::{
    contract, contractimpl, contracttype, testutils::Address as _, testutils::Ledger as _, Address,
    Env, Map, String,
};

#[contracttype]
#[derive(Clone)]
enum CoreKey {
    Commitment(String),
    Violations(String),
}

#[contract]
struct MockCoreContract;

#[contractimpl]
impl MockCoreContract {
    pub fn set_commitment(e: Env, commitment_id: String, commitment: Commitment) {
        e.storage()
            .persistent()
            .set(&CoreKey::Commitment(commitment_id), &commitment);
    }

    pub fn set_violations(e: Env, commitment_id: String, has_violations: bool) {
        e.storage()
            .persistent()
            .set(&CoreKey::Violations(commitment_id), &has_violations);
    }

    pub fn get_commitment(e: Env, commitment_id: String) -> Commitment {
        e.storage()
            .persistent()
            .get(&CoreKey::Commitment(commitment_id))
            .expect("missing commitment")
    }

    pub fn check_violations(e: Env, commitment_id: String) -> bool {
        e.storage()
            .persistent()
            .get(&CoreKey::Violations(commitment_id))
            .unwrap_or(false)
    }
}
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, String};
use commitment_core::{Commitment as CoreCommitment, CommitmentCoreContract, CommitmentRules as CoreCommitmentRules};

fn store_core_commitment(
    e: &Env,
    commitment_core_id: &Address,
    commitment_id: &str,
    owner: &Address,
    amount: i128,
    current_value: i128,
    max_loss_percent: u32,
    duration_days: u32,
    created_at: u64,
) {
    let expires_at = created_at + (duration_days as u64 * 86400);
    let commitment = CoreCommitment {
        commitment_id: String::from_str(e, commitment_id),
        owner: owner.clone(),
        nft_token_id: 1,
        rules: CoreCommitmentRules {
            duration_days,
            max_loss_percent,
            commitment_type: String::from_str(e, "balanced"),
            early_exit_penalty: 10,
            min_fee_threshold: 1000,
        },
        amount,
        asset_address: Address::generate(e),
        created_at,
        expires_at,
        current_value,
        status: String::from_str(e, "active"),
    };

    e.as_contract(commitment_core_id, || {
        let key = (symbol_short!("Commit"), commitment.commitment_id.clone());
        e.storage().persistent().set(&key, &commitment);
    });
}

// Helper function to set up test environment with registered commitment_core contract
fn setup_test_env() -> (Env, Address, Address, Address) {
    let e = Env::default();
    let admin = Address::generate(&e);
 feat/compliance-verification
    let commitment_core_id = e.register_contract(None, MockCoreContract);
    let _contract_id = e.register_contract(None, AttestationEngineContract);
    
    e.as_contract(&_contract_id, || {
        AttestationEngineContract::initialize(e.clone(), admin, commitment_core_id);
    });
}

#[test]
fn test_attest() {
    let e = Env::default();
    let verified_by = Address::generate(&e);
    let core_id = e.register_contract(None, MockCoreContract);
    let _contract_id = e.register_contract(None, AttestationEngineContract);

    e.as_contract(&_contract_id, || {
        AttestationEngineContract::initialize(e.clone(), Address::generate(&e), core_id.clone());
    });

    let commitment_id = String::from_str(&e, "c1");
    let owner = Address::generate(&e);

    let rules = CommitmentRules {
        duration_days: 10,
        max_loss_percent: 20,
        commitment_type: String::from_str(&e, "safe"),
        early_exit_penalty: 0,
        min_fee_threshold: 0,
    };
    let commitment = Commitment {
        commitment_id: commitment_id.clone(),
        owner,
        nft_token_id: 1,
        rules,
        amount: 1_000,
        asset_address: Address::generate(&e),
        created_at: 0,
        expires_at: 100,
        current_value: 1_000,
        status: String::from_str(&e, "active"),
    };

    e.as_contract(&core_id, || {
        MockCoreContract::set_commitment(e.clone(), commitment_id.clone(), commitment);
        MockCoreContract::set_violations(e.clone(), commitment_id.clone(), false);
    });
    
    let data = Map::<String, String>::new(&e);
    e.as_contract(&_contract_id, || {
        AttestationEngineContract::attest(
            e.clone(),
            commitment_id.clone(),
            String::from_str(&e, "health_check"),
            data,
            verified_by,
        );
    });

    let atts = e.as_contract(&_contract_id, || {
        AttestationEngineContract::get_attestations(e.clone(), commitment_id)
    });
    assert!(atts.len() == 1);
}

#[test]
fn test_verify_compliance() {
    let e = Env::default();
    // Set a deterministic ledger timestamp for duration checks.
    e.ledger().with_mut(|li| {
        li.timestamp = 50;
    });

    let core_id = e.register_contract(None, MockCoreContract);
    let _contract_id = e.register_contract(None, AttestationEngineContract);
    e.as_contract(&_contract_id, || {
        AttestationEngineContract::initialize(e.clone(), Address::generate(&e), core_id.clone());
    });

    let commitment_id = String::from_str(&e, "c1");
    let owner = Address::generate(&e);

    let base_rules = CommitmentRules {
        duration_days: 10,
        max_loss_percent: 20,
        commitment_type: String::from_str(&e, "safe"),
        early_exit_penalty: 0,
        min_fee_threshold: 100,
    };

    // Happy path: in-range drawdown, not expired, fees meet threshold, no violations.
    let mut commitment = Commitment {
        commitment_id: commitment_id.clone(),
        owner: owner.clone(),
        nft_token_id: 1,
        rules: base_rules.clone(),
        amount: 1_000,
        asset_address: Address::generate(&e),
        created_at: 0,
        expires_at: 100,
        current_value: 900, // 10% drawdown
        status: String::from_str(&e, "active"),
    };
    e.as_contract(&core_id, || {
        MockCoreContract::set_commitment(e.clone(), commitment_id.clone(), commitment.clone());
        MockCoreContract::set_violations(e.clone(), commitment_id.clone(), false);
    });
    e.as_contract(&_contract_id, || {
        AttestationEngineContract::record_fees(e.clone(), commitment_id.clone(), 100);
    });

    assert!(e.as_contract(&_contract_id, || {
        AttestationEngineContract::verify_compliance(e.clone(), commitment_id.clone())
    }));

    // Loss limit exceeded
    commitment.current_value = 700; // 30% drawdown
    e.as_contract(&core_id, || {
        MockCoreContract::set_commitment(e.clone(), commitment_id.clone(), commitment.clone());
    });
    assert!(!e.as_contract(&_contract_id, || {
        AttestationEngineContract::verify_compliance(e.clone(), commitment_id.clone())
    }));

    // Duration expired
    commitment.current_value = 900;
    commitment.expires_at = 40;
    e.as_contract(&core_id, || {
        MockCoreContract::set_commitment(e.clone(), commitment_id.clone(), commitment.clone());
    });
    assert!(!e.as_contract(&_contract_id, || {
        AttestationEngineContract::verify_compliance(e.clone(), commitment_id.clone())
    }));

    // Fee threshold not met
    commitment.expires_at = 100;
    e.as_contract(&core_id, || {
        MockCoreContract::set_commitment(e.clone(), commitment_id.clone(), commitment.clone());
    });
    // Reset engine fees by using a new commitment id
    let commitment_id2 = String::from_str(&e, "c2");
    commitment.commitment_id = commitment_id2.clone();
    e.as_contract(&core_id, || {
        MockCoreContract::set_commitment(e.clone(), commitment_id2.clone(), commitment.clone());
        MockCoreContract::set_violations(e.clone(), commitment_id2.clone(), false);
    });
    assert!(!e.as_contract(&_contract_id, || {
        AttestationEngineContract::verify_compliance(e.clone(), commitment_id2.clone())
    }));

    // Active violations
    e.as_contract(&core_id, || {
        MockCoreContract::set_violations(e.clone(), commitment_id2.clone(), true);
    });
    assert!(!e.as_contract(&_contract_id, || {
        AttestationEngineContract::verify_compliance(e.clone(), commitment_id2)
    }));
    
    // Edge: duration_days == 0 bypasses duration check
    let commitment_id3 = String::from_str(&e, "c3");
    let rules_no_duration = CommitmentRules {
        duration_days: 0,
        ..base_rules
    };
    let commitment3 = Commitment {
        commitment_id: commitment_id3.clone(),
        owner,
        nft_token_id: 3,
        rules: rules_no_duration,
        amount: 0, // edge: amount==0 -> drawdown=0
        asset_address: Address::generate(&e),
        created_at: 0,
        expires_at: 0,
        current_value: 0,
        status: String::from_str(&e, "active"),
    };
    e.as_contract(&core_id, || {
        MockCoreContract::set_commitment(e.clone(), commitment_id3.clone(), commitment3);
        MockCoreContract::set_violations(e.clone(), commitment_id3.clone(), false);
    });
    // fees not met but threshold is 100 -> still should fail; make threshold 0
    let mut commitment3b = e.as_contract(&core_id, || {
        MockCoreContract::get_commitment(e.clone(), commitment_id3.clone())
    });
    commitment3b.rules.min_fee_threshold = 0;
    e.as_contract(&core_id, || {
        MockCoreContract::set_commitment(e.clone(), commitment_id3.clone(), commitment3b);
    });
    assert!(e.as_contract(&_contract_id, || {
        AttestationEngineContract::verify_compliance(e.clone(), commitment_id3)
    }));

    
    // Register and initialize commitment_core contract
    let commitment_core_id = e.register_contract(None, CommitmentCoreContract);
    let nft_contract = Address::generate(&e);
    
    // Initialize commitment_core contract
    e.as_contract(&commitment_core_id, || {
        CommitmentCoreContract::initialize(e.clone(), admin.clone(), nft_contract.clone());
    });
    
    // Register attestation_engine contract
    let contract_id = e.register_contract(None, AttestationEngineContract);
    
    // Initialize attestation_engine contract
    e.as_contract(&contract_id, || {
        AttestationEngineContract::initialize(e.clone(), admin.clone(), commitment_core_id.clone());
    });
    
    (e, admin, commitment_core_id, contract_id)
}

#[test]
fn test_initialize() {
    let (e, admin, commitment_core, contract_id) = setup_test_env();
    
    // Verify initialization by checking that we can call other functions
    // (indirect verification through storage access)
    let commitment_id = String::from_str(&e, "test");
    let _attestations = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_attestations(e.clone(), commitment_id)
    });
}

#[test]
fn test_get_attestations_empty() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment_1");
    
    // Get attestations
    let attestations = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_attestations(e.clone(), commitment_id)
    });
    
    assert_eq!(attestations.len(), 0);
}

#[test]
fn test_get_health_metrics_basic() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment_1");
    
    // Seed a commitment in the core contract so get_commitment succeeds
    let owner = Address::generate(&e);
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment_1",
        &owner,
        1000,
        950,
        10,
        30,
        1000,
    );

    let metrics = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_health_metrics(e.clone(), commitment_id.clone())
    });
    
    assert_eq!(metrics.commitment_id, commitment_id);
    // Verify all fields are present
    assert!(metrics.compliance_score <= 100);
}

#[test]
fn test_get_health_metrics_drawdown_calculation() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment_1");
    let owner = Address::generate(&e);
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment_1",
        &owner,
        1000,
        900,
        10,
        30,
        1000,
    );
    let metrics = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_health_metrics(e.clone(), commitment_id)
    });
    
    // Verify drawdown calculation handles edge cases
    // initial=1000, current=900 => 10% drawdown
    assert_eq!(metrics.drawdown_percent, 10);
}

#[test]
fn test_get_health_metrics_zero_initial_value() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment_1");
    let owner = Address::generate(&e);
    // Explicitly store a zero-amount commitment to exercise the division-by-zero path
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment_1",
        &owner,
        0,
        0,
        10,
        30,
        1000,
    );
    let metrics = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_health_metrics(e.clone(), commitment_id)
    });
    
    // Should handle zero initial value gracefully (drawdown = 0)
    // This tests edge case handling
    assert!(metrics.drawdown_percent >= 0);
    assert_eq!(metrics.initial_value, 0);
}

#[test]
fn test_calculate_compliance_score_base() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment_1");
    let owner = Address::generate(&e);
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment_1",
        &owner,
        1000,
        950,
        10,
        30,
        1000,
    );
    let score = e.as_contract(&contract_id, || {
        AttestationEngineContract::calculate_compliance_score(e.clone(), commitment_id)
    });
    
    // Score should be clamped between 0 and 100
    assert!(score <= 100);

}

#[test]
fn test_calculate_compliance_score_clamping() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment_1");
    let owner = Address::generate(&e);
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment_1",
        &owner,
        1000,
        950,
        10,
        30,
        1000,
    );
    let score = e.as_contract(&contract_id, || {
        AttestationEngineContract::calculate_compliance_score(e.clone(), commitment_id)
    });
    
    // Verify score is clamped between 0 and 100
    assert!(score <= 100);
}

#[test]
fn test_get_health_metrics_includes_compliance_score() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment_1");
    let owner = Address::generate(&e);
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment_1",
        &owner,
        1000,
        950,
        10,
        30,
        1000,
    );
    let metrics = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_health_metrics(e.clone(), commitment_id)
    });
    
    // Verify compliance_score is included and valid
    assert!(metrics.compliance_score <= 100);
}

#[test]
fn test_get_health_metrics_last_attestation() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment_1");
    let owner = Address::generate(&e);
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment_1",
        &owner,
        1000,
        950,
        10,
        30,
        1000,
    );
    let metrics = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_health_metrics(e.clone(), commitment_id)
    });
    
    // With no attestations, last_attestation should be 0
    assert_eq!(metrics.last_attestation, 0);
}

#[test]
fn test_all_three_functions_work_together() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment_1");
    let owner = Address::generate(&e);
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment_1",
        &owner,
        1000,
        950,
        10,
        30,
        1000,
    );
    
    // Test all three functions work
    let attestations = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_attestations(e.clone(), commitment_id.clone())
    });
    let metrics = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_health_metrics(e.clone(), commitment_id.clone())
    });
    let score = e.as_contract(&contract_id, || {
        AttestationEngineContract::calculate_compliance_score(e.clone(), commitment_id.clone())
    });
    
    // Verify they all return valid data
    assert_eq!(attestations.len(), 0); // No attestations stored yet
    assert_eq!(metrics.commitment_id, commitment_id);
    assert!(score <= 100);
    assert_eq!(metrics.compliance_score, score); // Should match
}

#[test]
fn test_get_attestations_returns_empty_vec_when_none_exist() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    // Test with different commitment IDs
    let commitment_id1 = String::from_str(&e, "commitment_1");
    let commitment_id2 = String::from_str(&e, "commitment_2");
    
    let attestations1 = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_attestations(e.clone(), commitment_id1)
    });
    let attestations2 = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_attestations(e.clone(), commitment_id2)
    });
    
    assert_eq!(attestations1.len(), 0);
    assert_eq!(attestations2.len(), 0);
}

#[test]
fn test_health_metrics_structure() {
    let (e, _admin, _commitment_core, contract_id) = setup_test_env();
    
    let commitment_id = String::from_str(&e, "test_commitment");
    let owner = Address::generate(&e);
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment",
        &owner,
        1000,
        1000,
        10,
        30,
        1000,
    );
    let metrics = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_health_metrics(e.clone(), commitment_id.clone())
    });
    
    // Verify all required fields are present
    assert_eq!(metrics.commitment_id, commitment_id);
    assert_eq!(metrics.current_value, 1000);
    assert_eq!(metrics.initial_value, 1000);
    assert_eq!(metrics.drawdown_percent, 0);
    assert_eq!(metrics.fees_generated, 0);
    assert_eq!(metrics.volatility_exposure, 0);
    assert_eq!(metrics.last_attestation, 0);
    assert!(metrics.compliance_score <= 100);
}

#[test]
fn test_attest_and_get_metrics() {
    let (e, admin, _commitment_core, contract_id) = setup_test_env();
    
    // Set ledger timestamp to non-zero
    e.ledger().with_mut(|li| li.timestamp = 12345);
    
    let commitment_id = String::from_str(&e, "test_commitment_wf");
    let owner = Address::generate(&e);
    store_core_commitment(
        &e,
        &_commitment_core,
        "test_commitment_wf",
        &owner,
        1000,
        1000,
        10,
        30,
        1000,
    );
    let attestation_type = String::from_str(&e, "general");
    let mut data = Map::new(&e);
    data.set(String::from_str(&e, "note"), String::from_str(&e, "test attestation"));
    
    // Record an attestation
    e.as_contract(&contract_id, || {
        AttestationEngineContract::attest(
            e.clone(),
            commitment_id.clone(),
            attestation_type.clone(),
            data.clone(),
            admin.clone(),
        );
    });
    
    // Get attestations and verify
    let attestations = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_attestations(e.clone(), commitment_id.clone())
    });
    
    assert_eq!(attestations.len(), 1);
    assert_eq!(attestations.get(0).unwrap().attestation_type, attestation_type);
    
    // Get health metrics and verify last_attestation is updated
    let metrics = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_health_metrics(e.clone(), commitment_id.clone())
    });
    
    assert!(metrics.last_attestation > 0);
}
