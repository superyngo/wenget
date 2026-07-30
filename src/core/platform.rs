//! Platform detection and binary matching for wenget
//!
//! This module handles:
//! - Current platform detection (OS + Architecture)
//! - Binary selection from release assets based on platform
//! - Platform string normalization

use std::collections::HashMap;

/// Types of fallback compatibility
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Some variants used in tests and future features
pub enum FallbackType {
    /// Using musl on a GNU system (compatible - musl is statically linked)
    MuslOnGnu,
    /// Using GNU on a musl system (may require glibc installation)
    GnuOnMusl,
    /// 32-bit binary on 64-bit system
    Arch32On64,
    /// x86_64 on ARM via emulation (Rosetta 2, Windows 11)
    X64OnArm,
    /// Different compiler variant on Windows
    WindowsCompilerVariant,
}

impl FallbackType {
    /// Get user-friendly description of the fallback
    pub fn description(&self) -> &str {
        match self {
            FallbackType::MuslOnGnu => "musl-linked binary (statically linked, should work)",
            FallbackType::GnuOnMusl => "glibc-linked binary (may require glibc installation)",
            FallbackType::Arch32On64 => "32-bit binary on 64-bit system",
            FallbackType::X64OnArm => "x86_64 binary via emulation (Rosetta 2 / Windows 11)",
            FallbackType::WindowsCompilerVariant => "different compiler variant",
        }
    }

    /// Whether this fallback should require user confirmation
    pub fn requires_confirmation(&self) -> bool {
        match self {
            FallbackType::MuslOnGnu => false,              // Generally works
            FallbackType::GnuOnMusl => true,               // May not work
            FallbackType::Arch32On64 => true,              // User might want 64-bit
            FallbackType::X64OnArm => true,                // Performance impact
            FallbackType::WindowsCompilerVariant => false, // Usually works
        }
    }
}

/// Result of platform matching with compatibility information
#[derive(Debug, Clone)]
pub struct PlatformMatch {
    /// The platform identifier that matched
    pub platform_id: String,
    /// Whether this is an exact match or fallback
    #[allow(dead_code)] // Used for future features and debugging
    pub is_exact: bool,
    /// Type of fallback if not exact
    pub fallback_type: Option<FallbackType>,
    /// Score for this match (higher = better)
    pub score: usize,
}

/// Supported operating systems
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
    Windows,
    Linux,
    MacOS,
    FreeBSD,
}

impl Os {
    /// Get the current OS
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "macos") {
            Os::MacOS
        } else if cfg!(target_os = "freebsd") {
            Os::FreeBSD
        } else {
            panic!("Unsupported operating system")
        }
    }

    /// Get OS keywords for matching
    /// Note: "msvc" is a compiler keyword, not an OS keyword
    pub fn keywords(&self) -> &[&str] {
        match self {
            Os::Windows => &["windows", "win64", "win32", "pc-windows", "win"],
            Os::Linux => &["linux", "unknown-linux"],
            Os::MacOS => &["darwin", "macos", "apple", "osx", "mac"],
            Os::FreeBSD => &["freebsd"],
        }
    }

    /// Get the default architecture for this OS when none is specified
    /// - Windows/Linux: default to x86_64
    /// - Darwin: default to aarch64 (Apple Silicon, Rosetta 2 handles x86_64)
    /// - FreeBSD: no default (requires explicit arch)
    pub fn default_arch(&self) -> Option<Arch> {
        match self {
            Os::Windows | Os::Linux => Some(Arch::X86_64),
            Os::MacOS => Some(Arch::Aarch64),
            Os::FreeBSD => None,
        }
    }

    /// Convert to platform string component
    pub fn as_str(&self) -> &str {
        match self {
            Os::Windows => "windows",
            Os::Linux => "linux",
            Os::MacOS => "macos",
            Os::FreeBSD => "freebsd",
        }
    }
}

/// Supported architectures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    X86_64,
    I686,
    Aarch64,
    Armv7,
}

impl Arch {
    /// Get the current architecture
    pub fn current() -> Self {
        if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else if cfg!(target_arch = "x86") {
            Arch::I686
        } else if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else if cfg!(target_arch = "arm") {
            Arch::Armv7
        } else {
            panic!("Unsupported architecture")
        }
    }

    /// Get architecture keywords for matching
    pub fn keywords(&self) -> &[&str] {
        match self {
            Arch::X86_64 => &["x86_64", "x64", "amd64"],
            Arch::I686 => &["i686", "x86", "i386", "386", "win32"],
            Arch::Aarch64 => &["aarch64", "arm64"],
            Arch::Armv7 => &["armv7", "armhf", "armv6", "arm"],
        }
    }

    /// Convert to platform string component
    pub fn as_str(&self) -> &str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::I686 => "i686",
            Arch::Aarch64 => "aarch64",
            Arch::Armv7 => "armv7",
        }
    }

    /// Resolve the "x86" keyword based on OS context
    /// Darwin: x86 -> x86_64 (32-bit Mac is obsolete)
    /// Others: x86 -> i686
    pub fn resolve_x86_keyword(os: Os) -> Self {
        match os {
            Os::MacOS => Arch::X86_64,
            _ => Arch::I686,
        }
    }
}

/// Compiler/libc variant for platform-specific binaries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compiler {
    /// GNU libc (glibc)
    Gnu,
    /// musl libc (static linking, Alpine-compatible)
    Musl,
    /// Microsoft Visual C++
    Msvc,
}

/// Detected libc type on the current system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Unknown variant kept for completeness and future use
pub enum LibcType {
    /// musl libc (Alpine, Void Linux musl, etc.)
    Musl,
    /// GNU libc (most Linux distributions)
    Glibc,
    /// Unknown or not applicable (non-Linux)
    Unknown,
}

impl LibcType {
    /// Detect the libc type on the current system at runtime
    ///
    /// Detection methods (in order):
    /// 1. Check for /lib/ld-musl-* (musl dynamic linker)
    /// 2. Check for musl in ldd output
    /// 3. Default to Glibc on Linux, Unknown on other platforms
    #[cfg(target_os = "linux")]
    pub fn detect() -> Self {
        // Method 1: Check for musl dynamic linker
        // Alpine and other musl distros have /lib/ld-musl-<arch>.so.1
        let musl_linker_patterns = [
            "/lib/ld-musl-x86_64.so.1",
            "/lib/ld-musl-aarch64.so.1",
            "/lib/ld-musl-armhf.so.1",
            "/lib/ld-musl-i386.so.1",
        ];

        for pattern in &musl_linker_patterns {
            if std::path::Path::new(pattern).exists() {
                return LibcType::Musl;
            }
        }

        // Method 2: Check ldd output (fallback)
        if let Ok(output) = std::process::Command::new("ldd").arg("--version").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}{}", stdout, stderr).to_lowercase();

            if combined.contains("musl") {
                return LibcType::Musl;
            }
        }

        // Default to Glibc on Linux
        LibcType::Glibc
    }

    #[cfg(not(target_os = "linux"))]
    pub fn detect() -> Self {
        LibcType::Unknown
    }
}

impl Compiler {
    /// Get compiler keywords for matching
    pub fn keywords(&self) -> &[&str] {
        match self {
            Compiler::Gnu => &["gnu", "glibc"],
            Compiler::Musl => &["musl"],
            Compiler::Msvc => &["msvc"],
        }
    }

    /// Get priority for this compiler on a given OS
    /// Higher values = preferred
    pub fn priority(&self, os: Os) -> u8 {
        match os {
            Os::Linux => match self {
                Compiler::Musl => 3,
                Compiler::Gnu => 2,
                Compiler::Msvc => 1,
            },
            Os::Windows => match self {
                Compiler::Msvc => 3,
                Compiler::Gnu => 2,
                Compiler::Musl => 1,
            },
            Os::MacOS | Os::FreeBSD => 1, // Uniform priority
        }
    }

