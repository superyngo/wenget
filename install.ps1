#!/usr/bin/env pwsh
# wenget Remote Installation Script for Windows
# Usage: irm https://raw.githubusercontent.com/superyngo/wenget/main/install.ps1 | iex

param(
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

# Colors
function Write-Success { Write-Host $args -ForegroundColor Green }
function Write-Info { Write-Host $args -ForegroundColor Cyan }
function Write-Error { Write-Host $args -ForegroundColor Red }
function Write-Warning { Write-Host $args -ForegroundColor Yellow }

# Configuration
$APP_NAME = "wenget"
$REPO = "superyngo/wenget"

# Detect if running as Administrator for system-level installation
function Get-InstallMode {
    $isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

    if ($isAdmin) {
        $script:SYSTEM_INSTALL = $true
        $programFiles = $env:ProgramW6432
        if (-not $programFiles) {
            $programFiles = $env:ProgramFiles
        }
        $script:WENGET_HOME = "$programFiles\wenget"
        $script:INSTALL_DIR = "$programFiles\wenget\apps\wenget"
        $script:BIN_DIR = "$programFiles\wenget\bin"
        $script:BIN_PATH = "$script:INSTALL_DIR\$APP_NAME.exe"
        Write-Info "Running as Administrator - using system-level installation"
        Write-Info "  App directory: $programFiles\wenget\apps"
        Write-Info "  Bin directory: $programFiles\wenget\bin"
    } else {
        $script:SYSTEM_INSTALL = $false
        $script:WENGET_HOME = "$env:USERPROFILE\.wenget"
        $script:INSTALL_DIR = "$env:USERPROFILE\.wenget\apps\wenget"
        $script:BIN_DIR = "$env:USERPROFILE\.local\bin"
        $script:BIN_PATH = "$script:INSTALL_DIR\$APP_NAME.exe"
        Write-Info "Running as user - using user-level installation"
        Write-Info "  App directory: $env:USERPROFILE\.wenget\apps"
        Write-Info "  Bin directory: $env:USERPROFILE\.local\bin"
    }
    Write-Info ""
}

function Get-LatestRelease {
    try {
        $apiUrl = "https://api.github.com/repos/$REPO/releases/latest"
        Write-Info "Fetching latest release information..."

        $release = Invoke-RestMethod -Uri $apiUrl -Headers @{
            "User-Agent" = "wenget-installer"
        }

        return $release
    } catch {
        Write-Error "Failed to fetch release information: $_"
        exit 1
    }
}

function Get-Architecture {
    $arch = $env:PROCESSOR_ARCHITECTURE
    switch ($arch) {
        "AMD64" { return "x86_64" }
        "x86" { return "i686" }
        "ARM64" { return "aarch64" }
        default {
            Write-Warning "Unknown architecture: $arch, defaulting to x86_64"
            return "x86_64"
        }
    }
}

function New-Shim {
    Write-Info "Creating shim in $BIN_DIR..."

    # Create bin directory
    if (-not (Test-Path $BIN_DIR)) {
        New-Item -ItemType Directory -Path $BIN_DIR -Force | Out-Null
    }

    # Create shim (.cmd file that calls the exe)
    $shimPath = "$BIN_DIR\$APP_NAME.cmd"
    $shimContent = "@echo off`r`n`"$BIN_PATH`" %*`r`n"
    Set-Content -Path $shimPath -Value $shimContent -Encoding ASCII

    Write-Success "Shim created: $shimPath -> $BIN_PATH"
}

function Install-wenget {
    Write-Info "=== wenget Installation Script ==="
    Write-Info ""

    # Detect install mode first
    Get-InstallMode

    # Get latest release
    $release = Get-LatestRelease
    $version = $release.tag_name
    Write-Success "Latest version: $version"

    # Determine architecture
    $arch = Get-Architecture
    Write-Info "Detected architecture: $arch"

    # Find download URL for Windows
    $assetName = "$APP_NAME-windows-$arch.exe"
    $asset = $release.assets | Where-Object { $_.name -eq $assetName }

    if (-not $asset) {
        Write-Error "Could not find Windows release asset"
        Write-Info "Available assets:"
        $release.assets | ForEach-Object { Write-Info "  - $($_.name)" }
        Write-Info ""
        Write-Info "Looking for: $assetName"
        exit 1
    }

    $downloadUrl = $asset.browser_download_url
    Write-Info "Download URL: $downloadUrl"
    Write-Info ""

    # Create installation directory
    if (-not (Test-Path $INSTALL_DIR)) {
        New-Item -ItemType Directory -Path $INSTALL_DIR -Force | Out-Null
    }

    # Download binary directly
    Write-Info "Downloading $APP_NAME..."

    $ProgressPreference = 'SilentlyContinue'
    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $BIN_PATH -UseBasicParsing
        $ProgressPreference = 'Continue'
        Write-Success "Downloaded successfully!"
    } catch {
        $ProgressPreference = 'Continue'
        Write-Error "Download failed: $_"
        exit 1
    }

    Write-Info "Installed to: $INSTALL_DIR"
    Write-Success "Binary installed successfully!"
    Write-Info ""

    # Create shim
    New-Shim

    # Run wenget init
    Write-Info ""
    Write-Info "Running wenget init..."
    Write-Info ""

    try {
        & $BIN_PATH init --yes
    } catch {
        Write-Warning "Failed to run wenget init. You can run it manually later."
        Write-Info "  Run: wenget init"
    }

    Write-Info ""
    Write-Success "Installation completed!"
    Write-Info ""
    Write-Info "Installed version: $version"
    Write-Info "Installation path: $BIN_PATH"
    Write-Info ""
    Write-Warning "Note: You may need to restart your terminal for PATH changes to take effect."
}

function Uninstall-wenget {
    Write-Info "=== wenget Uninstallation Script ==="
    Write-Info ""

    # Detect install mode first
    Get-InstallMode

    # Check if wenget is available and run self-deletion
    if (Test-Path $BIN_PATH) {
        Write-Info "Running wenget self-deletion..."
        try {
            & $BIN_PATH del self --yes
            Write-Success "wenget uninstalled successfully!"
        } catch {
            Write-Warning "wenget self-deletion failed. Performing manual cleanup..."

            # Remove shim
            $shimPath = "$BIN_DIR\$APP_NAME.cmd"
            if (Test-Path $shimPath) {
                Remove-Item $shimPath -Force -ErrorAction SilentlyContinue
                Write-Success "Shim removed"
            }

            # Remove binary
            Write-Info "Removing binary..."
            Remove-Item $BIN_PATH -Force -ErrorAction SilentlyContinue
            Write-Success "Binary removed"

            # Remove installation directory if empty
            if (Test-Path $INSTALL_DIR) {
                $items = Get-ChildItem $INSTALL_DIR -ErrorAction SilentlyContinue
                if ($items.Count -eq 0) {
                    Remove-Item $INSTALL_DIR -Force
                    Write-Success "Installation directory removed"
                }
            }
        }
    } else {
        Write-Info "Binary not found (already removed?)"

        # Still try to clean up shim
        $shimPath = "$BIN_DIR\$APP_NAME.cmd"
        if (Test-Path $shimPath) {
            Write-Info "Removing orphaned shim..."
            Remove-Item $shimPath -Force -ErrorAction SilentlyContinue
            Write-Success "Shim removed"
        }
    }

    Write-Info ""
    Write-Success "Uninstallation completed!"
    Write-Warning "Note: You may need to restart your terminal for PATH changes to take effect."
}

# Main
if ($Uninstall) {
    Uninstall-wenget
} else {
    Install-wenget
}
