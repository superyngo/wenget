# Repo Asset (Package) Filtering Rules

> **This document is the single source of truth.**
> Before adding or changing any asset analysis / filtering rule, update this document first, then
> the code. Each rule names its implementation as `file` — `function` (function names are
> authoritative; there are no line numbers).

Analysis runs in the following stages, plus several auxiliary classification mechanisms:

| Stage | Content | Main code location |
|------|------|-------------|
| **Stage 1** | Release assets → per-platform buckets | `src/core/platform.rs` — `BinarySelector::extract_platforms` → `score_parsed` |
| **Stage 2** | Current platform → chosen platform bucket | `src/core/platform.rs` — `possible_identifiers` / `find_best_match` / `fallback_identifiers` / `match_override` |
| **Candidate selection** | Platform candidate binaries → chosen download asset (filter / dedupe / select) | `src/installer/package.rs` — `filter_binaries`; `src/commands/add.rs` — `dedupe_same_variant` / `select_packages_for_platform` |
| **Stage 3** | Extracted files → chosen executables | `src/installer/extractor.rs` — `find_executable_candidates`; `src/installer/package.rs` — `select_executables` |
| **Auxiliary** | Release fetching, variant extraction, command-name normalization, glob matching, etc. | See section 5 |

> **Scoring engine**: all Stage 1 asset scoring runs through one engine,
> `BinarySelector::score_parsed`; `select_for_platform` is a test-only wrapper around it
> (`#[cfg(test)]`).

---

## 0. Pre-filter: release level

| # | Rule | Implementation |
|---|------|---------|
| 0.1 | Only `/releases/latest` is used, which **excludes drafts and prereleases** (GitHub API behavior) | `src/providers/github.rs` — `GitHubProvider::fetch_latest_release` |
| 0.2 | For a specific version, try the tag both with a `v` prefix added and with it removed | `src/providers/github.rs` — `fetch_release_by_tag` |
| 0.3 | Release has no assets → error | `src/providers/github.rs` — `fetch_package` |
| 0.4 | Platform map empty after Stage 1 → error (no installable platform) | Same as above |

---

## 1. Stage 1: assets → platform buckets

Entry point: `BinarySelector::extract_platforms` (`src/core/platform.rs`).
Each asset is scored against **11 test platforms** (Windows x86_64/i686/aarch64, Linux
x86_64/i686/aarch64/armv7, macOS x86_64/aarch64, FreeBSD x86_64/aarch64); passing assets go into a
`platform_id → Vec<asset>` map. A platform_id is `{os}-{arch}` or `{os}-{arch}-{compiler}`.

### 1.1 Elimination gates (**run in order**; any hit drops the asset)

Implementation: `BinarySelector::score_parsed`.

| Order | Rule | Implementation |
|------|------|---------|
| G1 | **Filename blocklist**: dropped if the filename contains any of `source`, `.deb`, `.rpm`, `.apk`, `.dmg`, `.pkg`, `.msi`, `.sha256`, `.sha512`, `.asc`, `.sig`, `checksums`, `checksum`, `.txt`, `.md` | `BinarySelector::should_exclude` |
| G2 | **Unsupported architecture**: dropped if the filename contains any keyword of the `UNSUPPORTED_ARCHS` constant: s390x/s390, ppc64/ppc64le/ppc/powerpc/powerpc64/powerpc64le, riscv64/riscv32/riscv, mips/mips64/mipsel/mips64el, sparc64/sparc, alpha, sh4, hppa, ia64, loong64/loongarch64 | `ParsedAsset::contains_unsupported_arch` + `UNSUPPORTED_ARCHS` constant |
| G3 | **Unsupported extension**: dropped if `FileExtension::from_filename` returns `Unsupported` (supported list in 1.3) | `FileExtension::from_filename` |
| G4 | **OS must match**: no OS detected, or detected OS ≠ target OS → dropped | `score_parsed` OS matching |
| G5 | **Explicit arch mismatch**: an explicit arch is detected and ≠ target arch → dropped | `score_parsed` arch matching |
| G6 | **No arch detected**: (a) filename contains an "unknown arch pattern" (word-boundary match on `powerpc/ppc/riscv/mips/sparc/s390/alpha/sh4/hppa/ia64/loong`) → dropped; (b) the OS has no default arch (FreeBSD) → dropped | `ParsedAsset::contains_unknown_arch_pattern` + `Os::default_arch` |

