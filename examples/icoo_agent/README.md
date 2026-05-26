# icoo_agent

`icoo_agent` is a minimal Pi-style agent harness written in Icoo.

It demonstrates the same core shape as Pi without depending on a live LLM:

- project config through `agent.toml`
- message history
- an assistant turn loop
- tool call planning
- tool execution
- tool result messages
- context compaction summary
- session persistence
- structured lifecycle events
- local resource discovery
- tool metadata and preflight policy

Run it from the repository root:

```bash
cargo run -- run examples/icoo_agent
```

Or from this directory with a built `icoo` binary:

```bash
icoo run
```

The mock provider in `src/provider.icoo` is intentionally deterministic. Replace it with an HTTP-backed provider using `std.net.http.client` when the language grows a first-class LLM module.

To run against an OpenAI-compatible provider, set environment variables before launching:

```bash
ICOO_AGENT_API_KEY=... \
ICOO_AGENT_BASE_URL=https://ai-pixel.online \
ICOO_AGENT_MODEL=gpt-5.5 \
cargo run -- run examples/icoo_agent
```

When `ICOO_AGENT_API_KEY` is present, the example switches from `mock` mode to `real` mode and calls `src/llm_provider.icoo`.
The project `pkg.toml` sets a 60 second HTTP timeout because real model calls can take longer than the language runtime's default 5 second client timeout.

Tool policy can be adjusted in `agent.toml`:

```toml
[tools]
allow_bash = true
workspace_root = "."
```

Set `ICOO_AGENT_ALLOW_BASH=false` to disable shell execution without editing the config.

Expected output includes assistant turns, tool results, a compaction event, and a session path:

```text
Icoo Agent
tools: 3
resources: 2
mode: mock
[assistant] I will inspect the environment with a shell command.
[tool:bash] icoo-agent tool execution
[assistant] The command worked. I will persist a short session note.
[tool:write_file] wrote target/icoo_agent_note.txt
[assistant] I will read the note back to verify the file tool result.
[tool:read_file] icoo_agent completed a Pi-style tool loop
[compact] Context compacted after 8 messages...
events: 27
session: target/icoo_agent_session.json
```

The saved session includes `messages`, `summaries`, discovered `resources`, normalized `tool_results`, and structured `events`.
