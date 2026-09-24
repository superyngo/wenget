# wenget - Wen Package Manager

[![Version](https://img.shields.io/badge/version-3.9.0-blue.svg)](https://github.com/superyngo/wenget/releases)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](./LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/superyngo/wenget)

A cross-platform package manager for GitHub binaries, written in Rust.

wenget simplifies the installation and management of command-line tools and applications distributed through GitHub Releases. It automatically detects your platform, downloads the appropriate binaries, and manages them in an organized directory structure.

## Features

- **🚀 One-line Installation**: Remote installation scripts for quick setup
- **🔄 Auto-update**: Always checks for latest releases from GitHub Releases
- **📦 Bucket System**: Organize packages and scripts using bucket manifests
- **📜 Script Support**: Install and manage PowerShell, Bash, and Python scripts from buckets
- **🌐 Cross-platform**: Windows, macOS, Linux (multiple architectures)
- **📁 Organized Storage**: Packages stored under `~/.wenget/apps/` with shims/symlinks in `~/.local/bin/`
- **🔒 Checksum Verification**: SHA-256 validation against the manifest `checksum` or published release checksums; mismatches and failed lookups abort (`--skip-checksum` overrides a failed lookup)
- **⚡ Stage-and-Swap Installs**: Atomic extractions prevent partial or broken installations
- **🔍 Smart Search**: Fuzzy, ranked search across all configured buckets — prefixes, typos, abbreviations (`rgp` → ripgrep), and description keywords
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
| Root/Admin | Linux/macOS | `/opt/wenget/apps` | `/usr/local/bin` (symlinks) |
| Root/Admin | Windows | `%ProgramW6432%\wenget\apps` | `%ProgramW6432%\wenget\bin` |

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

# Update installed packages (always checks for a wenget update first)
wenget update

# Delete a package
wenget del ripgrep
```

## Commands

### Package Management

- `wenget add <name|url>...` (aliases: `install`, `a`) - Install packages (from bucket, GitHub repository URL, script, or local file)
  - `-c, --command <name>` - Custom command name (overrides default executable name)
  - `-v, --ver <version>` - Specify version to install (e.g., `v1.0.0`, `1.0.0`)
  - `-p, --platform <target>` - Specify target platform (e.g., `windows-x64`, `linux-x64`, `darwin-arm64`)
  - `--variant <name>` - Install a specific variant (e.g., `--variant baseline`)
  - `--no-suffix` - Don't append variant suffix to command name
  - `--skip-checksum` - Install unverified when the checksum lookup fails on the network (a mismatch still aborts)
  - `-y, --yes` - Skip confirmation prompts
- `wenget info <name|url>...` (alias: `i`) - Show package information from buckets or GitHub repository
- `wenget del <name>...` (aliases: `remove`, `rm`, `uninstall`) - Uninstall packages
  - `-f, --force` - Force deletion (allows removing wenget itself without interactive confirmation)
  - `--variant <name>` - Specify variant to delete (e.g., `baseline`, `profile`)
  - `-y, --yes` - Skip confirmation prompts
  - `wenget del self` - Uninstall wenget itself and its files; removes only the PATH entries `wenget init` added (recorded in `~/.wenget/path.json`)
- `wenget list` (alias: `ls`) - List installed packages (with source and description)
  - `-a, --all` - Show all available packages from buckets
- `wenget search <terms>...` (alias: `s`) - Search available packages (case-insensitive fuzzy match on names, plus description/repo keywords; `*` globs supported)
- `wenget update [name]...` (alias: `up`) - Update installed packages (always checks for a wenget update first)
  - `-p, --platform <target>` - Update for a specific platform (overrides `preferred_platform`)
  - `--skip-checksum` - Install unverified when the checksum lookup fails on the network (a mismatch still aborts)
  - `-y, --yes` - Skip confirmation prompts

### Bucket Management

- `wenget bucket add <name> <url>` - Add a bucket manifest URL
- `wenget bucket del <name>...` - Remove buckets
- `wenget bucket list` - List all configured buckets
- `wenget bucket refresh` - Rebuild package cache from all enabled buckets
- `wenget bucket create` - Generate a bucket manifest from source files or direct URLs

`add`, `update`, and `info` automatically send `GITHUB_TOKEN` when set, raising the GitHub API limit
from 60 to 5,000 requests per hour.

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
- `-r, --repos-src` - Source file(s) with GitHub repo URLs (comma-separated or multiple)
- `-s, --scripts-src` - Source file(s) with Gist/script URLs (comma-separated or multiple)
- `-d, --direct` - Direct URLs (comma-separated)
- `-o, --output` - Output file (default: manifest.json)
- `-t, --token` - GitHub personal access token
- `-u, --update-mode` - How to handle existing file: `overwrite` or `incremental`

### System

- `wenget init` - Initialize wenget directories and configuration
  - `-y, --yes` - Skip confirmation prompts
- `wenget config` (alias: `c`) - Edit user preferences (config.toml) with default editor
- `wenget rename <old> [new]` (aliases: `mv`, `rn`) - Rename an installed command
- `wenget repair` - Repair corrupted configuration files
  - `-f, --force` - Force rebuild all configuration files (not just corrupted ones)
- `wenget --version` - Show version information
- `wenget --help` - Show help message

### Global Options

- `--verbose` - Enable verbose logging (global; long form only; `-y/--yes` is a per-command option on `add`, `del`, `update`, and `init`)

## Directory Structure

### User-Level Installation (default)
```
~/.wenget/
├── apps/                  # Installed applications
│   ├── wenget/            # wenget itself
│   └── <package>/        # Each installed package
│       └── .wenget/
│           └── package.json  # This package's record (version, source, commands)
├── cache/                 # Download cache
│   └── downloads/        # Downloaded archives
├── manifest-cache.json    # Cached package list from buckets
├── config.toml           # User preferences (platform, paths, etc.)
└── buckets.json          # Bucket configuration

~/.local/bin/              # Symlinks/shims (added to PATH; %USERPROFILE%\.local\bin on Windows)
├── wenget                 # wenget symlink (Unix) / shim (Windows)
└── <package>             # Package symlinks / shims
```

### System-Level Installation (root/Administrator)

**Linux/macOS:**
```
/opt/wenget/
├── apps/                  # Installed applications
│   ├── wenget/
│   └── <package>/
├── cache/
│   └── downloads/
├── manifest-cache.json
└── buckets.json

/usr/local/bin/            # Symlinks to binaries
├── wenget -> /opt/wenget/apps/wenget/wenget
└── <package> -> ...
```

**Windows:**
```
%ProgramW6432%\wenget\
├── apps\                  # Installed applications
│   ├── wenget\
│   └── <package>\
├── bin\                   # Binaries (added to system PATH)
│   ├── wenget.exe
│   └── <package>.exe
├── cache\
│   └── downloads\
├── manifest-cache.json
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

### Environment Variables

- `WENGET_ROOT` - Override wenget root and bin directories (e.g., `env WENGET_ROOT=/tmp/test wenget ...`). Sets both application data and binary links under the specified directory; useful for testing and sandboxing without touching `~/.wenget/`.
- `GITHUB_TOKEN` - GitHub personal access token. Raises the GitHub API limit from 60 to 5,000 requests/hour; automatically honored by `add`, `update`, `info`, and `bucket create`.

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
      "version": "1.0.0",
      "platforms": {
        "windows-x86_64": [
          {
            "url": "https://github.com/username/repo/releases/download/v1.0.0/tool-windows-x64.zip",
            "size": 1234567,
            "asset_name": "tool-windows-x64.zip"
          }
        ],
        "linux-x86_64": [
          {
            "url": "https://github.com/username/repo/releases/download/v1.0.0/tool-linux-x64.tar.gz",
            "size": 1234567,
            "asset_name": "tool-linux-x64.tar.gz"
          }
        ]
      }
    }
  ],
  "scripts": [
    {
      "name": "my-script",
      "description": "Useful script",
      "repo": "https://github.com/username/repo",
      "homepage": "https://example.com",
      "license": "MIT",
      "platforms": {
        "powershell": {
          "url": "https://raw.githubusercontent.com/username/repo/main/script.ps1"
        },
        "bash": {
          "url": "https://raw.githubusercontent.com/username/repo/main/script.sh"
        }
      }
    }
  ]
}
```

#### Required Fields

**For Packages:**
- `name`: Package name (used in commands)
- `repo`: GitHub repository URL
- `description`: Brief package description
- `platforms`: Platform-specific binary entries as a map of platform identifiers (`windows-x86_64`, `linux-x86_64-musl`, `macos-aarch64`, etc.) to arrays of binary objects:
  - `url`: Download URL for the binary
  - `size`: File size in bytes
  - `asset_name`: Original asset filename from the release

**For Scripts:**
- `name`: Script name (used in commands)
- `description`: Brief script description
- `repo`: Repository URL (for reference, e.g. Gist URL)
- `platforms`: Map of script type (`powershell`, `bash`, `batch`, `python`) to platform info:
  - `url`: Direct URL to the script file

#### Optional Fields

- `homepage`: Project homepage URL (packages and scripts)
- `license`: Package or script license (packages and scripts)
- `version`: Package version string (packages)
- `checksum`: SHA-256 digest (`<hex>` or `sha256:<hex>`) verified against the download on `PlatformBinary` entries; takes precedence over probing release checksum files

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
| Windows | aarch64 (ARM64) | ✅ Supported |
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
4. **Download & Checksum**: Downloads the asset and verifies its SHA-256 against the manifest `checksum`, or else against checksums published next to the release asset (`.sha256`, `checksums.txt`, `SHA256SUMS`). A mismatch always aborts; a lookup that fails on the network aborts unless `--skip-checksum` is given; an asset with no published checksum installs unverified.
5. **Stage & Swap**: Extracts the archive into `.staging/` before atomically swapping into `~/.wenget/apps/<package>/`, ensuring interrupted runs never leave corrupted installs
6. **Launcher Creation**: Creates shims (Windows) or symlinks (Unix) in `~/.local/bin/` (or configured bin directory) for immediate command access

If any package in a multi-package install fails, wenget aborts and exits with a non-zero exit code (`1`).

## GitHub API Rate Limits

wenget uses the GitHub API to fetch package information and download binaries. Be aware of GitHub's API rate limits:

### Rate Limit Overview

| Authentication | Rate Limit | Impact |
|---------------|------------|--------|
| Unauthenticated | 60 requests/hour | Limited package searches and updates |
| Authenticated (`GITHUB_TOKEN`) | 5,000 requests/hour | Sufficient for normal usage |

Setting `GITHUB_TOKEN` in your environment (or passing `-t/--token` to `wenget bucket create`) automatically enables authenticated requests with 5,000 requests/hour.

### Impact on wenget Operations

**Operations that consume API calls:**
- `wenget add <name>` - 1 call per package (fetches latest release from GitHub API, reusing cached bucket metadata)
- `wenget add <url>` - 2 calls per URL (when installing from GitHub repository URL: repo info + release)
- `wenget info <url>` - 2 calls per URL (querying GitHub repository directly)
- `wenget update` - 1 call per installed package checked via GitHub API

**Operations that don't consume API calls:**
- `wenget info <name>` - Uses cached bucket data for bucket packages (0 API calls)
- `wenget search` - Uses cached bucket data (0 API calls)
- `wenget list` - Local only
- `wenget del` - Local only
- `wenget bucket` (`list`, `add`, `del`, `refresh`) - Local configuration / HTTP manifest downloads only

### Recommendations

1. **Use Buckets**: The bucket system caches package metadata, keeping installs down to a single release fetch
2. **Set `GITHUB_TOKEN`**: For heavy usage or CI environments, export `GITHUB_TOKEN=...` in your shell configuration
3. **Rate limit exceeded?** Wait an hour or use authenticated requests with `GITHUB_TOKEN`

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
wenget del tokei
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
│   ├── bucket.rs           # Bucket management
│   ├── cache.rs            # Package cache
│   ├── cli.rs              # CLI interface
│   ├── commands/           # Command implementations
│   ├── core/               # Core functionality
│   ├── downloader/         # Download logic
│   ├── installer/          # Installation logic
│   ├── main.rs             # Entry point
│   ├── package_resolver.rs # Package and script resolution
│   ├── providers/          # GitHub API integration
│   └── utils/              # Utilities (HTTP client, prompts)
├── install.ps1             # Windows installer
└── install.sh              # Unix installer
```

For architecture and documentation structure, see [CONTEXT.md](./CONTEXT.md).

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