### 1.2 Scoring (additive, order-independent; higher total wins)

| Item | Points | Notes |
|------|------|------|
| OS match (required) | +100 | See G4 |
| Explicit arch match | +50 | |
| No arch stated, matched via the OS default arch | +25 | Windows/Linux default x86_64; macOS defaults aarch64; FreeBSD has no default |
| Compiler/libc priority | +priority×10 | Linux: musl(3) > gnu(2) > msvc(1); Windows: msvc(3) > gnu(2) > musl(1); macOS/FreeBSD always 1. See `Compiler::priority` |
| File format preference | +2 to +5 | `.tar.gz/.tgz`(5) > `.tar.xz`(4) > `.zip`/`.tar.bz2`(3) > `.7z`/`.exe`(2) > uncompressed bare binary (1). See `FileExtension::format_score` |

### 1.3 Filename parsing sub-rules (`ParsedAsset::from_filename`)

#### Extension detection (`FileExtension::from_filename`, in order)
`.exe` → `.zip` → `.tar.gz`/`.tgz` → `.tar.xz` → `.tar.bz2` → `.7z` → bare-binary check → `Unsupported`.

**Bare-binary check** (`is_likely_binary_without_extension`, in order):
1. Exclude non-binary extensions: `.md .txt .rst .html .htm .json .yaml .yml .toml .xml .sha256 .sha512 .sig .asc .pub .pem .deb .rpm .apk .dmg .pkg .msi .appimage`
2. Exclude filenames containing `source` / `src`
3. Filename contains a platform keyword (windows/win64/win32/linux/darwin/macos/osx/mac/freebsd/x86_64/amd64/x64/aarch64/arm64/armv7/i686/x86/i386) → **treated as a bare binary**
4. No extension and the filename contains none of readme/license/copying/changelog/authors/news/todo/makefile/dockerfile/vagrantfile/gemfile/rakefile → treated as a bare binary

> Note: this list, G1 (`should_exclude`), and Stage 3's `is_excluded_file` are **three independent
> lists serving different stages; do not merge them**.

#### OS detection (`ParsedAsset::detect_os`, in order)
1. Explicit OS keywords, **checked in the fixed order [MacOS, FreeBSD, Linux, Windows]** ("darwin" contains the substring "win", so historically macOS had to come before Windows; the current word-boundary matching in `ParsedAsset::contains_keyword` also prevents such substring false positives, and the order is kept for determinism):
   - Windows: `windows win64 win32 pc-windows win`
   - Linux: `linux unknown-linux`
   - macOS: `darwin macos apple osx mac`
   - FreeBSD: `freebsd`
2. Linux distribution names → Linux: `ubuntu debian fedora centos alpine opensuse suse gentoo manjaro archlinux`
3. Arch Linux naming convention → Linux: filename contains `_arch-`/`-arch-` or ends with `_arch`/`-arch`
4. `.exe` extension → Windows
5. `.tar.gz`/`.tar.xz`/`.tar.bz2`/bare binary **and** the filename contains an arch keyword (x86_64/x64/amd64/aarch64/arm64/armv7/armhf/i686/i386/386) → Linux
6. None of the above → OS unknown (dropped by G4)

#### Arch detection (`ParsedAsset::detect_arch`, in order)
1. **`x86` special case first** (filename contains `x86` but not `x86_64`): macOS → x86_64 (32-bit Mac is obsolete); other/unknown OS → i686. See `Arch::resolve_x86_keyword`
2. Match keywords in the order [X86_64, Aarch64, Armv7, I686] (skipping `x86`):
   - X86_64: `x86_64 x64 amd64`
   - Aarch64: `aarch64 arm64`
   - Armv7: `armv7 armhf armv6 arm`
   - I686: `i686 x86 i386 386 win32`

#### Compiler/libc detection (`ParsedAsset::detect_compiler`, in the order [Musl, Msvc, Gnu])
- Musl: `musl`; Msvc: `msvc`; Gnu: `gnu glibc`

---

## 2. Stage 2: current platform → chosen platform bucket

### 2.1 Exact-match priority (`Platform::possible_identifiers`)

