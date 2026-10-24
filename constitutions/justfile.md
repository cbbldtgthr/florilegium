---
source_sha: 9039640
marshalled_at: 2026-10-24T02:34:27Z
---


# Justfile

Every project must have a Justfile. If one does not exist, create it before writing
any other scaffolding. The Justfile is the canonical interface for all development
lifecycle commands (build, test, run, migrate, lint, etc.). Both the human and the AI
interact with the project exclusively through `just`.

Requirements:
- The first recipe must always be a private `default` that runs `@just --list`.
  This prevents bare `just` from silently executing the first real recipe.
  ```
  [private]
  default:
      @just --list
  ```
- Every recipe must have a `# ...` comment immediately above it describing what it
  does, for human and machine consumption.
- Recipe commands must pass all flags explicitly — never rely on a tool's default
  behaviour being implicit. A reader must be able to understand exactly what will run
  without knowing the tool's defaults.
- New commands discovered during a session must be added to the Justfile before the
  session ends.
- If a recipe body exceeds one or two lines, extract it to a shell script in
  `just_scripts/` and call it from the recipe. The Justfile remains a readable index
  of available commands; the scripts contain the implementation detail.
