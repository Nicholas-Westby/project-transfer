# Builds Project Transfer and installs it for this Windows user. Any arguments
# go to the installer, for example: .\install.ps1 --dest C:\Apps\Test

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Rust isn't installed. Install it from https://rustup.rs, then run .\install.ps1 again." -ForegroundColor Red
    exit 1
}

# The current folder belongs to the whole PowerShell session, not just this
# script, so put the caller's back afterwards.
Push-Location -LiteralPath $PSScriptRoot
try {
    cargo xtask install @args
    $code = $LASTEXITCODE
}
finally {
    Pop-Location
}
exit $code
