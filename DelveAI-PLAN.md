# DelveAI V1 Delivery Plan

## Inputs Used

1. `DelveAI.md` product spec.
2. `DelveAI-RESEARCH.md` design review and recommendations.

## Delivery Approach

1. Build the Rust CLI and core orchestration first.
2. Freeze domain and storage contracts before frontend build.
3. Break work into small, parallelizable tasks suitable for sub-agents.
4. Preserve clear checkpoints at the end of each phase.

## Suggested Sub-Agent Lanes

1. Domain Agent: types, invariants, state transitions.
2. Storage Agent: filesystem layout, schemas, migrations.
3. CLI Agent: command UX, interactive shell, flags.
4. Orchestration Agent: provider abstraction, prompt/review loop.
5. Git Agent: branch/ref integration and safety checks.
6. API Agent: local HTTP server and DTO contracts.
7. Frontend Agent: local web app for session and artifact workflows.
8. QA Agent: test harnesses, fixtures, end-to-end validation.
9. Docs Agent: docs, examples, and release materials.

## Phase 0 - Product And Spec Hardening

Objective: Lock core semantics so implementation stays consistent.

Tasks:
- [ ] Define canonical glossary for Intent, Prompt, Artifact, Context, Implementation, Session.
- [ ] Document the approved rule that Prompt nodes can connect directly to child Prompt nodes.
- [ ] Define full node lifecycle states and allowed transitions.
- [ ] Define rules for `current_node` movement.
- [ ] Define completion criteria for an Intent.
- [ ] Define accepted vs rejected artifact policy language.
- [ ] Define whether multiple accepted implementations are allowed.
- [ ] Define behavior for superseded accepted artifacts.
- [ ] Define prompt continuation context policy for selecting accepted sibling artifacts as prompt inputs.
- [ ] Finalize label generation algorithm, sanitization, and collision handling policy.
- [ ] Finalize session folder naming and location defaults.
- [ ] Document Ctrl+C double-press behavior and edge cases.
- [ ] Decide minimum provider support for V1 (single vs multi-provider).
- [ ] Decide git-required vs git-optional behavior for implementation artifacts.
- [ ] Publish approved V1 scope boundaries and explicit out-of-scope list.

Phase exit criteria:
1. Product semantics and lifecycle are documented and approved.
2. All downstream phases have unambiguous implementation contracts.

## Phase 1 - Repository And Tooling Foundation

Objective: Create a buildable monorepo with CI, quality gates, and crate skeletons.

Tasks:
- [x] Initialize cargo workspace with required crates.
- [x] Add crate: `delve-domain`.
- [x] Add crate: `delve-storage`.
- [x] Add crate: `delve-orchestrator`.
- [x] Add crate: `delve-providers`.
- [x] Add crate: `delve-cli`.
- [x] Add crate: `delve-server`.
- [x] Add `apps/web` scaffold for future frontend phase.
- [x] Configure rustfmt and clippy with repo-wide settings.
- [x] Configure test command aliases and Makefile/task runner.
- [x] Configure CI pipeline for build, lint, and tests.
- [x] Add basic contributor documentation (`README`, setup guide).
- [x] Add `CONTRIBUTING.md` with coding standards and PR checklist.
- [x] Add issue templates and bug report template.
- [x] Add changelog and release note template.

Phase exit criteria:
1. Clean checkout builds successfully in CI.
2. All crates compile with placeholder APIs and tests running.

## Phase 2 - Domain Model And Invariants

Objective: Implement core session tree model and state rules.

Tasks:
- [x] Define `NodeKind` enum.
- [x] Define `ArtifactKind` enum.
- [x] Define `NodeStatus` enum.
- [x] Define `SessionState` enum.
- [x] Define `NodeId`, `SessionId`, and strongly typed identifiers.
- [x] Implement node struct with parent/children references.
- [x] Implement session aggregate root with `intent_node_id`.
- [x] Implement transition validator for node status changes.
- [x] Implement tree validator that allows `Prompt -> Prompt` edges and rejects invalid combinations.
- [x] Implement `current_node` update rules.
- [x] Implement helper to compute active lineage.
- [x] Implement helper to resolve eligible context nodes, including accepted sibling artifacts for continuation prompts.
- [x] Add property-based tests for tree invariants.
- [x] Add unit tests for transition constraints.
- [x] Add golden fixtures for valid and invalid trees, including prompt-to-prompt continuation cases.

Phase exit criteria:
1. Domain crate exposes stable typed APIs.
2. Invariants are enforced and covered by tests.

## Phase 3 - Storage And Persistence

Objective: Persist sessions robustly as markdown plus JSON with crash-safe behavior.

