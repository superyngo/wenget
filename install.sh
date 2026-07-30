#!/usr/bin/env bash
# wenget Remote Installation Script for Linux/macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/superyngo/wenget/main/install.sh | bash

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

print_success() { echo -e "${GREEN}$1${NC}"; }
print_info() { echo -e "${CYAN}$1${NC}"; }
print_error() { echo -e "${RED}$1${NC}"; }
print_warning() { echo -e "${YELLOW}$1${NC}"; }

# Configuration
APP_NAME="wenget"
REPO="superyngo/wenget"

# Detect if running as root for system-level installation
detect_install_mode() {
    if [ "$(id -u)" -eq 0 ]; then
        SYSTEM_INSTALL=true
        INSTALL_DIR="/opt/wenget/apps/wenget"
        BIN_DIR="/usr/local/bin"
        print_info "Running as root - using system-level installation"
        print_info "  App directory: /opt/wenget/apps"
        print_info "  Bin directory: /usr/local/bin"
    else
        SYSTEM_INSTALL=false
        INSTALL_DIR="$HOME/.wenget/apps/wenget"
        BIN_DIR="$HOME/.local/bin"
        print_info "Running as user - using user-level installation"
        print_info "  App directory: $HOME/.wenget/apps"
        print_info "  Bin directory: $HOME/.local/bin"
    fi
    BIN_PATH="$INSTALL_DIR/$APP_NAME"
    echo ""
}

# Detect OS and Architecture
detect_platform() {
    local os=$(uname -s | tr '[:upper:]' '[:lower:]')
    local arch=$(uname -m)

    case "$os" in
        linux*)
            OS="linux"
            ;;
        darwin*)
            OS="macos"
            ;;
        *)
            print_error "Unsupported OS: $os"
            exit 1
            ;;
    esac

    case "$arch" in
        x86_64|amd64)
            ARCH="x86_64"
            ;;
        aarch64|arm64)
            ARCH="aarch64"
            ;;
        armv7l)
            ARCH="armv7"
            ;;
        i686|i386)
            ARCH="i686"
            ;;
        *)
            print_error "Unsupported architecture: $arch"
            exit 1
            ;;
    esac

    print_info "Detected platform: $OS-$ARCH"
}

