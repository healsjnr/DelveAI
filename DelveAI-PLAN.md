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
- [x] Add session-level Thread ID and require it for valid intent/session state.
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
- [x] Extend provider contracts to support provider-managed thread lifecycle and thread-bound prompts.
- [x] Implement provider-driven next prompt suggestion by querying the provider with session thread context.
- [x] Persist orchestration decision metadata to events log.
- [x] Add end-to-end tests using mock provider fixtures.
- [x] Add failure-path tests for provider timeout/error handling.
- [x] Implement Amp provider adapter in `delve-providers` using `amp -x` execution mode.
- [x] Implement Claude provider adapter in `delve-providers` using `claude -p` execution mode.
- [x] Implement streaming support for Amp provider adapter.
- [x] Switch Amp prompt execution to `amp threads continue --stream-json`, streaming `assistant` `message.content[*].text` entries and taking the artifact from the final `result` entry.
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
- [x] Persist intent Thread ID on create and reuse the same Thread ID for all prompt execution in the intent tree.
- [x] For Amp provider, create a new Amp thread at intent creation and execute all prompts using `amp threads continue <thread_id> -x`.

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

## Phase 7 - Remove Next-Prompt Suggestion

Objective: Remove the next-prompt suggestion feature from orchestration, CLI, and auto-interactive loop.

Tasks:
- [x] Delete `suggest_next_prompt_with_provider()` function from `delve-orchestrator`.
- [x] Remove `next_prompt` field from `PromptExecutionResult` in `delve-orchestrator`.
- [x] Remove `suggest_next_prompt_for_provider()` wrapper and all call sites in `delve-cli` (`run_session_create`, `run_session_continue`, interactive loop, auto-interactive loop).
- [x] Remove `suggested_next_prompt` field from `SessionCreateOutput` and `SessionContinueOutput`.
- [x] Remove `OrchestrationDecision` event appends for `stage: "suggest_next_prompt"` across all CLI flows.
- [x] Remove or update the `suggest_next_prompt_uses_provider_and_thread_id` unit test in `delve-orchestrator`.
- [x] Remove "Suggested next prompt:" text output line and update auto-loop to rely solely on user-composed or review-driven prompts.
- [x] Remove the next-prompt suggestion task references from Phase 4 and Phase 6 task lists.
- [x] Compile, run `make check`, fix any dead-code warnings.

Phase exit criteria:
1. No next-prompt suggestion logic remains in any crate.
2. All existing tests pass with the suggestion code removed.
3. Auto-interactive loop functions without suggestion-driven continuation.

## Phase 8 - Direct Artifact Editing

Objective: Allow users to edit the content of proposed artifacts before accepting or rejecting them.

Tasks:
- [ ] Add `SessionNode::is_editable()` method to `delve-domain` returning true only when status is `Proposed`.
- [ ] Add `SessionEventKind::ArtifactEdited` variant to the event schema in `delve-storage`.
- [ ] Add `ArtifactEdit` command variant and `ArtifactEditArgs` struct to `delve-cli` with `--session`, `--artifact`, optional `--content <path>`, and `--stdin` flags.
- [ ] Implement `run_artifact_edit()` in `delve-cli`: load session, find artifact node, verify `is_editable()`, read new content (from file, stdin, or `$EDITOR`), overwrite `payload_ref` file, log `ArtifactEdited` event.
- [ ] Implement `$EDITOR` flow: resolve editor from `$EDITOR` env var (fallback to `vi`), spawn editor on `payload_ref` path, wait for exit, re-read file.
- [ ] Add "Edit" option to the interactive mode action menu, gated on `Proposed` status.
- [ ] Add integration tests: edit proposed artifact succeeds; edit accepted artifact fails with correct error.
- [ ] Add unit tests for `is_editable()` across all `NodeStatus` variants.

Phase exit criteria:
1. Proposed artifacts can be edited via CLI (non-interactive and interactive).
2. Editing is blocked for non-Proposed artifacts with a clear error message.
3. All edits are logged as `ArtifactEdited` events.

## Phase 9 - Artifact Regeneration

Objective: Allow users to re-execute the prompt that created an artifact, producing a new sibling artifact.

