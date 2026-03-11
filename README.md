# DelveAI

DelveAI is a CLI-first system for managing human and LLM collaboration as auditable session trees.

## CLI Quickstart

Build and run the CLI:

```sh
cargo run -p delve-cli -- session create --intent "Plan V1 launch" --provider echo
```

Common non-interactive commands:

```sh
delve session create --intent "Plan V1 launch" --provider echo
delve session continue --session <session-id> --prompt "Add test strategy" --provider echo
delve session list
delve session show --session <session-id>
delve artifact show --session <session-id> --artifact <artifact-id>
delve artifact accept --session <session-id> --artifact <artifact-id>
delve session complete --session <session-id>
```

Machine-friendly mode and quiet mode:

```sh
delve --json session list
delve --quiet session continue --session <session-id> --prompt "Refine risks" --provider echo
```

Interactive and auto-interactive flows:

```sh
delve session interactive --provider echo
delve session auto --session <session-id> --provider echo --max-steps 5
```

Shell completions:

```sh
delve completion --shell bash
delve completion --shell zsh
```

## Stable Exit Codes

1. `0`: success.
2. `1`: internal/unclassified failure.
3. `2`: CLI usage or argument parsing error.
4. `3`: not found.
5. `4`: conflict (for example, ambiguous artifact without `--session`).
6. `5`: invalid state or validation failure.
7. `6`: provider execution failure.
8. `130`: interrupted by Ctrl+C in auto mode.

## Repository Status

1. Product specification, research, and phased plan documents are in place.
2. Rust workspace scaffolding has been created for all core V1 crates.
3. CI now runs workspace quality gates (fmt, clippy, test).

## Repository Layout

```text
crates/
  delve-domain/
  delve-storage/
  delve-orchestrator/
  delve-providers/
  delve-cli/
  delve-server/
apps/
  web/
```

## Key Documents

1. `DelveAI.md` for product spec.
2. `DelveAI-RESEARCH.md` for design depth and architecture recommendations.
3. `DelveAI-PLAN.md` for phased delivery execution.
4. `CONTRIBUTING.md` for contribution and quality expectations.

## Workspace Commands

Once Rust is installed locally:

```sh
make fmt
make lint
make test
make check
```
