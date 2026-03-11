# DelveAI V1 Research And Design Review

## Purpose

This document reviews the current DelveAI product spec, provides implementation-focused feedback, and adds practical depth needed to build DelveAI V1.

## Executive Summary

The spec defines a strong product direction with two clear anchors:
1. Session-first orchestration around a tree model.
2. Artifact-first UX where generated outputs are first-class, auditable objects.

The core concept is promising, but several items must be clarified before implementation begins:
1. Canonical state model for nodes, statuses, and transitions.
2. Deterministic file and folder conventions (including collision handling).
3. Orchestration boundaries between Delve and model providers.
4. Acceptance/rejection semantics and context inclusion rules.
5. Reliable recovery behavior across crashes, Ctrl+C exits, and partial runs.

Without these clarifications, implementation will likely drift across CLI, storage, and frontend layers.

## Spec Strengths

1. Strong product framing around context efficiency and human-legible artifacts.
2. Correct sequencing of delivery: CLI first, frontend second.
3. Tree-based session abstraction is easy to reason about and extensible.
4. Explicit accepted/rejected artifact states create a clear quality loop.
5. Mix of interactive and non-interactive modes supports both exploration and automation.

## Gaps And Recommended Clarifications

### 1) Domain Model Ambiguities

One major ambiguity has now been resolved: Prompt nodes can have child Prompt nodes.

This is the correct model for continuation flows, because a new prompt often synthesizes accepted outputs from multiple sibling artifacts while remaining part of the same prompt lineage.

Needed clarifications:
1. Prompt-to-prompt links are allowed and should be first-class in the tree.
2. Whether an Intent can end with multiple accepted implementation leaves.
3. How to represent revisions of the same artifact (new sibling vs same node with versions).
4. Whether status transitions are immutable events or mutable fields.

Recommendation:
1. Keep node graph strict and typed.
2. Formally support `Prompt -> Prompt` continuation edges.
3. Persist provenance on each Prompt node via `input_node_ids` so multi-artifact sibling context is explicit and auditable.
4. Allow multiple accepted implementation leaves in V1.
5. Use immutable event log plus current computed status for auditability.

### 2) Label Generation And Collisions

The spec says labels are short text + random ID, but does not define sanitization or collision handling.

Recommendation:
1. Slug format: lowercase ascii, `a-z0-9-` only.
2. Prefix truncated semantic slug to 16 chars.
3. Suffix 8-char random base36 token.
4. If collision occurs, regenerate token up to N attempts.
5. Persist original full text in metadata for readability.

### 3) Storage Schema Is Underspecified

The spec says markdown + JSON, but not exact file names or required fields.

Recommendation:
1. Define one canonical `session.json` schema with versioning.
2. Add per-node metadata files where helpful for local reads.
3. Store event log separately for traceability.
4. Define stable path conventions now to avoid migration churn.

Proposed session folder:

```text
sessions/
  <intentLabel>/
    intent.md
    session.json
    events.jsonl
    prompts/
      <promptLabel>/
        prompt.md
        artifacts/
          <artifactLabel>.md
          <artifactLabel>.meta.json
    runtime/
      locks/
      checkpoints/
```

### 4) Acceptance/Rejection Lifecycle

The spec says rejected nodes must never be used as context, but it does not describe how decisions happen.

Recommendation:
1. Require explicit transition events: `proposed -> accepted|rejected`.
2. Enforce context filtering in one central function.
3. Add `superseded` status for accepted artifact replaced by better one.
4. Track `accepted_by` (`human`, `auto-review`, `rule-engine`) for transparency.

### 5) Auto-Interactive Review Is Underdetailed

Auto mode currently implies a loop but not how Delve reviews artifacts.

Recommendation:
1. Split generation and review into separate pipeline stages.
2. Add review rubric with fixed dimensions:
   - correctness
   - completeness
   - safety
   - alignment with intent
   - confidence
3. Require structured review output in JSON.
4. Gate continue automatically by configurable confidence threshold.

### 6) Git Integration Needs Guardrails

Spec states code-change artifacts map to branch names, but not branch lifecycle.

Recommendation:
1. Prefix branch names with intent and artifact label.
2. Capture repo cleanliness at start; fail fast if unsafe for auto mode.
3. Store branch name and optional commit hash in artifact metadata.
4. Define behavior when no git repo exists.

### 7) Interrupt And Recovery Behavior

Spec says Ctrl+C twice exits, but no guarantees on state consistency.

Recommendation:
1. First Ctrl+C enters graceful shutdown mode and writes checkpoint.
2. Second Ctrl+C hard exits.
3. On restart, detect in-progress run and offer resume.
4. Use lock files to prevent concurrent writers to same session.

### 8) Frontend Contract Needs Early Definition

Frontend is second, but if APIs and data contracts are not defined early it will force rework.

Recommendation:
1. Define local API contract while building CLI.
2. Use same typed domain DTOs for CLI and API server.
3. Ensure all artifacts and tree state are queryable without parsing markdown.

