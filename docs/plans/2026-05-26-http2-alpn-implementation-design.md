# HTTP/2 and ALPN Implementation Design

## Goal

Add a production path for HTTP/2 support in `std.net.http.client` without destabilizing the current blocking HTTP/1.1 implementation.

The current client is a deliberately small blocking HTTP/1.1 transport:

- `TcpStream` for plain HTTP.
- `rustls::StreamOwned<ClientConnection, TcpStream>` for HTTPS.
- Request serialization as textual `HTTP/1.1` start line and headers.
- Response parsing by reading bytes until `\r\n\r\n`, then interpreting status, headers, `Content-Length`, chunked transfer encoding, or connection close.
- `Connection: close` request behavior.

HTTP/2 is a different wire protocol. It is not an extension of this parser.

## Decision

Keep the default HTTPS ALPN policy as HTTP/1.1 only for this implementation batch. Do not advertise `h2` until the runtime has a real HTTP/2 execution path.

The first production HTTP/2 implementation should use the `h2` crate on top of Tokio I/O, or an equivalent maintained HTTP/2 stack. It should not be implemented by adding special cases to `src/interpreter/http_client.rs`.

## Why HTTP/2 Is Not a Small Patch

HTTP/1.1 and HTTP/2 differ at the transport and parser boundary:

- HTTP/1.1 uses textual request and response lines. HTTP/2 uses binary frames.
- HTTP/1.1 response parsing can treat headers as a byte prefix ending at `\r\n\r\n`. HTTP/2 headers are HPACK-compressed header blocks carried in frames.
- HTTP/1.1 request and response bodies are sequential byte streams. HTTP/2 multiplexes many logical streams over one connection.
- HTTP/1.1 uses `Content-Length`, chunked transfer encoding, or connection close. HTTP/2 uses DATA frames, END_STREAM flags, flow-control windows, and stream resets.
- HTTP/1.1 error handling is mostly connection-level in this client. HTTP/2 has both connection-level and stream-level errors.
- HTTP/1.1 can be implemented with blocking `Read`/`Write`. The Rust `h2` ecosystem is async and expects Tokio-compatible I/O.

Because of those differences, a "minimal" HTTP/2 patch inside the current parser would either be incomplete or would hide a second protocol stack inside a function designed around blocking HTTP/1.1 text I/O. That would increase risk for existing HTTP/HTTPS behavior and make future streaming, redirects, proxy tunnels, and timeouts harder to reason about.

## ALPN Policy

Current safe behavior:

- HTTPS clients advertise only `http/1.1` when ALPN is configured.
- Plain HTTP continues to use HTTP/1.1 without ALPN.
- If no protocol is negotiated, the client treats the connection as HTTP/1.1.

Future HTTP/2 behavior:

- A runtime configuration flag should enable an `h2, http/1.1` ALPN list.
- If TLS negotiates `h2`, dispatch the request to an HTTP/2 client path.
- If TLS negotiates `http/1.1`, dispatch to the existing HTTP/1.1 client path.
- If TLS negotiates an unknown protocol, fail before writing any HTTP request bytes.

The important invariant is that the client must not advertise `h2` unless it can handle an `h2` selection.

## Proposed Architecture

Add a protocol-selection layer before request serialization:

1. Parse URL and resolve runtime HTTP settings.
2. Establish TCP and TLS.
3. Read negotiated ALPN protocol from the TLS connection.
4. Dispatch by protocol:
   - `http/1.1` or no ALPN: current blocking HTTP/1.1 path.
   - `h2`: future HTTP/2 path.
   - unknown: runtime error.

The future HTTP/2 path should be separate from `http_client.rs`, for example:

- `src/interpreter/http_alpn.rs`: protocol constants and policy helpers.
- `src/interpreter/http2_client.rs`: HTTP/2 request execution.
- `src/runtime/http_config.rs`: runtime feature flag and timeout policy, integrated by the main thread.

