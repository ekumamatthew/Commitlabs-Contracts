# Plan: Issue #102 — Add tests for NFT mint with invalid commitment_type

## Context

The `commitment_nft` contract (`contracts/commitment_nft/src/lib.rs`) has a `mint` function that validates `commitment_type` via `is_valid_commitment_type()` (line 185-190). Only `"safe"`, `"balanced"`, and `"aggressive"` are accepted (case-sensitive exact match). Invalid values return `ContractError::InvalidCommitmentType` (error code `#12`).

Existing tests in `contracts/commitment_nft/src/tests.rs` cover basic minting but do **not** test invalid commitment_type values.

## Implementation Steps

### Step 1: Add invalid commitment_type test cases to `tests.rs`

Add the following test functions to `contracts/commitment_nft/src/tests.rs` under a new section `// Commitment Type Validation Tests`:

1. **`test_mint_empty_commitment_type`** — Mint with `commitment_type = ""` → expects `Error(Contract, #12)` (InvalidCommitmentType)
2. **`test_mint_invalid_commitment_type`** — Mint with `commitment_type = "invalid"` → expects `Error(Contract, #12)`
3. **`test_mint_wrong_case_commitment_type`** — Mint with `commitment_type = "Safe"` (capital S) → expects `Error(Contract, #12)` (confirms case-sensitivity)
4. **`test_mint_safe_commitment_type`** — Mint with `commitment_type = "safe"` → succeeds
5. **`test_mint_balanced_commitment_type`** — Mint with `commitment_type = "balanced"` → succeeds
6. **`test_mint_aggressive_commitment_type`** — Mint with `commitment_type = "aggressive"` → succeeds

All tests follow the existing pattern:
- Use `setup_contract()` + `client.initialize(&admin)`
- Invalid types use `#[should_panic(expected = "Error(Contract, #12)")]`
- Valid types assert `token_id` and `total_supply` after mint

### Step 2: Run tests to confirm they pass

Run `cargo test -p commitment_nft` to verify all new tests pass.

### Step 3: Commit and push

Commit changes to `tests.rs` on branch `claude/test-nft-mint-commitment-nTayZ` and push.

## Files Modified

- `contracts/commitment_nft/src/tests.rs` — Add 6 new test functions

## No Contract Changes Needed

The contract already correctly validates commitment_type and returns `InvalidCommitmentType` (error #12). No contract modifications required.