## Recommended V1 Architecture

## Layered Components

1. Domain Layer (pure Rust): node types, transitions, validation rules.
2. Persistence Layer: session storage, schema versioning, migration hooks.
3. Orchestrator Layer: model invocation, context packing, review loop.
4. Adapter Layer: CLI commands and local HTTP API.
5. UI Layer (later phase): local web frontend using HTTP API.

## Rust Crate Layout (Monorepo)

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

## Data Model Proposal

Core enums:
1. `NodeKind = Intent | Prompt | Artifact`
2. `ArtifactKind = Context | Implementation`
3. `NodeStatus = Proposed | Accepted | Rejected | Superseded`

Core node fields:
1. `id` (uuid)
2. `label` (human-safe slug + token)
3. `kind`
4. `status`
5. `parent_id`
6. `children_ids`
7. `input_node_ids` (accepted nodes explicitly used as context for this prompt)
8. `created_at`, `updated_at`
9. `payload_ref` (file path or git ref)

Session metadata:
1. `session_id`
2. `schema_version`
3. `intent_node_id`
4. `current_node_id`
5. `state` (`active|completed|abandoned`)

## Context Packing Strategy (V1)

Simple deterministic policy:
1. Include only accepted artifacts on active lineage by default.
2. For `Prompt -> Prompt` continuations, include accepted sibling artifacts selected in `input_node_ids` by default.
3. Exclude rejected and superseded nodes always.
4. Apply token budget and truncate oldest low-priority context first.
5. Persist final packed context summary per prompt for auditability.

## Orchestration Pipeline (Per Prompt)

1. Resolve eligible context set.
2. Build model prompt package.
3. Generate candidate artifact(s).
4. Run review stage.
5. Persist artifacts and review outputs.
6. Suggest next prompt.
7. Wait for user decision or continue in auto mode.

## CLI Experience Recommendations

Interactive mode should prioritize discoverability:
1. Session picker with recent sessions and status.
2. Tree view showing node status and artifact type.
3. Explicit actions: `prompt`, `accept`, `reject`, `show`, `resume`, `complete`.
4. Preview panel for artifact markdown and metadata.

Non-interactive mode should prioritize scriptability:
1. Stable exit codes.
2. Machine-readable JSON output option.
3. Optional `--no-color` and `--quiet` flags.
4. Deterministic command grammar for CI usage.

## Frontend V1 Scope Recommendations

Minimum viable frontend:
1. Session list and creation.
2. Tree explorer with status filters.
3. Artifact viewer with accept/reject actions.
4. Prompt composer and run controls.
5. Live activity log panel.

Defer from V1:
1. Real-time collaborative editing.
2. Multi-user role permissions.
3. Advanced visual analytics.

## Reliability, Security, And Privacy

1. No silent data deletion.
2. Atomic writes for `session.json`.
3. Crash-safe append-only event log.
4. Provider API keys stored via OS keychain when possible.
5. Redaction option for sensitive context in logs.
6. Strict path validation to prevent traversal bugs.

## Testing Strategy

1. Property tests for tree invariants and status transitions.
2. Unit tests for label generation and collision handling.
3. Integration tests for interactive/non-interactive command flows.
4. Golden tests for markdown and JSON output stability.
5. End-to-end tests with mocked provider responses.
6. Recovery tests for Ctrl+C and restart resume.

## Metrics To Track In V1

1. Session completion rate.
2. Average prompts per completed intent.
3. Acceptance ratio per artifact kind.
4. Auto-mode continuation acceptance by users.
5. Time-to-first-accepted-implementation.
6. Failure/recovery rate of interrupted sessions.

## Risks And Mitigations

1. Risk: context bloat degrades output quality.
   Mitigation: strict context eligibility and token budgeting.
2. Risk: unclear state transitions create user mistrust.
   Mitigation: event log and explicit status actions.
3. Risk: frontend diverges from CLI behavior.
   Mitigation: shared domain contracts and local API from day one.
4. Risk: provider differences cause unstable outputs.
   Mitigation: provider abstraction with normalized response schema.

## Recommended V1 Definition Of Done

1. Users can create, resume, and complete sessions via CLI.
2. Tree state is consistently persisted and reloadable.
3. Accepted/rejected context rules are always enforced.
4. Auto-interactive loop can run safely with user checkpoints.
5. Local frontend supports essential session operations.
6. Recovery from interruption is verified by tests.

## Open Questions Requiring Product Decisions

1. Should Delve support multiple model providers in V1 or single-provider first?
2. Can users manually mark context relevance weights?
3. How much automated continuation is acceptable before requiring user confirmation?
4. Should implementation artifacts always map to git branches, or optionally files only?
5. What is the expected scale: personal local use vs team-shared session stores?

## Final Recommendation

Proceed with a CLI-first V1 using a strict domain model, event-backed persistence, and deterministic orchestration. Define the API and schema contracts early so the local web frontend can be added without reworking storage or orchestration logic.
