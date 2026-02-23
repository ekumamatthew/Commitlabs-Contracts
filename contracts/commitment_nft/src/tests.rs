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

/// Setup contract with a registered "core" contract so settle() can be called (access control).
/// Returns (admin, client, core_contract_id). Use e.as_contract(&core_id, || { client.settle(&core_id, &token_id) }) to settle.
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

// ============================================
// Initialization Tests
// ============================================

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
#[should_panic(expected = "Error(Contract, #2)")] // AlreadyInitialized
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
#[should_panic(expected = "Error(Contract, #1)")] // NotInitialized
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
#[should_panic(expected = "Error(Contract, #3)")] // TokenNotFound
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

#[test]
#[should_panic(expected = "Error(Contract, #3)")] // TokenNotFound
fn test_owner_of_nonexistent_token() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);

    client.initialize(&admin);

    client.owner_of(&999);
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

#[test]
#[should_panic(expected = "Error(Contract, #3)")] // TokenNotFound
fn test_is_active_nonexistent_token() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);

    client.initialize(&admin);

    client.is_active(&999);
}

// ============================================
// Issue #107: NFT query functions with non-existent token_id (explicit error checks)
// ============================================

#[test]
fn test_get_metadata_nonexistent_token_returns_error() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    client.initialize(&admin);
    let result = client.try_get_metadata(&999);
    assert!(
        result.is_err(),
        "get_metadata(non-existent token_id) must return error, not panic"
    );
}

#[test]
fn test_owner_of_nonexistent_token_returns_error() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    client.initialize(&admin);
    let result = client.try_owner_of(&999);
    assert!(
        result.is_err(),
        "owner_of(non-existent token_id) must return error, not panic"
    );
}

#[test]
fn test_is_active_nonexistent_token_returns_error() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    client.initialize(&admin);
    let result = client.try_is_active(&999);
    assert!(
        result.is_err(),
        "is_active(non-existent token_id) must return error, not panic"
    );
}

// ============================================
// total_supply Tests
// ============================================

#[test]
fn test_total_supply_initial() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);

    client.initialize(&admin);

    assert_eq!(client.total_supply(), 0);
}

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

// Issue #111: total_supply unchanged after transfer or settle
#[test]
fn test_total_supply_unchanged_after_transfer_and_settle() {
    let e = Env::default();
    e.mock_all_auths();
    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner1 = Address::generate(&e);
    let owner2 = Address::generate(&e);
    let asset_address = Address::generate(&e);

    assert_eq!(client.total_supply(), 0);
    let token_id = client.mint(
        &owner1,
        &String::from_str(&e, "c1"),
        &1,
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );
    assert_eq!(client.total_supply(), 1);
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });
    e.as_contract(&core_id, || {
        client.settle(&core_id, &token_id);
    });
    assert_eq!(client.total_supply(), 1);
    client.transfer(&owner1, &owner2, &token_id);
    assert_eq!(client.total_supply(), 1);
}

// ============================================
// balance_of Tests
// ============================================

#[test]
fn test_balance_of_initial() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);

    client.initialize(&admin);

    // Owner with no NFTs should have balance 0
    assert_eq!(client.balance_of(&owner), 0);
}

#[test]
fn test_balance_of_after_minting() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner1 = Address::generate(&e);
    let owner2 = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    // Mint 3 NFTs for owner1
    for _ in 0..3 {
        client.mint(
            &owner1,
            &String::from_str(&e, "owner1_commitment"),
            &30,
            &10,
            &String::from_str(&e, "safe"),
            &1000,
            &asset_address,
            &5,
        );
    }

    // Mint 2 NFTs for owner2
    for _ in 0..2 {
        client.mint(
            &owner2,
            &String::from_str(&e, "owner2_commitment"),
            &30,
            &10,
            &String::from_str(&e, "safe"),
            &1000,
            &asset_address,
            &5,
        );
    }

    assert_eq!(client.balance_of(&owner1), 3);
    assert_eq!(client.balance_of(&owner2), 2);
}

