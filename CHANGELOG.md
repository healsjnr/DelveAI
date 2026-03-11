# Changelog

All notable changes to this project should be documented in this file.

The format follows Keep a Changelog principles and Semantic Versioning intent.

## [Unreleased]

### Added
1. Rust workspace scaffold with six initial crates: `delve-domain`, `delve-storage`, `delve-orchestrator`, `delve-providers`, `delve-cli`, and `delve-server`.
2. Root `Cargo.toml`, `Makefile`, `.gitignore`, and `apps/web` placeholder scaffold.
3. GitHub Actions workflow updated to run Rust fmt/clippy/test quality gates.
4. Session lock and checkpoint persistence primitives with interrupted-write regression tests.
5. Non-interactive CLI integration tests that cover create/list/show/continue/artifact mutation and completion workflows.
6. Shell completion generation command (`delve completion --shell <SHELL>`).
7. Release note template at `.github/release-note-template.md`.

### Changed
1. `delve` CLI now supports global `--json`, `--quiet`, and `--no-color` flags.
2. Stable CLI exit codes are enforced for usage, not found, conflict, validation, provider, and interrupt conditions.
3. Session create/continue/auto flows now append orchestration decision metadata to `events.jsonl`.
4. Interactive (`session interactive`) and auto-interactive (`session auto`) terminal workflows were added.

### Fixed

### Removed

## [0.0.0-phase1-scaffold] - 2026-03-11

### Added
1. Initial project operations scaffolding (`README`, `CONTRIBUTING`, issue templates, CI placeholder workflow).
