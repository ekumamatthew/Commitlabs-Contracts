#![cfg(test)]

extern crate std;

use crate::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    vec, Address, Env, IntoVal, String,
};

fn setup_contract(e: &Env) -> (Address, CommitmentNFTContractClient<'_>) {
    let contract_id = e.register_contract(None, CommitmentNFTContract);
    let client = CommitmentNFTContractClient::new(e, &contract_id);
    let admin = Address::generate(e);
    (admin, client)
}

/// Setup contract with a registered "core" contract.
/// Returns (admin, client, core_contract_id).
fn setup_contract_with_core(e: &Env) -> (Address, CommitmentNFTContractClient<'_>, Address) {
    e.mock_all_auths();
    let (admin, client) = setup_contract(e);
    client.initialize(&admin);
    let core_id = e.register_contract(None, CommitmentNFTContract);
    let _ = client.set_core_contract(&core_id);
    (admin, client, core_id)
}

fn create_test_metadata(
    e: &Env,
    asset_address: &Address,
) -> (String, u32, u32, String, i128, Address, u32) {
    (
        String::from_str(e, "commitment_001"),
        30, // duration_days
        10, // max_loss_percent
        String::from_str(e, "balanced"),
        1000, // initial_amount
        asset_address.clone(),
        5, // early_exit_penalty
    )
}

// ============================================================================
// Helper Functions
// ============================================================================

fn setup_env() -> (Env, Address, Address) {
    let e = Env::default();
    let (admin, contract_id) = {
        let (admin, client) = setup_contract(&e);

        // Initialize should succeed
        client.initialize(&admin);

        // Verify admin is set
        let stored_admin = client.get_admin();
        assert_eq!(stored_admin, admin);

        // Verify total supply is 0
        assert_eq!(client.total_supply(), 0);

        (admin, client.address)
    };

    (e, contract_id, admin)
}

/// Asserts that the sum of `balance_of` for all given owners equals `total_supply()`.
fn assert_balance_supply_invariant(
    client: &CommitmentNFTContractClient,
    owners: &[&Address],
) {
    let sum: u32 = owners.iter().map(|addr| client.balance_of(addr)).sum();
    assert_eq!(
        sum,
        client.total_supply(),
        "INV-2 violated: sum of balances ({}) != total_supply ({})",
        sum,
        client.total_supply()
    );
}

/// Convenience wrapper that mints a 1-day duration NFT with default params.
/// Returns the token_id.
fn mint_to_owner(
    e: &Env,
    client: &CommitmentNFTContractClient,
    owner: &Address,
    asset_address: &Address,
    label: &str,
) -> u32 {
    client.mint(
        owner,
        &String::from_str(e, label),
        &1, // 1 day duration — easy to settle
        &10,
        &String::from_str(e, "balanced"),
        &1000,
        asset_address,
        &5,
    )
}

// ============================================================================
// Initialization Tests
// ============================================================================

#[test]
#[should_panic(expected = "AlreadyInitialized")] 
fn test_initialize_twice_fails() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);

    client.initialize(&admin);
    client.initialize(&admin); // Should panic
}

// ============================================
// Mint Tests
// ============================================

#[test]
fn test_mint() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    let (commitment_id, duration, max_loss, commitment_type, amount, asset, penalty) =
        create_test_metadata(&e, &asset_address);

    let token_id = client.mint(
        &owner,
        &commitment_id,
        &duration,
        &max_loss,
        &commitment_type,
        &amount,
        &asset,
        &penalty,
    );

    assert_eq!(token_id, 0);
    assert_eq!(client.total_supply(), 1);
    assert_eq!(client.balance_of(&owner), 1);

    // Verify Mint event
    let events = e.events().all();
    let last_event = events.last().unwrap();

    assert_eq!(last_event.0, client.address);
    assert_eq!(
        last_event.1,
        vec![
            &e,
            symbol_short!("Mint").into_val(&e),
            token_id.into_val(&e),
            owner.into_val(&e)
        ]
    );
    let data: (String, u64) = last_event.2.into_val(&e);
    assert_eq!(data.0, commitment_id);
}

