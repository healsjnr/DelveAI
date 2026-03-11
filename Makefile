.PHONY: fmt lint test check

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace --all-targets

check:
	cargo check --workspace --all-targets
