# Contributing To DelveAI

Thanks for contributing. This project is being delivered in incremental phases with explicit contracts between domain, storage, orchestration, and interfaces.

## Workflow

1. Open or reference an issue before large changes.
2. Keep pull requests focused on one phase task group.
3. Add or update tests for behavior changes.
4. Update docs when command behavior, schemas, or contracts change.

## Branch And Commit Guidance

1. Use short, descriptive branch names.
2. Prefer small, reviewable commits.
3. Keep commit messages imperative and specific.

## Quality Expectations

Primary quality gates for Rust code are:
1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace --all-features`

If the Rust workspace or toolchain is not available yet, complete relevant placeholder checks and clearly call out what could not be run.

## Pull Request Checklist

1. Changes map to a specific plan phase/task.
2. New behavior is documented.
3. Tests were added or existing coverage rationale is provided.
4. CI passes (or has an explained placeholder-only run while scaffolding is in progress).
5. Changelog was updated when user-facing behavior changed.

## Coding Standards (Interim)

1. Prefer explicit domain types over ad hoc maps/strings.
2. Keep side effects isolated at boundaries (CLI/server/storage adapters).
3. Preserve deterministic behavior for orchestration and context packing paths.
4. Fail with actionable errors for invalid state or unsafe operations.