// Issue #110: balance_of decremented after transfer
#[test]
fn test_balance_of_decremented_after_transfer() {
    let e = Env::default();
    e.mock_all_auths();
    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner = Address::generate(&e);
    let recipient = Address::generate(&e);
    let asset_address = Address::generate(&e);

    assert_eq!(client.balance_of(&owner), 0);
    let token_id = client.mint(
        &owner,
        &String::from_str(&e, "c1"),
        &1,
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );
    assert_eq!(client.balance_of(&owner), 1);
    assert_eq!(client.balance_of(&recipient), 0);
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });
    e.as_contract(&core_id, || {
        client.settle(&core_id, &token_id);
    });
    client.transfer(&owner, &recipient, &token_id);
    assert_eq!(client.balance_of(&owner), 0);
    assert_eq!(client.balance_of(&recipient), 1);
}

// ============================================
// get_all_metadata Tests
// ============================================

#[test]
fn test_get_all_metadata_empty() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);

    client.initialize(&admin);

    let all_nfts = client.get_all_metadata();
    assert_eq!(all_nfts.len(), 0);
}

#[test]
fn test_get_all_metadata() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    // Mint 3 NFTs
    for _ in 0..3 {
        client.mint(
            &owner,
            &String::from_str(&e, "commitment"),
            &30,
            &10,
            &String::from_str(&e, "balanced"),
            &1000,
            &asset_address,
            &5,
        );
    }

    let all_nfts = client.get_all_metadata();
    assert_eq!(all_nfts.len(), 3);

    // Verify each NFT owner
    for nft in all_nfts.iter() {
        assert_eq!(nft.owner, owner);
    }
}

// ============================================
// get_nfts_by_owner Tests
// ============================================

#[test]
fn test_get_nfts_by_owner_empty() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);

    client.initialize(&admin);

    let nfts = client.get_nfts_by_owner(&owner);
    assert_eq!(nfts.len(), 0);
}

#[test]
fn test_get_nfts_by_owner() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner1 = Address::generate(&e);
    let owner2 = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    // Mint 2 NFTs for owner1
    for _ in 0..2 {
        client.mint(
            &owner1,
            &String::from_str(&e, "owner1"),
            &30,
            &10,
            &String::from_str(&e, "safe"),
            &1000,
            &asset_address,
            &5,
        );
    }

    // Mint 3 NFTs for owner2
    for _ in 0..3 {
        client.mint(
            &owner2,
            &String::from_str(&e, "owner2"),
            &30,
            &10,
            &String::from_str(&e, "safe"),
            &1000,
            &asset_address,
            &5,
        );
    }

    let owner1_nfts = client.get_nfts_by_owner(&owner1);
    let owner2_nfts = client.get_nfts_by_owner(&owner2);

    assert_eq!(owner1_nfts.len(), 2);
    assert_eq!(owner2_nfts.len(), 3);

    // Verify all owner1 NFTs belong to owner1
    for nft in owner1_nfts.iter() {
        assert_eq!(nft.owner, owner1);
    }
}

// ============================================
// Transfer Tests
// ============================================

#[test]
fn test_owner_of_not_found() {
    let (e, contract_id, _admin) = setup_env();
    let client = CommitmentNFTContractClient::new(&e, &contract_id);

    let result = client.try_owner_of(&999);
    assert!(result.is_err());
}

// ============================================================================
// Transfer Tests
// ============================================================================