#[test]
fn test_mint_multiple() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    // Mint 3 NFTs
    let token_id_0 = client.mint(
        &owner,
        &String::from_str(&e, "commitment_0"),
        &30,
        &10,
        &String::from_str(&e, "balanced"),
        &1000,
        &asset_address,
        &5,
    );
    assert_eq!(token_id_0, 0);

    let token_id_1 = client.mint(
        &owner,
        &String::from_str(&e, "commitment_1"),
        &30,
        &10,
        &String::from_str(&e, "balanced"),
        &1000,
        &asset_address,
        &5,
    );
    assert_eq!(token_id_1, 1);

    let token_id_2 = client.mint(
        &owner,
        &String::from_str(&e, "commitment_2"),
        &30,
        &10,
        &String::from_str(&e, "balanced"),
        &1000,
        &asset_address,
        &5,
    );
    assert_eq!(token_id_2, 2);

    assert_eq!(client.total_supply(), 3);
    assert_eq!(client.balance_of(&owner), 3);
}

#[test]
#[should_panic(expected = "NotInitialized")]
fn test_mint_without_initialize_fails() {
    let e = Env::default();
    let (_admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    let (commitment_id, duration, max_loss, commitment_type, amount, asset, penalty) =
        create_test_metadata(&e, &asset_address);

    client.mint(
        &owner,
        &commitment_id,
        &duration,
        &max_loss,
        &commitment_type,
        &amount,
        &asset,
        &penalty,
    );
}

// ============================================
// Commitment Type Validation Tests
// ============================================

#[test]
#[should_panic(expected = "InvalidCommitmentType")]
fn test_mint_empty_commitment_type() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    client.mint(
        &owner,
        &String::from_str(&e, "commitment_empty"),
        &30,
        &10,
        &String::from_str(&e, ""),
        &1000,
        &asset_address,
        &5,
    );
}

#[test]
#[should_panic(expected = "InvalidCommitmentType")]
fn test_mint_invalid_commitment_type() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    client.mint(
        &owner,
        &String::from_str(&e, "commitment_invalid"),
        &30,
        &10,
        &String::from_str(&e, "invalid"),
        &1000,
        &asset_address,
        &5,
    );
}

// ============================================
// get_metadata Tests
// ============================================

#[test]
fn test_get_metadata() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    let commitment_id = String::from_str(&e, "test_commitment");
    let duration = 30u32;
    let max_loss = 15u32;
    let commitment_type = String::from_str(&e, "aggressive");
    let amount = 5000i128;

    let token_id = client.mint(
        &owner,
        &commitment_id,
        &duration,
        &max_loss,
        &commitment_type,
        &amount,
        &asset_address,
        &10,
    );

    let nft = client.get_metadata(&token_id);

    assert_eq!(nft.metadata.commitment_id, commitment_id);
    assert_eq!(nft.metadata.duration_days, duration);
    assert_eq!(nft.metadata.max_loss_percent, max_loss);
    assert_eq!(nft.metadata.commitment_type, commitment_type);
    assert_eq!(nft.metadata.initial_amount, amount);
    assert_eq!(nft.metadata.asset_address, asset_address);
    assert_eq!(nft.owner, owner);
    assert_eq!(nft.token_id, token_id);
}

#[test]
#[should_panic(expected = "TokenNotFound")]
fn test_get_metadata_nonexistent_token() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);

    client.initialize(&admin);

    // Try to get metadata for non-existent token
    client.get_metadata(&999);
}

// ============================================
// owner_of Tests
// ============================================

#[test]
fn test_owner_of() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    let (commitment_id, duration, max_loss, commitment_type, amount, asset, penalty) =
        create_test_metadata(&e, &asset_address);

    let token_id = client.mint(
        &owner,
        &commitment_id,
        &duration,
        &max_loss,
        &commitment_type,
        &amount,
        &asset,
        &penalty,
    );

    let retrieved_owner = client.owner_of(&token_id);
    assert_eq!(retrieved_owner, owner);
}

