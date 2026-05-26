# HTTPS/TLS Native Design

**Date:** 2026-05-26

**Owner:** Worker D

**Scope:** Design only. Do not edit Rust code or add dependencies in the production native libraries parallel wave.

## Goal

Add `https://` support to the existing synchronous `std.net.http.client` without changing the Icoo API shape. The first implementation should keep the current HTTP/1.1 request parser, response parser, bytes/text split, streaming callbacks, runtime limits, and network permission behavior. Redirect handling remains explicit future work.

Current baseline:

- `std.net.http.client` exposes `get`, `post`, `put`, `delete`, `options`, matching `*_bytes` methods, and streaming variants.
- URLs must currently start with `http://`; other schemes fail with `only http:// URLs are supported`.
- The client uses blocking `TcpStream`, `Connection: close`, HTTP/1.1, and a 5 second read timeout.
- Text methods return `body: String`; bytes methods return `body: Bytes`; streaming methods return `{ status, headers, body: "", streamed: true, chunks }`.
- The response parser supports `Content-Length`, connection-close bodies, and chunked transfer decoding.
- Network permission is checked through the current `net_connect` path before opening the socket.

## TLS Options

### Option A: rustls TLS Engine

Use `rustls` for the TLS protocol and load host trust anchors through `rustls-native-certs`.

Expected dependencies for the later implementation branch:

```toml
[dependencies]
rustls = { version = "0.23", default-features = true, features = ["std"] }
rustls-native-certs = "0.8"
```

Test-only dependencies:

```toml
[dev-dependencies]
rcgen = "0.14"
```

The implementation should confirm current compatible crate versions when the HTTPS branch is opened. Avoid adopting a pre-release TLS crate version unless the branch also accepts its Rust compiler requirements.

Pros:

- Pure Rust TLS engine; avoids OpenSSL installation and linker differences on Windows, Linux, and macOS.
- Predictable behavior across platforms.
- Fits the repo's current small synchronous client because `rustls::StreamOwned<ClientConnection, TcpStream>` implements blocking `Read` and `Write`.
- Easy to test with generated local certificates by injecting a test root store inside Rust tests.
- Does not expose platform-specific TLS types to the Icoo runtime.

Cons:

- Not the platform TLS implementation, so behavior can differ from browser/system TLS in edge cases.
- Native root loading can fail or vary by platform and must produce clear runtime errors.
- Custom root injection needs a small internal abstraction; it should not be exposed in the Icoo language API in phase 1.

### Option B: Platform-Native TLS

Use a wrapper such as `native-tls`, backed by SChannel on Windows, Security Framework on macOS, and OpenSSL on many Unix builds.

Expected dependencies if this option were chosen:

```toml
[dependencies]
native-tls = "0.2"
```

Pros:

- Uses the host TLS implementation and trust behavior.
- Aligns with enterprise certificate stores and platform policies more naturally.
- Smaller TLS policy surface inside this repository.

Cons:

- Linux builds commonly depend on OpenSSL development packages, which adds CI and user setup friction.
- Platform behavior and error text differ more, making tests and diagnostics less stable.
- Harder to make local TLS tests deterministic across all supported OSes.
- Less direct control over protocol and certificate verification details.

## Recommendation

Choose Option A: `rustls` with `rustls-native-certs`.

The repo already uses a compact Rust-native implementation style for host APIs. A rustls-based client keeps the implementation portable and testable without introducing OpenSSL as an ambient build dependency. Loading native roots gives normal users the expected system CA behavior while keeping the TLS engine independent from platform-specific APIs.

Do not add language-level TLS configuration in the first phase. The first implementation should be secure by default:

- Certificate verification enabled.
- Hostname verification enabled using the URL host.
- No `insecure_skip_verify` option.
- No custom CA path in Icoo source code.
- Local tests can inject a root store through Rust-only test helpers.

## API Compatibility

No public Icoo method names, arities, or return map shapes should change.

All existing methods must accept both `http://` and `https://` URLs:

