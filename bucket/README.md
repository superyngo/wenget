# wenget Bucket

This directory contains the bucket configuration and manifest for the default wenget package repository.

## Files

- **`sources_repos.txt`** - List of GitHub repository URLs for binary packages
- **`sources_scripts.txt`** - List of Gist and raw script URLs
- **`manifest.json`** - Generated package manifest (auto-updated)

## Manifest Generation

The manifest is automatically generated and updated by the `Update Bucket Manifest` GitHub Actions
workflow, which downloads the latest wenget release binary (checked against its `SHA256SUMS`) to run
`wenget bucket create`:

1. **On Schedule**: Weekly on Monday at 00:00 UTC
2. **On Manual Trigger**: Via GitHub Actions workflow_dispatch
3. **On Source Changes**: When `sources_repos.txt` or `sources_scripts.txt` change on `main`

## Bucket URL

```
https://raw.githubusercontent.com/superyngo/wenget/refs/heads/main/bucket/manifest.json
```

## Adding Packages

To add a new package or script:

1. Edit `sources_repos.txt` (for binary packages) or `sources_scripts.txt` (for scripts)
2. Add the GitHub repository URL or Gist URL (one per line)
3. Commit and push changes
4. The manifest will be automatically regenerated

## Manual Generation

To manually generate the manifest locally from the repository root, with an installed `wenget`:

```bash
wenget bucket create \
  -r bucket/sources_repos.txt \
  -s bucket/sources_scripts.txt \
  -o bucket/manifest.json \
  -u incremental
```

For full `bucket create` options (update modes, token authentication, and direct URLs), see [Bucket Manifest Generator in README.md](../README.md#bucket-manifest-generator).
