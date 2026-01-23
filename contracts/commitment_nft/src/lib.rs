#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, contracterror, Address, Env, String, Vec, symbol_short};

// Storage keys for persistent data
#[contracttype]
pub enum DataKey {
    Admin,                    // Admin address
    TokenCounter,             // Current token counter for generating IDs
    NFT(u32),                 // NFT data by token_id
    OwnerBalance(Address),    // Balance count per owner
    OwnerTokens(Address),     // Vec of token IDs per owner
    TokenIds,                 // Vec of all token IDs
}

// Contract errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    TokenNotFound = 3,
    InvalidTokenId = 4,
    NotOwner = 5,
    NotAuthorized = 6,
    TransferNotAllowed = 7,
    AlreadySettled = 8,
    NotExpired = 9,
}

#[cfg(test)]
mod tests;

// ============================================================================
// Error Types
// ============================================================================

/// Contract errors for structured error handling
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// Contract has not been initialized
    NotInitialized = 1,
    /// Contract has already been initialized
    AlreadyInitialized = 2,
    /// Caller is not authorized to perform this action
    Unauthorized = 3,
    /// Invalid duration (must be > 0)
    InvalidDuration = 4,
    /// Invalid max loss percent (must be 0-100)
    InvalidMaxLoss = 5,
    /// Invalid commitment type (must be safe, balanced, or aggressive)
    InvalidCommitmentType = 6,
    /// Invalid amount (must be > 0)
    InvalidAmount = 7,
    /// NFT with the given token_id does not exist
    TokenNotFound = 8,
    /// NFT has already been settled
    AlreadySettled = 9,
    /// Commitment has not expired yet
    NotExpired = 10,
    /// Caller is not the owner of the NFT
    NotOwner = 11,
}

// ============================================================================
// Data Types
// ============================================================================

/// Metadata associated with a commitment NFT
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentMetadata {
    pub commitment_id: String,
    pub duration_days: u32,
    pub max_loss_percent: u32,
    pub commitment_type: String, // "safe", "balanced", "aggressive"
    pub created_at: u64,
    pub expires_at: u64,
    pub initial_amount: i128,
    pub asset_address: Address,
}

/// The Commitment NFT structure
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitmentNFT {
    pub owner: Address,
    pub token_id: u32,
    pub metadata: CommitmentMetadata,
    pub is_active: bool,
    pub early_exit_penalty: u32,
}

/// Storage keys for the contract
#[contracttype]
pub enum DataKey {
    /// Admin address (singleton)
    Admin,
    /// Counter for generating unique token IDs / Total supply
    TotalSupply,
    /// NFT data storage (token_id -> CommitmentNFT)
    Nft(u32),
    /// Owner mapping (token_id -> Address)
    Owner(u32),
    /// Authorized minter addresses (from upstream)
    AuthorizedMinter(Address),
    /// Authorized commitment_core contract address (for settlement)
    CoreContract,
    /// Active status (token_id -> bool)
    ActiveStatus(u32),
}

// Events
const MINT: soroban_sdk::Symbol = symbol_short!("mint");

// ============================================================================
// Contract Implementation
// ============================================================================

#[contract]
pub struct CommitmentNFTContract;

#[contractimpl]
impl CommitmentNFTContract {
    /// Initialize the NFT contract
    pub fn initialize(e: Env, admin: Address) -> Result<(), ContractError> {
        // Check if already initialized
        if e.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }

        // Store admin address
        e.storage().instance().set(&DataKey::Admin, &admin);

        // Initialize token counter to 0
        e.storage().instance().set(&DataKey::TokenCounter, &0u32);

        // Initialize empty token IDs vector
        let token_ids: Vec<u32> = Vec::new(&e);
        e.storage().instance().set(&DataKey::TokenIds, &token_ids);