The HTTP/2 path should own its own response mapping into the existing language-level response shape: `status`, `headers`, `body`, `body_bytes`, `streamed`, and `chunks` where applicable.

## Dependency Impact

The production HTTP/2 implementation needs dependencies beyond this worker's scope:

- `h2` for HTTP/2 framing, HPACK, stream state, flow control, and request/response handling.
- Tokio-compatible TLS I/O. The current `rustls::StreamOwned` is blocking and implements `std::io::Read/Write`, while `h2` expects async I/O traits.
- Likely `tokio-rustls` or a small adapter layer if the project standardizes on Tokio networking.
- Possibly `http` crate types if using `h2::client::SendRequest<Bytes>` and structured request/response parts.
- A bytes buffer type such as `bytes::Bytes` if not already introduced by the chosen stack.

Although `tokio` is already present, the current HTTP client API blocks synchronously from interpreter native calls. Integration must choose one of these approaches:

- Run HTTP/2 requests inside the existing runtime event loop when called from async contexts.
- Create a scoped Tokio runtime for blocking native calls.
- Split public behavior into blocking HTTP/1.1 and async-capable HTTP/2 internals behind a common language response builder.

That decision belongs in the main integration phase, not in the ALPN helper.

## Server Negotiates `h2`

With the current default policy, this should not happen because the client does not advertise `h2`.

When HTTP/2 is enabled in the future:

- `h2` selected: call the HTTP/2 implementation.
- `http/1.1` selected: call the existing HTTP/1.1 implementation.
- no ALPN selected: call the HTTP/1.1 implementation for compatibility.
- unknown protocol selected: return an error such as `unsupported negotiated HTTP protocol: <name>`.

The client must never send an HTTP/1.1 textual request on a connection where ALPN selected `h2`.

## Proxy and Redirect Interactions

Proxy and redirect features should stay protocol-neutral at the policy layer:

- Redirect decisions operate on response status and headers after a protocol-specific request completes.
- Plain HTTP through an HTTP proxy stays HTTP/1.1 unless a future h2c design is added.
- HTTPS through an HTTP proxy first establishes `CONNECT`, then runs TLS and ALPN inside the tunnel.
- HTTP/2 over proxy tunnels requires the same ALPN dispatch after tunnel establishment.

## Test Strategy

Unit tests:

- Verify the default ALPN policy returns only `http/1.1`.
- Verify the future HTTP/2 policy returns `h2` before `http/1.1`.
- Verify negotiated protocol classification for no ALPN, `http/1.1`, `h2`, and unknown bytes.

Integration tests for the future implementation:

- Local TLS server that advertises only `http/1.1`; client completes an existing HTTPS request.
- Local TLS server that advertises `h2` and validates that the client uses HTTP/2 frames.
- Local TLS server that selects `h2` while the HTTP/2 feature is disabled; this should be impossible if the client only advertises `http/1.1`, but a direct helper test should still prove dispatch rejects forced `h2`.
- Local TLS server with no ALPN selection; client falls back to HTTP/1.1.
- Redirect and streaming tests over both HTTP/1.1 and HTTP/2 once the HTTP/2 path exists.

Use local certificates through the existing test root injection path. Avoid internet-dependent tests.

## Minimal Hook in This Batch

This batch added protocol constants and policy helpers, then wired the default policy into TLS client config construction.

The safe default is:

```text
ALPN: ["http/1.1"]
```

The future HTTP/2-enabled policy is:

```text
ALPN: ["h2", "http/1.1"]
```

Advertising order matters because the most capable supported protocol should be offered first once the implementation can actually handle it.

Current integrated behavior:

- `rustls::ClientConfig` advertises only `http/1.1`.
- The HTTPS path reads the negotiated ALPN value after the TLS handshake.
- `http/1.1` and missing ALPN continue through the existing HTTP/1.1 client.
- `h2` and unknown negotiated protocols fail before request bytes are written.