#[test]
fn test_transfer() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner1 = Address::generate(&e);
    let owner2 = Address::generate(&e);
    let asset_address = Address::generate(&e);

    // Mint with 1 day duration so we can settle it
    let token_id = client.mint(
        &owner1,
        &String::from_str(&e, "commitment_001"),
        &1, // 1 day duration
        &10,
        &String::from_str(&e, "balanced"),
        &1000,
        &asset_address,
        &5,
    );

    // Verify initial state
    assert_eq!(client.owner_of(&token_id), owner1);
    assert_eq!(client.balance_of(&owner1), 1);
    assert_eq!(client.balance_of(&owner2), 0);

    // Fast forward time past expiration and settle (as core contract)
    e.ledger().with_mut(|li| {
        li.timestamp = 172800; // 2 days
    });
    e.as_contract(&core_id, || {
        client.settle(&core_id, &token_id);
    });

    // Verify NFT is now inactive (unlocked)
    assert_eq!(client.is_active(&token_id), false);

    // Transfer NFT
    client.transfer(&owner1, &owner2, &token_id);

    // Verify transfer
    assert_eq!(client.owner_of(&token_id), owner2);
    assert_eq!(client.balance_of(&owner1), 0);
    assert_eq!(client.balance_of(&owner2), 1);

    // Verify Transfer event
    let events = e.events().all();
    let last_event = events.last().unwrap();

    assert_eq!(last_event.0, client.address);
    assert_eq!(
        last_event.1,
        vec![
            &e,
            symbol_short!("Transfer").into_val(&e),
            owner1.into_val(&e),
            owner2.into_val(&e)
        ]
    );
    let data: (u32, u64) = last_event.2.into_val(&e);
    assert_eq!(data.0, token_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")] // NotOwner
fn test_transfer_not_owner() {
    let e = Env::default();
    e.mock_all_auths();

    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let not_owner = Address::generate(&e);
    let recipient = Address::generate(&e);
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

    // Try to transfer from non-owner (should fail)
    client.transfer(&not_owner, &recipient, &token_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")] // TokenNotFound
fn test_transfer_nonexistent_token() {
    let e = Env::default();
    e.mock_all_auths();

    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let recipient = Address::generate(&e);

    client.initialize(&admin);

    client.transfer(&owner, &recipient, &999);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")] // TransferToZeroAddress
fn test_transfer_to_self() {
    let e = Env::default();
    e.mock_all_auths();

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

    // Try to transfer to self (should fail)
    client.transfer(&owner, &owner, &token_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #19)")] // NFTLocked
fn test_transfer_locked_nft() {
    let e = Env::default();
    e.mock_all_auths();

    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let recipient = Address::generate(&e);
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

    // Verify NFT is active (locked)
    assert_eq!(client.is_active(&token_id), true);

    // Try to transfer active/locked NFT (should fail)
    client.transfer(&owner, &recipient, &token_id);
}

#[test]
fn test_transfer_after_settlement() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner = Address::generate(&e);
    let recipient = Address::generate(&e);
    let asset_address = Address::generate(&e);

    // Mint with 1 day duration
    let token_id = client.mint(
        &owner,
        &String::from_str(&e, "test_commitment"),
        &1, // 1 day duration
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );

    // Verify NFT is active (locked) initially
    assert_eq!(client.is_active(&token_id), true);

    // Fast forward time past expiration (2 days = 172800 seconds)
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });

    // Settle the NFT (as core contract)
    e.as_contract(&core_id, || {
        client.settle(&core_id, &token_id);
    });

    // Verify NFT is now inactive (unlocked)
    assert_eq!(client.is_active(&token_id), false);

    // Transfer should now succeed
    client.transfer(&owner, &recipient, &token_id);

    // Verify transfer was successful
    assert_eq!(client.owner_of(&token_id), recipient);
    assert_eq!(client.balance_of(&owner), 0);
    assert_eq!(client.balance_of(&recipient), 1);
}

// ============================================
// Settle Tests
// ============================================

#[test]
fn test_settle() {
    let e = Env::default();
    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    // Mint with 1 day duration
    let token_id = client.mint(
        &owner,
        &String::from_str(&e, "test_commitment"),
        &1, // 1 day duration
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );

    // NFT should be active initially
    assert_eq!(client.is_active(&token_id), true);

    // Fast forward time past expiration (2 days = 172800 seconds)
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });

    // Verify it's expired
    assert_eq!(client.is_expired(&token_id), true);

    // Settle the NFT (as authorized core contract)
    e.as_contract(&core_id, || {
        client.settle(&core_id, &token_id);
    });

    // NFT should now be inactive
    assert_eq!(client.is_active(&token_id), false);

    // Verify Settle event
    let events = e.events().all();
    let last_event = events.last().unwrap();

    assert_eq!(last_event.0, client.address);
    assert_eq!(
        last_event.1,
        vec![
            &e,
            symbol_short!("Settle").into_val(&e),
            token_id.into_val(&e)
        ]
    );
    let data: u64 = last_event.2.into_val(&e);
    assert_eq!(data, e.ledger().timestamp());
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")] // NotExpired
fn test_settle_not_expired() {
    let e = Env::default();
    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    let token_id = client.mint(
        &owner,
        &String::from_str(&e, "test_commitment"),
        &30, // 30 days duration
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );

    // Try to settle before expiration (as core, should fail with NotExpired)
    e.as_contract(&core_id, || {
        client.settle(&core_id, &token_id);
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")] // AlreadySettled
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

    // Fast forward time
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });

    e.as_contract(&core_id, || {
        client.settle(&core_id, &token_id);
    });
    e.as_contract(&core_id, || {
        client.settle(&core_id, &token_id); // Should fail
    });
}

// ============================================
// Issue #108: NFT settle access control
// ============================================

#[test]
#[should_panic(expected = "Error(Contract, #6)")] // NotAuthorized
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
    // Call settle with a random address (not core or admin) — expect NotAuthorized
    let random_address = Address::generate(&e);
    client.settle(&random_address, &token_id);
}

#[test]
fn test_settle_by_core_contract_succeeds() {
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
        client.settle(&core_id, &token_id);
    });
    assert_eq!(client.is_active(&token_id), false);
}