```text
client.get(url: String, headers: Map<String, String>?) -> Map<String, Any>
client.get_bytes(url: String, headers: Map<String, String>?) -> Map<String, Any>
client.post(url: String, body: String, headers: Map<String, String>?) -> Map<String, Any>
client.post_bytes(url: String, body: Bytes, headers: Map<String, String>?) -> Map<String, Any>
client.stream_get(url: String, headers: Map<String, String>?, handler: Function) -> Map<String, Any>
client.stream_get_bytes(url: String, headers: Map<String, String>?, handler: Function) -> Map<String, Any>
```

The same rule applies to `put`, `put_bytes`, `delete`, `delete_bytes`, `options`, `options_bytes`, `stream_post`, `stream_post_bytes`, `stream_put`, `stream_put_bytes`, `stream_delete`, and `stream_options`.

Return values stay compatible:

- Text request methods keep `body` as `String`, using the current lossy UTF-8 conversion.
- Bytes request methods keep `body` as `Bytes`.
- Streaming methods keep `body: ""`, `streamed: true`, and `chunks`.
- Header names stay lowercased in the returned `headers` map.
- Existing runtime body and stream chunk size limits still apply after TLS decryption.

Unsupported schemes should fail with a new message that names both accepted schemes:

```text
only http:// and https:// URLs are supported
```

## URL Parsing

Replace the current HTTP-only parser with a scheme-aware parser that returns:

```rust
enum HttpScheme {
    Http,
    Https,
}

struct ParsedHttpUrl {
    scheme: HttpScheme,
    host: String,
    port: u16,
    path: String,
}
```

Parsing rules:

- `http://host/path` defaults to port `80`.
- `https://host/path` defaults to port `443`.
- Explicit ports override defaults for both schemes.
- Empty host remains `URL host is required`.
- Invalid port remains `URL port must be between 1 and 65535`.
- Path defaults to `/` when omitted.

The first phase can keep the current host parser limitations, including no special IPv6 bracket handling. IPv6 URL parsing should be tracked separately if needed.

## Connection Architecture

Introduce a small internal stream abstraction so the request and response code can remain shared:

```rust
enum HttpClientStream {
    Plain(std::net::TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>>),
}
```

Implement or delegate `Read` and `Write` for this enum. Then change the lower-level client helpers to operate on `&mut dyn Read` / `&mut dyn Write` or on `HttpClientStream`.

Recommended split:

1. `parse_http_url(url, span) -> ParsedHttpUrl`
2. `connect_tcp(parsed, permissions, span) -> TcpStream`
3. `wrap_tls_if_needed(parsed, tcp, span) -> HttpClientStream`
4. `write_http_request(stream, parsed, method, body, content_type, headers, span)`
5. Existing response read/decode functions consume the stream.

Keep TLS construction outside the native module dispatch layer. `src/native_modules/net_http_client.rs` should continue validating Icoo argument shapes and forwarding to interpreter HTTP helpers.

## Certificate Verification

Phase 1 behavior:

- Always verify the certificate chain.
- Always verify the server name against the URL host.
- Use the OS native root store through `rustls-native-certs`.
- If root loading fails completely, return an Icoo runtime error before connecting or before the TLS handshake. Suggested message:

```text
https client failed to load native certificate roots: <reason>
```

- If the TLS handshake or certificate verification fails, return:

```text
https client TLS handshake failed: <reason>
```

Implementation notes:

- Convert the parsed host into a `rustls::pki_types::ServerName`.
- Reject invalid DNS names before handshake with:

```text
invalid HTTPS server name
```

- Do not use the `Host` header value for TLS verification; verification must use the URL host.
- Do not allow request headers to override certificate verification.

Future, not phase 1:

- Runtime-level custom CA bundles for embedders.
- Per-runtime certificate store caching.
- Certificate pinning.
- An explicit insecure mode for local debugging, if ever accepted, gated behind runtime configuration rather than Icoo source code.

## Timeout Behavior

Preserve the current timeout contract as closely as possible:

