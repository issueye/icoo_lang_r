# icoo_agent Pi-Style Development Plan

## Objective

Turn `examples/icoo_agent` from a scripted loop demo into a small but inspectable Pi-style harness example.

The first implementation pass will add:

- Structured lifecycle events.
- Better tool metadata and preflight policy.
- Resource discovery for local skills and prompt files.
- A richer system prompt that includes resources and tool specs.
- Session persistence that captures events and resources as well as messages.

## Design

### Session Shape

Extend the session map with:

- `id`: stable-ish session identifier.
- `events`: ordered event records.
- `resources`: discovered resources used by the run.
- `tool_results`: normalized tool execution results.

Event record shape:

```icoo
{
    "type": "turn_start",
    "index": 0,
    "message_count": 2
}
```

The example keeps JSON-object persistence for now. JSONL storage is a future step.

### Event Types

Initial event names:

- `agent_start`
- `agent_end`
- `turn_start`
- `turn_end`
- `message_append`
- `tool_start`
- `tool_end`
- `tool_blocked`
- `compaction`

### Tool Specs

Each tool should include:

- `name`
- `description`
- `args`
- `enabled`
- `dangerous`
- `timeout_ms`
- `retry_safe`

The prompt should serialize these specs so real providers can call tools more reliably.

### Tool Policy

Add preflight checks:

- Block disabled tools.
- Block `bash` when `tools.allow_bash` is false.
- Block file reads/writes outside configured `workspace_root`.
- Normalize missing args and unknown tools into structured error results.

Configuration additions:

```toml
[tools]
allow_bash = true
workspace_root = "."
```

### Resources

Discover optional files:

- `.pi/skills/*.md`
- `.pi/prompts/*.md`
- `AGENTS.md`
- `README.md`
- `examples/icoo_agent/README.md`

The first increment records title/path/content snippets in session resources and includes a compact resource summary in the system prompt.

### Provider Boundary

Keep current mock/real provider split, but pass the system prompt and tool specs through the session context rather than hardcoding everything inside providers.

First pass:

- Mock provider remains deterministic.
- Real provider prompt receives tool specs and resource summary.
- Provider output still uses JSON text parsing.

Future work:

- Validate provider output against tool specs.
- Support provider-native tool calling where available.
- Add streaming events.

## Implementation Tasks

1. Add `resources.icoo` for local resource discovery and formatting.
2. Extend `session.icoo` with event/resource/tool-result helpers.
3. Extend `tools.icoo` with rich specs and preflight policy.
4. Update `provider.icoo` system prompt to include resource and tool context.
5. Update `agent.icoo` to record lifecycle events and pass context.
6. Update `agent.toml` and README.
7. Add/adjust tests for the example run.

## Acceptance Criteria

- `cargo run -- run examples/icoo_agent` still succeeds in mock mode.
- Output still includes assistant/tool lines and session path.
- Saved session JSON includes `events`, `resources`, and `tool_results`.
- Tool specs include argument metadata.
- `bash` can be disabled through config.
- `cargo test --test cli examples_icoo_agent_project_runs` passes.
