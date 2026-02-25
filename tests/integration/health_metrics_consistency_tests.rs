// Health Metrics Consistency Tests for Issue #150
// Tests for get_health_metrics consistency after multiple fee and drawdown records

#![cfg(test)]

use crate::harness::{ TestHarness, SECONDS_PER_DAY };
use soroban_sdk::{ testutils::{ Address as _, Ledger }, Address, Env, Map, String };

use attestation_engine::AttestationEngineContract;
use commitment_core::{ CommitmentCoreContract, CommitmentRules };
use commitment_nft::CommitmentNFTContract;

// ============================================
// Fee Aggregation Tests
// ============================================

#[test]
fn test_multiple_record_fees_cumulative_sum() {
    let harness = TestHarness::new();
    let user = &harness.accounts.user1;
    let verifier = &harness.accounts.verifier;
    let amount = 1_000_000_000_000i128;

    // Approve tokens and create commitment
    harness.approve_tokens(user, &harness.contracts.commitment_core, amount);

    let commitment_id = harness.env.as_contract(&harness.contracts.commitment_core, || {
        CommitmentCoreContract::create_commitment(
            harness.env.clone(),
            user.clone(),
            amount,
            harness.contracts.token.clone(),
            harness.default_rules()
        )
    });

    // Initialize attestation engine and add verifier
    harness.env.as_contract(&harness.contracts.attestation_engine, || {
        AttestationEngineContract::initialize(
            harness.env.clone(),
            harness.accounts.admin.clone(),
            harness.contracts.commitment_core.clone()
        )
    });

    harness.env.as_contract(&harness.contracts.attestation_engine, || {
        AttestationEngineContract::add_verifier(
            harness.env.clone(),
            harness.accounts.admin.clone(),
            verifier.clone()
        )
    });

    // Record multiple fees
    harness.env.as_contract(&harness.contracts.attestation_engine, || {
        AttestationEngineContract::record_fees(
            harness.env.clone(),
            verifier.clone(),
            commitment_id.clone(),
            10_0000000
        )
    });

    harness.env.as_contract(&harness.contracts.attestation_engine, || {
        AttestationEngineContract::record_fees(
            harness.env.clone(),
            verifier.clone(),
            commitment_id.clone(),
            20_0000000
        )
    });

    harness.env.as_contract(&harness.contracts.attestation_engine, || {
        AttestationEngineContract::record_fees(
            harness.env.clone(),
            verifier.clone(),
            commitment_id.clone(),
            5_0000000
        )
    });

    // Verify cumulative sum: 10 + 20 + 5 = 35
    let metrics = harness.env.as_contract(&harness.contracts.attestation_engine, || {
        AttestationEngineContract::get_health_metrics(harness.env.clone(), commitment_id.clone())
    });
    assert_eq!(metrics.fees_generated, 35_0000000);
}

#[test]
fn test_record_fees_zero_amount() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record zero fee
    fixture.attestation_client.record_fees(&fixture.verifier, &commitment_id, &0);

    let metrics = fixture.attestation_client.get_health_metrics(&commitment_id);
    assert_eq!(metrics.fees_generated, 0);
}

#[test]
fn test_record_fees_large_amounts() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record large fees to test overflow protection
    let large_fee1 = i128::MAX / 4;
    let large_fee2 = i128::MAX / 4;

    fixture.attestation_client.record_fees(&fixture.verifier, &commitment_id, &large_fee1);
    fixture.attestation_client.record_fees(&fixture.verifier, &commitment_id, &large_fee2);

    let metrics = fixture.attestation_client.get_health_metrics(&commitment_id);

    // Should handle large numbers without overflow
    assert!(metrics.fees_generated > 0);
}

// ============================================
// Drawdown Aggregation Tests
// ============================================

#[test]
fn test_multiple_record_drawdown_latest_value() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record multiple drawdowns
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &5);
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &10);
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &3);

    // Verify latest drawdown value is stored (not cumulative)
    let metrics = fixture.attestation_client.get_health_metrics(&commitment_id);
    assert_eq!(metrics.drawdown_percent, 3);
}

#[test]
fn test_record_drawdown_compliance_check() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record compliant drawdown (within 10% threshold)
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &5);

    let metrics = fixture.attestation_client.get_health_metrics(&commitment_id);
    assert_eq!(metrics.drawdown_percent, 5);

    // Verify compliance is still true
    let is_compliant = fixture.attestation_client.verify_compliance(&commitment_id);
    assert!(is_compliant);
}

#[test]
fn test_record_drawdown_non_compliant() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record non-compliant drawdown (exceeds 10% threshold)
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &15);

    let metrics = fixture.attestation_client.get_health_metrics(&commitment_id);
    assert_eq!(metrics.drawdown_percent, 15);

    // Verify compliance is false
    let is_compliant = fixture.attestation_client.verify_compliance(&commitment_id);
    assert!(!is_compliant);
}