Candidate ids are ordered by **runtime libc detection** (`LibcType::detect`: first check for the
`/lib/ld-musl-*` dynamic linker, then fall back to `ldd --version` output containing "musl",
otherwise Glibc):

| System | Priority |
|------|--------|
| Linux (musl, e.g. Alpine) | `{base}-musl` > `{base}` > `{base}-gnu` |
| Linux (glibc / unknown) | `{base}-gnu` > `{base}` > `{base}-musl` |
| Windows | `{base}-msvc` > `{base}` > `{base}-gnu` |
| macOS / FreeBSD | `{base}` only |

### 2.2 Matching flow (`Platform::find_best_match`)

1. **Phase 1, exact match**: libc and compiler variants (`-gnu` / `-musl` / `-msvc`) are all handled here. `Platform::possible_identifiers` produces exact identifiers in order based on the runtime libc (score `1000 - priority index`, i.e. 1000, 999, 998), each checked against the platform map.
2. **Phase 2, compatible fallback** (only when Phase 1 finds nothing; produced by `Platform::fallback_identifiers`):

| Current platform | Fallback targets | Type | Score | Needs user confirmation |
|----------|--------------|------|------|:---:|
| Linux x86_64 | `linux-i686`, `linux-i686-musl`, `linux-i686-gnu` | Arch32On64 | 300 | ✅ |
| macOS aarch64 | `macos-x86_64` (Rosetta 2) | X64OnArm | 200 | ✅ |
| Windows x86_64 | `windows-i686`, `windows-i686-msvc`, `windows-i686-gnu` | Arch32On64 | 300 | ✅ |
| Windows aarch64 | `windows-x86_64`, `windows-x86_64-msvc`, `windows-i686` | X64OnArm | 200 | ✅ |

Whether confirmation is needed: see `FallbackType::requires_confirmation`. Results are sorted by
score, highest first.

> Note: `FallbackType` has only two variants, `Arch32On64` and `X64OnArm`; libc / compiler variants
> never use fallback and are always matched exactly in Phase 1.

### 2.3 User override (`Platform::match_override`, the `-p/--platform` flag or the `preferred_platform` setting)

1. Override string **exactly equals** a platform map key → used directly (score 1000).
2. Otherwise parsed loosely with `ParsedAsset::from_filename` (accepts Rust target triples such as `aarch64-unknown-linux-musl` and loose forms such as `windows-x64`):
   - Both OS and arch parsed → go through `find_best_match`
   - Only OS → fill in the arch with `Os::default_arch` (fails for FreeBSD, which has no default)
3. If the override names a compiler/libc (e.g. musl) and that variant exists → **moved to first place** (score 2000).

---

## 2.5 Candidate binary filtering and selection (platform bucket → download asset)

Entry point: `prepare_plan_binaries` (`src/commands/add.rs`).
After Stage 2 picks the target platform's binary list (`Vec<PlatformBinary>`) and before download
and extraction, `add` and `update` run these three steps in order:

1. **Candidate filtering (`filter_binaries`, `src/installer/package.rs`)**:
   - **Update mode**: first match against the asset filename of the existing install (`normalize_asset_for_matching` strips the extension and version segments to form a template); on a hit, keep only that asset.
   - **Explicit variant**: if there was no hit or this is not update mode, and the user passed `--variant` (or entered `repo::variant`), keep only assets whose `extract_variant_from_asset` result matches that variant.
   - **No filter**: if neither applies, keep every candidate binary for the platform. An empty result after filtering is an error.
2. **Same-variant dedupe (`dedupe_same_variant`, `src/commands/add.rs`)**:
   - When one variant is published in several archive formats (e.g. both `.tar.gz` and `.zip`), installing them all would produce the same install key and overwrite each other.
   - Among candidates with the same extracted variant name (`extract_variant_from_asset`), keep the one with the highest `FileExtension::format_score`; on a tie keep the one listed first in the manifest (introduced in commit a8a031b).
3. **Platform package selection (`select_packages_for_platform`, `src/commands/add.rs`)**:
   - **One candidate**: selected automatically.
   - **Several candidates**:
     - With `--yes` / `-y`: in update mode (meaning the asset template match failed and the release layout may have changed), print a warning and pick the first binary as a best effort; outside update mode (first install), select all of them.
     - Without `--yes`: list every candidate's name and size in a `ui.multi_select` menu for the user to choose.

