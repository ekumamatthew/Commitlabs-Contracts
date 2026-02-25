#![no_std]

use shared_utils::{emit_error_event, Pausable, RateLimiter, SafeMath, TimeUtils, Validation};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, log, symbol_short, token, Address, Env,
    IntoVal, String, Symbol, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CommitmentError {
    InvalidDuration = 1,
    InvalidMaxLossPercent = 2,
    InvalidCommitmentType = 3,
    InvalidAmount = 4,
    InsufficientBalance = 5,
    TransferFailed = 6,
    MintingFailed = 7,
    CommitmentNotFound = 8,
    Unauthorized = 9,
    AlreadyInitialized = 10,
    AlreadySettled = 11,
    ReentrancyDetected = 12,
    NotActive = 13,
    InvalidStatus = 14,
    NotInitialized = 15,
    NotExpired = 16,
    ValueUpdateViolation = 17,
    NotAuthorizedUpdater = 18,
    ZeroAddress = 19,
    /// Duration would cause expires_at to overflow u64
    ExpirationOverflow = 20,
}

impl CommitmentError {
    /// Human-readable message for debugging and error events.
    pub fn message(&self) -> &'static str {
        match self {
            CommitmentError::InvalidDuration => "Invalid duration: must be greater than zero",
            CommitmentError::InvalidMaxLossPercent => "Invalid max loss: must be 0-100",
            CommitmentError::InvalidCommitmentType => "Invalid commitment type",
            CommitmentError::InvalidAmount => "Invalid amount: must be greater than zero",
            CommitmentError::InsufficientBalance => "Insufficient balance",
            CommitmentError::TransferFailed => "Token transfer failed",
            CommitmentError::MintingFailed => "NFT minting failed",
            CommitmentError::CommitmentNotFound => "Commitment not found",
            CommitmentError::Unauthorized => "Unauthorized: caller not allowed",
            CommitmentError::AlreadyInitialized => "Contract already initialized",
            CommitmentError::AlreadySettled => "Commitment already settled",
            CommitmentError::ReentrancyDetected => "Reentrancy detected",
            CommitmentError::NotActive => "Commitment is not active",
            CommitmentError::InvalidStatus => "Invalid commitment status for this operation",
            CommitmentError::NotInitialized => "Contract not initialized",
            CommitmentError::NotExpired => "Commitment has not expired yet",
            CommitmentError::ValueUpdateViolation => "Commitment has value update voilation",
            CommitmentError::NotAuthorizedUpdater => "Commitment has not auth updater",
            CommitmentError::ZeroAddress => "Zero address is not allowed",
            CommitmentError::ExpirationOverflow => "Duration would cause expiration timestamp overflow",
        }
    }
}

/// Emit error event and panic with standardized message (for indexers and UX).
fn fail(e: &Env, err: CommitmentError, context: &str) -> ! {
    emit_error_event(e, err as u32, context);
    panic!("{}", err.message());
}