    /// Convert to platform string component
    pub fn as_str(&self) -> &str {
        match self {
            Compiler::Gnu => "gnu",
            Compiler::Musl => "musl",
            Compiler::Msvc => "msvc",
        }
    }
}

/// Supported file extensions for binary assets
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileExtension {
    Exe,
    Zip,
    TarGz,
    TarXz,
    TarBz2,
    SevenZ,
    /// Uncompressed binary (no extension or unrecognized extension)
    UncompressedBinary,
    Unsupported,
}

impl FileExtension {
    /// Detect file extension from filename
    pub fn from_filename(filename: &str) -> Self {
        let lower = filename.to_lowercase();
        if lower.ends_with(".exe") {
            FileExtension::Exe
        } else if lower.ends_with(".zip") {
            FileExtension::Zip
        } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            FileExtension::TarGz
        } else if lower.ends_with(".tar.xz") {
            FileExtension::TarXz
        } else if lower.ends_with(".tar.bz2") {
            FileExtension::TarBz2
        } else if lower.ends_with(".7z") {
            FileExtension::SevenZ
        } else if Self::is_likely_binary_without_extension(filename) {
            FileExtension::UncompressedBinary
        } else {
            FileExtension::Unsupported
        }
    }

    /// Check if a filename looks like an uncompressed binary without a file extension
    /// This is used for GitHub releases that distribute raw executables
    fn is_likely_binary_without_extension(filename: &str) -> bool {
        let lower = filename.to_lowercase();

        // Exclude common non-binary files
        let excluded_extensions = [
            ".md",
            ".txt",
            ".rst",
            ".html",
            ".htm",
            ".json",
            ".yaml",
            ".yml",
            ".toml",
            ".xml",
            ".sha256",
            ".sha512",
            ".sig",
            ".asc",
            ".pub",
            ".pem",
            ".deb",
            ".rpm",
            ".apk",
            ".dmg",
            ".pkg",
            ".msi",
            ".appimage",
        ];
        if excluded_extensions.iter().any(|ext| lower.ends_with(ext)) {
            return false;
        }

        // Exclude source archives
        if lower.contains("source") || lower.contains("src") {
            return false;
        }

        // Check for platform keywords in filename (strong indicator of binary)
        let platform_keywords = [
            "windows", "win64", "win32", "linux", "darwin", "macos", "osx", "mac", "freebsd",
            "x86_64", "amd64", "x64", "aarch64", "arm64", "armv7", "i686", "x86", "i386",
        ];
        let has_platform_keyword = platform_keywords.iter().any(|kw| lower.contains(kw));

        // If filename contains platform keywords and has no common archive extension,
        // it's likely an uncompressed binary
        if has_platform_keyword {
            return true;
        }

        // Also check for files that have no extension at all (common for Unix binaries)
        // Only if they don't look like common non-binary files
        let excluded_names = [
            "readme",
            "license",
            "copying",
            "changelog",
            "authors",
            "news",
            "todo",
            "makefile",
            "dockerfile",
            "vagrantfile",
            "gemfile",
            "rakefile",
        ];
        let base_name = filename.split('/').next_back().unwrap_or(filename);
        let has_no_extension = !base_name.contains('.') || base_name.starts_with('.');
        let is_excluded_name = excluded_names.iter().any(|n| lower.contains(n));

        has_no_extension && !is_excluded_name
    }

    /// Get format preference score (higher = preferred)
    pub fn format_score(&self) -> usize {
        match self {
            FileExtension::TarGz => 5,
            FileExtension::TarXz => 4,
            FileExtension::Zip => 3,
            FileExtension::TarBz2 => 3,
            FileExtension::SevenZ => 2,
            FileExtension::Exe => 2,
            FileExtension::UncompressedBinary => 1, // Lower preference than archives
            FileExtension::Unsupported => 0,
        }
    }
}

/// Parsed asset information from filename
#[derive(Debug)]
pub struct ParsedAsset {
    pub extension: FileExtension,
    pub os: Option<Os>,
    pub arch: Option<Arch>,
    pub compiler: Option<Compiler>,
}

/// Unsupported architectures to filter out
const UNSUPPORTED_ARCHS: &[&str] = &[
    // IBM System z
    "s390x",
    "s390",
    // PowerPC variants (all forms)
    "ppc64",
    "ppc64le",
    "ppc",
    "powerpc",
    "powerpc64",
    "powerpc64le",
    // RISC-V
    "riscv64",
    "riscv32",
    "riscv",
    // MIPS variants
    "mips",
    "mips64",
    "mipsel",
    "mips64el",
    // SPARC
    "sparc64",
    "sparc",
    // Other exotic architectures
    "alpha",
    "sh4",
    "hppa",
    "ia64",
    "loong64",
    "loongarch64",
];

impl ParsedAsset {
    /// Parse an asset filename into its components
    pub fn from_filename(filename: &str) -> Self {
        let lower = filename.to_lowercase();
        let extension = FileExtension::from_filename(filename);

        // Detect OS
        let (os, _os_inferred) = Self::detect_os(&lower, extension);

        // Detect architecture (context-aware for x86 keyword)
        let arch = Self::detect_arch(&lower, os);

        // Detect compiler
        let compiler = Self::detect_compiler(&lower);

        ParsedAsset {
            extension,
            os,
            arch,
            compiler,
        }
    }

    /// Check if filename contains unsupported architecture keywords
    pub fn contains_unsupported_arch(filename: &str) -> bool {
        let lower = filename.to_lowercase();
        UNSUPPORTED_ARCHS.iter().any(|arch| lower.contains(arch))
    }

    /// Check if filename contains an unrecognized architecture-like pattern
    /// This catches patterns that look like architectures but aren't in our supported list
    fn contains_unknown_arch_pattern(filename: &str) -> bool {
        let lower = filename.to_lowercase();

        // Pattern: word boundary + arch-like keyword + word boundary
        // These are patterns that look like architectures but aren't supported
        let arch_patterns = [
            "powerpc", "ppc", "riscv", "mips", "sparc", "s390", "alpha", "sh4", "hppa", "ia64",
            "loong",
        ];

        arch_patterns.iter().any(|p| {
            // Check for word boundaries (not part of a larger word)
            if let Some(pos) = lower.find(p) {
                let before = pos == 0 || !lower.chars().nth(pos - 1).unwrap().is_alphanumeric();
                let after_pos = pos + p.len();
                let after = after_pos >= lower.len()
                    || !lower.chars().nth(after_pos).unwrap().is_alphanumeric()
                    || lower[after_pos..].starts_with("64")
                    || lower[after_pos..].starts_with("le");
                before && after
            } else {
                false
            }
        })
    }

    /// Detect OS from filename
    fn detect_os(filename: &str, ext: FileExtension) -> (Option<Os>, bool) {
        // Check explicit OS keywords
        // Note: Order matters! MacOS must be checked before Windows
        // because "darwin" contains "win" as substring
        let all_os = [Os::MacOS, Os::FreeBSD, Os::Linux, Os::Windows];
        for os in all_os {
            for keyword in os.keywords() {
                if filename.contains(keyword) {
                    return (Some(os), false);
                }
            }
        }

        // Check Linux distribution names (e.g. "ubuntu", "arch" for Arch Linux)
        // These strongly imply Linux even without the "linux" keyword
        let linux_distros = [
            "ubuntu",
            "debian",
            "fedora",
            "centos",
            "alpine",
            "opensuse",
            "suse",
            "gentoo",
            "manjaro",
            "archlinux",
        ];
        if linux_distros.iter().any(|d| filename.contains(d)) {
            return (Some(Os::Linux), true);
        }

        // Special case: "_arch-" or "-arch-" as distribution suffix (Arch Linux naming convention)
        // e.g. "youtube-tui-default_arch-x86_64" where "_arch-" signals Arch Linux
        if filename.contains("_arch-")
            || filename.contains("-arch-")
            || filename.ends_with("_arch")
            || filename.ends_with("-arch")
        {
            return (Some(Os::Linux), true);
        }

        // .exe implies Windows
        if ext == FileExtension::Exe {
            return (Some(Os::Windows), true);
        }

        // .tar.gz / .tar.xz / .tar.bz2 / bare binaries without any OS keyword implies Linux
        // (e.g. "nnn-static-5.2.x86_64.tar.gz" or "tool-x86_64" are Linux-only conventions)
        if matches!(
            ext,
            FileExtension::TarGz
                | FileExtension::TarXz
                | FileExtension::TarBz2
                | FileExtension::UncompressedBinary
        ) {
            let arch_keywords = [
                "x86_64", "x64", "amd64", "aarch64", "arm64", "armv7", "armhf", "i686", "i386",
                "386",
            ];
            if arch_keywords.iter().any(|kw| filename.contains(kw)) {
                return (Some(Os::Linux), true);
            }
        }

        (None, false)
    }

