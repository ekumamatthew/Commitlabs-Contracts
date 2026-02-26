#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Map, String};

#[test]
fn test_initialize_and_getters() {
    let e = Env::default();
    let contract_id = e.register_contract(None, AttestationEngineContract);
    let admin = Address::generate(&e);
    let core = Address::generate(&e);

    let init = e.as_contract(&contract_id, || {
        AttestationEngineContract::initialize(e.clone(), admin.clone(), core.clone())
    });
    assert_eq!(init, Ok(()));

    let stored_admin = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_admin(e.clone()).unwrap()
    });
    let stored_core = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_core_contract(e.clone()).unwrap()
    });

    assert_eq!(stored_admin, admin);
    assert_eq!(stored_core, core);
}

#[test]
fn test_initialize_twice_fails() {
    let e = Env::default();
    let contract_id = e.register_contract(None, AttestationEngineContract);
    let admin = Address::generate(&e);
    let core = Address::generate(&e);

    e.as_contract(&contract_id, || {
        AttestationEngineContract::initialize(e.clone(), admin.clone(), core.clone()).unwrap();
    });

    let second = e.as_contract(&contract_id, || {
        AttestationEngineContract::initialize(e.clone(), admin.clone(), core.clone())
    });
    assert_eq!(second, Err(AttestationError::AlreadyInitialized));
}

#[test]
fn test_attest_without_initialize_fails() {
    let e = Env::default();
    e.mock_all_auths();
    let contract_id = e.register_contract(None, AttestationEngineContract);

    let caller = Address::generate(&e);
    let commitment_id = String::from_str(&e, "c_uninitialized");
    let attestation_type = String::from_str(&e, "health_check");
    let data = Map::<String, String>::new(&e);

    let result = e.as_contract(&contract_id, || {
        AttestationEngineContract::attest(
            e.clone(),
            caller.clone(),
            commitment_id.clone(),
            attestation_type.clone(),
            data.clone(),
            true,
        )
    });

    assert_eq!(result, Err(AttestationError::Unauthorized));
}

#[test]
fn test_get_admin_not_initialized_returns_error() {
    let e = Env::default();
    let contract_id = e.register_contract(None, AttestationEngineContract);

    let result = e.as_contract(&contract_id, || AttestationEngineContract::get_admin(e.clone()));

    assert_eq!(result, Err(AttestationError::NotInitialized));
}

#[test]
fn test_get_core_contract_not_initialized_returns_error() {
    let e = Env::default();
    let contract_id = e.register_contract(None, AttestationEngineContract);

    let result =
        e.as_contract(&contract_id, || AttestationEngineContract::get_core_contract(e.clone()));

    assert_eq!(result, Err(AttestationError::NotInitialized));
}

#[test]
fn test_get_attestations_not_initialized_returns_empty() {
    let e = Env::default();
    let contract_id = e.register_contract(None, AttestationEngineContract);
    let commitment_id = String::from_str(&e, "uninitialized");

    let attestations = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_attestations(e.clone(), commitment_id.clone())
    });

    assert_eq!(attestations.len(), 0);
}

#[test]
fn test_get_attestation_count_not_initialized_returns_zero() {
    let e = Env::default();
    let contract_id = e.register_contract(None, AttestationEngineContract);
    let commitment_id = String::from_str(&e, "uninitialized");

    let count = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_attestation_count(e.clone(), commitment_id.clone())
    });

    assert_eq!(count, 0);
}

#[test]
fn test_get_stored_health_metrics_not_initialized_returns_none() {
    let e = Env::default();
    let contract_id = e.register_contract(None, AttestationEngineContract);
    let commitment_id = String::from_str(&e, "uninitialized");

    let metrics = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_stored_health_metrics(e.clone(), commitment_id.clone())
    });

    assert!(metrics.is_none());
}

#[test]
fn test_fee_queries_not_initialized_return_defaults() {
    let e = Env::default();
    let contract_id = e.register_contract(None, AttestationEngineContract);
    let asset = Address::generate(&e);

    let (fee_amount, fee_asset) =
        e.as_contract(&contract_id, || AttestationEngineContract::get_attestation_fee(e.clone()));
    assert_eq!(fee_amount, 0);
    assert!(fee_asset.is_none());

    let fee_recipient =
        e.as_contract(&contract_id, || AttestationEngineContract::get_fee_recipient(e.clone()));
    assert!(fee_recipient.is_none());

    let collected_fees = e.as_contract(&contract_id, || {
        AttestationEngineContract::get_collected_fees(e.clone(), asset.clone())
    });
    assert_eq!(collected_fees, 0);
}

