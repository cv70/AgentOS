# AgentOS Autonomous Iteration Plan

## Goal

Push AgentOS closer to the combined design philosophy of Codex and Hermes Agent:

- Codex-style structured provenance, observability, and execution feedback
- Hermes-style closed learning loop where experience continuously updates future behavior

## Current Gap

AgentOS already has:

- `strategy_sources` on drafted tasks
- `strategy_trace` on Hermes responses
- learning reports, strategic clusters, weighting, degradation, and pruning

But it still lacks a first-class event timeline showing **when** a strategy cluster was:

1. surfaced during planning
2. adopted by a task
3. validated or falsified by execution

Without this timeline, the system has feedback metrics, but not a machine-friendly audit trail for future automatic policy tuning.

## This Iteration

### 1. Add strategy evaluation events

Create a durable event model that records strategy-cluster usage outcomes over time.

Each event should capture:

- task id
- execution id
- strategy source key
- event kind
- outcome status
- short summary / evidence
- timestamp

### 2. Persist and aggregate the timeline

Store evaluation events in SQLite and expose query methods so the runtime can reconstruct strategy usage history.

### 3. Connect execution -> strategy feedback

When a task finishes, automatically emit one evaluation event per `strategy_source`, using the execution result as evidence.

### 4. Expose a runtime API

Add an API endpoint returning the strategy evaluation timeline so the frontend and later autonomous ranking logic can consume it.

### 5. Surface the timeline in UI

Add a dedicated panel showing:

- recent strategy evaluation events
- success/failure evidence
- which strategy clusters are being validated or falsified over time

### 6. Validate end-to-end

Run backend tests and frontend build after implementation.

## Implementation Notes

- Prefer additive changes over refactors.
- Reuse existing provenance fields (`strategy_sources`, `source_strategy_keys`) instead of inventing parallel identifiers.
- Keep the event schema simple so it can later support automatic policy tuning or replay.