Tasks:
- [x] Define `session.json` schema version `v1`.
- [x] Define `events.jsonl` event schema.
- [x] Implement deterministic folder path builder.
- [x] Implement label generator for intent labels.
- [x] Implement label generator for prompt labels.
- [x] Implement label generator for artifact labels.
- [x] Implement collision retry logic for labels.
- [x] Implement atomic write utility for JSON files.
- [x] Implement append-only event writer.
- [x] Implement session loader with schema version checks.
- [x] Implement migration hook interface for future versions.
- [x] Implement lock file mechanism for active sessions.
- [x] Implement checkpoint writer for in-progress runs.
- [x] Add integration tests for interrupted writes.
- [x] Add round-trip tests for load-save-load parity.

Phase exit criteria:
1. Sessions can be created, persisted, and reloaded reliably.
2. Storage survives interruption without corrupting state.

## Phase 4 - Provider Abstraction And Orchestration Core

Objective: Build deterministic prompt generation, context packing, and review flow.

Tasks:
- [x] Define provider trait for completion/generation calls.
- [x] Define normalized provider request schema.
- [x] Define normalized provider response schema.
- [x] Implement mock provider for deterministic tests.
- [x] Define provider streaming contract for incremental output chunks and completion events.
- [x] Implement context packer with token budget input and prompt `input_node_ids` support.
- [x] Implement context exclusion for rejected/superseded nodes.
- [x] Implement prompt package builder that merges lineage context with selected accepted sibling artifacts.
- [x] Implement artifact proposal generator interface.
- [x] Implement review rubric schema and parser.
- [x] Implement review executor stage.
- [x] Implement confidence threshold decision gate.
- [x] Implement next-prompt suggestion stage.
- [x] Persist orchestration decision metadata to events log.
- [x] Add end-to-end tests using mock provider fixtures.
- [x] Add failure-path tests for provider timeout/error handling.
- [x] Implement Amp provider adapter in `delve-providers` using `amp -x` execution mode.
- [x] Implement Claude provider adapter in `delve-providers` using `claude -p` execution mode.
- [x] Implement streaming support for Amp provider adapter.
- [x] Implement streaming support for Claude provider adapter.

Phase exit criteria:
1. Orchestration pipeline works end-to-end with mock provider.
2. Review and continuation decisions are deterministic and auditable.

## Phase 5 - CLI Non-Interactive Commands

Objective: Deliver script-friendly command surface for create/continue workflows.

Tasks:
- [x] Define top-level CLI command layout.
- [x] Implement `session create --intent` command.
- [x] Implement `session continue --session --prompt` command.
- [x] Execute `session create --intent` by sending the Intent text to the configured provider.
- [x] Stream provider output live to the CLI during `session create` execution.
- [x] Persist `session create` provider output as one or more artifact nodes with markdown payloads.
- [x] Execute `session continue --session --prompt` by sending the prompt text to the configured provider.
- [x] Stream provider output live to the CLI during `session continue` execution.
- [x] Persist `session continue` provider output as one or more artifact nodes with markdown payloads.
- [x] Implement `session show --session` command.
- [x] Implement `session list` command.
- [x] Implement `artifact show` command.
- [x] Implement `artifact accept` command.
- [x] Implement `artifact reject` command.
- [x] Implement `session complete` command.
- [x] Add `--json` machine-readable output mode.
- [x] Add stable exit codes documentation and enforcement.
- [x] Add `--quiet` and `--no-color` flags.
- [x] Add command-level help text and examples.
- [x] Add integration tests for all non-interactive commands.
- [x] Add integration tests for provider-backed `session create` and `session continue`, including streamed output behavior.
- [x] Add shell completion generation.

Phase exit criteria:
1. All required non-interactive workflows run in CI.
2. `session create` and `session continue` execute via providers with streamed CLI output and persisted artifacts.
3. Commands are scriptable with stable outputs and exit codes.

## Phase 6 - CLI Interactive And Auto-Interactive Flows

Objective: Deliver terminal UX for iterative and autonomous session progression.

Tasks:
- [x] Implement interactive session launcher.
- [x] Implement session picker with recent sessions.
- [x] Implement tree visualization in terminal.
- [x] Implement prompt composer in interactive mode.
- [x] Implement artifact browsing panel.
- [x] Implement action menu for accept/reject/show.
- [x] Implement auto-interactive command entrypoint.
- [x] Implement auto loop: generate artifacts.
- [x] Implement auto loop: review artifacts.
- [x] Implement auto loop: propose next prompt.
- [x] Implement user confirmation checkpoint in auto loop.
- [x] Implement completion detection signal for auto loop.
- [x] Implement first Ctrl+C graceful shutdown behavior.
- [x] Implement second Ctrl+C immediate termination behavior.
- [x] Add resume-from-checkpoint flow and tests.

Phase exit criteria:
1. Interactive and auto-interactive modes work reliably.
2. Interrupt and resume behavior is predictable and tested.

## Phase 7 - Git-Backed Implementation Artifacts

Objective: Support code-change artifacts with safe branch/ref lifecycle.

