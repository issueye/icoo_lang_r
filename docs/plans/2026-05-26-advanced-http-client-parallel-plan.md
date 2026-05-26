# Advanced HTTP Client Parallel Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extend `std.net.http.client` beyond basic HTTP/HTTPS by adding redirect support, runtime-configurable timeouts, proxy foundations, complete IPv6 URL parsing, and an HTTP/2/ALPN implementation path.

**Architecture:** Split high-conflict work into low-conflict helper modules first. Workers own focused files under `src/interpreter/` or `src/runtime/` plus dedicated tests. The main thread integrates helpers into the existing `src/interpreter/http_client.rs` request path after worker results land.

**Tech Stack:** Rust 2021, blocking HTTP/1.1 client, rustls 0.23, rustls-native-certs, local integration tests.

---

## Current Baseline

Already completed in this branch:

- `std.net.http.client` supports `http://` and `https://`.
- TLS uses rustls with certificate and hostname verification.
- TLS `ClientConfig` is cached per interpreter instance.
- Network permissions check concrete endpoints.
- Existing APIs return the same response map shape.

## Parallel Work Rules

- Workers are not alone in the codebase and must not revert edits made by others.
- Workers should avoid editing `src/interpreter/http_client.rs` unless explicitly assigned.
- Main thread owns integration into `src/interpreter/http_client.rs`, `src/interpreter/mod.rs`, and public lib helpers.
- Workers must list changed files and tests run.

## Worker A: IPv6 URL Parser

**Goal:** Replace ad hoc host/port parsing with a robust parser for `http://`, `https://`, bracketed IPv6, empty hosts, default ports, explicit ports, and path/query preservation.

**Files Owned:**
- Create: `src/interpreter/http_url.rs`
- Create or modify focused tests inside that module only.

**Scope:**
- Define `HttpScheme` and `ParsedHttpUrl`.
- Parse:
  - `http://example.com/path`
  - `https://example.com`
  - `http://[::1]:8080/path`
  - `https://[2001:db8::1]/`
- Preserve `path` including query string after `/`.
- Provide `host_header()` that brackets IPv6 and includes non-default ports.
- Provide `connect_host()` suitable for `TcpStream::connect`.
- Keep unsupported scheme error text stable.

**Verification:**

```text
cargo test http_url --lib
```

## Worker B: Runtime HTTP Client Configuration

**Goal:** Add runtime configuration for HTTP client timeouts and redirect/proxy policy without exposing incomplete language-level APIs.

**Files Owned:**
- Create: `src/runtime/http_config.rs`
- Modify: `src/runtime/mod.rs`
- Create: `tests/http_runtime_config.rs`

**Scope:**
- Define `RuntimeHttpConfig`.
- Fields:
  - `connect_timeout: Duration`
  - `read_timeout: Duration`
  - `write_timeout: Duration`
  - `max_redirects: usize`
  - `proxy: Option<HttpProxyConfig>`
- Defaults:
  - connect/read/write timeout: 5 seconds
  - max redirects: 0 initially, so current behavior remains unless main integration enables it.
  - proxy: none
- Add constructors/builders for embedder use.
- Do not wire into `Interpreter` in the worker; main thread integrates.

**Verification:**

```text
cargo test --test http_runtime_config
```

## Worker C: Redirect Policy Helper

**Goal:** Implement redirect decision logic independent of socket I/O.

**Files Owned:**
- Create: `src/interpreter/http_redirect.rs`
- Add module-local tests only.

**Scope:**
- Define redirect status detection for `301`, `302`, `303`, `307`, `308`.
- Extract lowercased `location` header from response headers.
- Resolve absolute redirect targets.
- Resolve relative paths against the previous URL once Worker A's parser lands, or accept string inputs with a narrow helper if avoiding dependency.
- Enforce max redirect count.
- Define method rewrite rules:
  - `303` becomes `GET`.
  - `301`/`302` rewrite `POST` to `GET` for browser-compatible behavior.
  - `307`/`308` preserve method and body.

**Verification:**

```text
cargo test http_redirect --lib
```

## Worker D: Proxy Foundation

**Goal:** Add proxy configuration and request-building helpers without changing current default behavior.

**Files Owned:**
- Create: `src/interpreter/http_proxy.rs`
- Add module-local tests only.

**Scope:**
- Support HTTP proxy configuration shape:
  - proxy host
  - proxy port
  - optional basic auth header string
- Build absolute-form request targets for plain HTTP through proxy.
- Build `CONNECT host:port HTTP/1.1` request for HTTPS tunnel establishment.
- Validate proxy host/port.
- Do not read environment variables in this worker.
- Do not integrate sockets in this worker; main thread wires it after tests.

**Verification:**

```text
cargo test http_proxy --lib
```

## Worker E: HTTP/2 and ALPN Design/Minimal Hook

**Goal:** Define the production path for HTTP/2 without destabilizing the current blocking HTTP/1.1 client.

**Files Owned:**
- Create: `docs/plans/2026-05-26-http2-alpn-implementation-design.md`
- Optionally create: `src/interpreter/http_alpn.rs` with ALPN protocol constants and tests if useful.

**Scope:**
- Decide whether first implementation should use `h2` + Tokio or keep HTTP/1.1 only with ALPN disabled.
- Identify dependency and architecture impact.
- Define behavior when server negotiates `h2`.
- Define test strategy with local TLS ALPN server.
- If adding code, keep it limited to constants/helpers; no full HTTP/2 client in this batch.

**Verification:**

```text
cargo test http_alpn --lib
```

## Main Thread Integration Plan

After workers finish:

1. [x] Integrate Worker A parser into `http_client.rs`.
2. [x] Integrate Worker B config into `Interpreter` and public embedder constructors.
3. [x] Integrate Worker C redirects with default `max_redirects = 0` unless the embedder opts in.
4. [x] Integrate Worker D proxy for plain HTTP absolute-form requests and HTTPS `CONNECT` tunnels.
5. [x] Keep Worker E HTTP/2 as design/minimal hook; advertise only `http/1.1` by default.
6. [x] Add integration tests for redirects, plain HTTP proxy, HTTPS proxy tunnels, and runtime config.
7. [ ] Run the full final verification suite:

```text
cargo fmt --check
cargo test --test http_client_headers
cargo test --test http_client_https
cargo test --test http_client_redirects
cargo test --test http_client_proxy
cargo test --test http_runtime_config
cargo test --test permissions_matrix
cargo test
```

## Integration Status

Completed in the main integration pass:

- `src/interpreter/http_client.rs` now uses `ParsedHttpUrl` for default ports, bracketed IPv6, path/query preservation, host header formatting, and socket authority formatting.
- `RuntimeHttpConfig` is stored on `Interpreter` and exposed through embedder helpers in `src/lib.rs`.
- Connect, read, and write timeouts come from `RuntimeHttpConfig`.
- Non-streaming client requests can follow redirects when `max_redirects > 0`; default `0` preserves the old behavior.
- Plain HTTP proxy requests use absolute-form request targets.
- HTTPS proxy requests establish a `CONNECT` tunnel, then run rustls and ALPN inside the tunnel.
- TLS config advertises `http/1.1` through the ALPN helper and rejects unsupported negotiated protocols before sending request bytes.

Remaining production work outside this batch:

- Full HTTP/2 client implementation using a real HTTP/2 stack.
- Language-level HTTP client configuration API, if scripts need to opt into redirects/proxy/timeouts without embedder configuration.
- Connection pooling and keep-alive.
- Response decompression for `gzip`, `deflate`, and `br`.
- Cookie jar, multipart request bodies, and richer request options.