    /// Detect architecture from filename (context-aware)
    fn detect_arch(filename: &str, os: Option<Os>) -> Option<Arch> {
        // Handle special "x86" keyword first (before generic keyword matching)
        // x86 on Darwin -> x86_64 (32-bit Mac is obsolete)
        // x86 on others -> i686
        if filename.contains("x86") && !filename.contains("x86_64") {
            if let Some(os) = os {
                return Some(Arch::resolve_x86_keyword(os));
            }
            // Default to i686 if OS is unknown
            return Some(Arch::I686);
        }

        // Check all architecture keywords
        let all_arch = [Arch::X86_64, Arch::Aarch64, Arch::Armv7, Arch::I686];
        for arch in all_arch {
            for keyword in arch.keywords() {
                // Skip "x86" since we handled it above
                if *keyword == "x86" {
                    continue;
                }
                if filename.contains(keyword) {
                    return Some(arch);
                }
            }
        }

        None
    }

    /// Detect compiler/libc from filename
    fn detect_compiler(filename: &str) -> Option<Compiler> {
        let all_compilers = [Compiler::Musl, Compiler::Msvc, Compiler::Gnu];
        for compiler in all_compilers {
            for keyword in compiler.keywords() {
                if filename.contains(keyword) {
                    return Some(compiler);
                }
            }
        }
        None
    }
}

/// Platform information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
    pub compiler: Option<Compiler>,
}

impl Platform {
    /// Get the current platform
    pub fn current() -> Self {
        Self {
            os: Os::current(),
            arch: Arch::current(),
            compiler: None,
        }
    }

    /// Create a platform from components
    pub fn new(os: Os, arch: Arch) -> Self {
        Self {
            os,
            arch,
            compiler: None,
        }
    }

    /// Create a platform with compiler specification
    #[allow(dead_code)]
    pub fn with_compiler(os: Os, arch: Arch, compiler: Compiler) -> Self {
        Self {
            os,
            arch,
            compiler: Some(compiler),
        }
    }

    /// Get all possible platform identifiers for this platform
    ///
    /// Returns variants in priority order based on detected libc:
    /// - On musl systems (Alpine): musl > base > gnu
    /// - On glibc systems: gnu > base > musl
    /// - On Windows: msvc > base > gnu
    ///
    /// Examples:
    /// - "linux-x86_64-musl" (highest on Alpine)
    /// - "linux-x86_64"
    /// - "linux-x86_64-gnu"
    /// - "windows-x86_64-msvc"
    /// - "windows-x86_64-gnu"
    pub fn possible_identifiers(&self) -> Vec<String> {
        let base = format!("{}", self);
        let mut identifiers = Vec::new();

        // Add compiler variants in priority order based on detected libc
        match self.os {
            Os::Linux => {
                let libc = LibcType::detect();
                match libc {
                    LibcType::Musl => {
                        // On musl systems: prefer musl > base > gnu
                        identifiers.push(format!("{}-musl", base));
                        identifiers.push(base.clone());
                        identifiers.push(format!("{}-gnu", base));
                    }
                    LibcType::Glibc | LibcType::Unknown => {
                        // On glibc systems: prefer gnu > base > musl
                        // (musl binaries are statically linked and work on glibc too)
                        identifiers.push(format!("{}-gnu", base));
                        identifiers.push(base.clone());
                        identifiers.push(format!("{}-musl", base));
                    }
                }
            }
            Os::Windows => {
                // On Windows: prefer msvc > base > gnu
                identifiers.push(format!("{}-msvc", base));
                identifiers.push(base.clone());
                identifiers.push(format!("{}-gnu", base));
            }
            _ => {
                // macOS, FreeBSD: just base
                identifiers.push(base);
            }
        }

        identifiers
    }

    /// Find best matching platform from available options
    ///
    /// Returns matches in priority order:
    /// 1. Exact match with preferred compiler
    /// 2. Exact match with different compiler
    /// 3. Compatible fallback (with confirmation if needed)
    ///
    /// Note: Works with Vec<PlatformBinary> - checks if platform key exists,
    /// regardless of how many binaries are available for that platform.
    pub fn find_best_match(
        &self,
        available_platforms: &HashMap<String, Vec<crate::core::manifest::PlatformBinary>>,
    ) -> Vec<PlatformMatch> {
        let mut matches = Vec::new();

        // Phase 1: Try exact matches (same OS + arch)
        let exact_ids = self.possible_identifiers();
        for (priority, id) in exact_ids.iter().enumerate() {
            if available_platforms.contains_key(id) {
                matches.push(PlatformMatch {
                    platform_id: id.clone(),
                    is_exact: true,
                    fallback_type: None,
                    score: 1000 - priority, // Higher priority = higher score
                });
            }
        }

        // Phase 2: If no exact matches, try fallbacks
        if matches.is_empty() {
            let fallback_ids = self.fallback_identifiers();
            for (id, fallback_type) in fallback_ids {
                if available_platforms.contains_key(&id) {
                    let score = match fallback_type {
                        FallbackType::MuslOnGnu => 500,
                        FallbackType::GnuOnMusl => 400,
                        FallbackType::WindowsCompilerVariant => 450,
                        FallbackType::Arch32On64 => 300,
                        FallbackType::X64OnArm => 200,
                    };
                    matches.push(PlatformMatch {
                        platform_id: id,
                        is_exact: false,
                        fallback_type: Some(fallback_type),
                        score,
                    });
                }
            }
        }

        // Sort by score descending
        matches.sort_by(|a, b| b.score.cmp(&a.score));
        matches
    }

    /// Resolve platform matches for an explicit override string.
    ///
    /// The override may come from the `-p/--platform` flag or the
    /// `preferred_platform` config setting. It accepts both internal platform
    /// identifiers (e.g. `linux-aarch64-musl`) and Rust-style target triples
    /// (e.g. `aarch64-unknown-linux-musl`) or loose forms (e.g. `windows-x64`).
    ///
    /// Resolution order:
    /// 1. Exact match against an available platform key.
    /// 2. Parse os/arch/compiler from the string and use `find_best_match`,
    ///    preferring the requested compiler/libc variant when it is available.
    pub fn match_override(
        override_str: &str,
        available_platforms: &HashMap<String, Vec<crate::core::manifest::PlatformBinary>>,
    ) -> Vec<PlatformMatch> {
        // 1. Direct exact key match (internal identifier form).
        if available_platforms.contains_key(override_str) {
            return vec![PlatformMatch {
                platform_id: override_str.to_string(),
                is_exact: true,
                fallback_type: None,
                score: 1000,
            }];
        }

        // 2. Parse a lenient platform string (Rust triples, loose forms, etc.).
        let parsed = ParsedAsset::from_filename(override_str);
        let (os, arch) = match (parsed.os, parsed.arch) {
            (Some(os), Some(arch)) => (os, arch),
            // Arch missing but OS present: fall back to the OS default arch.
            (Some(os), None) => match os.default_arch() {
                Some(arch) => (os, arch),
                None => return Vec::new(),
            },
            _ => return Vec::new(),
        };

        let platform = Platform::new(os, arch);
        let mut matches = platform.find_best_match(available_platforms);

        // If a specific compiler/libc was requested (e.g. musl), prefer that
        // variant by boosting it to the front when it is available.
        if let Some(compiler) = parsed.compiler {
            let preferred_id = format!("{}-{}", platform, compiler.as_str());
            if let Some(pos) = matches.iter().position(|m| m.platform_id == preferred_id) {
                let mut chosen = matches.remove(pos);
                chosen.score = 2000;
                matches.insert(0, chosen);
            }
        }

        matches
    }