/// Event emitted when a commitment is successfully settled.
#[contracttype]
#[derive(Clone)]
pub struct CommitmentSettledEvent {
    pub commitment_id: String,
    pub owner: Address,
    pub settlement_amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct CommitmentCreatedEvent {
    pub commitment_id: String,
    pub owner: Address,
    pub amount: i128,
    pub asset_address: Address,
    pub nft_token_id: u32,
    pub rules: CommitmentRules,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentRules {
    pub duration_days: u32,
    pub max_loss_percent: u32,
    pub commitment_type: String, // "safe", "balanced", "aggressive"
    pub early_exit_penalty: u32,
    pub min_fee_threshold: i128,
    pub grace_period_days: u32,
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
#[derive(Clone)]
pub enum DataKey {
    Admin,
    NftContract,
    AllocationContract,        // authorized allocation logic contract
    Commitment(String),        // commitment_id -> Commitment
    OwnerCommitments(Address), // owner -> Vec<commitment_id>
    TotalCommitments,          // counter
    ReentrancyGuard,           // reentrancy protection flag
    TotalValueLocked,          // aggregate value locked across active commitments
    AuthorizedUpdaters,        // whitelist of authorized updaters
}

// ─── Token helpers ────────────────────────────────────────────────────────────

fn is_zero_address(e: &Env, address: &Address) -> bool {
    let zero_str = String::from_str(e, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF");
    
    // Correct 1-argument method for the native Address type
    let zero_addr = Address::from_string(&zero_str);
    
    address == &zero_addr
}

fn check_sufficient_balance(
    e: &Env,
    owner: &Address,
    asset_address: &Address,
    amount: i128,
) {
    let token_client = token::Client::new(e, asset_address);
    let balance = token_client.balance(owner);
    if balance < amount {
        log!(e, "Insufficient balance: {} < {}", balance, amount);
        fail(e, CommitmentError::InsufficientBalance, "check_sufficient_balance");
    }
}

/// Transfer assets from owner to contract.
fn transfer_assets(e: &Env, from: &Address, to: &Address, asset_address: &Address, amount: i128) {
    let token_client = token::Client::new(e, asset_address);
    token_client.transfer(from, to, &amount);
}

/// Call the NFT contract mint function.
fn call_nft_mint(
    e: &Env,
    nft_contract: &Address,
    owner: &Address,
    commitment_id: &String,
    duration_days: u32,
    max_loss_percent: u32,
    commitment_type: &String,
    initial_amount: i128,
    asset_address: &Address,
) -> u32 {
    let mut args = Vec::new(e);
    args.push_back(owner.clone().into_val(e));
    args.push_back(commitment_id.clone().into_val(e));
    args.push_back(duration_days.into_val(e));
    args.push_back(max_loss_percent.into_val(e));
    args.push_back(commitment_type.clone().into_val(e));
    args.push_back(initial_amount.into_val(e));
    args.push_back(asset_address.clone().into_val(e));

    e.invoke_contract::<u32>(nft_contract, &Symbol::new(e, "mint"), args)
}

// ─── Storage helpers ──────────────────────────────────────────────────────────

fn read_commitment(e: &Env, commitment_id: &String) -> Option<Commitment> {
    e.storage()
        .instance()
        .get::<_, Commitment>(&DataKey::Commitment(commitment_id.clone()))
}

fn set_commitment(e: &Env, commitment: &Commitment) {
    e.storage().instance().set(
        &DataKey::Commitment(commitment.commitment_id.clone()),
        commitment,
    );
}

fn has_commitment(e: &Env, commitment_id: &String) -> bool {
    e.storage()
        .instance()
        .has(&DataKey::Commitment(commitment_id.clone()))
}

fn require_no_reentrancy(e: &Env) {
    let guard: bool = e
        .storage()
        .instance()
        .get::<_, bool>(&DataKey::ReentrancyGuard)
        .unwrap_or(false);

    if guard {
        fail(
            e,
            CommitmentError::ReentrancyDetected,
            "require_no_reentrancy",
        );
    }
}

fn set_reentrancy_guard(e: &Env, value: bool) {
    e.storage()
        .instance()
        .set(&DataKey::ReentrancyGuard, &value);
}

/// Require that the caller is the admin stored in this contract.
fn require_admin(e: &Env, caller: &Address) {
    caller.require_auth();
    let admin = e
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::Admin)
        .unwrap_or_else(|| fail(e, CommitmentError::NotInitialized, "require_admin"));
    if *caller != admin {
        fail(e, CommitmentError::Unauthorized, "require_admin");
    }
}

fn require_authorized_updater(e: &Env, caller: &Address) {
    caller.require_auth();
    let updaters: Vec<Address> = e
        .storage()
        .instance()
        .get::<_, Vec<Address>>(&DataKey::AuthorizedUpdaters)
        .unwrap_or(Vec::new(e));
    if !updaters.contains(caller) {
        fail(e, CommitmentError::NotAuthorizedUpdater, "Unauthorized");
    }
}

fn add_authorized_updater(e: &Env, updater: &Address) {
    let mut updaters: Vec<Address> = e
        .storage()
        .instance()
        .get::<_, Vec<Address>>(&DataKey::AuthorizedUpdaters)
        .unwrap_or(Vec::new(e));
    if !updaters.contains(updater) {
        updaters.push_back(updater.clone());
        e.storage()
            .instance()
            .set(&DataKey::AuthorizedUpdaters, &updaters);
    }
}

fn remove_authorized_updater(e: &Env, updater: &Address) {
    let mut updaters: Vec<Address> = e
        .storage()
        .instance()
        .get::<_, Vec<Address>>(&DataKey::AuthorizedUpdaters)
        .unwrap_or(Vec::new(e));
    if let Some(idx) = updaters.iter().position(|a| a == *updater) {
        updaters.remove(idx as u32);
        e.storage()
            .instance()
            .set(&DataKey::AuthorizedUpdaters, &updaters);
    }
}

/// Remove a commitment from an owner's commitment list
fn remove_from_owner_commitments(e: &Env, owner: &Address, commitment_id: &String) {
    let mut owner_commitments: Vec<String> = e
        .storage()
        .instance()
        .get::<_, Vec<String>>(&DataKey::OwnerCommitments(owner.clone()))
        .unwrap_or(Vec::new(e));

    if let Some(idx) = owner_commitments.iter().position(|id| id == *commitment_id) {
        owner_commitments.remove(idx as u32);
        e.storage()
            .instance()
            .set(&DataKey::OwnerCommitments(owner.clone()), &owner_commitments);
    }
}

#[contract]
pub struct CommitmentCoreContract;

#[contractimpl]
impl CommitmentCoreContract {

    /// Pause the contract. Caller must be admin.
    pub fn pause(e: Env, caller: Address) {
        require_admin(&e, &caller);
        Pausable::pause(&e);
    }

    /// Unpause the contract. Caller must be admin.
    pub fn unpause(e: Env, caller: Address) {
        require_admin(&e, &caller);
        Pausable::unpause(&e);
    }

    /// Returns `true` if the contract is currently paused.
    pub fn is_paused(e: Env) -> bool {
        Pausable::is_paused(&e)
    }

    /// Validate commitment rules using shared utilities
    fn validate_rules(e: &Env, rules: &CommitmentRules) {
        // Duration must be > 0
        Validation::require_valid_duration(rules.duration_days);

        // Max loss percent must be between 0 and 100
        Validation::require_valid_percent(rules.max_loss_percent);

        // Commitment type must be valid
        let valid_types = ["safe", "balanced", "aggressive"];
        Validation::require_valid_commitment_type(e, &rules.commitment_type, &valid_types);
    }

    /// Generate unique commitment ID
    /// Optimized: Uses counter to create unique ID efficiently
    fn generate_commitment_id(e: &Env, counter: u64) -> String {
        let mut buf = [0u8; 32];
        let prefix = b"c_";
        buf[0] = prefix[0];
        buf[1] = prefix[1];

        // Convert counter to string representation
        let mut n = counter;
        let mut i = 2;
        if n == 0 {
            buf[i] = b'0';
            i += 1;
        } else {
            let mut digits = [0u8; 20];
            let mut digit_count = 0;
            while n > 0 {
                digits[digit_count] = (n % 10) as u8 + b'0';
                n /= 10;
                digit_count += 1;
            }
            // Reverse digits
            for j in 0..digit_count {
                buf[i] = digits[digit_count - 1 - j];
                i += 1;
            }
        }

        String::from_str(e, core::str::from_utf8(&buf[..i]).unwrap_or("c_0"))
    }

    /// Initialize the core commitment contract
    pub fn initialize(e: Env, admin: Address, nft_contract: Address) {
        // Check if already initialized
        if e.storage().instance().has(&DataKey::Admin) {
            fail(&e, CommitmentError::AlreadyInitialized, "initialize");
        }

        // Store admin and NFT contract address
        e.storage().instance().set(&DataKey::Admin, &admin);
        e.storage()
            .instance()
            .set(&DataKey::NftContract, &nft_contract);

        // Initialize total commitments counter
        e.storage()
            .instance()
            .set(&DataKey::TotalCommitments, &0u64);

        // Initialize total value locked counter
        e.storage()
            .instance()
            .set(&DataKey::TotalValueLocked, &0i128);

        // Initialize paused state (default: not paused)
        e.storage().instance().set(&Pausable::PAUSED_KEY, &false);
    }

    /// Create a new commitment
    ///
    /// # Reentrancy Protection
    /// This function uses checks-effects-interactions pattern:
    /// 1. Checks: Validate inputs
    /// 2. Effects: Update state (commitment storage, counters)
    /// 3. Interactions: External calls (token transfer, NFT mint)
    /// Reentrancy guard prevents recursive calls.
    pub fn create_commitment(
        e: Env,
        owner: Address,
        amount: i128,
        asset_address: Address,
        rules: CommitmentRules,
    ) -> String {
        // Reentrancy protection
        require_no_reentrancy(&e);
        set_reentrancy_guard(&e, true);

        // Check if contract is paused
        Pausable::require_not_paused(&e);

        // Rate limit: per-owner commitment creation
        let fn_symbol = symbol_short!("create");
        
        // Reject zero address owner
        if is_zero_address(&e, &owner) {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::ZeroAddress, "create_commitment");
        }
        RateLimiter::check(&e, &owner, &fn_symbol);

        // Validate amount > 0 using shared utilities
        Validation::require_positive(amount);

        // Validate rules
        Self::validate_rules(&e, &rules);

        // CHECKS: Verify sufficient balance BEFORE any state modifications (CEI pattern)
        check_sufficient_balance(&e, &owner, &asset_address, amount);

        // Reject duration_days that would cause expires_at to overflow u64
        let expires_at = TimeUtils::checked_calculate_expiration(&e, rules.duration_days)
            .unwrap_or_else(|| {
                set_reentrancy_guard(&e, false);
                fail(&e, CommitmentError::ExpirationOverflow, "create_commitment")
            });

        // OPTIMIZATION: Read both counters and NFT contract once to minimize storage operations
        let (current_total, current_tvl, nft_contract) = {
            let total = e
                .storage()
                .instance()
                .get::<_, u64>(&DataKey::TotalCommitments)
                .unwrap_or(0);
            let tvl = e
                .storage()
                .instance()
                .get::<_, i128>(&DataKey::TotalValueLocked)
                .unwrap_or(0);
            let nft = e
                .storage()
                .instance()
                .get::<_, Address>(&DataKey::NftContract)
                .unwrap_or_else(|| {
                    set_reentrancy_guard(&e, false);
                    fail(&e, CommitmentError::NotInitialized, "create_commitment")
                });
            (total, tvl, nft)
        };

        // Generate unique commitment ID using counter
        let commitment_id = Self::generate_commitment_id(&e, current_total);

        // CHECKS: Validate commitment doesn't already exist
        if has_commitment(&e, &commitment_id) {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::InvalidStatus, "create_commitment");
        }

        // EFFECTS: Update state before external calls
        let current_timestamp = TimeUtils::now(&e);

        // Create commitment data
        let commitment = Commitment {
            commitment_id: commitment_id.clone(),
            owner: owner.clone(),
            nft_token_id: 0, // Will be set after NFT mint
            rules: rules.clone(),
            amount,
            asset_address: asset_address.clone(),
            created_at: current_timestamp,
            expires_at,
            current_value: amount, // Initially same as amount
            status: String::from_str(&e, "active"),
        };

        // Store commitment data (before external calls)
        set_commitment(&e, &commitment);

        // Update owner's commitment list
        let mut owner_commitments = e
            .storage()
            .instance()
            .get::<_, Vec<String>>(&DataKey::OwnerCommitments(owner.clone()))
            .unwrap_or(Vec::new(&e));
        owner_commitments.push_back(commitment_id.clone());
        e.storage().instance().set(
            &DataKey::OwnerCommitments(owner.clone()),
            &owner_commitments,
        );

        // OPTIMIZATION: Increment both counters using already-read values
        e.storage()
            .instance()
            .set(&DataKey::TotalCommitments, &(current_total + 1));
        e.storage()
            .instance()
            .set(&DataKey::TotalValueLocked, &(current_tvl + amount));

        // INTERACTIONS: External calls (token transfer, NFT mint)
        // Transfer assets from owner to contract
        let contract_address = e.current_contract_address();
        transfer_assets(&e, &owner, &contract_address, &asset_address, amount);

        // Mint NFT
        let nft_token_id = call_nft_mint(
            &e,
            &nft_contract,
            &owner,
            &commitment_id,
            rules.duration_days,
            rules.max_loss_percent,
            &rules.commitment_type,
            amount,
            &asset_address,
        );

        // Update commitment with NFT token ID
        let mut updated_commitment = commitment;
        updated_commitment.nft_token_id = nft_token_id;
        set_commitment(&e, &updated_commitment);

        // Clear reentrancy guard
        set_reentrancy_guard(&e, false);

        // Emit creation event
        e.events().publish(
            (
                symbol_short!("Created"),
                commitment_id.clone(),
                owner.clone(),
            ),
            (amount, rules, nft_token_id, e.ledger().timestamp()),
        );
        commitment_id
    }

    /// Get commitment details
    pub fn get_commitment(e: Env, commitment_id: String) -> Commitment {
        read_commitment(&e, &commitment_id)
            .unwrap_or_else(|| fail(&e, CommitmentError::CommitmentNotFound, "get_commitment"))
    }

    /// Get all commitments for an owner
    pub fn get_owner_commitments(e: Env, owner: Address) -> Vec<String> {
        e.storage()
            .instance()
            .get::<_, Vec<String>>(&DataKey::OwnerCommitments(owner))
            .unwrap_or(Vec::new(&e))
    }

    /// Get total number of commitments
    pub fn get_total_commitments(e: Env) -> u64 {
        e.storage()
            .instance()
            .get::<_, u64>(&DataKey::TotalCommitments)
            .unwrap_or(0)
    }

    /// Get total value locked across all active commitments.
    pub fn get_total_value_locked(e: Env) -> i128 {
        e.storage()
            .instance()
            .get::<_, i128>(&DataKey::TotalValueLocked)
            .unwrap_or(0)
    }

    /// Get admin address
    pub fn get_admin(e: Env) -> Address {
        e.storage()
            .instance()
            .get::<_, Address>(&DataKey::Admin)
            .unwrap_or_else(|| fail(&e, CommitmentError::NotInitialized, "get_admin"))
    }

    /// Get NFT contract address
    pub fn get_nft_contract(e: Env) -> Address {
        e.storage()
            .instance()
            .get::<_, Address>(&DataKey::NftContract)
            .unwrap_or_else(|| fail(&e, CommitmentError::NotInitialized, "get_nft_contract"))
    }

    /// Set allocation contract address (only admin can call)
    pub fn set_allocation_contract(e: Env, caller: Address, allocation_contract: Address) {
        require_admin(&e, &caller);
        e.storage()
            .instance()
            .set(&DataKey::AllocationContract, &allocation_contract);
    }

    /// Get allocation contract address
    pub fn get_allocation_contract(e: Env) -> Option<Address> {
        e.storage()
            .instance()
            .get::<_, Address>(&DataKey::AllocationContract)
    }

    /// Update commitment value (called by allocation logic or oracle-fed keeper).
    /// Persists new_value to commitment.current_value and updates TotalValueLocked.
    pub fn update_value(e: Env, caller: Address, commitment_id: String, new_value: i128) {
        // Access control: only admin or authorized allocation contract can update value
        let admin = e
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::Admin)
            .unwrap_or_else(|| fail(&e, CommitmentError::NotInitialized, "update_value"));

        let allocation_contract = e
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::AllocationContract);

