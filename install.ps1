# Builds Project Transfer and installs it for this Windows user, offering to
# install Rust first if it's missing. Any arguments go to the installer, for
# example: .\install.ps1 --dest C:\Apps\Test

# Where rustup puts cargo. A terminal opened before Rust was installed doesn't
# have it on its PATH yet. It stays on this session's PATH afterwards, so cargo
# works here straight after an install.
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$cargoBin = Join-Path $cargoHome 'bin'
if (($env:Path -split ';') -notcontains $cargoBin) {
    $env:Path += ";$cargoBin"
}

# Whether cargo runs, not just whether it's there: rustup puts its cargo in
# place before it downloads Rust, so an interrupted install leaves one that
# can't run anything.
function Test-Rust {
    try { $null = cargo --version 2>&1; $LASTEXITCODE -eq 0 } catch { $false }
}

if (-not (Test-Rust)) {
    Write-Host "Rust isn't installed, and Project Transfer needs it to build."
    $answer = Read-Host "Install Rust now from https://rustup.rs? [Y/n]"
    # No answer at all, as when nobody is there to type one, counts as no.
    if ($null -eq $answer -or $answer -notmatch '^\s*(y|$)') {
        Write-Host "To install Rust yourself, download and run rustup-init.exe from https://rustup.rs, then run .\install.ps1 again." -ForegroundColor Red
        exit 1
    }
    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
    $rustupInit = Join-Path $env:TEMP 'rustup-init.exe'
    try {
        # Windows PowerShell can leave TLS 1.2 off, and its progress bar slows
        # downloads right down.
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        $ProgressPreference = 'SilentlyContinue'
        Invoke-WebRequest "https://static.rust-lang.org/rustup/dist/$arch-pc-windows-msvc/rustup-init.exe" -OutFile $rustupInit -UseBasicParsing
        # No -y: -y would also skip rustup's offer to install the Visual Studio
        # C++ build tools the build needs, so rustup asks its own questions too.
        & $rustupInit
    }
    catch {
        Write-Host $_ -ForegroundColor Red
    }
    finally {
        Remove-Item -LiteralPath $rustupInit -ErrorAction SilentlyContinue
    }
    if (-not (Test-Rust)) {
        Write-Host "Rust didn't install. Run .\install.ps1 to try again, or install Rust yourself from https://rustup.rs." -ForegroundColor Red
        exit 1
    }
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
