# icoo_agent Pi-Style Agent Analysis

## Scope

This document compares `examples/icoo_agent` with the AI agent design used by `earendil-works/pi`, especially `packages/agent`. The goal is not to copy Pi's TypeScript harness, but to identify which agent-harness ideas are useful and realistic for the Icoo example.

## Pi Design Points Used For Comparison

Pi's agent layer is organized around these ideas:

- A harness owns orchestration above the low-level agent loop.
- Messages are stored as agent messages and converted to provider messages at the boundary.
- The loop emits structured lifecycle events such as agent start/end, turn start/end, message start/end, and tool execution start/end.
- Tools are registered with metadata, argument contracts, execution behavior, and hooks such as preflight and postprocessing.
- Session state is durable and append-oriented, not just a final JSON dump.
- Runtime config changes apply to future turn snapshots instead of mutating in-flight provider requests.
- Resources such as skills, prompt templates, and project context are resolved into the turn snapshot.
- Compaction is a first-class session operation with durable summary state.
- Observability is based on stable structured events that other systems can transform into logs, traces, or metrics.

## Current icoo_agent Shape

`examples/icoo_agent` currently demonstrates:

- Project config through `agent.toml`.
- A deterministic mock provider and an OpenAI-compatible HTTP provider.
- Message history in memory.
- A simple assistant/tool loop.
- Tools for shell execution, file write, and file read.
- One-shot context compaction summary.
- Final session persistence as JSON.

This is enough to show a minimal Pi-like loop, but the behavior is still closer to a scripted demo than a durable agent harness.

## Gaps

### 1. No Structured Event Log

The example prints human-readable lines, but it does not record structured lifecycle events. That makes it hard to inspect a run after the fact, compare provider behavior, or debug tool failures.

Impact:

- No durable trace of turn start/end.
- Tool attempts and failures are only indirectly visible through messages.
- Tests can only assert console output or final session snapshots.

### 2. Session Persistence Is Final-State Only

The session is written once at the end as a JSON object. Pi treats session as durable state and records important changes as they happen.

Impact:

- A crash loses all progress.
- There is no append-only transcript or event history.
- Resume can only read a final JSON file if the previous run completed.

### 3. Tool Metadata Is Too Thin

Tools expose names and descriptions only. There are no argument schemas, safety metadata, timeout declarations, retry-safety flags, or active-tool filtering.

Impact:

- Provider prompts cannot describe tool arguments precisely.
- Unknown or unsafe tool calls are handled late.
- Shell execution is always available.

### 4. No Tool Preflight Or Policy Layer

Pi supports hooks such as `beforeToolCall` and `afterToolCall`. The example dispatches directly to tool functions.

Impact:

- There is no central policy for blocking `bash`.
- Path-sensitive file tools are unrestricted.
- Tool errors cannot be normalized consistently.

### 5. No Resource Or Skill Discovery

Pi can load resources such as skills and prompt templates into the turn snapshot. The example hardcodes a short system prompt.

Impact:

- The agent cannot discover local instructions.
- The provider prompt has no view of workspace resources.
- Extending behavior requires code edits instead of adding files.

### 6. Provider Contract Is Brittle

`llm_provider.icoo` asks the model to return JSON in text and parses it. It does not validate that the shape is safe before returning it to the loop.

Impact:

- Malformed provider output can crash the run.
- Tool call arguments may be absent or wrong type.
- The mock and real provider behavior can diverge.

### 7. Compaction Is Not Integrated With Context Transformation

Compaction creates a summary entry but the provider still receives the raw message list. Pi's model separates persisted messages from transformed provider context.

Impact:

- Compaction is visible in the session but does not reduce provider context.
- Future real-provider runs can still grow indefinitely.

### 8. No Turn Snapshot Concept

Config, tools, resources, and messages are read directly during the loop. Pi snapshots state per turn so in-flight work is stable and later changes affect future turns.

Impact:

- Future dynamic config would be hard to reason about.
- Debugging “what did the model see?” is difficult.

## Recommended Development Direction

The first useful increment should stay small and language-friendly:

1. Add structured events to the session.
2. Enrich tool specs with argument schema and safety metadata.
3. Add a preflight policy before tool execution.
4. Add simple local resource discovery for skills and prompt files.
5. Include discovered resources in the system prompt.
6. Persist session state with messages, summaries, resources, and events.

This gives the example a real harness shape without requiring a full Pi-style durable tree or streaming provider implementation.

## Out Of Scope For First Increment

- True streaming assistant message deltas.
- Parallel tool execution.
- Durable recovery from partial provider/tool execution.
- Branch navigation and tree-shaped transcripts.
- Full JSONL append-only storage.
- General extension hooks.