// ============================================
// is_expired Tests
// ============================================

#[test]
fn test_is_expired() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    let token_id = client.mint(
        &owner,
        &String::from_str(&e, "test_commitment"),
        &1, // 1 day
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );

    // Should not be expired initially
    assert_eq!(client.is_expired(&token_id), false);

    // Fast forward 2 days
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });

    // Should now be expired
    assert_eq!(client.is_expired(&token_id), true);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")] // TokenNotFound
fn test_is_expired_nonexistent_token() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);

    client.initialize(&admin);

    client.is_expired(&999);
}

// ============================================
// token_exists Tests
// ============================================

#[test]
fn test_token_exists() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    // Token 0 should not exist yet
    assert_eq!(client.token_exists(&0), false);

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

    // Token should now exist
    assert_eq!(client.token_exists(&token_id), true);

    // Non-existent token should return false
    assert_eq!(client.token_exists(&999), false);
}

// ============================================
// get_admin Tests
// ============================================

#[test]
fn test_get_admin() {
    let e = Env::default();
    let (admin, client) = setup_contract(&e);

    client.initialize(&admin);

    assert_eq!(client.get_admin(), admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")] // NotInitialized
fn test_get_admin_not_initialized() {
    let e = Env::default();
    let (_admin, client) = setup_contract(&e);

    client.get_admin();
}

// ============================================
// Edge Cases
// ============================================

#[test]
fn test_metadata_timestamps() {
    let e = Env::default();

    // Set initial ledger timestamp
    e.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });

    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    let token_id = client.mint(
        &owner,
        &String::from_str(&e, "test"),
        &30, // 30 days
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );

    let metadata = client.get_metadata(&token_id);

    // Verify timestamps
    assert_eq!(metadata.metadata.created_at, 1000);
    // expires_at should be created_at + (30 days * 86400 seconds)
    assert_eq!(metadata.metadata.expires_at, 1000 + (30 * 86400));
}

#[test]
fn test_balance_updates_after_transfer() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner1 = Address::generate(&e);
    let owner2 = Address::generate(&e);
    let asset_address = Address::generate(&e);

    // Mint multiple NFTs for owner1 with 1 day duration so we can settle them
    client.mint(
        &owner1,
        &String::from_str(&e, "commitment_0"),
        &1, // 1 day duration
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );
    client.mint(
        &owner1,
        &String::from_str(&e, "commitment_1"),
        &1, // 1 day duration
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );
    client.mint(
        &owner1,
        &String::from_str(&e, "commitment_2"),
        &1, // 1 day duration
        &10,
        &String::from_str(&e, "safe"),
        &1000,
        &asset_address,
        &5,
    );

    assert_eq!(client.balance_of(&owner1), 3);
    assert_eq!(client.balance_of(&owner2), 0);

    // Fast forward time past expiration and settle all NFTs (as core contract)
    e.ledger().with_mut(|li| {
        li.timestamp = 172800; // 2 days
    });
    e.as_contract(&core_id, || {
        client.settle(&core_id, &0);
        client.settle(&core_id, &1);
        client.settle(&core_id, &2);
    });

    // Transfer one NFT
    client.transfer(&owner1, &owner2, &0);

    assert_eq!(client.balance_of(&owner1), 2);
    assert_eq!(client.balance_of(&owner2), 1);

    // Transfer another
    client.transfer(&owner1, &owner2, &1);

    assert_eq!(client.balance_of(&owner1), 1);
    assert_eq!(client.balance_of(&owner2), 2);

    // Verify get_nfts_by_owner reflects the transfers
    let owner1_nfts = client.get_nfts_by_owner(&owner1);
    let owner2_nfts = client.get_nfts_by_owner(&owner2);

    assert_eq!(owner1_nfts.len(), 1);
    assert_eq!(owner2_nfts.len(), 2);
}