Tasks:
- [ ] Implement repo detection and validation.
- [ ] Implement clean-worktree safety check.
- [ ] Implement branch naming strategy using artifact labels.
- [ ] Implement branch creation for implementation artifacts.
- [ ] Implement optional commit creation flow.
- [ ] Persist branch and commit refs in artifact metadata.
- [ ] Implement behavior when git is unavailable.
- [ ] Implement fallback to markdown-only artifact mode.
- [ ] Implement command to show linked branch for artifact.
- [ ] Add tests for branch naming collisions.
- [ ] Add tests for detached HEAD or invalid repo state.
- [ ] Add rollback handling for failed branch operations.
- [ ] Add docs for git workflows and caveats.
- [ ] Add integration tests in temporary git repos.
- [ ] Add telemetry event for git operation outcomes.

Phase exit criteria:
1. Implementation artifacts can reliably reference git branches/commits.
2. Unsafe repo states fail fast with actionable guidance.

## Phase 8 - Local API Server For Frontend

Objective: Expose stable local APIs so web frontend reuses CLI/domain logic.

Tasks:
- [ ] Choose local server framework and runtime model.
- [ ] Define API versioning strategy (`/api/v1`).
- [ ] Define session list endpoint.
- [ ] Define session detail endpoint with tree payload.
- [ ] Define create intent endpoint.
- [ ] Define continue session endpoint with prompt payload.
- [ ] Define artifact accept/reject endpoints.
- [ ] Define artifact content endpoint.
- [ ] Define live activity endpoint (polling or SSE).
- [ ] Implement DTO mappings from domain types.
- [ ] Implement standardized API error schema.
- [ ] Add API request validation.
- [ ] Add authentication model decision (local-only trusted boundary for V1).
- [ ] Add API integration tests.
- [ ] Publish OpenAPI spec for frontend use.

Phase exit criteria:
1. Frontend can consume all required workflow APIs.
2. API contracts are stable and documented.

## Phase 9 - Local Web Frontend V1

Objective: Provide intuitive artifact-centric UX on top of local API.

Tasks:
- [ ] Scaffold web app with routing and state management.
- [ ] Build session list page.
- [ ] Build create session/intention flow.
- [ ] Build session detail page with tree explorer.
- [ ] Build node detail panel with metadata.
- [ ] Build artifact markdown viewer.
- [ ] Build accept/reject action controls.
- [ ] Build prompt composer and submit workflow.
- [ ] Build run status/activity timeline panel.
- [ ] Build auto-interactive run controls.
- [ ] Add loading, empty, and error states.
- [ ] Add keyboard shortcuts for core actions.
- [ ] Add responsive layout for laptop and desktop widths.
- [ ] Add accessibility pass for navigation and labels.
- [ ] Add frontend integration tests against mock API.

Phase exit criteria:
1. Users can complete core session workflows without CLI.
2. Frontend behavior matches CLI/domain rules.

## Phase 10 - Quality, Hardening, And Release

Objective: Validate reliability, polish docs, and ship V1 release candidate.

Tasks:
- [ ] Run full cross-platform test matrix.
- [ ] Add performance benchmarks for large session trees.
- [ ] Add memory profile checks for long auto loops.
- [ ] Add fuzz tests for malformed session files.
- [ ] Add recovery tests for kill/restart during writes.
- [ ] Add security review for path handling and key storage.
- [ ] Finalize CLI reference documentation.
- [ ] Finalize API documentation and examples.
- [ ] Finalize frontend user guide.
- [ ] Create sample sessions for demos.
- [ ] Prepare release notes and upgrade notes.
- [ ] Tag release candidate and run smoke suite.
- [ ] Conduct pilot with 3-5 target users.
- [ ] Triage pilot feedback into post-V1 backlog.
- [ ] Ship V1.0.0 and publish known limitations.

Phase exit criteria:
1. Release candidate passes all quality gates.
2. V1 ships with documented constraints and post-V1 roadmap inputs.

## Parallelization Notes

1. Phase 1 can run in parallel across tooling, docs, and crate scaffolding.
2. In Phase 2 and 3, Domain and Storage agents can work in parallel with shared schema checkpoints.
3. In Phase 4, provider adapters and review pipeline can split across two sub-agents.
4. In Phase 5 and 6, interactive and non-interactive CLI work can be split once shared command core exists.
5. In Phase 8 and 9, API and frontend can run in parallel after endpoint contracts are frozen.

## Dependencies Summary

1. Phase 0 must complete before implementation.
2. Phase 1 must complete before all code phases.
3. Phase 2 and 3 must complete before robust orchestration.
4. Phase 4 should complete before auto-interactive UX stabilizes.
5. Phase 8 should complete before frontend feature completion.
6. Phase 10 begins once all functional phases reach baseline completeness.

## Milestone Definitions

1. Milestone A: CLI can create and continue sessions with persisted trees.
2. Milestone B: Auto-interactive loop runs with review gating and resume support.
3. Milestone C: Git-backed implementation artifacts are stable.
4. Milestone D: Web frontend supports end-to-end local workflows.
5. Milestone E: V1 release candidate validated and shipped.
