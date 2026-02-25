Write-Host "Testing Rust compilation..." -ForegroundColor Cyan

# Test commitment_core compilation
Write-Host "`nChecking commitment_core..." -ForegroundColor Yellow
cargo check --package commitment_core 2>&1 | Out-Null

if ($LASTEXITCODE -eq 0) {
    Write-Host "[OK] commitment_core compiles successfully!" -ForegroundColor Green
} else {
    Write-Host "[FAIL] commitment_core has errors" -ForegroundColor Red
    cargo check --package commitment_core
    exit 1
}

# Test commitment_marketplace compilation
Write-Host "`nChecking commitment_marketplace..." -ForegroundColor Yellow
cargo check --package commitment-marketplace 2>&1 | Out-Null

if ($LASTEXITCODE -eq 0) {
    Write-Host "[OK] commitment_marketplace compiles successfully!" -ForegroundColor Green
} else {
    Write-Host "[FAIL] commitment_marketplace has errors" -ForegroundColor Red
    cargo check --package commitment-marketplace
    exit 1
}

Write-Host "`n[SUCCESS] All checks passed! Ready to push." -ForegroundColor Green