        let is_authorized = caller == admin ||
            (allocation_contract.is_some() && caller == allocation_contract.unwrap());

        if !is_authorized {
            fail(&e, CommitmentError::Unauthorized, "update_value");
        }

        // Global per-function rate limit (per contract instance)
        let fn_symbol = symbol_short!("upd_val");
        let contract_address = e.current_contract_address();
        RateLimiter::check(&e, &contract_address, &fn_symbol);

        Validation::require_non_negative(new_value);

        let mut commitment = read_commitment(&e, &commitment_id)
            .unwrap_or_else(|| fail(&e, CommitmentError::CommitmentNotFound, "update_value"));

        let active_status = String::from_str(&e, "active");
        if commitment.status != active_status {
            fail(&e, CommitmentError::NotActive, "update_value");
        }

        let old_value = commitment.current_value;
        commitment.current_value = new_value;

        // Violation detection
        let loss_percent = if commitment.amount > 0 {
            (commitment.amount - new_value) * 100 / commitment.amount
        } else {
            0
        };

        let violated = loss_percent > commitment.rules.max_loss_percent as i128;
        if violated {
            commitment.status = String::from_str(&e, "violated");
            e.events().publish(
                (symbol_short!("Violated"), commitment_id.clone()),
                (
                    loss_percent,
                    commitment.rules.max_loss_percent,
                    e.ledger().timestamp(),
                ),
            );
        }

