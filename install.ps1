[CmdletBinding()]
param(
    [switch]$AddToPath,
    [switch]$NoPathPrompt
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectDirectory = $PSScriptRoot
$installDirectory = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\picopilot\bin'
$sourceExecutable = Join-Path $projectDirectory 'target\release\picopilot.exe'
$installedExecutable = Join-Path $installDirectory 'picopilot.exe'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Cargo was not found. Install Rust from https://rustup.rs/ and reopen this terminal.'
}

Write-Host 'Building picopilot in release mode...'
Push-Location $projectDirectory
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) {
        throw "Release build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

New-Item -ItemType Directory -Path $installDirectory -Force | Out-Null
Copy-Item -LiteralPath $sourceExecutable -Destination $installedExecutable -Force
Write-Host "Installed picopilot to $installedExecutable"

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$pathEntries = @($userPath -split ';' | Where-Object { $_ })
$alreadyOnPath = $pathEntries | Where-Object {
    [string]::Equals($_.TrimEnd('\'), $installDirectory.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
}

if (-not $alreadyOnPath) {
    $shouldAddToPath = $AddToPath
    if (-not $shouldAddToPath -and -not $NoPathPrompt) {
        $answer = Read-Host "Add $installDirectory to your user PATH? [Y/n]"
        $shouldAddToPath = [string]::IsNullOrWhiteSpace($answer) -or $answer -match '^[Yy]'
    }

    if ($shouldAddToPath) {
        $newUserPath = if ([string]::IsNullOrWhiteSpace($userPath)) {
            $installDirectory
        }
        else {
            "$userPath;$installDirectory"
        }
        [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
        $env:Path = "$env:Path;$installDirectory"
        Write-Host 'Added the install directory to your user PATH.'
        Write-Host 'Open a new terminal to use picopilot everywhere.'
    }
    else {
        Write-Host "PATH was not changed. Run picopilot from $installedExecutable"
    }
}
else {
    Write-Host 'The install directory is already on your user PATH.'
}