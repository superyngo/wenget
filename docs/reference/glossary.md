# Glossary

Canonical vocabulary for wenget. Code identifiers, UI strings, commit messages, and every other
document use these terms.

**Bucket**:
A remote or local manifest source added to wenget's config, identified by name. Buckets supply
`manifest.json` entries that map package names to release sources.
_Avoid_: Repo (when referring to the bucket concept, not the underlying GitHub repository).

**Manifest**:
The `manifest.json` produced by a bucket, mapping package names to their `DirectRepo`/`DirectUrl`
source and cached metadata. Backed by `src/core/manifest.rs`.
_Avoid_: Registry, index.

**Package**:
A named, installable piece of software resolved from a **Bucket** or a direct GitHub URL. May
have zero or more **Variant**s.
_Avoid_: App, tool (when referring to the resolved package specifically).

**Variant**:
A named alternate build of the same **Package** released under one repo (e.g. `bun-baseline`,
`bun-profile`). Extracted from asset filenames by `extract_variant_from_asset`. Installed key is
`{repo_name}::{variant}`.
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
An entry in `installed.json` recording what was actually installed: source, version, resolved
**Variant**, and installed command name(s).
_Avoid_: Install record.

**Provider**:
A `SourceProvider` implementation (currently `GitHubProvider`) that fetches release/asset data
for a **Package** from an external source.
_Avoid_: Backend, source (source is used for the resolved input kind, see below).

**Package input**:
The parsed form of what the user typed on the CLI: `DirectUrl` (a GitHub URL) or `CacheName` (a
bucket-relative package name, optionally with `::variant` or a `*` glob). See
`PackageInput::parse` in `src/package_resolver.rs`.
_Avoid_: Query, spec.

**Shim**:
A generated Windows launcher (`.exe` or batch wrapper) that forwards to an installed binary,
created by `src/installer/shim.rs`.
_Avoid_: Wrapper, launcher (generic terms — use Shim for the Windows mechanism specifically).

**Symlink**:
The Unix equivalent of a **Shim**: a symbolic link from the wenget bin directory to an installed
binary, created by `src/installer/symlink.rs`.
_Avoid_: Shim (Unix uses Symlink, not Shim).
