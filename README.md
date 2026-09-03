# wenget - Wen Package Manager

[![Version](https://img.shields.io/badge/version-3.8.7-blue.svg)](https://github.com/superyngo/wenget/releases)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](./LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/superyngo/wenget)

A cross-platform package manager for GitHub binaries, written in Rust.

wenget simplifies the installation and management of command-line tools and applications distributed through GitHub Releases. It automatically detects your platform, downloads the appropriate binaries, and manages them in an organized directory structure.

## Features

- **🚀 One-line Installation**: Remote installation scripts for quick setup
- **🔄 Auto-update**: Always installs the latest version from GitHub Releases
- **📦 Bucket System**: Organize packages and scripts using bucket manifests
- **📜 Script Support**: Install and manage PowerShell, Bash, and Python scripts from buckets
- **🌐 Cross-platform**: Windows, macOS, Linux (multiple architectures)
- **📁 Organized Storage**: All packages in `~/.wenget/` with proper structure
- **🔍 Smart Search**: Search packages and scripts across all configured buckets
- **⚡ Fast Downloads**: Multi-threaded downloads with caching
- **🎯 Platform Detection**: Automatically selects the correct binary for your system
- **🔧 Smart Command Naming**: Automatically removes platform suffixes from executable names

## Quick Install

### Method 1: Winget (Windows, Recommended)

```powershell
winget install wenget
```

After installation, run initialization:
```powershell
wenget init
```

### Method 2: Installation Script

#### Windows (PowerShell)
```powershell
irm https://raw.githubusercontent.com/superyngo/wenget/main/install.ps1 | iex
```

#### Linux/macOS (Bash)
```bash
curl -fsSL https://raw.githubusercontent.com/superyngo/wenget/main/install.sh | bash
```

### System-Level Installation

The install scripts automatically detect elevated privileges and switch to system-level paths:

| Mode | Platform | App Directory | Bin Directory |
|------|----------|---------------|---------------|
| User | Linux/macOS | `~/.wenget/apps` | `~/.local/bin` |
| User | Windows | `%USERPROFILE%\.wenget\apps` | `%USERPROFILE%\.local\bin` |
| Root/Admin | Linux/macOS | `/opt/wenget/app` | `/usr/local/bin` (symlinks) |
| Root/Admin | Windows | `%ProgramW6432%\wenget\app` | `%ProgramW6432%\wenget\bin` |

**When to use Administrator/Root:**
- When you want packages available to all users on the system
- When installing to system directories like `/usr/local/bin` or `%ProgramW6432%`

**Linux/macOS (as root):**
```bash
sudo curl -fsSL https://raw.githubusercontent.com/superyngo/wenget/main/install.sh | sudo bash
```

**Windows (as Administrator):**
```powershell
# Run PowerShell as Administrator, then:
irm https://raw.githubusercontent.com/superyngo/wenget/main/install.ps1 | iex
```

> **⚠️ Important Notice for Existing Users (v0.2.x → v0.3.0)**
> 
> Version 0.3.0 changes the default bin directory for user-level installations:
> - **Unix**: `~/.wenget/bin/` → `~/.local/bin/`
> - **Windows**: `%USERPROFILE%\.wenget\bin\` → `%USERPROFILE%\.local\bin\`
> 
> **Migration Required:**
> 1. Uninstall existing version: `wenget del self --yes` (or use `install.sh uninstall` / `install.ps1 -Uninstall`)
> 2. Remove old PATH entry for `~/.wenget/bin` from your shell config
> 3. Reinstall with the script above
> 4. Reinstall your packages
> 
> System-level installations (root/Administrator) are **not affected** by this change.

### Method 3: Manual Installation

Download the latest release from [GitHub Releases](https://github.com/superyngo/wenget/releases) and place it in your PATH, or build from source:

```bash
git clone https://github.com/superyngo/wenget.git
cd wenget
cargo build --release
```

The binary will be at `target/release/wenget` (or `wenget.exe` on Windows).

## Quick Start

```bash
# Initialize wenget (done automatically with install scripts)
wenget init

# Add the official wenget bucket (if not added during init)
wenget bucket add wenget https://raw.githubusercontent.com/superyngo/wenget/refs/heads/main/bucket/manifest.json

# Search for packages
wenget search ripgrep

# Install a package
wenget add ripgrep

# List installed packages
wenget list

# Update installed packages
wenget update

# Upgrade wenget itself
wenget update self

# Delete a package
wenget delete ripgrep
```

## Commands

### Package Management

- `wenget add <name|url>...` - Install packages (from bucket or GitHub URL)
  - `--variant <name>` - Install a specific variant (e.g., `--variant baseline`)
  - `--no-suffix` - Don't append variant suffix to command name
- `wenget info <name|url>` - Show package information
- `wenget delete <name>...` - Uninstall packages
  - `wenget del self` - Uninstall wenget itself
- `wenget list` - List installed packages (with source and description)
  - `wenget list --all` - Show all available packages from buckets
- `wenget search <keyword>` - Search available packages
- `wenget update [name]` - Update installed packages
  - `wenget update self` - Upgrade wenget itself to the latest version
  - `wenget update [name] -p <target>` - Update for a specific platform (overrides `preferred_platform`)

### Bucket Management

- `wenget bucket add <name> <url>` - Add a bucket
- `wenget bucket del <name>` - Remove a bucket
- `wenget bucket list` - List all buckets
- `wenget bucket refresh` - Rebuild package cache
- `wenget bucket create` - Generate a bucket manifest from source files

### Bucket Manifest Generator

The `bucket create` command generates bucket manifests from GitHub repositories and scripts:

```bash
# Generate manifest from source files
wenget bucket create -r sources_repos.txt -s sources_scripts.txt -o manifest.json

# Use with GitHub token for higher API rate limit (5000/hour vs 60/hour)
wenget bucket create -r repos.txt -o manifest.json -t YOUR_TOKEN

# Or use GITHUB_TOKEN environment variable
export GITHUB_TOKEN=your_token
wenget bucket create -r repos.txt -o manifest.json

# Update modes (when manifest.json already exists)
wenget bucket create -r repos.txt -o manifest.json -u overwrite     # Replace entire file
wenget bucket create -r repos.txt -o manifest.json -u incremental   # Merge with existing

# Add direct URLs
wenget bucket create -d https://github.com/user/repo,https://gist.github.com/user/id
```

**Options:**
- `-r, --repos-src` - Source file(s) with GitHub repo URLs (one per line)
- `-s, --scripts-src` - Source file(s) with Gist/script URLs (one per line)
- `-d, --direct` - Direct URLs (comma-separated)
- `-o, --output` - Output file (default: manifest.json)
- `-t, --token` - GitHub personal access token
- `-u, --update-mode` - How to handle existing file: `overwrite` or `incremental`

### System

- `wenget init` - Initialize wenget directories and configuration
- `wenget config` - Edit user preferences (config.toml) with default editor
- `wenget rename <old> [new]` - Rename an installed command
- `wenget repair` - Repair corrupted configuration files
- `wenget --version` - Show version information
- `wenget --help` - Show help message

### Global Options

- `--yes`, `-y` - Skip confirmation prompts
- `--verbose`, `-v` - Enable verbose logging

## Directory Structure

### User-Level Installation (default)
```
~/.wenget/
├── apps/                  # Installed applications
│   ├── wenget/            # wenget itself
│   └── <package>/        # Each installed package
│       └── .wenget/
│           └── package.json  # This package's record (version, source, commands)
├── bin/                   # Symlinks/shims (added to PATH)
│   ├── wenget.cmd         # wenget shim (Windows)
│   ├── wenget             # wenget symlink (Unix)
│   └── <package>.cmd     # Package shims
├── cache/                 # Download and package cache
│   ├── manifest-cache.json  # Cached package list
│   └── downloads/        # Downloaded archives
├── config.toml           # User preferences (platform, paths, etc.)
└── buckets.json          # Bucket configuration
```

### System-Level Installation (root/Administrator)

**Linux/macOS:**
```
/opt/wenget/
├── app/                   # Installed applications
│   ├── wenget/
│   └── <package>/
├── cache/
└── buckets.json

/usr/local/bin/            # Symlinks to binaries
├── wenget -> /opt/wenget/app/wenget/wenget
└── <package> -> ...
```

**Windows:**
```
%ProgramW6432%\wenget\
├── app\                   # Installed applications
│   ├── wenget\
│   └── <package>\
├── bin\                   # Binaries (added to system PATH)
│   ├── wenget.exe
│   └── <package>.exe
├── cache\
└── buckets.json
```

## Configuration

wenget supports user preferences via `~/.wenget/config.toml`. Edit with:

```bash
wenget config
```

### Available Settings

**Preferred Platform** - Override automatic platform detection:
```toml
preferred_platform = "x86_64-unknown-linux-musl"
```

Common platform identifiers:
- Linux x86_64 (glibc): `x86_64-unknown-linux-gnu`
- Linux x86_64 (musl): `x86_64-unknown-linux-musl`
- Linux ARM64: `aarch64-unknown-linux-gnu`
- macOS Intel: `x86_64-apple-darwin`
- macOS Apple Silicon: `aarch64-apple-darwin`
- Windows x86_64: `x86_64-pc-windows-msvc`
- Windows ARM64: `aarch64-pc-windows-msvc`

`preferred_platform` is applied by both `wenget add` and `wenget update`. It also
accepts internal identifiers (e.g. `linux-aarch64-musl`). When a libc/compiler
variant such as `musl` is requested, it is preferred when available and falls
back to a compatible build otherwise. A `-p/--platform` flag overrides it.

**Custom Bin Directory** - Override default bin location:
```toml
custom_bin_path = "/usr/local/bin"
```

Useful for custom PATH setups or when `~/.local/bin` cannot be added to PATH.

## Bucket System

Buckets are collections of package and script manifests hosted online. The official wenget bucket provides curated open-source tools.

### Official Bucket

```bash
wenget bucket add wenget https://raw.githubusercontent.com/superyngo/wenget/refs/heads/main/bucket/manifest.json
```

### Creating Your Own Bucket

You can create custom buckets to distribute your own package and script collections. See the [official wenget bucket](https://github.com/superyngo/wenget/tree/main/bucket) for a complete example.

#### Bucket Structure

Create a `manifest.json` with the following structure:

```json
{
  "packages": [
    {
      "name": "my-tool",
      "repo": "https://github.com/username/repo",
      "description": "Tool description",
      "homepage": "https://example.com",
      "license": "MIT",
      "platforms": {
        "windows-x86_64": {
          "url": "https://github.com/username/repo/releases/download/v1.0.0/tool-windows-x64.zip",
          "size": 1234567
        },
        "linux-x86_64": {
          "url": "https://github.com/username/repo/releases/download/v1.0.0/tool-linux-x64.tar.gz",
          "size": 1234567
        }
      }
    }
  ],
  "scripts": [
    {
      "name": "my-script",
      "description": "Useful script",
      "url": "https://raw.githubusercontent.com/username/repo/main/script.ps1",
      "script_type": "powershell",
      "repo": "https://github.com/username/repo",
      "homepage": "https://example.com",
      "license": "MIT"
    }
  ]
}
```

#### Required Fields

**For Packages:**
- `name`: Package name (used in commands)
- `repo`: GitHub repository URL
- `description`: Brief package description
- `platforms`: Platform-specific binary information
  - `url`: Download URL for the binary
  - `size`: File size in bytes

**For Scripts:**
- `name`: Script name (used in commands)
- `description`: Brief script description
- `url`: Direct URL to the script file
- `script_type`: Script type (`powershell`, `bash`, `batch`, or `python`)
- `repo`: Repository URL (for reference)

#### Optional Fields

- `homepage`: Project homepage URL
- `license`: Package/script license
- `checksum`: SHA256 checksum for verification

#### Hosting Your Bucket

**GitHub (Recommended)**:
```bash
# Create a new repository
# Add manifest.json to the repository
# Use raw.githubusercontent.com URL
wenget bucket add my-bucket https://raw.githubusercontent.com/username/my-bucket/main/manifest.json
```

**Other Hosting**:
- Any web server that serves JSON with HTTPS
- GitHub Gists
- CDN services

#### Example: Official wenget Bucket

The official bucket is maintained at: https://github.com/superyngo/wenget/tree/main/bucket

It includes curated packages with:
- Verified working binaries across platforms
- Updated package metadata
- Categorized by tool type
- Regular updates and maintenance

You can use it as a template for creating your own bucket:

```bash
# Clone the wenget repo and use its bucket directory as a template
git clone https://github.com/superyngo/wenget my-bucket-template
cd my-bucket-template/bucket
# Copy manifest.json into your own repository and edit it with your packages
```

#### Testing Your Bucket

```bash
# Add your bucket locally
wenget bucket add test-bucket https://example.com/manifest.json

# Verify packages are listed
wenget search <package-name>

# Test installation
wenget add <package-name>
```

## Platform Support

wenget supports the following platforms:

| Platform | Architecture | Status |
|----------|--------------|--------|
| Windows | x86_64 (64-bit) | ✅ Supported |
| Windows | i686 (32-bit) | ✅ Supported |
| Linux | x86_64 | ✅ Supported |
| Linux | i686 | ✅ Supported |
| Linux | aarch64 (ARM64) | ✅ Supported |
| Linux | armv7 | ✅ Supported |
| macOS | x86_64 (Intel) | ✅ Supported |
| macOS | aarch64 (Apple Silicon) | ✅ Supported |

## How It Works

1. **Platform Detection**: wenget automatically detects your OS and architecture
2. **Package Resolution**: Searches buckets for the requested package
3. **Binary Selection**: Identifies the appropriate binary from GitHub Releases
4. **Download**: Downloads and caches the binary
5. **Installation**: Extracts and places the binary in `~/.wenget/apps/<package>/`
6. **Shim Creation**: Creates a shim/symlink in `~/.local/bin/` for easy access

## GitHub API Rate Limits

wenget uses the GitHub API to fetch package information and download binaries. Be aware of GitHub's API rate limits:

### Rate Limit Overview

| Authentication | Rate Limit | Impact |
|---------------|------------|--------|
| Unauthenticated | 60 requests/hour | Limited package searches and updates |
| Authenticated | 5,000 requests/hour | Sufficient for normal usage |

### Impact on wenget Operations

**Operations that consume API calls:**
- `wenget add <url>` - 2 calls per URL (when installing from GitHub URL)
- `wenget info <url>` - 1 call per URL (when querying GitHub URL)
- `wenget update` - 1 call per installed package to check for updates

**Operations that don't consume API calls:**
- `wenget add <name>` - Uses cached bucket data (no API calls)
- `wenget info <name>` - Uses cached bucket data for bucket packages
- `wenget list` - Local only
- `wenget delete` - Local only
- `wenget bucket list/add/remove` - Local only
- `wenget search` - Uses cached bucket data

### Recommendations

1. **Use Buckets**: The bucket system caches package information, reducing API calls significantly
2. **Run `wenget update` periodically** rather than before each search
3. **For heavy usage**: Consider authenticating with GitHub (future feature)
4. **Rate limit exceeded?** Wait an hour or use buckets for cached package data

The official wenget bucket is updated regularly, so most users won't need to worry about rate limits when using bucket-based package management.

## Examples

### Install Popular Tools

```bash
# Modern alternatives to classic Unix tools
wenget add ripgrep fd bat

# Git TUI
wenget add gitui lazygit

# System monitoring
wenget add bottom

# Shell prompt
wenget add starship

# Directory navigation
wenget add zoxide
```

### Manage Packages

```bash
# Search for a tool
wenget search rust

# Update metadata and install
wenget update
wenget add tokei

# List what's installed
wenget list

# Remove a package
wenget delete tokei
```

## Important Disclaimer

**⚠️ NO WARRANTIES OR GUARANTEES**

wenget is a package manager that facilitates downloading and installing applications from GitHub Releases. **wenget DOES NOT:**

- ❌ Verify the authenticity or safety of packages
- ❌ Maintain or update the applications themselves
- ❌ Provide usage information or support for installed applications
- ❌ Guarantee the security, stability, or functionality of any package
- ❌ Take responsibility for any damage caused by installed applications

**Users are responsible for:**
- ✅ Verifying the trustworthiness of package sources
- ✅ Understanding what each package does before installing
- ✅ Reviewing the source repositories and releases
- ✅ Accepting all risks associated with installing third-party software

**By using wenget, you acknowledge that you install packages at your own risk.**

wenget acts only as a convenience tool for downloading and organizing binaries. The responsibility for verifying, securing, and using applications rests entirely with the user.

## Uninstallation

### Using wenget
```bash
wenget del self
```

This will:
1. Remove wenget from PATH
2. Delete all wenget directories and installed packages
3. Remove the wenget executable itself

### Manual Uninstallation

**Windows:**
```powershell
# Remove from PATH, then delete:
Remove-Item -Recurse -Force "$env:USERPROFILE\.wenget"
```

**Linux/macOS:**
```bash
# Remove from PATH, then delete:
rm -rf ~/.wenget
```

## Development

### Building from Source

```bash
git clone https://github.com/superyngo/wenget.git
cd wenget
cargo build --release
```

### Running Tests

```bash
cargo test
```

### Project Structure

```
wenget/
├── src/
│   ├── bucket.rs         # Bucket management
│   ├── cache.rs          # Package cache
│   ├── cli.rs            # CLI interface
│   ├── commands/         # Command implementations
│   ├── core/             # Core functionality
│   ├── downloader/       # Download logic
│   ├── installer/        # Installation logic
│   ├── providers/        # GitHub API integration
│   └── utils/            # Utilities (HTTP client, prompts)
├── install.ps1           # Windows installer
└── install.sh            # Unix installer
```

## Troubleshooting

### PATH Not Updated

After installation, you may need to restart your terminal or run:

**Windows:**
```powershell
refreshenv
```

**Linux/macOS:**
```bash
source ~/.bashrc  # or ~/.zshrc, ~/.profile
```

### Package Not Found

```bash
# Update package metadata
wenget update

# Check available buckets
wenget bucket list

# Rebuild cache
wenget bucket refresh
```

### Permission Errors (Linux/macOS)

```bash
# Ensure ~/.local/bin is in PATH and has correct permissions
chmod +x ~/.local/bin/*
```

## Contributing

Contributions are welcome! Please feel free to submit issues, feature requests, or pull requests.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

MIT License - Copyright (c) 2025 wen

See [LICENSE](./LICENSE) for details.

## Credits

Inspired by:
- [Scoop](https://scoop.sh/) - Windows package manager
- [Homebrew](https://brew.sh/) - macOS package manager
- [Obtainium](https://github.com/ImranR98/Obtainium) - Android app manager

## Links

- **GitHub**: https://github.com/superyngo/wenget
- **Releases**: https://github.com/superyngo/wenget/releases
- **Issues**: https://github.com/superyngo/wenget/issues
- **Official Bucket**: https://github.com/superyngo/wenget/tree/main/bucket

## Changelog

See [CHANGELOG.md](./CHANGELOG.md) for detailed release notes and version history.