    /// Get fallback platform identifiers for cross-compatibility
    fn fallback_identifiers(&self) -> Vec<(String, FallbackType)> {
        let mut fallbacks = Vec::new();

        match (self.os, self.arch) {
            // Linux x86_64: can run i686 binaries
            (Os::Linux, Arch::X86_64) => {
                fallbacks.push(("linux-i686".to_string(), FallbackType::Arch32On64));
                fallbacks.push(("linux-i686-musl".to_string(), FallbackType::Arch32On64));
                fallbacks.push(("linux-i686-gnu".to_string(), FallbackType::Arch32On64));
            }
            // macOS ARM: can run x86_64 via Rosetta 2
            (Os::MacOS, Arch::Aarch64) => {
                fallbacks.push(("macos-x86_64".to_string(), FallbackType::X64OnArm));
            }
            // Windows x86_64: can run i686 binaries
            (Os::Windows, Arch::X86_64) => {
                fallbacks.push(("windows-i686".to_string(), FallbackType::Arch32On64));
                fallbacks.push(("windows-i686-msvc".to_string(), FallbackType::Arch32On64));
                fallbacks.push(("windows-i686-gnu".to_string(), FallbackType::Arch32On64));
            }
            // Windows ARM: can run x86_64 via emulation (Windows 11)
            (Os::Windows, Arch::Aarch64) => {
                fallbacks.push(("windows-x86_64".to_string(), FallbackType::X64OnArm));
                fallbacks.push(("windows-x86_64-msvc".to_string(), FallbackType::X64OnArm));
                fallbacks.push(("windows-i686".to_string(), FallbackType::X64OnArm));
            }
            _ => {}
        }

        fallbacks
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.compiler {
            Some(compiler) => write!(
                f,
                "{}-{}-{}",
                self.os.as_str(),
                self.arch.as_str(),
                compiler.as_str()
            ),
            None => write!(f, "{}-{}", self.os.as_str(), self.arch.as_str()),
        }
    }
}

/// Binary asset information
#[derive(Debug, Clone)]
pub struct BinaryAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// Binary selector for choosing the right asset from releases
pub struct BinarySelector;

impl BinarySelector {
    /// Select the best binary asset for a given platform
    ///
    /// # Arguments
    /// * `assets` - List of available assets
    /// * `platform` - Target platform
    ///
    /// # Returns
    /// The best matching asset, or None if no suitable asset found
    #[allow(dead_code)] // Kept for backward compatibility and future use
    pub fn select_for_platform(assets: &[BinaryAsset], platform: Platform) -> Option<BinaryAsset> {
        let mut scored_assets: Vec<(usize, &BinaryAsset)> = assets
            .iter()
            .filter_map(|asset| {
                let score = Self::score_asset(&asset.name, platform)?;
                Some((score, asset))
            })
            .collect();

        // Sort by score (highest first)
        scored_assets.sort_by(|a, b| b.0.cmp(&a.0));

        scored_assets.first().map(|(_, asset)| (*asset).clone())
    }

    /// Select ALL matching binary assets for a given platform, with scores
    ///
    /// Returns a vector of (score, BinaryAsset, Compiler) tuples, sorted by score descending.
    /// Unlike `select_for_platform()`, this returns ALL matching assets, not just the best.
    ///
    /// # Arguments
    /// * `assets` - List of available assets
    /// * `platform` - Target platform
    ///
    /// # Returns
    /// Vector of (score, asset, compiler_variant) sorted by score (highest first)
    #[allow(dead_code)] // extract_platforms now inlines this logic to parse each asset once; kept as a public helper.
    pub fn select_all_for_platform(
        assets: &[BinaryAsset],
        platform: Platform,
    ) -> Vec<(usize, BinaryAsset, Option<Compiler>)> {
        let mut scored_assets: Vec<(usize, BinaryAsset, Option<Compiler>)> = assets
            .iter()
            .filter_map(|asset| {
                let score = Self::score_asset(&asset.name, platform)?;
                let compiler = Self::detect_compiler_from_filename(&asset.name);
                Some((score, asset.clone(), compiler))
            })
            .collect();

        // Sort by score (highest first)
        scored_assets.sort_by(|a, b| b.0.cmp(&a.0));
        scored_assets
    }

    /// Extract compiler from filename (helper method)
    ///
    /// # Arguments
    /// * `filename` - The asset filename to analyze
    ///
    /// # Returns
    /// The detected compiler variant, or None if not detected
    #[allow(dead_code)] // only used by select_all_for_platform; kept alongside it.
    fn detect_compiler_from_filename(filename: &str) -> Option<Compiler> {
        let lower = filename.to_lowercase();
        if lower.contains("musl") {
            Some(Compiler::Musl)
        } else if lower.contains("msvc") {
            Some(Compiler::Msvc)
        } else if lower.contains("gnu") || lower.contains("glibc") {
            Some(Compiler::Gnu)
        } else {
            None
        }
    }

    /// Score an asset filename based on how well it matches the platform
    ///
    /// New 4-component scoring algorithm:
    /// - OS match: +100 (mandatory)
    /// - Explicit arch match: +50
    /// - Default arch match: +25
    /// - Compiler priority: +10/20/30 based on OS preference
    /// - File format: +2 to +5
    ///
    /// Returns None if the asset should be excluded
    fn score_asset(filename: &str, platform: Platform) -> Option<usize> {
        let filename_lower = filename.to_lowercase();

        // Exclude certain files
        if Self::should_exclude(&filename_lower) {
            return None;
        }

        // Filter out unsupported architectures
        if ParsedAsset::contains_unsupported_arch(&filename_lower) {
            return None;
        }

        // Parse the asset filename
        let parsed = ParsedAsset::from_filename(filename);

        // Skip if extension is unsupported
        if parsed.extension == FileExtension::Unsupported {
            return None;
        }

        let mut score = 0;

        // OS matching (mandatory)
        let os_matches = match parsed.os {
            Some(os) => os == platform.os,
            None => false,
        };

        if !os_matches {
            return None;
        }
        score += 100;

        // Architecture matching
        match parsed.arch {
            Some(arch) if arch == platform.arch => {
                // Explicit architecture match
                score += 50;
            }
            Some(_) => {
                // Explicit architecture mismatch - exclude
                return None;
            }
            None => {
                // No explicit architecture detected.
                // Unsupported arch keywords were already filtered above (line ~975),
                // so only the unknown-arch-pattern check remains here: it catches
                // arch-like strings (e.g. "powerpc64") that aren't in UNSUPPORTED_ARCHS.
                if ParsedAsset::contains_unknown_arch_pattern(filename) {
                    return None;
                }

                // Fall back to OS default arch
                if let Some(default_arch) = platform.os.default_arch() {
                    if platform.arch == default_arch {
                        // Use default architecture (lower score than explicit)
                        score += 25;
                    }
                    // If platform arch doesn't match default, still allow but no arch bonus
                } else {
                    // OS has no default (FreeBSD) - require explicit arch
                    return None;
                }
            }
        }

        // Compiler scoring based on OS-specific priority
        if let Some(compiler) = parsed.compiler {
            let priority = compiler.priority(platform.os);
            score += (priority as usize) * 10;
        }

        // File format preference
        score += parsed.extension.format_score();

        Some(score)
    }