#[test]
#[should_panic(expected = "Contract is paused - operation not allowed")]
fn test_mint_blocked_when_paused() {
    let e = Env::default();
    e.mock_all_auths();

    let (admin, client) = setup_contract(&e);
    let owner = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);
    client.pause();

    client.mint(
        &owner,
        &String::from_str(&e, "paused_commitment"),
        &30,
        &10,
        &String::from_str(&e, "balanced"),
        &1000,
        &asset_address,
        &5,
    );
}

#[test]
#[should_panic(expected = "Contract is paused - operation not allowed")]
fn test_transfer_blocked_when_paused() {
    let e = Env::default();
    e.mock_all_auths();

    let (admin, client) = setup_contract(&e);
    let owner1 = Address::generate(&e);
    let owner2 = Address::generate(&e);
    let asset_address = Address::generate(&e);

    client.initialize(&admin);

    let token_id = client.mint(
        &owner1,
        &String::from_str(&e, "commitment_001"),
        &30,
        &10,
        &String::from_str(&e, "balanced"),
        &1000,
        &asset_address,
        &5,
    );

    client.pause();
    client.transfer(&owner1, &owner2, &token_id);
}

// #[test]
fn _test_unpause_restores_transfer() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner1 = Address::generate(&e);
    let owner2 = Address::generate(&e);
    let asset_address = Address::generate(&e);

    let token_id = client.mint(
        &owner1,
        &String::from_str(&e, "commitment_002"),
        &1, // 1 day duration so we can settle
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
        client.settle(&core_id, &token_id);
    });

    client.pause();
    client.unpause();

    client.transfer(&owner1, &owner2, &token_id);
    assert_eq!(client.owner_of(&token_id), owner2);
}

// ============================================================================
// Balance / Supply Invariant Tests
// ============================================================================
//
// Formally documented invariants:
//
// INV-1 (Supply Monotonicity):
//   `total_supply()` equals the number of successful mints and is never
//   decremented. Neither `settle()` nor `transfer()` changes the counter.
//
// INV-2 (Balance-Supply Conservation):
//   sum(balance_of(addr) for all owners) == total_supply()
//   Relies on the ownership check at L534 guaranteeing from_balance >= 1 on
//   transfer, so the conditional decrement at L570 is always taken.
//
// INV-3 (Settle Independence):
//   `settle()` does not change `total_supply()` or any `balance_of()`.
//   It only flips `nft.is_active` to false.
//
// INV-4 (Transfer Conservation):
//   `transfer()` decreases the sender's balance by 1, increases the
//   receiver's balance by 1, and leaves `total_supply()` unchanged.
// ============================================================================

#[test]
fn test_invariant_balance_sum_equals_supply_after_mints() {
    let e = Env::default();
    e.mock_all_auths();

    let (admin, client) = setup_contract(&e);
    let asset = Address::generate(&e);

    let owner_a = Address::generate(&e);
    let owner_b = Address::generate(&e);
    let owner_c = Address::generate(&e);
    let owner_d = Address::generate(&e);
    let owners: [&Address; 4] = [&owner_a, &owner_b, &owner_c, &owner_d];

    client.initialize(&admin);

    // Base case: empty state
    assert_eq!(client.total_supply(), 0);
    assert_balance_supply_invariant(&client, &owners);

    // Mint 4 to owner_a
    for i in 0..4 {
        mint_to_owner(&e, &client, &owner_a, &asset, &std::format!("a_{i}"));
        assert_balance_supply_invariant(&client, &owners);
    }

    // Mint 1 to owner_b
    mint_to_owner(&e, &client, &owner_b, &asset, "b_0");
    assert_balance_supply_invariant(&client, &owners);

    // Mint 3 to owner_c
    for i in 0..3 {
        mint_to_owner(&e, &client, &owner_c, &asset, &std::format!("c_{i}"));
        assert_balance_supply_invariant(&client, &owners);
    }

    // Mint 2 to owner_d
    for i in 0..2 {
        mint_to_owner(&e, &client, &owner_d, &asset, &std::format!("d_{i}"));
        assert_balance_supply_invariant(&client, &owners);
    }

    // Final state: 4+1+3+2 = 10
    assert_eq!(client.total_supply(), 10);
    assert_eq!(client.balance_of(&owner_a), 4);
    assert_eq!(client.balance_of(&owner_b), 1);
    assert_eq!(client.balance_of(&owner_c), 3);
    assert_eq!(client.balance_of(&owner_d), 2);
    assert_balance_supply_invariant(&client, &owners);
}