        Ok(())
    }

    /// Validate commitment type
    fn is_valid_commitment_type(e: &Env, commitment_type: &String) -> bool {
        let safe = String::from_str(e, "safe");
        let balanced = String::from_str(e, "balanced");
        let aggressive = String::from_str(e, "aggressive");
        *commitment_type == safe || *commitment_type == balanced || *commitment_type == aggressive
    }

    /// Set the authorized commitment_core contract address for settlement
    /// Only the admin can call this function
    pub fn set_core_contract(e: Env, core_contract: Address) -> Result<(), Error> {
        let admin: Address = e
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        e.storage()
            .instance()
            .set(&DataKey::CoreContract, &core_contract);

        // Emit event for access control change
        e.events()
            .publish((Symbol::new(&e, "CoreContractSet"),), (core_contract,));

        Ok(())
    }

    /// Get the authorized commitment_core contract address
    pub fn get_core_contract(e: Env) -> Result<Address, Error> {
        e.storage()
            .instance()
            .get(&DataKey::CoreContract)
            .ok_or(Error::NotInitialized)
    }

    /// Get the admin address
    pub fn get_admin(e: Env) -> Result<Address, Error> {
        e.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    // ========================================================================
    // NFT Minting
    // ========================================================================

    /// Mint a new Commitment NFT
    ///
    /// # Arguments
    /// * `caller` - The address calling the mint function (must be authorized)
    /// * `owner` - The address that will own the NFT
    /// * `commitment_id` - Unique identifier for the commitment
    /// * `duration_days` - Duration of the commitment in days
    /// * `max_loss_percent` - Maximum allowed loss percentage (0-100)
    /// * `commitment_type` - Type of commitment ("safe", "balanced", "aggressive")
    /// * `initial_amount` - Initial amount committed
    /// * `asset_address` - Address of the asset contract
    ///
    /// # Returns
    /// The token_id of the newly minted NFT
    pub fn mint(
        e: Env,
        owner: Address,
        commitment_id: String,
        duration_days: u32,
        max_loss_percent: u32,
        commitment_type: String,
        initial_amount: i128,
        asset_address: Address,
        early_exit_penalty: u32,
    ) -> Result<u32, ContractError> {
        // Verify contract is initialized
        if !e.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::NotInitialized);
        }

        // Generate unique token_id
        let token_id: u32 = e.storage().instance().get(&DataKey::TokenCounter).unwrap_or(0);
        let next_token_id = token_id + 1;
        e.storage().instance().set(&DataKey::TokenCounter, &next_token_id);

        // Calculate timestamps
        let created_at = e.ledger().timestamp();
        let seconds_per_day: u64 = 86400;
        let expires_at = created_at + (duration_days as u64 * seconds_per_day);

        // Create CommitmentMetadata
        let metadata = CommitmentMetadata {
            commitment_id,
            duration_days,
            max_loss_percent,
            commitment_type,
            created_at,
            expires_at,
            initial_amount,
            asset_address,
        };

        // Create CommitmentNFT
        let nft = CommitmentNFT {
            owner: owner.clone(),
            token_id,
            metadata,
            is_active: true,
            early_exit_penalty,
        };

        // Store NFT data
        e.storage().persistent().set(&DataKey::NFT(token_id), &nft);

        // Update owner balance
        let current_balance: u32 = e.storage().persistent().get(&DataKey::OwnerBalance(owner.clone())).unwrap_or(0);
        e.storage().persistent().set(&DataKey::OwnerBalance(owner.clone()), &(current_balance + 1));

        // Update owner tokens list
        let mut owner_tokens: Vec<u32> = e.storage().persistent().get(&DataKey::OwnerTokens(owner.clone())).unwrap_or(Vec::new(&e));
        owner_tokens.push_back(token_id);
        e.storage().persistent().set(&DataKey::OwnerTokens(owner.clone()), &owner_tokens);

        // Add token_id to the list of all tokens
        let mut token_ids: Vec<u32> = e.storage().instance().get(&DataKey::TokenIds).unwrap_or(Vec::new(&e));
        token_ids.push_back(token_id);
        e.storage().instance().set(&DataKey::TokenIds, &token_ids);

        // Emit mint event
        e.events().publish((symbol_short!("mint"), owner), token_id);

        Ok(token_id)
    }

    // ========================================================================
    // NFT Query Functions
    // ========================================================================

    /// Get NFT metadata by token_id
    pub fn get_metadata(e: Env, token_id: u32) -> Result<CommitmentNFT, ContractError> {
        e.storage()
            .persistent()
            .get(&DataKey::NFT(token_id))
            .ok_or(ContractError::TokenNotFound)
    }


    /// Get owner of NFT
    pub fn owner_of(e: Env, token_id: u32) -> Result<Address, ContractError> {
        let nft: CommitmentNFT = e
            .storage()
            .persistent()
            .get(&DataKey::NFT(token_id))
            .ok_or(ContractError::TokenNotFound)?;

        Ok(nft.owner)
    }

    /// Transfer NFT to new owner
    pub fn transfer(e: Env, from: Address, to: Address, token_id: u32) -> Result<(), ContractError> {
        // Require authorization from the sender
        from.require_auth();

        // Get the NFT
        let mut nft: CommitmentNFT = e
            .storage()
            .persistent()
            .get(&DataKey::NFT(token_id))
            .ok_or(ContractError::TokenNotFound)?;

        // Verify ownership
        if nft.owner != from {
            return Err(ContractError::NotOwner);
        }

        // Check if NFT is still active (active NFTs may have transfer restrictions)
        // For now, we allow transfers regardless of active status
        // Uncomment below to restrict transfers of active NFTs:
        // if nft.is_active {
        //     return Err(ContractError::TransferNotAllowed);
        // }

        // Update owner
        nft.owner = to.clone();
        e.storage().persistent().set(&DataKey::NFT(token_id), &nft);

        // Update balance counts
        let from_balance: u32 = e.storage().persistent().get(&DataKey::OwnerBalance(from.clone())).unwrap_or(0);
        if from_balance > 0 {
            e.storage().persistent().set(&DataKey::OwnerBalance(from.clone()), &(from_balance - 1));
        }

        let to_balance: u32 = e.storage().persistent().get(&DataKey::OwnerBalance(to.clone())).unwrap_or(0);
        e.storage().persistent().set(&DataKey::OwnerBalance(to.clone()), &(to_balance + 1));

        // Update owner tokens lists
        let mut from_tokens: Vec<u32> = e.storage().persistent().get(&DataKey::OwnerTokens(from.clone())).unwrap_or(Vec::new(&e));
        if let Some(index) = from_tokens.iter().position(|id| id == token_id) {
            from_tokens.remove(index as u32);
        }
        e.storage().persistent().set(&DataKey::OwnerTokens(from.clone()), &from_tokens);

        let mut to_tokens: Vec<u32> = e.storage().persistent().get(&DataKey::OwnerTokens(to.clone())).unwrap_or(Vec::new(&e));
        to_tokens.push_back(token_id);
        e.storage().persistent().set(&DataKey::OwnerTokens(to.clone()), &to_tokens);

        // Emit transfer event
        e.events().publish((symbol_short!("transfer"), from, to), token_id);

        Ok(())
    }

    /// Check if NFT is active
    pub fn is_active(e: Env, token_id: u32) -> Result<bool, ContractError> {
        let nft: CommitmentNFT = e
            .storage()
            .persistent()
            .get(&DataKey::NFT(token_id))
            .ok_or(ContractError::TokenNotFound)?;

        Ok(nft.is_active)
    }

    /// Get total supply of NFTs minted
    pub fn total_supply(e: Env) -> u32 {
        e.storage().instance().get(&DataKey::TokenCounter).unwrap_or(0)
    }

    /// Get NFT count for a specific owner
    pub fn balance_of(e: Env, owner: Address) -> u32 {
        e.storage()
            .persistent()
            .get(&DataKey::OwnerBalance(owner))
            .unwrap_or(0)
    }

    /// Get all NFTs metadata (for frontend)
    pub fn get_all_metadata(e: Env) -> Vec<CommitmentNFT> {
        let token_ids: Vec<u32> = e
            .storage()
            .instance()
            .get(&DataKey::TokenIds)
            .unwrap_or(Vec::new(&e));

        let mut nfts: Vec<CommitmentNFT> = Vec::new(&e);

        for token_id in token_ids.iter() {
            if let Some(nft) = e.storage().persistent().get::<DataKey, CommitmentNFT>(&DataKey::NFT(token_id)) {
                nfts.push_back(nft);
            }
        }

        nfts
    }

    /// Get all NFTs owned by a specific address
    pub fn get_nfts_by_owner(e: Env, owner: Address) -> Vec<CommitmentNFT> {
        let token_ids: Vec<u32> = e
            .storage()
            .persistent()
            .get(&DataKey::OwnerTokens(owner))
            .unwrap_or(Vec::new(&e));

        let mut owned_nfts: Vec<CommitmentNFT> = Vec::new(&e);

        for token_id in token_ids.iter() {
            if let Some(nft) = e.storage().persistent().get::<DataKey, CommitmentNFT>(&DataKey::NFT(token_id)) {
                owned_nfts.push_back(nft);
            }
        }

        owned_nfts
    }

    // ========================================================================
    // NFT Transfer
    // ========================================================================

    /// Transfer NFT to new owner
    ///
    /// # Arguments
    /// * `from` - Current owner address
    /// * `to` - New owner address
    /// * `token_id` - Token ID to transfer
    ///
    /// # Errors
    /// * `TokenNotFound` - If the NFT does not exist
    /// * `NotOwner` - If the caller is not the owner
    pub fn transfer(e: Env, from: Address, to: Address, token_id: u32) -> Result<(), Error> {
        // Require authorization from the current owner
        from.require_auth();

        // Verify ownership
        let current_owner: Address = e
            .storage()
            .persistent()
            .get(&DataKey::Owner(token_id))
            .ok_or(Error::TokenNotFound)?;
        if current_owner != from {
            return Err(Error::NotOwner);
        }

        // Update owner in storage
        e.storage().persistent().set(&DataKey::Owner(token_id), &to);

        // Update NFT data to reflect new owner
        if let Some(mut nft) = e
            .storage()
            .persistent()
            .get::<DataKey, CommitmentNFT>(&DataKey::Nft(token_id))
        {
            nft.owner = to.clone();
            e.storage().persistent().set(&DataKey::Nft(token_id), &nft);
        }

        // Emit Transfer event
        e.events().publish(
            (Symbol::new(&e, "Transfer"), token_id),
            (from, to, e.ledger().timestamp()),
        );

        Ok(())
    }

    // ========================================================================
    // Settlement (Issue #5 - Main Implementation)
    // ========================================================================

    /// Mark NFT as settled (after maturity)
    pub fn settle(e: Env, token_id: u32) -> Result<(), ContractError> {
        // Get the NFT
        let mut nft: CommitmentNFT = e
            .storage()
            .persistent()
            .get(&DataKey::NFT(token_id))
            .ok_or(ContractError::TokenNotFound)?;

        // Check if already settled
        if !nft.is_active {
            return Err(ContractError::AlreadySettled);
        }

        // Verify the commitment has expired
        let current_time = e.ledger().timestamp();
        if current_time < nft.metadata.expires_at {
            return Err(ContractError::NotExpired);
        }

        // Mark as inactive (settled)
        nft.is_active = false;
        e.storage().persistent().set(&DataKey::NFT(token_id), &nft);

        // Emit settle event
        e.events().publish((symbol_short!("settle"),), token_id);

        Ok(())
    }

    /// Check if an NFT has expired (based on time)
    pub fn is_expired(e: Env, token_id: u32) -> Result<bool, ContractError> {
        let nft: CommitmentNFT = e
            .storage()
            .persistent()
            .get(&DataKey::NFT(token_id))
            .ok_or(ContractError::TokenNotFound)?;

        let current_time = e.ledger().timestamp();
        Ok(current_time >= nft.metadata.expires_at)
    }

    /// Check if a token exists
    pub fn token_exists(e: Env, token_id: u32) -> bool {
        e.storage().persistent().has(&DataKey::NFT(token_id))
    }

    /// Get the admin address
    pub fn get_admin(e: Env) -> Result<Address, ContractError> {
        e.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }
}

mod tests;