// ============================================
// Compliance Score Update Tests
// ============================================

#[test]
fn test_compliance_score_updates_after_fees() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Initial compliance score should be 100
    let initial_metrics = fixture.attestation_client.get_health_metrics(&commitment_id);
    assert_eq!(initial_metrics.compliance_score, 100);

    // Record fees (compliant action)
    fixture.attestation_client.record_fees(&fixture.verifier, &commitment_id, &10_0000000);

    let metrics_after_fees = fixture.attestation_client.get_health_metrics(&commitment_id);

    // Compliance score should increase or stay the same for compliant fee generation
    assert!(metrics_after_fees.compliance_score >= 100);
    assert!(metrics_after_fees.compliance_score <= 100); // Capped at 100
}

#[test]
fn test_compliance_score_updates_after_drawdown() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record compliant drawdown
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &5);

    let metrics_after_compliant = fixture.attestation_client.get_health_metrics(&commitment_id);

    // Should maintain high compliance score for compliant drawdown
    assert!(metrics_after_compliant.compliance_score >= 90);

    // Record non-compliant drawdown
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &15);

    let metrics_after_non_compliant = fixture.attestation_client.get_health_metrics(&commitment_id);

    // Compliance score should decrease for non-compliant drawdown
    assert!(
        metrics_after_non_compliant.compliance_score < metrics_after_compliant.compliance_score
    );
}

#[test]
fn test_compliance_score_with_violation_attestation() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record a violation attestation
    let mut data = Map::new(&fixture.env);
    data.set(
        String::from_str(&fixture.env, "violation_type"),
        String::from_str(&fixture.env, "protocol_breach")
    );
    data.set(String::from_str(&fixture.env, "severity"), String::from_str(&fixture.env, "high"));

    fixture.attestation_client.attest(
        &fixture.verifier,
        &commitment_id,
        &String::from_str(&fixture.env, "violation"),
        &data,
        &false // Non-compliant
    );

    let metrics = fixture.attestation_client.get_health_metrics(&commitment_id);

    // Compliance score should decrease significantly for high severity violation
    assert!(metrics.compliance_score <= 70); // 100 - 30 (high severity penalty)
}

// ============================================
// Mixed Operations Tests
// ============================================

#[test]
fn test_mixed_fees_and_drawdown_operations() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Mix of operations
    fixture.attestation_client.record_fees(&fixture.verifier, &commitment_id, &10_0000000);
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &5);
    fixture.attestation_client.record_fees(&fixture.verifier, &commitment_id, &20_0000000);
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &8);

    let metrics = fixture.attestation_client.get_health_metrics(&commitment_id);

    // Verify cumulative fees
    assert_eq!(metrics.fees_generated, 30_0000000);

    // Verify latest drawdown
    assert_eq!(metrics.drawdown_percent, 8);

    // Verify compliance score is reasonable
    assert!(metrics.compliance_score >= 50);
    assert!(metrics.compliance_score <= 100);
}

#[test]
fn test_health_metrics_persistence() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record some operations
    fixture.attestation_client.record_fees(&fixture.verifier, &commitment_id, &15_0000000);
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id, &7);

    // Get metrics first time
    let metrics1 = fixture.attestation_client.get_health_metrics(&commitment_id);

    // Get metrics again (should be consistent)
    let metrics2 = fixture.attestation_client.get_health_metrics(&commitment_id);

    assert_eq!(metrics1.fees_generated, metrics2.fees_generated);
    assert_eq!(metrics1.drawdown_percent, metrics2.drawdown_percent);
    assert_eq!(metrics1.compliance_score, metrics2.compliance_score);
    assert_eq!(metrics1.commitment_id, metrics2.commitment_id);
}

// ============================================
// Edge Cases Tests
// ============================================

#[test]
fn test_empty_attestations_health_metrics() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Get health metrics without any attestations
    let metrics = fixture.attestation_client.get_health_metrics(&commitment_id);

    assert_eq!(metrics.fees_generated, 0);
    assert_eq!(metrics.compliance_score, 100); // Default compliance score
    assert_eq!(metrics.commitment_id, commitment_id);
}

#[test]
fn test_single_attestation_types() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Test single fee record
    fixture.attestation_client.record_fees(&fixture.verifier, &commitment_id, &25_0000000);

    let metrics = fixture.attestation_client.get_health_metrics(&commitment_id);
    assert_eq!(metrics.fees_generated, 25_0000000);

    // Reset with new commitment for drawdown test
    let commitment_id2 = fixture.create_test_commitment();
    fixture.attestation_client.record_drawdown(&fixture.verifier, &commitment_id2, &12);

    let metrics2 = fixture.attestation_client.get_health_metrics(&commitment_id2);
    assert_eq!(metrics2.drawdown_percent, 12);
}