- Keep a 5 second read timeout on the underlying `TcpStream`.
- Also set a 5 second write timeout when HTTPS is implemented, because the TLS handshake writes before the HTTP request body is sent.
- The TLS handshake uses the same socket timeouts; there is no separate public handshake timeout.
- Existing response body and stream read operations continue to use socket read timeout behavior.

Errors should keep the existing categories where possible:

- TCP connect failure: `http client connection failed: <reason>`
- Plain HTTP write failure: `http client write failed: <reason>`
- HTTPS write failure after handshake: keep `http client write failed: <reason>`
- TLS handshake failure: `https client TLS handshake failed: <reason>`
- HTTPS read failures after handshake: keep existing `http client read failed` or `http client stream read failed` wording from the response reader.

Future runtime configuration can make the timeout adjustable, but no Icoo API parameter should be added in phase 1.

## Redirect Handling

Redirect handling is explicit future work. Phase 1 must not automatically follow `3xx` responses.

Behavior in phase 1:

- Return the status and headers exactly as received.
- If a `Location` header is present, it appears in the existing lowercased `headers` map as `location`.
- Do not issue a second request.
- Do not rewrite methods.
- Do not apply extra permission checks beyond the original connection.

Future redirect design must define:

- Maximum redirect count.
- Method rewrite behavior for `301`, `302`, `303`, `307`, and `308`.
- Relative `Location` resolution.
- Cross-host and cross-scheme permission checks.
- Whether `Authorization` and `Cookie` headers are stripped on cross-origin redirects.
- Streaming request behavior on redirects.

## Permissions

HTTPS uses the same network permission class as HTTP.

When Worker A's resource-aware permission helpers are integrated, the HTTP client should check the parsed endpoint:

```rust
permissions.check_net_connect_endpoint(&parsed.host, parsed.port, span)?;
```

Until then, preserve the current coarse check:

```rust
permissions.check_net_connect(span)?;
```

For `https://example.com/`, the endpoint is `example.com:443`. For `https://example.com:8443/`, the endpoint is `example.com:8443`.

## Header Behavior

Keep existing header behavior:

- Continue rejecting CR or LF in header names and values.
- Continue adding `Host`, `Connection: close`, `Content-Length`, and `Content-Type` as currently done.
- Keep custom headers after built-in headers.

Recommended phase 1 adjustment:

- Include the port in the `Host` header only when the URL explicitly includes a non-default port.
- For default `http:80` and `https:443`, keep `Host: <host>`.

Do not add ALPN, HTTP/2, compression, cookies, or proxy support in phase 1.

## Test Strategy

No Cargo tests are required for this design-only task. The implementation branch should add focused tests instead of relying on public internet hosts.

### Local TLS Server Tests

Create a new integration test file such as `tests/http_client_https.rs`.

Use `rcgen` to generate a local certificate for `localhost` and/or `127.0.0.1`, then start a local TLS server with `rustls::ServerConnection` over `TcpListener`.

Test cases:

- `client.get("https://localhost:<port>/hello")` returns `status == 200` and expected text body.
- `client.get_bytes("https://localhost:<port>/bin")` returns `body` as `Bytes` and preserves arbitrary bytes.
- `client.post("https://localhost:<port>/submit", "payload")` sends the expected request line, `Host`, `Content-Length`, and body.
- `client.stream_get("https://localhost:<port>/stream", on_chunk)` handles chunked responses.
- `client.stream_get_bytes(...)` passes raw `Bytes` chunks.
- `https://localhost:<port>/` defaults to TLS and does not send cleartext HTTP.
- `https://localhost/` parses default port `443`; avoid binding to 443 in tests unless available.
- Invalid certificate/root mismatch fails with `https client TLS handshake failed`.
- Hostname mismatch fails when the certificate is valid for `localhost` but the URL uses another name.
- `http://` tests continue to pass unchanged.
- Unsupported schemes such as `ftp://...` produce `only http:// and https:// URLs are supported`.