#[test]
fn test_invariant_supply_unchanged_after_settle() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let owner = Address::generate(&e);
    let asset = Address::generate(&e);

    // Mint 3 NFTs (1-day duration)
    let t0 = mint_to_owner(&e, &client, &owner, &asset, "s_0");
    let t1 = mint_to_owner(&e, &client, &owner, &asset, "s_1");
    let t2 = mint_to_owner(&e, &client, &owner, &asset, "s_2");

    let supply_before = client.total_supply();
    let balance_before = client.balance_of(&owner);
    assert_eq!(supply_before, 3);
    assert_eq!(balance_before, 3);

    // Fast-forward past expiration
    e.ledger().with_mut(|li| {
        li.timestamp = 172800; // 2 days
    });

    // Settle each — supply and balance must not change
    for token_id in [t0, t1, t2] {
        e.as_contract(&core_id, || {
            client.settle(&core_id, &token_id);
        });
        assert_eq!(client.total_supply(), supply_before);
        assert_eq!(client.balance_of(&owner), balance_before);
    }
}

#[test]
fn test_invariant_balance_unchanged_after_settle_multi_owner() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let asset = Address::generate(&e);

    let alice = Address::generate(&e);
    let bob = Address::generate(&e);
    let carol = Address::generate(&e);
    let owners: [&Address; 3] = [&alice, &bob, &carol];

    // Alice: 2, Bob: 2, Carol: 1 => 5 total
    let a0 = mint_to_owner(&e, &client, &alice, &asset, "a0");
    let _a1 = mint_to_owner(&e, &client, &alice, &asset, "a1");
    let b0 = mint_to_owner(&e, &client, &bob, &asset, "b0");
    let b1 = mint_to_owner(&e, &client, &bob, &asset, "b1");
    let _c0 = mint_to_owner(&e, &client, &carol, &asset, "c0");

    assert_eq!(client.total_supply(), 5);
    assert_balance_supply_invariant(&client, &owners);

    // Fast-forward past expiration
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });

    // Partial settle: only a0, b0, b1
    for token_id in [a0, b0, b1] {
        e.as_contract(&core_id, || {
            client.settle(&core_id, &token_id);
        });
    }

    // All balances and supply unchanged
    assert_eq!(client.balance_of(&alice), 2);
    assert_eq!(client.balance_of(&bob), 2);
    assert_eq!(client.balance_of(&carol), 1);
    assert_eq!(client.total_supply(), 5);
    assert_balance_supply_invariant(&client, &owners);
}

#[test]
fn test_invariant_transfer_balance_conservation() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let asset = Address::generate(&e);

    let from = Address::generate(&e);
    let to = Address::generate(&e);
    let owners: [&Address; 2] = [&from, &to];

    // Mint 3 to `from`, 1 to `to`
    let t0 = mint_to_owner(&e, &client, &from, &asset, "f0");
    let _t1 = mint_to_owner(&e, &client, &from, &asset, "f1");
    let _t2 = mint_to_owner(&e, &client, &from, &asset, "f2");
    let _t3 = mint_to_owner(&e, &client, &to, &asset, "to0");

    assert_eq!(client.total_supply(), 4);
    assert_balance_supply_invariant(&client, &owners);

    // Settle t0 so it can be transferred
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });
    e.as_contract(&core_id, || {
        client.settle(&core_id, &t0);
    });

    let supply_before = client.total_supply();
    let from_bal_before = client.balance_of(&from);
    let to_bal_before = client.balance_of(&to);

    // Transfer t0: from -> to
    client.transfer(&from, &to, &t0);

    // INV-4: sender -1, receiver +1, supply unchanged
    assert_eq!(client.balance_of(&from), from_bal_before - 1);
    assert_eq!(client.balance_of(&to), to_bal_before + 1);
    assert_eq!(client.total_supply(), supply_before);
    // INV-2: sum still equals supply
    assert_balance_supply_invariant(&client, &owners);
}

