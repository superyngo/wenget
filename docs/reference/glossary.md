# Glossary

Canonical vocabulary for wenget. Code identifiers, UI strings, commit messages, and every other
document use these terms.

**Bucket**:
A remote or local **Manifest** source added to wenget's config, identified by name. Buckets supply
`manifest.json` entries that map **Package** names to release sources.
_Avoid_: Repo (when referring to the bucket concept, not the underlying GitHub repository).

**Manifest**:
The `manifest.json` produced by a **Bucket**, mapping **Package** names to release metadata and
binaries. Installed packages record their origin via `PackageSource` source kinds (`Bucket`,
`DirectRepo`, or `Script`). Backed by `src/core/manifest.rs`.
_Avoid_: Registry, index.

**Manifest cache**:
The local cache file (`{root}/manifest-cache.json`, `WenPaths::manifest_cache_json`) that merges
**Bucket** sources into a unified catalog with a 24-hour TTL (`src/core/cache.rs`). Rebuilt
automatically from configured **Bucket**s when expired or corrupted.
_Avoid_: Index, registry.

**Package**:
A named, installable piece of software resolved from a **Bucket** or a direct GitHub URL. May
have zero or more **Variant**s.
_Avoid_: App, tool (when referring to the resolved package specifically).

**Variant**:
A named alternate build of the same **Package** released under one repo (e.g. `bun-baseline`,
`bun-profile`). Extracted from **Asset** filenames by `extract_variant_from_asset`. **Installed key**
is `{repo_name}::{variant}`.
_Avoid_: Flavor, edition.

**Asset**:
A single downloadable file attached to a GitHub release. Assets are scored and filtered per
**Platform** in `src/core/platform.rs`; see `docs/reference/RESOURCE_FILTERING_RULES.md`.
_Avoid_: Artifact, binary (an asset may be an archive, not just a binary).

**Platform**:
An `{os}-{arch}` (or `{os}-{arch}-{compiler}`) identifier used to bucket **Asset**s and to match
the current machine to the best bucket. See `Platform::possible_identifiers` /
`Platform::find_best_match`.
_Avoid_: Target (reserved for Rust target triples specifically).

**Installed package**:
A **Package** wenget has installed, described by its **Package record**: source, version,
resolved **Variant**, and installed command name(s).
_Avoid_: —

**Package record**:
The `{app_dir}/.wenget/package.json` file in an **App directory**. The authoritative statement that
a **Package** is installed, holding its version, **Platform**, source, and executable map. Written
by `InstalledStore::save_package` in `src/core/store.rs`. There is no global index.
_Avoid_: Meta, meta file, ledger, registry, index, install record, `installed.json`.

**App directory**:
`{root}/apps/<sanitized-name>/` — the directory holding one **Package**'s files and its **Package
record**. Named by `sanitize_path_component`, which is lossy, so the directory name is never
parsed for identity.
_Avoid_: Payload, install dir.

**Untracked app directory**:
A directory under `{root}/apps/` with no readable **Package record**. `wenget repair` reports it
and otherwise leaves it alone; wenget never fabricates a record for it.
_Avoid_: Orphan (reserved for **Launcher**s in the bin directory).

**Staging**:
The temporary directory (`{root}/apps/.staging/`, `WenPaths::staging_dir`) where a **Package** is
extracted and its **Package record** written before atomically swapping into place via directory
rename (`src/installer/staging.rs`). Ensures an install or update either fully succeeds or leaves
the previous state untouched.
_Avoid_: Temp dir, install dir.

**Installed key**:
`{repo_name}` or `{repo_name}::{variant}` — the identity of an **Installed package**, produced by
`generate_installed_key`. Reconstructed from **Package record** content on load, never parsed
from the **App directory** name.
_Avoid_: Package id, slug.

**Installed set**:
The in-memory collection of every loaded **Package record** (`InstalledSet`). Rebuilt on each
command from a scan of `{root}/apps/`; never serialized as a whole.
_Avoid_: Installed manifest, installed.json.

**Provider**:
`GitHubProvider`, the concrete type that fetches release and **Asset** data for a **Package** from
GitHub releases (there is no trait). Backed by `src/providers/github.rs`.
_Avoid_: Backend, source (source is used for the resolved input kind, see below).

**Package input**:
The parsed form of what the user typed on the CLI: `DirectUrl` (a GitHub URL) or `CacheName` (a
**Bucket**-relative package name, optionally with `::variant` or a `*` glob). See
`PackageInput::parse` in `src/package_resolver.rs`.
_Avoid_: Query, spec.

**Launcher**:
The symlink or shim in the bin directory (`WenPaths::bin_dir`) that runs an installed
**Package**'s executable. Implemented as a **Symlink** on Unix (`src/installer/symlink.rs`) and a
**Shim** on Windows (`src/installer/shim.rs`).
_Avoid_: Wrapper.

**Shim**:
The Windows implementation of a **Launcher**: a `.cmd` batch wrapper that forwards to an
installed binary, created by `src/installer/shim.rs`.
_Avoid_: Symlink (Windows uses **Shim**, not **Symlink**), wrapper.

**Symlink**:
The Unix implementation of a **Launcher**: a symbolic link from the wenget bin directory to an
installed binary, created by `src/installer/symlink.rs`.
_Avoid_: Shim (Unix uses **Symlink**, not **Shim**).