# Get latest release from GitHub API
get_latest_release() {
    local api_url="https://api.github.com/repos/$REPO/releases/latest"

    print_info "Fetching latest release information..."

    if command -v curl >/dev/null 2>&1; then
        RELEASE_DATA=$(curl -fsSL "$api_url")
    elif command -v wget >/dev/null 2>&1; then
        RELEASE_DATA=$(wget -qO- "$api_url")
    else
        print_error "Error: Neither curl nor wget is available. Please install one of them."
        exit 1
    fi

    # Extract version tag
    VERSION=$(echo "$RELEASE_DATA" | grep '"tag_name":' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')

    if [ -z "$VERSION" ]; then
        print_error "Failed to get latest version"
        exit 1
    fi

    print_success "Latest version: $VERSION"
}

# Download and extract release
download_release() {
    local asset_name=""
    local download_url=""

    # For Linux, try musl version first, then fall back to gnu
    if [ "$OS" = "linux" ]; then
        local musl_asset="$APP_NAME-$OS-$ARCH-musl.tar.gz"
        local gnu_asset="$APP_NAME-$OS-$ARCH.tar.gz"

        # Try musl version first
        download_url=$(echo "$RELEASE_DATA" | grep "\"browser_download_url\".*$musl_asset" | sed -E 's/.*"browser_download_url": *"([^"]+)".*/\1/')

        if [ -n "$download_url" ]; then
            asset_name="$musl_asset"
            print_info "Using musl version: $asset_name"
        else
            # Fall back to gnu version
            print_info "musl version not found, trying gnu version..."
            download_url=$(echo "$RELEASE_DATA" | grep "\"browser_download_url\".*$gnu_asset" | sed -E 's/.*"browser_download_url": *"([^"]+)".*/\1/')
            asset_name="$gnu_asset"
        fi
    else
        # For macOS, use the standard naming
        asset_name="$APP_NAME-$OS-$ARCH.tar.gz"
        download_url=$(echo "$RELEASE_DATA" | grep "\"browser_download_url\".*$asset_name" | sed -E 's/.*"browser_download_url": *"([^"]+)".*/\1/')
    fi

    if [ -z "$download_url" ]; then
        print_error "Could not find release asset: $asset_name"
        print_info "Available assets:"
        echo "$RELEASE_DATA" | grep '"name":' | sed -E 's/.*"name": *"([^"]+)".*/  - \1/'
        echo ""
        print_info "Looking for: $asset_name"
        exit 1
    fi

    print_info "Download URL: $download_url"
    print_info ""

    # Create temporary directory
    local temp_dir=$(mktemp -d)
    trap "rm -rf '$temp_dir'" EXIT

    # Download archive
    local archive_path="$temp_dir/$asset_name"
    print_info "Downloading $APP_NAME..."

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$archive_path" "$download_url"
    else
        wget -qO "$archive_path" "$download_url"
    fi

    print_success "Downloaded successfully!"

    # Extract archive
    print_info "Extracting archive..."
    tar -xzf "$archive_path" -C "$temp_dir"

    # Find binary
    local binary=$(find "$temp_dir" -name "$APP_NAME" -type f | head -n 1)

    if [ ! -f "$binary" ]; then
        print_error "Could not find $APP_NAME binary in archive"
        exit 1
    fi

    # Create installation directory
    mkdir -p "$INSTALL_DIR"

    # Install binary
    print_info "Installing to: $INSTALL_DIR"
    cp "$binary" "$BIN_PATH"
    chmod +x "$BIN_PATH"

    print_success "Binary installed successfully!"
}

# Create symlink in bin directory
create_symlink() {
    print_info "Creating symlink in $BIN_DIR..."
    mkdir -p "$BIN_DIR"
    ln -sf "$BIN_PATH" "$BIN_DIR/$APP_NAME"
    print_success "Symlink created: $BIN_DIR/$APP_NAME -> $BIN_PATH"
}

# Run wenget init
run_init() {
    print_info ""
    print_info "Running wenget init..."
    print_info ""

    if [ -x "$BIN_PATH" ]; then
        "$BIN_PATH" init --yes || {
            print_warning "Failed to run wenget init. You can run it manually later."
            print_info "  Run: wenget init"
        }
    else
        print_warning "Binary not executable. Skipping initialization."
    fi
}

# Installation function
install_wenget() {
    print_info "=== wenget Installation Script ==="
    echo ""

    detect_install_mode
    detect_platform
    get_latest_release
    download_release
    create_symlink
    run_init

    echo ""
    print_success "Installation completed!"
    echo ""
    print_info "Installed version: $VERSION"
    print_info "Installation path: $BIN_PATH"
    echo ""
    print_warning "Note: You may need to restart your terminal for PATH changes to take effect."
}

# Uninstallation function
uninstall_wenget() {
    print_info "=== wenget Uninstallation Script ==="
    echo ""

    detect_install_mode

    # Check if wenget is available and run self-deletion
    if [ -x "$BIN_PATH" ]; then
        print_info "Running wenget self-deletion..."
        if "$BIN_PATH" del self --yes; then
            print_success "wenget uninstalled successfully!"
        else
            print_warning "wenget self-deletion failed. Performing manual cleanup..."

            # Remove symlink
            if [ -L "$BIN_DIR/$APP_NAME" ]; then
                print_info "Removing symlink..."
                rm -f "$BIN_DIR/$APP_NAME"
                print_success "Symlink removed"
            fi

            # Remove binary
            print_info "Removing binary..."
            rm -f "$BIN_PATH"
            print_success "Binary removed"

            # Remove installation directory if empty
            if [ -d "$INSTALL_DIR" ]; then
                if [ -z "$(ls -A "$INSTALL_DIR")" ]; then
                    rmdir "$INSTALL_DIR"
                    print_success "Installation directory removed"
                fi
            fi
        fi
    else
        print_info "Binary not found (already removed?)"

        # Still try to clean up symlink
        if [ -L "$BIN_DIR/$APP_NAME" ]; then
            print_info "Removing orphaned symlink..."
            rm -f "$BIN_DIR/$APP_NAME"
            print_success "Symlink removed"
        fi
    fi

    echo ""
    print_success "Uninstallation completed!"
}

# Main
ACTION="${1:-install}"

case "$ACTION" in
    install)
        install_wenget
        ;;
    uninstall)
        uninstall_wenget
        ;;
    *)
        print_error "Unknown action: $ACTION"
        print_info "Usage: $0 [install|uninstall]"
        exit 1
        ;;
esac