#[test]
fn test_invariant_complex_mint_settle_transfer_scenario() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let asset = Address::generate(&e);

    let alice = Address::generate(&e);
    let bob = Address::generate(&e);
    let carol = Address::generate(&e);
    let owners: [&Address; 3] = [&alice, &bob, &carol];

    // --- Phase 1: Mint 6 NFTs ---
    // Alice: 3, Bob: 2, Carol: 1
    let a0 = mint_to_owner(&e, &client, &alice, &asset, "a0");
    let a1 = mint_to_owner(&e, &client, &alice, &asset, "a1");
    let a2 = mint_to_owner(&e, &client, &alice, &asset, "a2");
    let b0 = mint_to_owner(&e, &client, &bob, &asset, "b0");
    let b1 = mint_to_owner(&e, &client, &bob, &asset, "b1");
    let c0 = mint_to_owner(&e, &client, &carol, &asset, "c0");

    assert_eq!(client.total_supply(), 6);
    assert_balance_supply_invariant(&client, &owners);

    // --- Phase 2: Settle 4 of 6 ---
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });

    for token_id in [a0, a1, b0, c0] {
        e.as_contract(&core_id, || {
            client.settle(&core_id, &token_id);
        });
    }

    // INV-3: supply and balances unchanged
    assert_eq!(client.total_supply(), 6);
    assert_eq!(client.balance_of(&alice), 3);
    assert_eq!(client.balance_of(&bob), 2);
    assert_eq!(client.balance_of(&carol), 1);
    assert_balance_supply_invariant(&client, &owners);

    // --- Phase 3: Transfer 3 settled NFTs ---
    // a0: alice -> bob
    client.transfer(&alice, &bob, &a0);
    assert_balance_supply_invariant(&client, &owners);

    // a1: alice -> carol
    client.transfer(&alice, &carol, &a1);
    assert_balance_supply_invariant(&client, &owners);

    // b0: bob -> carol
    client.transfer(&bob, &carol, &b0);
    assert_balance_supply_invariant(&client, &owners);

    assert_eq!(client.total_supply(), 6);
    assert_eq!(client.balance_of(&alice), 1); // had 3, transferred 2
    assert_eq!(client.balance_of(&bob), 2);   // had 2, received 1, transferred 1
    assert_eq!(client.balance_of(&carol), 3); // had 1, received 2

    // --- Phase 4: Settle remaining active NFTs ---
    for token_id in [a2, b1] {
        e.as_contract(&core_id, || {
            client.settle(&core_id, &token_id);
        });
    }
    assert_eq!(client.total_supply(), 6);
    assert_balance_supply_invariant(&client, &owners);

    // --- Phase 5: Mint 2 more (still active, no settle) ---
    mint_to_owner(&e, &client, &alice, &asset, "a3");
    mint_to_owner(&e, &client, &bob, &asset, "b2");

    assert_eq!(client.total_supply(), 8);
    assert_eq!(client.balance_of(&alice), 2);
    assert_eq!(client.balance_of(&bob), 3);
    assert_eq!(client.balance_of(&carol), 3);
    assert_balance_supply_invariant(&client, &owners);
}

#[test]
fn test_invariant_transfer_chain_preserves_supply() {
    let e = Env::default();
    e.mock_all_auths();

    let (_admin, client, core_id) = setup_contract_with_core(&e);
    let asset = Address::generate(&e);

    let a = Address::generate(&e);
    let b = Address::generate(&e);
    let c = Address::generate(&e);
    let d = Address::generate(&e);
    let owners: [&Address; 4] = [&a, &b, &c, &d];

    // Single token, chain: A -> B -> C -> D
    let token = mint_to_owner(&e, &client, &a, &asset, "chain");

    assert_eq!(client.total_supply(), 1);
    assert_balance_supply_invariant(&client, &owners);

    // Settle so we can transfer
    e.ledger().with_mut(|li| {
        li.timestamp = 172800;
    });
    e.as_contract(&core_id, || {
        client.settle(&core_id, &token);
    });

    // A -> B
    client.transfer(&a, &b, &token);
    assert_eq!(client.total_supply(), 1);
    assert_balance_supply_invariant(&client, &owners);
    assert_eq!(client.balance_of(&a), 0);
    assert_eq!(client.balance_of(&b), 1);

    // B -> C
    client.transfer(&b, &c, &token);
    assert_eq!(client.total_supply(), 1);
    assert_balance_supply_invariant(&client, &owners);
    assert_eq!(client.balance_of(&b), 0);
    assert_eq!(client.balance_of(&c), 1);

    // C -> D
    client.transfer(&c, &d, &token);
    assert_eq!(client.total_supply(), 1);
    assert_balance_supply_invariant(&client, &owners);
    assert_eq!(client.balance_of(&c), 0);
    assert_eq!(client.balance_of(&d), 1);
}
