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
5. Sessions now persist a single intent `thread_id`, and all prompts in that intent tree execute on the same provider thread.
6. Amp provider now creates a new thread with `amp threads new` for intent creation and executes prompts with `amp threads continue <thread_id> -x`.
7. Next-prompt suggestions are now provider-driven and thread-aware, using the session thread context.
8. `session interactive` now runs on a ratatui-based terminal UI with session picker, in-session actions, and modal prompt/intent composition.
9. Interactive prompt/intent execution now runs provider calls in background worker threads and streams output into a dedicated "Current Session Output" panel.

### Fixed
1. AMP provider flows now refresh incompatible legacy session `thread_id` values before continue/auto execution so artifact generation and next-prompt suggestions always run through valid AMP threads.

### Removed

## [0.0.0-phase1-scaffold] - 2026-03-11

### Added
1. Initial project operations scaffolding (`README`, `CONTRIBUTING`, issue templates, CI placeholder workflow).
