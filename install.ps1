Write-Host "dasiiProxy v3 - Rust installer" -ForegroundColor Cyan

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Rust/Cargo is required. Install it from https://rustup.rs and run this script again." -ForegroundColor Red
    exit 1
}

cargo install --path . --force
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installed dasiproxy. Run: dasiproxy tui" -ForegroundColor Green