Tasks:
- [ ] Add `SessionTree::find_parent_prompt(artifact_node_id) -> Option<&SessionNode>` helper to `delve-domain`.
- [ ] Add `read_prompt_text(session_dir, prompt_node) -> Result<String>` helper to `delve-storage` that reads the prompt's `payload_ref` markdown file.
- [ ] Add `ArtifactRegenerate` command variant and `ArtifactRegenerateArgs` struct to `delve-cli` with `--session` and `--artifact` flags.
- [ ] Implement `run_artifact_regenerate()` in `delve-cli`: load session, find parent prompt, read prompt text, call `execute_provider_prompt_streaming()`, append new artifact node as sibling under the same prompt, save session, log events.
- [ ] Add "Regenerate" option to the interactive mode action menu alongside Accept/Reject.
- [ ] Add integration test: create session, get artifact, regenerate, verify two sibling artifacts exist under the same prompt.
- [ ] Add unit test for `find_parent_prompt` helper.

Phase exit criteria:
1. Users can regenerate any artifact, producing a new sibling under the same prompt.
2. Original artifact is preserved and can be independently accepted or rejected.
3. Regeneration uses the same provider and thread context as the original execution.

## Phase 10 - Prompt Execution Error Recovery

Objective: Ensure provider errors during prompt execution are handled gracefully, preserving all work done up to the point of failure, and allowing re-execution of the failed prompt.

Tasks:
- [ ] Add `NodeStatus::Failed` variant to `delve-domain`.
- [ ] Update transition validator to allow `Failed -> Proposed` (for re-execution) and prevent other transitions from `Failed`.
- [ ] Add `SessionEventKind::PromptFailed` variant to `delve-storage`.
- [ ] Update all prompt execution paths (`session create`, `session continue`, interactive, auto-interactive) so that when a provider error occurs mid-execution: the prompt node is persisted, any partial artifact output received before the error is saved as a `Proposed` artifact node, the prompt node is marked `Failed`, a `PromptFailed` event is logged with the error details, and the session is saved to disk.
- [ ] Ensure the session remains in a valid, loadable state after a prompt failure (no dangling references, tree invariants hold).
- [ ] Add `PromptRetry` command variant and `PromptRetryArgs` struct to `delve-cli` with `--session` and `--prompt-node` flags.
- [ ] Implement `run_prompt_retry()` in `delve-cli`: load session, verify the prompt node has `Failed` status, read prompt text from `payload_ref`, re-execute via the provider, append a new artifact node under the same prompt, transition prompt status back to `Proposed`, save session, log events.
- [ ] Add "Retry" option to the interactive mode action menu, gated on `Failed` prompt status.
- [ ] Update tree visualization to visually distinguish `Failed` prompt nodes (e.g., with a marker or status indicator).
- [ ] Add unit tests for `Failed` status transitions and invariants.
- [ ] Add integration test: trigger a provider error mid-execution, verify prompt is marked `Failed`, partial artifact is saved, session is loadable, and retry produces a new artifact.

Phase exit criteria:
1. Provider errors never leave the session in a corrupt or unloadable state.
2. Prompt text and any partial artifacts are preserved on failure.
3. Failed prompts can be retried, producing new artifacts under the same prompt node.
4. All failures are recorded as `PromptFailed` events with error details.

## Phase 11 - LLM-Powered Label Generation

Objective: Replace truncated-text label slugs with short LLM-generated summaries for readable labels in the tree view.

Tasks:
- [ ] Add `generate_short_label(text: &str) -> Result<String, ProviderError>` utility to `delve-providers` with a system prompt instructing: "Return a 3-5 word summary suitable as a filesystem-safe label. No punctuation, no explanation."
- [ ] Change `generate_intent_label`, `generate_prompt_label`, `generate_artifact_label` signatures in `delve-storage` to accept an optional `short_label: Option<&str>` parameter.
- [ ] Add `generate_label_with_summary(kind, source, short_label)` to `delve-storage` that uses the short label for the slug but preserves the original source for hash stability.
- [ ] Update all label generation call sites in `delve-cli` (`session create`, `session continue`, `append_generated_prompt_and_artifact`) to call `generate_short_label()` before generating the label.
- [ ] Wrap labelling LLM calls with fallback: if the call fails, log a warning and fall back to existing truncation behavior.
- [ ] Update unit tests for label generation to cover the short-label path.
- [ ] Add integration test verifying the tree view displays LLM-generated labels with mock provider.

Phase exit criteria:
1. The tree view uses LLM-generated labels for artefacts, prompts, and intents when a provider is available.
2. Label generation gracefully falls back to truncation when the labelling call fails.
3. LLM labelling calls are not recorded as prompt/artifact nodes in the session tree.