To avoid exposing test-only certificate configuration to Icoo code, add an internal Rust helper that accepts a custom `RootCertStore` under `#[cfg(test)]` or through a private test constructor.

### Existing Test Updates

Update the current registry test that expects `https://example.invalid/` to fail with HTTP-only support. After HTTPS lands, replace it with an unsupported scheme assertion:

```text
client.get("ftp://example.invalid/")
```

Keep existing HTTP header, bytes, stream, permissions, and native module matrix tests. Add HTTPS variants only where TLS behavior changes the connection layer.

### Network Independence

Do not test against `https://example.com` or any public endpoint. Tests must be local and deterministic:

- Bind `127.0.0.1:0` or `localhost:0`.
- Use short socket timeouts.
- Join server threads at the end of every test.
- Avoid depending on the machine's root store for positive tests; inject the generated root certificate into the test client path.

## Risks

- `rustls-native-certs` can return different roots or errors across OSes. Mitigation: positive tests use an injected root store; production errors mention certificate root loading clearly.
- Server name handling can reject IP literals or unusual hosts. Mitigation: support DNS names first; document IPv6/IP SAN behavior as follow-up if local tests require it.
- Adding TLS may tempt a larger HTTP client rewrite. Mitigation: keep the existing HTTP/1.1 parser and only abstract the stream boundary.
- Root store loading on every request may be slow. Mitigation: phase 1 may accept it for simplicity; phase 2 can cache `ClientConfig` in runtime state.
- Blocking TLS handshakes can stall interpreter execution. This matches the existing blocking network model and should be revisited only with a broader async runtime design.
- Error messages from TLS libraries may vary. Tests should assert stable prefixes, not full third-party error text.

## Phased Implementation

### Phase 0: Design Only

Files:

- Create `docs/plans/2026-05-26-https-tls-native-design.md`

Verification:

- No Cargo test required.

### Phase 1: Scheme Parsing and Stream Abstraction

Likely files:

- Modify `src/interpreter/http_client.rs`
- Test `tests/http_client_headers.rs` or new focused parser-facing tests
- Test `tests/native_modules_matrix.rs`

Tasks:

1. Replace `ParsedHttpUrl` with scheme-aware parsing.
2. Accept `http://` and `https://`; keep `http://` behavior unchanged.
3. Introduce the internal stream abstraction while still only opening plain HTTP.
4. Update unsupported scheme error text.
5. Keep existing tests green.

### Phase 2: rustls Connection Path

Likely files:

- Modify `Cargo.toml`
- Modify `src/interpreter/http_client.rs`
- Create `tests/http_client_https.rs`

Tasks:

1. Add `rustls` and `rustls-native-certs`.
2. Load native root certificates into a `RootCertStore`.
3. Build a `ClientConfig`.
4. Wrap `TcpStream` with `ClientConnection` for HTTPS.
5. Reuse the existing request writer and response reader over the TLS stream.
6. Add local HTTPS tests with generated certificates and private test root injection.

### Phase 3: Timeout and Error Polish

Likely files:

- Modify `src/interpreter/http_client.rs`
- Extend `tests/http_client_https.rs`

Tasks:

1. Add write timeout alongside the existing read timeout.
2. Stabilize runtime error prefixes for root loading, invalid server name, and handshake failure.
3. Add tests for certificate mismatch and unsupported schemes.

### Phase 4: Permission Integration

Likely files after Worker A integration:

- Modify `src/interpreter/http_client.rs`
- Extend `tests/permissions_matrix.rs`

Tasks:

1. Switch from coarse `check_net_connect` to endpoint-aware checks.
2. Verify `https` default port `443` is checked.
3. Verify explicit HTTPS ports are checked.
4. Preserve `http` default port `80` behavior.

### Phase 5: Future Enhancements

Separate designs or branches:

- Redirect following.
- Runtime timeout configuration.
- Runtime TLS configuration for embedders.
- Cached `ClientConfig`.
- IPv6 URL parsing.
- HTTP proxy support.
- HTTP/2 and ALPN.