    /// Check if a filename should be excluded from selection
    fn should_exclude(filename: &str) -> bool {
        let excludes = [
            "source",
            ".deb",
            ".rpm",
            ".apk",
            ".dmg",
            ".pkg",
            ".msi",
            ".sha256",
            ".sha512",
            ".asc",
            ".sig",
            "checksums",
            "checksum",
            ".txt",
            ".md",
        ];

        excludes.iter().any(|&e| filename.contains(e))
    }

    /// Extract platform information from available assets
    ///
    /// Returns a map of platform identifiers to a VECTOR of assets.
    /// Includes ALL matching variants for each platform.
    /// For example, if both musl and gnu variants exist for linux-x86_64,
    /// both will be included in the result. Also captures multiple package
    /// variants like baseline, desktop, etc.
    pub fn extract_platforms(assets: &[BinaryAsset]) -> HashMap<String, Vec<BinaryAsset>> {
        let mut platforms: HashMap<String, Vec<BinaryAsset>> = HashMap::new();

        // Parse each asset once and cache the data that scoring needs.
        // The previous implementation re-parsed every asset 11 times (once per
        // test platform) via score_asset -> ParsedAsset::from_filename.
        struct Preparsed<'a> {
            asset: &'a BinaryAsset,
            parsed: ParsedAsset,
            filename_lower: String,
            excluded: bool,
            unsupported_arch: bool,
            unknown_arch_pattern: bool,
        }

        let preparsed: Vec<Preparsed> = assets
            .iter()
            .map(|asset| {
                let filename_lower = asset.name.to_lowercase();
                let parsed = ParsedAsset::from_filename(&asset.name);
                Preparsed {
                    excluded: Self::should_exclude(&filename_lower),
                    unsupported_arch: ParsedAsset::contains_unsupported_arch(&filename_lower),
                    unknown_arch_pattern: ParsedAsset::contains_unknown_arch_pattern(&asset.name),
                    parsed,
                    filename_lower,
                    asset,
                }
            })
            .collect();

        // Try all common platform combinations
        let test_platforms = vec![
            // Windows
            Platform::new(Os::Windows, Arch::X86_64),
            Platform::new(Os::Windows, Arch::I686),
            Platform::new(Os::Windows, Arch::Aarch64),
            // Linux
            Platform::new(Os::Linux, Arch::X86_64),
            Platform::new(Os::Linux, Arch::I686),
            Platform::new(Os::Linux, Arch::Aarch64),
            Platform::new(Os::Linux, Arch::Armv7),
            // macOS
            Platform::new(Os::MacOS, Arch::X86_64),
            Platform::new(Os::MacOS, Arch::Aarch64),
            // FreeBSD
            Platform::new(Os::FreeBSD, Arch::X86_64),
            Platform::new(Os::FreeBSD, Arch::Aarch64),
        ];

        for platform in test_platforms {
            // Score every asset against this platform using the cached parse.
            // Mirrors score_asset + select_all_for_platform, but without re-parsing.
            let mut scored: Vec<(usize, &BinaryAsset, Option<Compiler>)> = Vec::new();
            for p in &preparsed {
                let Some(score) = Self::score_parsed(
                    &p.parsed,
                    &p.filename_lower,
                    p.excluded,
                    p.unsupported_arch,
                    p.unknown_arch_pattern,
                    platform,
                ) else {
                    continue;
                };
                scored.push((score, p.asset, p.parsed.compiler));
            }
            // Sort by score (highest first) — matches select_all_for_platform ordering.
            scored.sort_by(|a, b| b.0.cmp(&a.0));

            for (_score, asset, compiler) in scored {
                // Build platform identifier with compiler variant
                let platform_id = match compiler {
                    Some(c) => format!("{}-{}", platform, c.as_str()),
                    None => platform.to_string(),
                };

                // Push all matching assets into the vector
                platforms
                    .entry(platform_id)
                    .or_default()
                    .push(asset.clone());
            }
        }