## Phase 12 - Git-Backed Implementation Artifacts

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

## Phase 13 - Local API Server For Frontend

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
- [ ] Define artifact edit and regenerate endpoints.
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

## Phase 14 - Local Web Frontend V1

Objective: Provide intuitive artifact-centric UX on top of local API.

Tasks:
- [ ] Scaffold web app with routing and state management.
- [ ] Build session list page.
- [ ] Build create session/intention flow.
- [ ] Build session detail page with tree explorer.
- [ ] Build node detail panel with metadata.
- [ ] Build artifact markdown viewer.
- [ ] Build accept/reject action controls.
- [ ] Build artifact edit controls with inline editor.
- [ ] Build artifact regenerate action and loading state.
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

## Phase 15 - Sub-Agent Artifact Capture

Objective: Capture sub-agent results from provider responses as sibling artifacts alongside the main prompt result.

Tasks:
- [ ] Extend `ProviderResponse` in `delve-providers` to expose structured message entries including `type`, `content` array, and `parent_tool_use_id` fields.
- [ ] Add sub-agent detection logic in `delve-orchestrator`: identify response entries where `type` is `assistant`, content `type` is `text`, and `parent_tool_use_id` is not null.
- [ ] Extend `ArtifactProposal` to carry an optional `source_tool_use_id` field for traceability.
- [ ] Update `generate_artifact_streaming_with_thread` (or equivalent) to split a single provider response into multiple `ArtifactProposal` entries: one for the main result, one per sub-agent result.
- [ ] Persist each sub-agent artifact as a sibling node under the same prompt node in the session tree.
- [ ] Include `parent_tool_use_id` in artifact event metadata for auditability.
- [ ] Update `session show` tree visualization to distinguish sub-agent artifacts (e.g., with a label suffix or icon).
- [ ] Update interactive mode artifact browsing to display sub-agent artifacts grouped under their parent prompt.
- [ ] Add unit tests for sub-agent detection logic with mock provider responses containing zero, one, and multiple sub-agent entries.
- [ ] Add integration test: execute a prompt that returns sub-agent results, verify all are persisted as sibling artifacts.
- [ ] Add test for edge case: provider response with no sub-agent results produces a single artifact as before.

Phase exit criteria:
1. Sub-agent results are automatically captured as individual sibling artifacts.
2. Existing single-result behavior is unchanged when no sub-agent entries are present.
3. Sub-agent artifacts are visible and manageable (accept/reject/edit/regenerate) like any other artifact.

## Phase 16 - Quality, Hardening, And Release

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
5. Phase 7 is independent and can start immediately after Phase 6.
6. Phases 8, 9, and 10 can run in parallel after Phase 7 completes (no cross-dependencies).
7. Phase 15 can run in parallel with Phases 12, 13, and 14 (independent provider/orchestration work).
8. In Phase 13 and 14, API and frontend can run in parallel after endpoint contracts are frozen.

## Dependencies Summary

1. Phase 0 must complete before implementation.
2. Phase 1 must complete before all code phases.
3. Phase 2 and 3 must complete before robust orchestration.
4. Phase 4 should complete before auto-interactive UX stabilizes.
5. Phase 7 depends on Phase 4, 5, and 6 (removes code introduced in those phases).
6. Phase 8 and 9 depend on Phase 7 (suggestion removal simplifies the flows they modify).
7. Phase 10 depends on Phase 7 (modifies prompt execution error paths cleaned up in Phase 7) and Phase 9 (re-execution builds on the regeneration sibling pattern).
8. Phase 11 depends on Phase 4 (extends provider trait) and Phase 3 (modifies label generation).
9. Phase 15 depends on Phase 4 (extends provider response schema) and Phase 9 (sibling artifact pattern).
10. Phase 13 should complete before frontend feature completion.
11. Phase 16 begins once all functional phases reach baseline completeness.

## Milestone Definitions

1. Milestone A: CLI can create and continue sessions with persisted trees.
2. Milestone B: Auto-interactive loop runs with review gating and resume support.
3. Milestone B2: Next-prompt suggestion removed, artifact editing and regeneration available, prompt error recovery with retry, tree view uses LLM-generated labels.
4. Milestone C: Git-backed implementation artifacts are stable.
5. Milestone C2: Sub-agent results captured as individual artifacts.
6. Milestone D: Web frontend supports end-to-end local workflows.
7. Milestone E: V1 release candidate validated and shipped.
