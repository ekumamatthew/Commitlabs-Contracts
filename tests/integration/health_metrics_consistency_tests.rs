// Health Metrics Consistency Tests for Issue #150
// Tests for get_health_metrics consistency after multiple fee and drawdown records

#![cfg(test)]

use attestation_engine::{AttestationEngineContract, AttestationEngineContractClient};
use commitment_core::{CommitmentCoreContract, CommitmentCoreContractClient, CommitmentRules};
use commitment_nft::{CommitmentNFTContract, CommitmentNFTContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Map, String,
};

struct HealthMetricsTestFixture {
    env: Env,
    admin: Address,
    owner: Address,
    verifier: Address,
    nft_client: CommitmentNFTContractClient<'static>,
    core_client: CommitmentCoreContractClient<'static>,
    attestation_client: AttestationEngineContractClient<'static>,
    asset_address: Address,
}

impl HealthMetricsTestFixture {
    fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let verifier = Address::generate(&env);
        let asset_address = Address::generate(&env);

        // Deploy NFT contract
        let nft_contract_id = env.register_contract(None, CommitmentNFTContract);
        let nft_client = CommitmentNFTContractClient::new(&env, &nft_contract_id);
        nft_client.initialize(&admin);

        // Deploy Core contract
        let core_contract_id = env.register_contract(None, CommitmentCoreContract);
        let core_client = CommitmentCoreContractClient::new(&env, &core_contract_id);
        core_client.initialize(&admin, &nft_contract_id);

        // Deploy Attestation Engine contract
        let attestation_contract_id = env.register_contract(None, AttestationEngineContract);
        let attestation_client =
            AttestationEngineContractClient::new(&env, &attestation_contract_id);
        attestation_client.initialize(&admin, &core_contract_id);

        // Add verifier to attestation engine
        attestation_client.add_verifier(&admin, &verifier);

        HealthMetricsTestFixture {
            env,
            admin,
            owner,
            verifier,
            nft_client,
            core_client,
            attestation_client,
            asset_address,
        }
    }

    fn create_test_commitment(&self) -> String {
        let rules = CommitmentRules {
            duration_days: 30,
            max_loss_percent: 10,
            commitment_type: String::from_str(&self.env, "safe"),
            early_exit_penalty: 5,
            min_fee_threshold: 100_0000000,
            grace_period_days: 0,
        };

        self.core_client.create_commitment(
            &self.owner,
            &1000_0000000,
            &self.asset_address,
            &rules,
        )
    }
}

// ============================================
// Fee Aggregation Tests
// ============================================

#[test]
fn test_multiple_record_fees_cumulative_sum() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record multiple fees
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &10_0000000);
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &20_0000000);
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &5_0000000);

    // Verify cumulative sum: 10 + 20 + 5 = 35
    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    assert_eq!(metrics.fees_generated, 35_0000000);
}

#[test]
fn test_record_fees_zero_amount() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record zero fee
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &0);

    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    assert_eq!(metrics.fees_generated, 0);
}

#[test]
fn test_record_fees_large_amounts() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record large fees to test overflow protection
    let large_fee1 = i128::MAX / 4;
    let large_fee2 = i128::MAX / 4;

    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &large_fee1);
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &large_fee2);

    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    
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
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &5);
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &10);
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &3);

    // Verify latest drawdown value is stored (not cumulative)
    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    assert_eq!(metrics.drawdown_percent, 3);
}

#[test]
fn test_record_drawdown_compliance_check() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record compliant drawdown (within 10% threshold)
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &5);

    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
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
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &15);

    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
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
    let initial_metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    assert_eq!(initial_metrics.compliance_score, 100);

    // Record fees (compliant action)
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &10_0000000);

    let metrics_after_fees = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    
    // Compliance score should increase or stay the same for compliant fee generation
    assert!(metrics_after_fees.compliance_score >= 100);
    assert!(metrics_after_fees.compliance_score <= 100); // Capped at 100
}

#[test]
fn test_compliance_score_updates_after_drawdown() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record compliant drawdown
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &5);

    let metrics_after_compliant = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    
    // Should maintain high compliance score for compliant drawdown
    assert!(metrics_after_compliant.compliance_score >= 90);

    // Record non-compliant drawdown
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &15);

    let metrics_after_non_compliant = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    
    // Compliance score should decrease for non-compliant drawdown
    assert!(metrics_after_non_compliant.compliance_score < metrics_after_compliant.compliance_score);
}

#[test]
fn test_compliance_score_with_violation_attestation() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Record a violation attestation
    let mut data = Map::new(&fixture.env);
    data.set(
        String::from_str(&fixture.env, "violation_type"),
        String::from_str(&fixture.env, "protocol_breach"),
    );
    data.set(
        String::from_str(&fixture.env, "severity"),
        String::from_str(&fixture.env, "high"),
    );

    fixture.attestation_client.attest(
        &fixture.verifier,
        &commitment_id,
        &String::from_str(&fixture.env, "violation"),
        &data,
        &false, // Non-compliant
    );

    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    
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
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &10_0000000);
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &5);
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &20_0000000);
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &8);

    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    
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
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &15_0000000);
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id, &7);

    // Get metrics first time
    let metrics1 = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);

    // Get metrics again (should be consistent)
    let metrics2 = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);

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
    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);

    assert_eq!(metrics.fees_generated, 0);
    assert_eq!(metrics.compliance_score, 100); // Default compliance score
    assert_eq!(metrics.commitment_id, commitment_id);
}

#[test]
fn test_single_attestation_types() {
    let fixture = HealthMetricsTestFixture::setup();
    let commitment_id = fixture.create_test_commitment();

    // Test single fee record
    fixture
        .attestation_client
        .record_fees(&fixture.verifier, &commitment_id, &25_0000000);

    let metrics = fixture
        .attestation_client
        .get_health_metrics(&commitment_id);
    assert_eq!(metrics.fees_generated, 25_0000000);

    // Reset with new commitment for drawdown test
    let commitment_id2 = fixture.create_test_commitment();
    fixture
        .attestation_client
        .record_drawdown(&fixture.verifier, &commitment_id2, &12);

    let metrics2 = fixture
        .attestation_client
        .get_health_metrics(&commitment_id2);
    assert_eq!(metrics2.drawdown_percent, 12);
}