        platforms
    }

    /// Score a pre-parsed asset against a platform.
    ///
    /// This is the parse-once form of `score_asset`: callers precompute the
    /// `ParsedAsset`, lowercased filename, and the exclude/unsupported-arch flags
    /// once per asset, then this pure-scoring function runs in O(1) per
    /// (asset, platform) pair. Behavior is identical to `score_asset`.
    fn score_parsed(
        parsed: &ParsedAsset,
        filename_lower: &str,
        excluded: bool,
        unsupported_arch: bool,
        unknown_arch_pattern: bool,
        platform: Platform,
    ) -> Option<usize> {
        // Exclude certain files
        if excluded {
            return None;
        }

        // Filter out unsupported architectures
        if unsupported_arch {
            return None;
        }

        // Skip if extension is unsupported
        if parsed.extension == FileExtension::Unsupported {
            return None;
        }

        let mut score = 0;

        // OS matching (mandatory)
        let os_matches = match parsed.os {
            Some(os) => os == platform.os,
            None => false,
        };

        if !os_matches {
            return None;
        }
        score += 100;

        // Architecture matching
        match parsed.arch {
            Some(arch) if arch == platform.arch => {
                // Explicit architecture match
                score += 50;
            }
            Some(_) => {
                // Explicit architecture mismatch - exclude
                return None;
            }
            None => {
                // No explicit architecture detected.
                // Unsupported arch keywords were already filtered above, so only
                // the unknown-arch-pattern check remains: it catches arch-like
                // strings (e.g. "powerpc64") that aren't in UNSUPPORTED_ARCHS.
                if unknown_arch_pattern {
                    return None;
                }

                // Fall back to OS default arch
                if let Some(default_arch) = platform.os.default_arch() {
                    if platform.arch == default_arch {
                        // Use default architecture (lower score than explicit)
                        score += 25;
                    }
                    // If platform arch doesn't match default, still allow but no arch bonus
                } else {
                    // OS has no default (FreeBSD) - require explicit arch
                    return None;
                }
            }
        }

        // Compiler scoring based on OS-specific priority
        if let Some(compiler) = parsed.compiler {
            let priority = compiler.priority(platform.os);
            score += (priority as usize) * 10;
        }

        // File format preference
        score += parsed.extension.format_score();

        // Suppress unused-variable warning for filename_lower: it is computed by
        // callers to drive the exclude/unsupported-arch flags above, and kept as a
        // parameter so the signature mirrors score_asset's inputs.
        let _ = filename_lower;

        Some(score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_platform() {
        let platform = Platform::current();
        println!("Current platform: {}", platform);
        assert!(matches!(
            platform.os,
            Os::Windows | Os::Linux | Os::MacOS | Os::FreeBSD
        ));
        assert!(matches!(
            platform.arch,
            Arch::X86_64 | Arch::I686 | Arch::Aarch64 | Arch::Armv7
        ));
    }

    #[test]
    fn test_platform_string() {
        let platform = Platform::new(Os::Windows, Arch::X86_64);
        assert_eq!(platform.to_string(), "windows-x86_64");

        let platform = Platform::new(Os::Linux, Arch::Aarch64);
        assert_eq!(platform.to_string(), "linux-aarch64");

        // Test platform with compiler
        let platform = Platform::with_compiler(Os::Linux, Arch::X86_64, Compiler::Musl);
        assert_eq!(platform.to_string(), "linux-x86_64-musl");
    }

    #[test]
    fn test_binary_selection() {
        let assets = vec![
            BinaryAsset {
                name: "app-windows-x86_64.zip".to_string(),
                url: "https://example.com/windows.zip".to_string(),
                size: 1000000,
            },
            BinaryAsset {
                name: "app-linux-x86_64-musl.tar.gz".to_string(),
                url: "https://example.com/linux.tar.gz".to_string(),
                size: 1000000,
            },
            BinaryAsset {
                name: "source.tar.gz".to_string(),
                url: "https://example.com/source.tar.gz".to_string(),
                size: 500000,
            },
        ];

        let windows_platform = Platform::new(Os::Windows, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, windows_platform);
        assert!(selected.is_some());
        assert!(selected.unwrap().name.contains("windows"));

        let linux_platform = Platform::new(Os::Linux, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, linux_platform);
        assert!(selected.is_some());
        assert!(selected.unwrap().name.contains("linux"));
    }

    #[test]
    fn test_should_exclude() {
        assert!(BinarySelector::should_exclude("source.tar.gz"));
        assert!(BinarySelector::should_exclude("app.deb"));
        assert!(BinarySelector::should_exclude("checksums.txt"));
        assert!(!BinarySelector::should_exclude("app-linux-x86_64.tar.gz"));
    }

    #[test]
    fn test_linux_prefers_musl_over_gnu() {
        let assets = vec![
            BinaryAsset {
                name: "app-linux-x86_64-gnu.tar.gz".to_string(),
                url: "https://example.com/gnu.tar.gz".to_string(),
                size: 1000000,
            },
            BinaryAsset {
                name: "app-linux-x86_64-musl.tar.gz".to_string(),
                url: "https://example.com/musl.tar.gz".to_string(),
                size: 1000000,
            },
        ];

        let linux_platform = Platform::new(Os::Linux, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, linux_platform);
        assert!(selected.is_some());
        // Should prefer musl (priority 3) over gnu (priority 2)
        assert!(
            selected.unwrap().name.contains("musl"),
            "Linux should prefer musl over gnu"
        );
    }

    #[test]
    fn test_windows_prefers_msvc_over_gnu() {
        let assets = vec![
            BinaryAsset {
                name: "app-windows-x86_64-gnu.zip".to_string(),
                url: "https://example.com/gnu.zip".to_string(),
                size: 1000000,
            },
            BinaryAsset {
                name: "app-windows-x86_64-msvc.zip".to_string(),
                url: "https://example.com/msvc.zip".to_string(),
                size: 1000000,
            },
        ];

        let windows_platform = Platform::new(Os::Windows, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, windows_platform);
        assert!(selected.is_some());
        // Should prefer msvc (priority 3) over gnu (priority 2)
        assert!(
            selected.unwrap().name.contains("msvc"),
            "Windows should prefer msvc over gnu"
        );
    }

    #[test]
    fn test_exe_implies_windows() {
        let assets = vec![BinaryAsset {
            name: "tool.exe".to_string(),
            url: "https://example.com/tool.exe".to_string(),
            size: 1000000,
        }];

        // .exe should match Windows (inferred OS)
        let windows_platform = Platform::new(Os::Windows, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, windows_platform);
        assert!(
            selected.is_some(),
            ".exe file should match Windows platform"
        );

        // .exe should NOT match Linux
        let linux_platform = Platform::new(Os::Linux, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, linux_platform);
        assert!(
            selected.is_none(),
            ".exe file should NOT match Linux platform"
        );
    }

    #[test]
    fn test_x86_darwin_is_x86_64() {
        // On Darwin, x86 keyword should resolve to x86_64 (32-bit Mac is obsolete)
        let assets = vec![BinaryAsset {
            name: "tool-darwin-x86.tar.gz".to_string(),
            url: "https://example.com/tool.tar.gz".to_string(),
            size: 1000000,
        }];

        // Should match x86_64 (not i686)
        let macos_x64 = Platform::new(Os::MacOS, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, macos_x64);
        assert!(selected.is_some(), "darwin-x86 should match macOS x86_64");

        // Should NOT match i686
        let macos_i686 = Platform::new(Os::MacOS, Arch::I686);
        let selected = BinarySelector::select_for_platform(&assets, macos_i686);
        assert!(selected.is_none(), "darwin-x86 should NOT match macOS i686");
    }

    #[test]
    fn test_x86_linux_is_i686() {
        // On Linux, x86 keyword should resolve to i686
        let assets = vec![BinaryAsset {
            name: "tool-linux-x86.tar.gz".to_string(),
            url: "https://example.com/tool.tar.gz".to_string(),
            size: 1000000,
        }];

        // Should match i686
        let linux_i686 = Platform::new(Os::Linux, Arch::I686);
        let selected = BinarySelector::select_for_platform(&assets, linux_i686);
        assert!(selected.is_some(), "linux-x86 should match Linux i686");

        // Should NOT match x86_64
        let linux_x64 = Platform::new(Os::Linux, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, linux_x64);
        assert!(
            selected.is_none(),
            "linux-x86 should NOT match Linux x86_64"
        );
    }

    #[test]
    fn test_darwin_defaults_to_aarch64() {
        // Darwin without explicit arch should default to aarch64 (Rosetta 2 handles x86_64)
        let assets = vec![BinaryAsset {
            name: "tool-darwin.tar.gz".to_string(),
            url: "https://example.com/tool.tar.gz".to_string(),
            size: 1000000,
        }];

        // Should match aarch64 (default for Darwin)
        let macos_aarch64 = Platform::new(Os::MacOS, Arch::Aarch64);
        let selected = BinarySelector::select_for_platform(&assets, macos_aarch64);
        assert!(
            selected.is_some(),
            "darwin without arch should match macOS aarch64 (default)"
        );
    }

    #[test]
    fn test_windows_defaults_to_x86_64() {
        // Windows without explicit arch should default to x86_64
        let assets = vec![BinaryAsset {
            name: "tool-windows.zip".to_string(),
            url: "https://example.com/tool.zip".to_string(),
            size: 1000000,
        }];

        // Should match x86_64 (default for Windows)
        let windows_x64 = Platform::new(Os::Windows, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, windows_x64);
        assert!(
            selected.is_some(),
            "windows without arch should match Windows x86_64 (default)"
        );
    }

    #[test]
    fn test_skip_unsupported_architectures() {
        let unsupported_assets = vec![
            BinaryAsset {
                name: "tool-linux-s390x.tar.gz".to_string(),
                url: "https://example.com/s390x.tar.gz".to_string(),
                size: 1000000,
            },
            BinaryAsset {
                name: "tool-linux-ppc64le.tar.gz".to_string(),
                url: "https://example.com/ppc64le.tar.gz".to_string(),
                size: 1000000,
            },
            BinaryAsset {
                name: "tool-linux-riscv64.tar.gz".to_string(),
                url: "https://example.com/riscv64.tar.gz".to_string(),
                size: 1000000,
            },
        ];

        // None of these should match any supported platform
        let linux_x64 = Platform::new(Os::Linux, Arch::X86_64);
        for asset in &unsupported_assets {
            let assets = vec![asset.clone()];
            let selected = BinarySelector::select_for_platform(&assets, linux_x64);
            assert!(
                selected.is_none(),
                "Unsupported arch {} should be filtered out",
                asset.name
            );
        }
    }

    #[test]
    fn test_freebsd_support() {
        let assets = vec![BinaryAsset {
            name: "tool-freebsd-x86_64.tar.gz".to_string(),
            url: "https://example.com/freebsd.tar.gz".to_string(),
            size: 1000000,
        }];

        let freebsd_x64 = Platform::new(Os::FreeBSD, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, freebsd_x64);
        assert!(selected.is_some(), "Should match FreeBSD x86_64");

        // FreeBSD without explicit arch should NOT match (no default arch)
        let assets_no_arch = vec![BinaryAsset {
            name: "tool-freebsd.tar.gz".to_string(),
            url: "https://example.com/freebsd.tar.gz".to_string(),
            size: 1000000,
        }];
        let selected = BinarySelector::select_for_platform(&assets_no_arch, freebsd_x64);
        assert!(
            selected.is_none(),
            "FreeBSD without arch should NOT match (requires explicit arch)"
        );
    }

    #[test]
    fn test_fallback_detection_gitui() {
        // Test fallback detection for gitui-style filenames
        let test_cases = vec![
            (
                "gitui-win.tar.gz",
                Platform::new(Os::Windows, Arch::X86_64),
                true,
            ),
            (
                "gitui-mac.tar.gz",
                Platform::new(Os::MacOS, Arch::Aarch64),
                true,
            ),
            (
                "gitui-linux-x86_64.tar.gz",
                Platform::new(Os::Linux, Arch::X86_64),
                true,
            ),
            // .msi files should be excluded
            (
                "gitui-win.msi",
                Platform::new(Os::Windows, Arch::X86_64),
                false,
            ),
        ];

        for (filename, platform, should_match) in test_cases {
            let assets = vec![BinaryAsset {
                name: filename.to_string(),
                url: format!("https://example.com/{}", filename),
                size: 1000000,
            }];

            let selected = BinarySelector::select_for_platform(&assets, platform);

            if should_match {
                assert!(
                    selected.is_some(),
                    "Expected {} to match platform {}",
                    filename,
                    platform
                );
            } else {
                assert!(
                    selected.is_none(),
                    "Expected {} NOT to match platform {}",
                    filename,
                    platform
                );
            }
        }
    }

    #[test]
    fn test_compiler_priority() {
        assert_eq!(Compiler::Musl.priority(Os::Linux), 3);
        assert_eq!(Compiler::Gnu.priority(Os::Linux), 2);
        assert_eq!(Compiler::Msvc.priority(Os::Windows), 3);
        assert_eq!(Compiler::Gnu.priority(Os::Windows), 2);
    }

    #[test]
    fn test_os_default_arch() {
        assert_eq!(Os::Windows.default_arch(), Some(Arch::X86_64));
        assert_eq!(Os::Linux.default_arch(), Some(Arch::X86_64));
        assert_eq!(Os::MacOS.default_arch(), Some(Arch::Aarch64));
        assert_eq!(Os::FreeBSD.default_arch(), None);
    }

    #[test]
    fn test_arch_resolve_x86_keyword() {
        assert_eq!(Arch::resolve_x86_keyword(Os::MacOS), Arch::X86_64);
        assert_eq!(Arch::resolve_x86_keyword(Os::Linux), Arch::I686);
        assert_eq!(Arch::resolve_x86_keyword(Os::Windows), Arch::I686);
    }

    #[test]
    fn test_extract_all_platform_variants() {
        // Test that both musl and gnu variants are extracted
        let assets = vec![
            BinaryAsset {
                name: "app-linux-x86_64-gnu.tar.gz".to_string(),
                url: "https://example.com/gnu.tar.gz".to_string(),
                size: 1000000,
            },
            BinaryAsset {
                name: "app-linux-x86_64-musl.tar.gz".to_string(),
                url: "https://example.com/musl.tar.gz".to_string(),
                size: 1000000,
            },
        ];

        let platforms = BinarySelector::extract_platforms(&assets);

        // Both should be present
        assert!(
            platforms.contains_key("linux-x86_64-musl"),
            "Should include musl variant"
        );
        assert!(
            platforms.contains_key("linux-x86_64-gnu"),
            "Should include gnu variant"
        );
        assert_eq!(platforms.len(), 2, "Should have exactly 2 platforms");
    }

    #[test]
    fn test_platform_fallback_matching() {
        use crate::core::manifest::PlatformBinary;

        let mut available = std::collections::HashMap::new();
        available.insert(
            "linux-i686".to_string(),
            vec![PlatformBinary {
                url: "test".to_string(),
                size: 0,
                checksum: None,
                asset_name: "test-linux-i686.tar.gz".to_string(),
            }],
        );

        let platform = Platform::new(Os::Linux, Arch::X86_64);
        let matches = platform.find_best_match(&available);

        assert_eq!(matches.len(), 1);
        assert!(!matches[0].is_exact);
        assert_eq!(matches[0].fallback_type, Some(FallbackType::Arch32On64));
    }

    #[test]
    fn test_match_override_rust_triple_prefers_musl() {
        use crate::core::manifest::PlatformBinary;

        let bin = |name: &str| PlatformBinary {
            url: "test".to_string(),
            size: 0,
            checksum: None,
            asset_name: name.to_string(),
        };

        let mut available = std::collections::HashMap::new();
        available.insert(
            "linux-aarch64".to_string(),
            vec![bin("tool-linux-aarch64.tar.gz")],
        );
        available.insert(
            "linux-aarch64-musl".to_string(),
            vec![bin("tool-linux-aarch64-musl.tar.gz")],
        );
        available.insert(
            "linux-aarch64-gnu".to_string(),
            vec![bin("tool-linux-aarch64-gnu.tar.gz")],
        );

        // Rust target triple with musl should resolve to the musl variant.
        let matches = Platform::match_override("aarch64-unknown-linux-musl", &available);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].platform_id, "linux-aarch64-musl");
    }

    #[test]
    fn test_match_override_exact_internal_id() {
        use crate::core::manifest::PlatformBinary;

        let mut available = std::collections::HashMap::new();
        available.insert(
            "linux-aarch64-musl".to_string(),
            vec![PlatformBinary {
                url: "test".to_string(),
                size: 0,
                checksum: None,
                asset_name: "tool-linux-aarch64-musl.tar.gz".to_string(),
            }],
        );

        let matches = Platform::match_override("linux-aarch64-musl", &available);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].is_exact);
        assert_eq!(matches[0].platform_id, "linux-aarch64-musl");
    }

    #[test]
    fn test_match_override_falls_back_when_compiler_absent() {
        use crate::core::manifest::PlatformBinary;

        // Package only ships a plain aarch64 build (no musl variant).
        let mut available = std::collections::HashMap::new();
        available.insert(
            "linux-aarch64".to_string(),
            vec![PlatformBinary {
                url: "test".to_string(),
                size: 0,
                checksum: None,
                asset_name: "tool-linux-aarch64.tar.gz".to_string(),
            }],
        );

        // Requesting musl should still resolve to the available aarch64 build.
        let matches = Platform::match_override("aarch64-unknown-linux-musl", &available);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].platform_id, "linux-aarch64");
    }

    #[test]
    fn test_macos_rosetta_fallback() {
        use crate::core::manifest::PlatformBinary;

        let mut available = std::collections::HashMap::new();
        available.insert(
            "macos-x86_64".to_string(),
            vec![PlatformBinary {
                url: "test".to_string(),
                size: 0,
                checksum: None,
                asset_name: "test-macos-x64.tar.gz".to_string(),
            }],
        );

        let platform = Platform::new(Os::MacOS, Arch::Aarch64);
        let matches = platform.find_best_match(&available);

        assert_eq!(matches.len(), 1);
        assert!(!matches[0].is_exact);
        assert_eq!(matches[0].fallback_type, Some(FallbackType::X64OnArm));
    }

    #[test]
    fn test_fallback_confirmation_required() {
        // Arch fallback should require confirmation
        assert!(FallbackType::Arch32On64.requires_confirmation());
        assert!(FallbackType::X64OnArm.requires_confirmation());
        assert!(FallbackType::GnuOnMusl.requires_confirmation());

        // Compiler variants should not require confirmation
        assert!(!FallbackType::MuslOnGnu.requires_confirmation());
        assert!(!FallbackType::WindowsCompilerVariant.requires_confirmation());
    }

    #[test]
    fn test_exact_match_preferred_over_fallback() {
        use crate::core::manifest::PlatformBinary;

        let mut available = std::collections::HashMap::new();
        available.insert(
            "linux-x86_64-musl".to_string(),
            vec![PlatformBinary {
                url: "musl".to_string(),
                size: 0,
                checksum: None,
                asset_name: "test-linux-x64-musl.tar.gz".to_string(),
            }],
        );
        available.insert(
            "linux-i686".to_string(),
            vec![PlatformBinary {
                url: "i686".to_string(),
                size: 0,
                checksum: None,
                asset_name: "test-linux-i686.tar.gz".to_string(),
            }],
        );

        let platform = Platform::new(Os::Linux, Arch::X86_64);
        let matches = platform.find_best_match(&available);

        // Should prefer exact match (musl) over fallback (i686)
        assert!(!matches.is_empty());
        assert!(matches[0].is_exact);
        assert_eq!(matches[0].platform_id, "linux-x86_64-musl");
    }

    #[test]
    fn test_libc_detection() {
        // Test that libc detection works
        let libc = LibcType::detect();
        println!("Detected libc: {:?}", libc);

        // On Linux, should be either Musl or Glibc
        #[cfg(target_os = "linux")]
        {
            assert!(
                libc == LibcType::Musl || libc == LibcType::Glibc,
                "Linux should detect either Musl or Glibc"
            );
        }

        // On non-Linux, should be Unknown
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(libc, LibcType::Unknown);
        }
    }

    #[test]
    fn test_possible_identifiers_order_on_musl() {
        // This test verifies the priority order is correct for musl systems
        // The actual detection depends on the runtime environment
        let platform = Platform::new(Os::Linux, Arch::X86_64);
        let identifiers = platform.possible_identifiers();

        // Should have 3 identifiers for Linux
        assert_eq!(identifiers.len(), 3);

        // On musl systems (like Alpine), the order should be:
        // 1. linux-x86_64-musl
        // 2. linux-x86_64
        // 3. linux-x86_64-gnu
        let libc = LibcType::detect();
        if libc == LibcType::Musl {
            assert_eq!(identifiers[0], "linux-x86_64-musl");
            assert_eq!(identifiers[1], "linux-x86_64");
            assert_eq!(identifiers[2], "linux-x86_64-gnu");
        } else {
            // On glibc systems, the order should be:
            // 1. linux-x86_64-gnu
            // 2. linux-x86_64
            // 3. linux-x86_64-musl
            assert_eq!(identifiers[0], "linux-x86_64-gnu");
            assert_eq!(identifiers[1], "linux-x86_64");
            assert_eq!(identifiers[2], "linux-x86_64-musl");
        }
    }

    #[test]
    fn test_uncompressed_binary_detection() {
        // Files with platform keywords should be detected as uncompressed binaries
        assert_eq!(
            FileExtension::from_filename("m3u8-downloader-linux-amd64"),
            FileExtension::UncompressedBinary
        );
        assert_eq!(
            FileExtension::from_filename("m3u8-downloader-darwin-amd64"),
            FileExtension::UncompressedBinary
        );
        assert_eq!(
            FileExtension::from_filename("m3u8-downloader-windows-amd64.exe"),
            FileExtension::Exe
        );
        assert_eq!(
            FileExtension::from_filename("tool-linux-x86_64"),
            FileExtension::UncompressedBinary
        );
        assert_eq!(
            FileExtension::from_filename("app-darwin-arm64"),
            FileExtension::UncompressedBinary
        );

        // Non-binary files should be unsupported
        assert_eq!(
            FileExtension::from_filename("README.md"),
            FileExtension::Unsupported
        );
        assert_eq!(
            FileExtension::from_filename("config.json"),
            FileExtension::Unsupported
        );
        assert_eq!(
            FileExtension::from_filename("checksums.sha256"),
            FileExtension::Unsupported
        );

        // Archive files should be their respective types
        assert_eq!(
            FileExtension::from_filename("app-linux-x64.tar.gz"),
            FileExtension::TarGz
        );
        assert_eq!(
            FileExtension::from_filename("app-windows-x64.zip"),
            FileExtension::Zip
        );
    }

    #[test]
    fn test_uncompressed_binary_platform_extraction() {
        // Test that uncompressed binaries are correctly extracted as platforms
        let assets = vec![
            BinaryAsset {
                name: "m3u8-downloader-linux-amd64".to_string(),
                url: "https://example.com/m3u8-downloader-linux-amd64".to_string(),
                size: 10000000,
            },
            BinaryAsset {
                name: "m3u8-downloader-darwin-amd64".to_string(),
                url: "https://example.com/m3u8-downloader-darwin-amd64".to_string(),
                size: 10000000,
            },
            BinaryAsset {
                name: "m3u8-downloader-windows-amd64.exe".to_string(),
                url: "https://example.com/m3u8-downloader-windows-amd64.exe".to_string(),
                size: 10000000,
            },
        ];

        let platforms = BinarySelector::extract_platforms(&assets);

        // Should detect all three platforms
        assert!(
            platforms.contains_key("linux-x86_64"),
            "Should detect Linux platform from uncompressed binary"
        );
        assert!(
            platforms.contains_key("macos-x86_64"),
            "Should detect macOS platform from uncompressed binary"
        );
        assert!(
            platforms.contains_key("windows-x86_64"),
            "Should detect Windows platform from .exe"
        );
    }

    #[test]
    fn test_nnn_style_assets_no_os_keyword() {
        // nnn releases don't include OS in their filenames; they should be inferred as Linux
        let assets = vec![
            BinaryAsset {
                name: "nnn-static-5.2.x86_64.tar.gz".to_string(),
                url: "https://example.com/nnn-static.tar.gz".to_string(),
                size: 100000,
            },
            BinaryAsset {
                name: "nnn-musl-static-5.2.x86_64.tar.gz".to_string(),
                url: "https://example.com/nnn-musl-static.tar.gz".to_string(),
                size: 100000,
            },
            BinaryAsset {
                name: "nnn-5.2.tar.gz.sig".to_string(),
                url: "https://example.com/sig".to_string(),
                size: 100,
            },
        ];

        let platforms = BinarySelector::extract_platforms(&assets);

        assert!(
            !platforms.is_empty(),
            "Should find platforms for nnn-style assets (no OS keyword in filename)"
        );
        assert!(
            platforms.contains_key("linux-x86_64") || platforms.contains_key("linux-x86_64-musl"),
            "Should map nnn assets to linux-x86_64"
        );
    }

    #[test]
    fn test_arch_linux_distro_naming() {
        // Assets like "youtube-tui-default_arch-x86_64" use "_arch-" for Arch Linux distro
        let assets = vec![
            BinaryAsset {
                name: "youtube-tui-default_arch-x86_64".to_string(),
                url: "https://example.com/default.bin".to_string(),
                size: 18000000,
            },
            BinaryAsset {
                name: "youtube-tui-full_arch-x86_64".to_string(),
                url: "https://example.com/full.bin".to_string(),
                size: 18000000,
            },
            BinaryAsset {
                name: "youtube-tui-minimal_arch-x86_64".to_string(),
                url: "https://example.com/minimal.bin".to_string(),
                size: 15000000,
            },
        ];

        let platforms = BinarySelector::extract_platforms(&assets);

        assert!(
            !platforms.is_empty(),
            "Should find platforms for _arch- style asset names (Arch Linux)"
        );
        assert!(
            platforms.contains_key("linux-x86_64"),
            "Should map _arch-x86_64 assets to linux-x86_64, got: {:?}",
            platforms.keys().collect::<Vec<_>>()
        );

        let linux_x64 = Platform::new(Os::Linux, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, linux_x64);
        assert!(
            selected.is_some(),
            "Should select a binary for Linux x86_64 from _arch- style assets"
        );
    }

    #[test]
    fn test_ubuntu_distro_implies_linux() {
        let assets = vec![BinaryAsset {
            name: "tool-ubuntu-x86_64.tar.gz".to_string(),
            url: "https://example.com/ubuntu.tar.gz".to_string(),
            size: 5000000,
        }];

        let linux_x64 = Platform::new(Os::Linux, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, linux_x64);
        assert!(
            selected.is_some(),
            "ubuntu distro name should match Linux platform"
        );
    }

    #[test]
    fn test_bare_binary_arch_keyword_implies_linux() {
        // Bare binary (no extension) with arch keyword should be treated as Linux
        let assets = vec![BinaryAsset {
            name: "mytool-x86_64".to_string(),
            url: "https://example.com/mytool-x86_64".to_string(),
            size: 5000000,
        }];

        let linux_x64 = Platform::new(Os::Linux, Arch::X86_64);
        let selected = BinarySelector::select_for_platform(&assets, linux_x64);
        assert!(
            selected.is_some(),
            "bare binary with x86_64 should match Linux x86_64"
        );
    }
}