---

## 3. Stage 3: extracted files → chosen executables

Entry point: `find_executable_candidates` (`src/installer/extractor.rs`).
Executable selection after extraction is handled by `PackageInstaller::select_executables`
(`src/installer/package.rs`) and may select several executables:
- One candidate: selected automatically.
- Update mode (`select_executables_update`): keep the previously installed executable paths; if an old filename is gone, prompt the user for a replacement (or skip it with `--yes`).
- First install / several candidates (`select_executables_multi`): when there are ≤ 3 candidates (`score > 0` or Unix executable permission) or `--yes` is given, select all; with > 3 and no `--yes`, choose interactively via `MultiSelect`.
`find_executable` takes only the top-scoring candidate and is used solely for self-update (`upgrade_self_with_provider` in `src/commands/update.rs`).

### 3.1 Elimination gates (**run in order**; any hit skips the file)

| Order | Rule | Implementation |
|------|------|---------|
| G1 | **Exclude docs/config files** (`is_excluded_file`):<br>(a) document extensions: `.md .txt .rst .html .htm .pdf .doc .docx` and man pages `.1`–`.8`<br>(b) filename contains: `license licence copying unlicense notice readme changelog changes history authors contributors credits thanks todo news`<br>(c) config extensions: `.yml .yaml .toml .json .xml .ini .cfg .conf`<br>(d) `.fish .bash .zsh .ps1` completion files under a `complete`/`completion` directory<br>(e) files starting with `_` under a completion directory (e.g. zsh's `_rg`) | `is_excluded_file` |
| G2 | **Executable shape check** (`could_be_executable`):<br>Windows: must end with `.exe`.<br>Unix: in a `bin/` directory **or** no extension **or** a `.sh` script — any one of the three | `could_be_executable` |
| G3 | **Exclude test/debug files**: filename (name only, not the path) contains any of `test`, `debug`, `bench`, `example` → dropped | Inside `find_executable_candidates` |

### 3.2 Scoring (additive; `.exe` is stripped before name comparisons)

| Rule | Points | Notes |
|------|------|------|
| Rule 0: executable permission (Unix, mode & 0o111) | +35 | `has_executable_permission` |
| Rule 0b: magic bytes of a native binary (ELF `\x7fELF` / PE `MZ` / Mach-O magics) | +60 | `detect_executable_type` (strongest signal) |
| Rule 0b': shebang script (`#!`, recognizes python/bash/sh/node/ruby/perl) | +30 | `detect_script_type` (mutually exclusive with +60; binary wins) |
| Rule 1: filename == package name (exact) | +100 | |
| Rule 2: filename and package name contain each other (partial match) | +50 | |
| Rule 3: likely abbreviation (e.g. ripgrep → rg: segment initials, or a package-name prefix) | +40 | `is_likely_abbreviation` |
| Rule 4: inside a `bin/` directory | +40 | |
| Rule 5: inside `target/release/` (Rust projects) | +25 | |
| Rule 6: shallow directory depth: depth ≤1 | +20; depth ≤2 | +10 | |
| Rule 7: simple filename (no `-` or `_`) | +5 | |

**Selection threshold**: `score > 0` **or** executable permission (Unix). Sorted by score, highest
first.

### 3.3 Bare-binary check at install time (`is_standalone_executable`)

Decides whether the download is copied directly or goes through extraction:
- Windows: `.exe` → bare binary
- Unix: `.AppImage` → bare binary; a filename without any archive extension (`.zip .tar .gz .xz .bz2 .7z .rar .tbz .tgz`) → treated as a bare binary
- Supported archive formats (checked in order by `extract_archive`): `.tar.gz/.tgz` → `.tar.xz` → `.tar.bz2/.tbz` → `.zip` → `.7z`; anything else is an error

---

## 4. Variant extraction rules

Implementation: `extract_variant_from_asset` (`src/core/manifest.rs`). Used to tell apart several
variants of one repo (e.g. `bun` / `bun-baseline` / `bun-profile`). A token parser runs these
steps in order:

1. **Strip known extensions**: repeatedly trim the filename end against the `ASSET_EXTENSIONS` constant (case-insensitive; common archive and binary extensions such as `.tar.gz`, `.tar.xz`, `.zip`, `.7z`, `.exe`, `.dmg`, `.deb`).
2. **Tokenize and drop version numbers**: the `split` helper splits on `-` and `_`; tokens in dotted-version form are filtered out (`is_dotted_version`: optional `v`/`V` prefix, contains a dot, and every dot-separated segment is non-empty and all digits, e.g. `1.2.3`, `v0.8`); the remaining tokens are split on `.` and empty tokens dropped.
3. **Strip the package-name prefix**: run `split` on the package name (`repo_name`) to get `repo_tokens`; if the asset tokens start with exactly `repo_tokens` (case-insensitive), remove that prefix (`tokens.drain(..repo_tokens.len())`).
4. **Filter platform tokens**: walk the remaining tokens and compare against the `PLATFORM_TOKENS` constant (main OS, arch, vendor, and libc keywords such as `windows`, `linux`, `darwin`, `x86_64`, `amd64`, `arm64`, `unknown`, `gnu`, `musl`, `msvc`; other non-primary platform words such as `netbsd`, `android`, `i386` are kept so variants stay distinguishable):
   - Special case: a token `x86` immediately followed by `64` (because `_` splits `x86_64`) skips both.
   - Every token matching `PLATFORM_TOKENS` case-insensitively is removed; the rest are kept.
5. **Build the variant name**: if no tokens remain, there is no variant (returns `None`); otherwise join the remaining tokens with `-` as the variant name (e.g. `baseline`, `desktop`).

Install key format (`generate_installed_key`): no variant → `{repo_name}`; with a variant →
`{repo_name}::{variant}`.

---

## 5. Other classification and matching mechanisms

### 5.1 Command-name normalization (`normalize_command_name`, `src/installer/extractor.rs`)

Strips platform suffixes to produce a clean command name (e.g. `cate-windows-x86_64.exe` → `cate`):
1. Filename contains any platform keyword (`windows linux darwin macos freebsd netbsd openbsd x86_64 aarch64 arm64 armv7 i686 x64 x86 pc unknown gnu musl msvc`, case-insensitive) → truncate at the **first** `-` or `_`
2. Always strip a trailing `.exe`

> Side effect: names without platform keywords (e.g. `git-lfs.exe`) keep their hyphens.

### 5.2 Package input parsing and glob matching (`src/package_resolver.rs`)

- **Input classification** (`PackageInput::parse`): starts with `http://`, `https://`, or `github.com/` → DirectUrl (normalized by `normalize_github_url`: http→https, add https, strip the trailing slash, strip `.git`); otherwise → CacheName.
- **Cache name resolution** (`resolve_from_cache`, in order):
  1. For `repo::variant`, take the base name before `::`
  2. Contains glob wildcards (`is_glob` checks `*`, `?`, `[...]`) → glob match with `glob::Pattern` (`glob_match`); otherwise exact match
  3. Cache miss and not a glob → look up installed packages (each package's `package.json` record) with a DirectRepo source and resolve via the URL instead
  4. Still no match → error depending on context; non-glob names get "Did you mean" suggestions from `core::fuzzy::suggest` (`add` covers both package and script names)

`wenget search` does not use this path; it ranks with `core::fuzzy::score` (case-insensitive):
glob (contains `* ? [`) / exact > prefix > word-boundary substring > substring > subsequence (span
≤ 3× length, ≥3 chars) > typo tolerance (4–5 chars distance ≤1, 6+ chars ≤2) > word-prefix match
on description / repo (≥3 chars).

### 5.3 Post-install command-name conflict resolution (reference only, not asset filtering)

`resolve_command_name` (`src/commands/add.rs`): custom name → used as-is or with a numeric suffix;
variant → append the variant suffix (not repeated if already present), then a numeric suffix
`-1`–`-99` on conflict. The `--no-suffix` flag skips the variant suffix. See the function for
details; out of scope here.

---

## 6. Checklist when changing rules

- [ ] Update the matching section of this document first, then the code
- [ ] The three exclusion lists (1.3 bare-binary exclusions, 1.1 G1, 3.1 G1) are **independent**; make sure you change the right one
- [ ] When changing order-sensitive rules (1.3 OS detection order, 3.1 gate order), make sure existing tests cover the ordering
- [ ] Run `cargo test` (the platform, extractor, and manifest modules all have behavior tests)