        set_commitment(&e, &commitment);

        // Update TVL
        let current_tvl = e
            .storage()
            .instance()
            .get::<_, i128>(&DataKey::TotalValueLocked)
            .unwrap_or(0);
        e.storage().instance().set(
            &DataKey::TotalValueLocked,
            &(current_tvl - old_value + new_value),
        );

        e.events().publish(
            (symbol_short!("ValUpd"), commitment_id),
            (old_value, new_value, violated, e.ledger().timestamp()),
        );
    }

    /// Check if commitment rules are violated
    pub fn check_violations(e: Env, commitment_id: String) -> bool {
        let commitment = read_commitment(&e, &commitment_id)
            .unwrap_or_else(|| fail(&e, CommitmentError::CommitmentNotFound, "check_violations"));

        // Skip check if already settled or violated
        let active_status = String::from_str(&e, "active");
        if commitment.status != active_status {
            return false; // Already processed
        }

        let current_time = e.ledger().timestamp();

        let loss_percent = if commitment.amount > 0 {
            SafeMath::loss_percent(commitment.amount, commitment.current_value)
        } else {
            0
        };

        let max_loss = commitment.rules.max_loss_percent as i128;
        let loss_violated = loss_percent > max_loss;
        let duration_violated = current_time >= commitment.expires_at;

        let violated = loss_violated || duration_violated;

        if violated {
            e.events().publish(
                (symbol_short!("Violated"), commitment_id),
                (symbol_short!("RuleViol"), e.ledger().timestamp()),
            );
        }

        violated
    }

    /// Get detailed violation information
    pub fn get_violation_details(e: Env, commitment_id: String) -> (bool, bool, bool, i128, u64) {
        let commitment = read_commitment(&e, &commitment_id).unwrap_or_else(|| {
            fail(
                &e,
                CommitmentError::CommitmentNotFound,
                "get_violation_details",
            )
        });

        let current_time = e.ledger().timestamp();

        let loss_amount = commitment.amount - commitment.current_value;
        let loss_percent = if commitment.amount > 0 {
            (loss_amount * 100) / commitment.amount
        } else {
            0
        };

        let max_loss = commitment.rules.max_loss_percent as i128;
        let loss_violated = loss_percent > max_loss;
        let duration_violated = current_time >= commitment.expires_at;

        let time_remaining = if current_time < commitment.expires_at {
            commitment.expires_at - current_time
        } else {
            0
        };

        let has_violations = loss_violated || duration_violated;

        (
            has_violations,
            loss_violated,
            duration_violated,
            loss_percent,
            time_remaining,
        )
    }

    /// Settle commitment at maturity
    pub fn settle(e: Env, commitment_id: String) {
        require_no_reentrancy(&e);
        set_reentrancy_guard(&e, true);

        Pausable::require_not_paused(&e);

        let mut commitment = read_commitment(&e, &commitment_id).unwrap_or_else(|| {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::CommitmentNotFound, "settle")
        });

        let current_time = e.ledger().timestamp();
        if current_time < commitment.expires_at {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::NotExpired, "settle");
        }

        let active_status = String::from_str(&e, "active");
        let settled_status = String::from_str(&e, "settled");
        if commitment.status == settled_status {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::AlreadySettled, "settle");
        }
        if commitment.status != active_status {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::NotActive, "settle");
        }

        let settlement_amount = commitment.current_value;
        let owner = commitment.owner.clone();
        commitment.status = settled_status;
        set_commitment(&e, &commitment);

        remove_from_owner_commitments(&e, &owner, &commitment_id);

        let current_tvl = e
            .storage()
            .instance()
            .get::<_, i128>(&DataKey::TotalValueLocked)
            .unwrap_or(0);
        let new_tvl = if current_tvl > settlement_amount {
            current_tvl - settlement_amount
        } else {
            0
        };
        e.storage()
            .instance()
            .set(&DataKey::TotalValueLocked, &new_tvl);

        let contract_address = e.current_contract_address();
        let token_client = token::Client::new(&e, &commitment.asset_address);
        token_client.transfer(&contract_address, &owner, &settlement_amount);

        let nft_contract = e
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::NftContract)
            .unwrap_or_else(|| {
                set_reentrancy_guard(&e, false);
                fail(&e, CommitmentError::NotInitialized, "settle")
            });

        let mut args = Vec::new(&e);
        args.push_back(contract_address.into_val(&e));
        args.push_back(commitment.nft_token_id.into_val(&e));
        e.invoke_contract::<()>(&nft_contract, &Symbol::new(&e, "settle"), args);

        set_reentrancy_guard(&e, false);

        let timestamp = e.ledger().timestamp();
        e.events().publish(
            (symbol_short!("Settled"), commitment_id.clone(), owner.clone()),
            (settlement_amount, timestamp),
        );
    }

    pub fn early_exit(e: Env, commitment_id: String, caller: Address) {
        require_no_reentrancy(&e);
        set_reentrancy_guard(&e, true);

        Pausable::require_not_paused(&e);

        let mut commitment = read_commitment(&e, &commitment_id).unwrap_or_else(|| {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::CommitmentNotFound, "early_exit")
        });

        caller.require_auth();
        if commitment.owner != caller {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::Unauthorized, "early_exit");
        }

        let active_status = String::from_str(&e, "active");
        if commitment.status != active_status {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::NotActive, "early_exit");
        }

        let penalty_amount = SafeMath::penalty_amount(
            commitment.current_value,
            commitment.rules.early_exit_penalty,
        );
        let returned_amount = SafeMath::sub(commitment.current_value, penalty_amount);

        commitment.status = String::from_str(&e, "early_exit");
        commitment.current_value = 0; 
        set_commitment(&e, &commitment);

        let current_tvl = e
            .storage()
            .instance()
            .get::<_, i128>(&DataKey::TotalValueLocked)
            .unwrap_or(0);
        let new_tvl = current_tvl - commitment.current_value;
        e.storage()
            .instance()
            .set(&DataKey::TotalValueLocked, &new_tvl);

        let contract_address = e.current_contract_address();
        let token_client = token::Client::new(&e, &commitment.asset_address);

        if returned_amount > 0 {
            token_client.transfer(&contract_address, &commitment.owner, &returned_amount);
        }

        let nft_contract = e
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::NftContract)
            .unwrap_or_else(|| {
                set_reentrancy_guard(&e, false);
                fail(&e, CommitmentError::NotInitialized, "early_exit")
            });

        let core_address = e.current_contract_address();
        let mut args = Vec::new(&e);
        args.push_back(core_address.into_val(&e));
        args.push_back(commitment.nft_token_id.into_val(&e));
        e.invoke_contract::<()>(&nft_contract, &Symbol::new(&e, "settle"), args);

        set_reentrancy_guard(&e, false);

        e.events().publish(
            (
                symbol_short!("EarlyExt"),
                commitment_id.clone(),
                caller.clone(),
            ),
            (penalty_amount, returned_amount, e.ledger().timestamp()),
        );
    }

    /// Allocate liquidity (called by allocation strategy)
    pub fn allocate(e: Env, commitment_id: String, target_pool: Address, amount: i128) {
        require_no_reentrancy(&e);
        set_reentrancy_guard(&e, true);

        Pausable::require_not_paused(&e);

        let fn_symbol = symbol_short!("alloc");
        RateLimiter::check(&e, &target_pool, &fn_symbol);

        if amount <= 0 {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::InvalidAmount, "allocate");
        }

        let commitment = read_commitment(&e, &commitment_id).unwrap_or_else(|| {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::CommitmentNotFound, "allocate")
        });

        let active_status = String::from_str(&e, "active");
        if commitment.status != active_status {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::NotActive, "allocate");
        }

        if commitment.current_value < amount {
            set_reentrancy_guard(&e, false);
            fail(&e, CommitmentError::InsufficientBalance, "allocate");
        }

        let mut updated_commitment = commitment;
        updated_commitment.current_value = updated_commitment.current_value - amount;
        set_commitment(&e, &updated_commitment);

        let contract_address = e.current_contract_address();
        let token_client = token::Client::new(&e, &updated_commitment.asset_address);
        token_client.transfer(&contract_address, &target_pool, &amount);

        set_reentrancy_guard(&e, false);

        e.events().publish(
            (symbol_short!("Alloc"), commitment_id, target_pool),
            (amount, e.ledger().timestamp()),
        );
    }

    /// Configure rate limits for this contract's functions.
    pub fn set_rate_limit(
        e: Env,
        caller: Address,
        function: Symbol,
        window_seconds: u64,
        max_calls: u32,
    ) {
        require_admin(&e, &caller);
        RateLimiter::set_limit(&e, &function, window_seconds, max_calls);
    }

    /// Set or clear rate limit exemption for an address.
    pub fn set_rate_limit_exempt(e: Env, caller: Address, address: Address, exempt: bool) {
        require_admin(&e, &caller);
        RateLimiter::set_exempt(&e, &address, exempt);
    }

    pub fn add_updater(e: Env, caller: Address, updater: Address) {
        require_admin(&e, &caller);
        add_authorized_updater(&e, &updater);
    }

    pub fn remove_updater(e: Env, caller: Address, updater: Address) {
        require_admin(&e, &caller);
        remove_authorized_updater(&e, &updater);
    }

    pub fn get_authorized_updaters(e: Env) -> Vec<Address> {
        e.storage()
            .instance()
            .get::<_, Vec<Address>>(&DataKey::AuthorizedUpdaters)
            .unwrap_or(Vec::new(&e))
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "benchmark"))]
mod benchmarks;