// ============================================
// is_active Tests
// ============================================

#[test]
fn test_is_active() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    let (commitment_id, duration, max_loss, commitment_type, amount, asset, penalty) =
        create_test_metadata(&e, &asset_address);

    let token_id = client.mint(
        &owner,
        &commitment_id,
        &duration,
        &max_loss,
        &commitment_type,
        &amount,
        &asset,
        &penalty,
    );

    // Newly minted NFT should be active
    assert_eq!(client.is_active(&token_id), true);
}

// ============================================
// total_supply Tests
// ============================================

#[test]
fn test_total_supply_after_minting() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    // Mint 5 NFTs
    for _ in 0..5 {
        client.mint(
            &owner,
            &String::from_str(&e, "commitment"),
            &30,
            &10,
            &String::from_str(&e, "safe"),
            &1000,
            &asset_address,
            &5,
        );
    }

    assert_eq!(client.total_supply(), 5);
}

// ============================================
// Transfer Tests
// ============================================

#[test]
fn test_transfer() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner1 = Address::generate(&e);
    let owner2 = Address::generate(&e);
    let asset_address = Address::generate(&e);

    let token_id = client.mint(
        &owner1,
        &String::from_str(&e, "commitment_001"),
        &1, 
        &10,
        &String::from_str(&e, "balanced"),
        &1000,
        &asset_address,
        &5,
    );

    e.ledger().with_mut(|li| {
        li.timestamp = 172800; 
    });
    
    e.as_contract(&core_id, || {
        client.settle(&token_id);
    });

    client.transfer(&owner1, &owner2, &token_id);

    assert_eq!(client.owner_of(&token_id), owner2);
    assert_eq!(client.balance_of(&owner1), 0);
    assert_eq!(client.balance_of(&owner2), 1);
}

// ============================================
// Settle Tests - Resolving Conflicts
// ============================================

#[test]
#[should_panic(expected = "AlreadySettled")]
fn test_settle_already_settled() {
    let e = Env::default();
    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    let token_id = client.mint(
        &owner,
        &String::from_str(&e, "test_commitment"),
        &1,
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );

    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });

    e.as_contract(&core_id, || {
        client.settle(&token_id);
    });
    
    e.as_contract(&core_id, || {
        client.settle(&token_id); // This should fail
    });
}

#[test]
#[should_panic(expected = "Unauthorized")] 
fn test_settle_by_random_address_fails() {
    let e = Env::default();
    let (_admin, client, _core_id) = setup_contract_with_core(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    let token_id = client.mint(
        &owner,
        &String::from_str(&e, "test_commitment"),
        &1,
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );
    
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });
    
    let random_address = Address::generate(&e);
    
    // Attempting to settle without mocking auth of core or admin should fail
    client.settle(&token_id);
}

// ============================================
// Issue #140: Zero Address Tests
// ============================================

#[test]
#[should_panic(expected = "ZeroAddress")]
fn test_mint_to_zero_address_fails() {
    let e = Env::default();
    e.mock_all_auths();
    let (admin, client) = setup_contract(&e);
    client.initialize(&admin);

    let zero_address = Address::from_string(&String::from_str(&e, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"));
    let asset = Address::generate(&e);

    client.mint(
        &zero_address,
        &String::from_str(&e, "c_123"),
        &30,
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset,
        &100
    );
}

#[test]
#[should_panic(expected = "ZeroAddress")]
fn test_transfer_to_zero_address_fails() {
    let e = Env::default();
    e.mock_all_auths();
    let (_admin, client, core_id) = setup_contract_with_core(&e);

    let owner = Address::generate(&e);
    let asset = Address::generate(&e);
    
    let token_id = client.mint(
        &owner,
        &String::from_str(&e, "c_123"),
        &1,
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset,
        &100
    );

    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });
    
    e.as_contract(&core_id, || {
        client.settle(&token_id);
    });

    let zero_address = Address::from_string(&String::from_str(&e, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"));
    
    client.transfer(&owner, &zero_address, &token_id);
}
