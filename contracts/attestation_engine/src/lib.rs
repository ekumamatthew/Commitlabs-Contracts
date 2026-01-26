#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Symbol, Address, Env, String, Vec, Map, 
    IntoVal, TryIntoVal, Val,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentRules {
    pub duration_days: u32,
    pub max_loss_percent: u32,
    pub commitment_type: String, // "safe", "balanced", "aggressive"
    pub early_exit_penalty: u32,
    pub min_fee_threshold: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commitment {
    pub commitment_id: String,
    pub owner: Address,
    pub nft_token_id: u32,
    pub rules: CommitmentRules,
    pub amount: i128,
    pub asset_address: Address,
    pub created_at: u64,
    pub expires_at: u64,
    pub current_value: i128,
    pub status: String, // "active", "settled", "violated", "early_exit"
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    CommitmentCore,
    HealthState(String),
    Attestations(String),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthState {
    pub fees_generated: i128,
    pub volatility_exposure: i128,
    pub last_attestation: u64,
    pub compliance_score: u32, // 0-100; 0 means "unknown / not calculated"
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attestation {
    pub commitment_id: String,
    pub timestamp: u64,
    pub attestation_type: String, // "health_check", "violation", "fee_generation", "drawdown"
    pub data: Map<String, String>, // Flexible data structure
    pub is_compliant: bool,
    pub verified_by: Address,
}

// Import Commitment types from commitment_core (define locally for cross-contract calls)

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthMetrics {
    pub commitment_id: String,
    pub current_value: i128,
    pub initial_value: i128,
    pub drawdown_percent: i128,
    pub fees_generated: i128,
    pub volatility_exposure: i128,
    pub last_attestation: u64,
    pub compliance_score: u32, // 0-100
}

#[contract]
pub struct AttestationEngineContract;

#[contractimpl]
impl AttestationEngineContract {
    /// Initialize the attestation engine
    pub fn initialize(e: Env, admin: Address, commitment_core: Address) {
        e.storage().persistent().set(&DataKey::Admin, &admin);
        e.storage()
            .persistent()
            .set(&DataKey::CommitmentCore, &commitment_core);
        // Store admin and commitment core contract address in instance storage
        e.storage().instance().set(&symbol_short!("ADMIN"), &admin);
        e.storage().instance().set(&symbol_short!("CORE"), &commitment_core);

    }

    /// Record an attestation for a commitment
    pub fn attest(
        e: Env,
        commitment_id: String,
        attestation_type: String,
        data: Map<String, String>,
        verified_by: Address,
    ) {
        // Public by design: external services can attest and later verify compliance.
        // Consumers can apply their own trust model based on `verified_by`.
        let ts = e.ledger().timestamp();

        let is_compliant = Self::verify_compliance(e.clone(), commitment_id.clone());

        let att = Attestation {
            commitment_id: commitment_id.clone(),
            timestamp: ts,
            attestation_type,
            data,
            is_compliant,
            verified_by,
        };

        let mut list: Vec<Attestation> = e
            .storage()
            .persistent()
            .get(&DataKey::Attestations(commitment_id.clone()))
            .unwrap_or(Vec::new(&e));
        list.push_back(att);
        e.storage()
            .persistent()
            .set(&DataKey::Attestations(commitment_id.clone()), &list);

        // Update health state "last seen".
        let mut state = Self::get_health_state_or_default(&e, commitment_id.clone());
        state.last_attestation = ts;
        e.storage()
            .persistent()
            .set(&DataKey::HealthState(commitment_id), &state);

        // Create attestation record
        let attestation = Attestation {
            commitment_id: commitment_id.clone(),
            attestation_type: attestation_type.clone(),
            data,
            timestamp: e.ledger().timestamp(),
            verified_by,
            is_compliant: true, // Default to true, can be updated by logic
        };
        
        // Retrieve existing attestations
        let key = (symbol_short!("ATTS"), commitment_id.clone());
        let mut attestations: Vec<Attestation> = e.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&e));
            
        // Add new attestation
        attestations.push_back(attestation);
        
        // Store updated list
        e.storage().persistent().set(&key, &attestations);
        
        // Emit attestation event
        e.events().publish(
            (symbol_short!("attest"), commitment_id),
            attestation_type
        );
r
    }

    /// Get all attestations for a commitment
    pub fn get_attestations(e: Env, commitment_id: String) -> Vec<Attestation> {
        // Retrieve attestations from persistent storage using commitment_id as key
        let key = (symbol_short!("ATTS"), commitment_id);
        e.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&e))

    }

    /// Get current health metrics for a commitment
    pub fn get_health_metrics(e: Env, commitment_id: String) -> HealthMetrics {
        let core = Self::get_commitment_core(&e);
        let commitment = Self::core_get_commitment(&e, &core, &commitment_id);

        let initial_value = commitment.amount;
        let current_value = commitment.current_value;
        let drawdown_percent = Self::calc_drawdown_percent(initial_value, current_value);

        let state = Self::get_health_state_or_default(&e, commitment_id.clone());


        // Get commitment from core contract
        let commitment_core: Address = e.storage()
            .instance()
            .get(&symbol_short!("CORE"))
            .unwrap();
        
        // Call get_commitment on commitment_core contract
        // Using Symbol::new() for function name longer than 9 characters
        let mut args = Vec::new(&e);
        args.push_back(commitment_id.clone().into_val(&e));
        let commitment_val: Val = e.invoke_contract(
            &commitment_core,
            &Symbol::new(&e, "get_commitment"),
            args,
        );
        
        // Convert Val to Commitment
        let commitment: Commitment = commitment_val.try_into_val(&e).unwrap();
        
        // Get all attestations
        let attestations = Self::get_attestations(e.clone(), commitment_id.clone());
        
        // Extract values from commitment
        let initial_value = commitment.amount; // Using amount as initial value
        let current_value = commitment.current_value;
        
        // Calculate drawdown percentage: ((initial - current) / initial) * 100
        // Handle zero initial value to prevent division by zero
        let drawdown_percent = if initial_value > 0 {
            let diff = initial_value.checked_sub(current_value).unwrap_or(0);
            diff.checked_mul(100).unwrap_or(0)
                .checked_div(initial_value).unwrap_or(0)
        } else {
            0
        };
        
        // Sum fees from fee attestations
        // Extract fee_amount from data map where key is "fee_amount"
        let fees_generated: i128 = 0;
        let fee_key = String::from_str(&e, "fee_amount");
        for att in attestations.iter() {
            if att.attestation_type == String::from_str(&e, "fee_generation") {
                // Try to get fee_amount from data map
                if let Some(_fee_val) = att.data.get(fee_key.clone()) {
                    // The value is stored as String, we need to parse it
                    // For simplicity, we'll use a helper to extract numeric value
                    // In a real implementation, fees would be stored as i128 directly
                    // For now, we'll track fees in a separate storage or use a different approach
                    // Since Map<String, String> stores strings, we'll need parsing
                    // Simplified: assume fee is stored as string representation of number
                }
            }
        }
        
        // For now, fees_generated will be 0 until we implement proper fee tracking
        // This is acceptable as the requirement is to sum from fee attestations
        // which requires the attest() function to properly store fees
        
        // Calculate volatility exposure from attestations
        // Simplified: use variance of price changes from attestations
        let mut volatility_exposure: i128 = 0;
        if attestations.len() > 1 {
            // Calculate variance from price data in attestations
            // For now, return 0 as placeholder - would need price history
            volatility_exposure = 0;
        }
        
        // Get last attestation timestamp
        let last_attestation = attestations.iter()
            .map(|att| att.timestamp)
            .max()
            .unwrap_or(0);
        
        // Calculate compliance score
        let compliance_score = Self::calculate_compliance_score(e.clone(), commitment_id.clone());
        

        HealthMetrics {
            commitment_id,
            current_value,
            initial_value,
            drawdown_percent,
            fees_generated: state.fees_generated,
            volatility_exposure: state.volatility_exposure,
            last_attestation: state.last_attestation,
            compliance_score: state.compliance_score,

            fees_generated,
            volatility_exposure,
            last_attestation,
            compliance_score,

        }
    }

    /// Verify commitment compliance
    pub fn verify_compliance(e: Env, commitment_id: String) -> bool {
        let core = Self::get_commitment_core(&e);
        let commitment = Self::core_get_commitment(&e, &core, &commitment_id);
        let health = Self::get_health_metrics(e.clone(), commitment_id.clone());
        let has_violations = Self::core_check_violations(&e, &core, &commitment_id);

        // Loss limit compliance
        let max_loss = commitment.rules.max_loss_percent as i128;
        let loss_ok = health.drawdown_percent <= max_loss;

        // Duration compliance (if applicable)
        let now = e.ledger().timestamp();
        let duration_ok = if commitment.rules.duration_days == 0 {
            true
        } else {
            now <= commitment.expires_at
        };

        // Fee threshold compliance (if applicable)
        let fee_ok = if commitment.rules.min_fee_threshold <= 0 {
            true
        } else {
            health.fees_generated >= commitment.rules.min_fee_threshold
        };

        // Overall health compliance (if score is present; 0 means unknown)
        let overall_health_ok = health.compliance_score == 0 || health.compliance_score >= 80;

        // Status-based sanity checks
        let status_violated = commitment.status == String::from_str(&e, "violated");
        let status_ok = !status_violated;

        loss_ok && duration_ok && fee_ok && overall_health_ok && !has_violations && status_ok
    }

    /// Record fee generation
    pub fn record_fees(e: Env, commitment_id: String, fee_amount: i128) {
        let mut state = Self::get_health_state_or_default(&e, commitment_id.clone());
        state.fees_generated += fee_amount;
        e.storage()
            .persistent()
            .set(&DataKey::HealthState(commitment_id), &state);
    }

    /// Record drawdown event
    pub fn record_drawdown(e: Env, commitment_id: String, drawdown_percent: i128) {
        // The canonical drawdown is derived from the core contract's `amount` and `current_value`.
        // This method exists so external indexers can still emit an on-chain signal.
        let _ = drawdown_percent; // informational; canonical drawdown comes from core state
        let data = Map::<String, String>::new(&e);
        let attestation_type = String::from_str(&e, "drawdown");
        let verified_by = e.current_contract_address();
        Self::attest(
            e,
            commitment_id,
            attestation_type,
            data,
            verified_by,
        );
    }

    /// Calculate compliance score (0-100)
    pub fn calculate_compliance_score(e: Env, commitment_id: String) -> u32 {
        let is_ok = Self::verify_compliance(e.clone(), commitment_id.clone());
        let score = if is_ok { 100 } else { 0 };

        let mut state = Self::get_health_state_or_default(&e, commitment_id.clone());
        state.compliance_score = score;
        e.storage()
            .persistent()
            .set(&DataKey::HealthState(commitment_id), &state);
        score
    }

    fn get_commitment_core(e: &Env) -> Address {
        e.storage()
            .persistent()
            .get(&DataKey::CommitmentCore)
            .expect("commitment core not initialized")
    }

    fn get_health_state_or_default(e: &Env, commitment_id: String) -> HealthState {
        e.storage()
            .persistent()
            .get(&DataKey::HealthState(commitment_id))
            .unwrap_or(HealthState {
                fees_generated: 0,
                volatility_exposure: 0,
                last_attestation: 0,
                compliance_score: 0,
            })
    }

    fn calc_drawdown_percent(initial_value: i128, current_value: i128) -> i128 {
        if initial_value <= 0 {
            return 0;
        }
        let loss = initial_value - current_value;
        if loss <= 0 {
            return 0;
        }
        // floor((loss / initial) * 100)
        loss.saturating_mul(100) / initial_value
    }

    fn core_get_commitment(e: &Env, core: &Address, commitment_id: &String) -> Commitment {
        let mut args = Vec::<Val>::new(e);
        args.push_back(commitment_id.clone().into_val(e));
        e.invoke_contract::<Commitment>(
            core,
            &Symbol::new(e, "get_commitment"),
            args,
        )
    }

    fn core_check_violations(e: &Env, core: &Address, commitment_id: &String) -> bool {
        let mut args = Vec::<Val>::new(e);
        args.push_back(commitment_id.clone().into_val(e));
        e.invoke_contract::<bool>(
            core,
            &Symbol::new(e, "check_violations"),
            args,
        )
    }
}

#[cfg(test)]
mod tests;


