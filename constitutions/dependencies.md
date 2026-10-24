---
source_sha: 9039640
marshalled_at: 2026-10-24T02:34:27Z
---


# Dependencies and Packages

Before adding or upgrading a dependency, verify the latest stable version using a live
source — run the appropriate registry command (`uv add --dry-run`, `npm info <pkg>
version`, `cargo search`, etc.) or perform a web search. Do not rely on training-data
knowledge of version numbers; it is stale. Use the package manager CLI to add and pin
dependencies — never edit lock files or dependency manifests by hand.
