# Runtime Size Limits Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add runtime size limits for Bytes, Buffer, file byte reads, HTTP client bodies, HTTP stream chunks, and WebIno request bodies.

**Architecture:** Keep limits centralized in `src/runtime/limits.rs` so native modules and interpreter helpers share one policy. Enforce limits at allocation and ingress boundaries: byte constructors, Buffer growth, file byte reads, HTTP response reads, HTTP stream chunks, and WebIno request reads. Defaults remain fixed constants for now; a future runtime config can replace these call sites without changing behavior.

**Tech Stack:** Rust 2021, existing AST interpreter, existing Cargo integration tests, no new dependencies.

---

## Defaults

- `MAX_BYTES_LEN`: 64 MiB for individual `Bytes` values and `Buffer` snapshots.
- `MAX_HTTP_BODY_BYTES`: 16 MiB for non-streaming HTTP client response bodies.
- `MAX_HTTP_STREAM_CHUNK_BYTES`: 64 KiB for each HTTP streaming chunk delivered to a handler.
- `MAX_WEB_INO_REQUEST_BYTES`: 16 MiB for a full WebIno request body.

## Task 1: Centralize Limit Constants

**Files:**
- Create: `src/runtime/limits.rs`
- Modify: `src/runtime/mod.rs`
- Test: existing compile checks

**Step 1: Create the runtime limits module**

Add constants and helper functions:

```rust
pub const MAX_BYTES_LEN: usize = 64 * 1024 * 1024;
pub const MAX_HTTP_BODY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_HTTP_STREAM_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_WEB_INO_REQUEST_BYTES: usize = 16 * 1024 * 1024;
```

Helpers should return `IcooResult<()>` for interpreter/native call sites and `Result<(), String>` for low-level WebIno request reads that do not have a `Span`.

**Step 2: Export the module**

Add `pub mod limits;` to `src/runtime/mod.rs`.

**Step 3: Verify**

Run: `cargo fmt --check && cargo test --test bytes`

Expected: all tests pass.

## Task 2: Enforce Bytes, Buffer, and File Byte Limits

**Files:**
- Modify: `src/native_modules/bytes.rs`
- Modify: `src/native_modules/io_fs.rs`
- Modify: `src/interpreter/methods.rs`
- Test: `tests/bytes.rs`

**Step 1: Write failing tests**

Add focused tests that exceed `MAX_BYTES_LEN` through:

- `Bytes.from_string(...)`
- `String.to_bytes()`
- `std.io.fs.read_bytes(...)`
- `Buffer.append(...)`
- `Bytes.concat(...)`

Expected error text should include `bytes value exceeds maximum size`.

**Step 2: Add checks**

Check size before creating or returning new byte storage. For append and concat, use checked addition to avoid overflow before comparing the limit.

**Step 3: Verify**

Run:

```text
cargo test --test bytes
cargo test --test native_modules_matrix
```

Expected: all tests pass.

## Task 3: Enforce HTTP Client Limits

**Files:**
- Modify: `src/interpreter/http_client.rs`
- Test: `tests/http_client_bytes.rs`
- Test: `tests/http_client_headers.rs`

**Step 1: Write failing tests**

Add tests for:

- non-streaming HTTP response body larger than `MAX_HTTP_BODY_BYTES`
- chunked HTTP response larger than `MAX_HTTP_BODY_BYTES`
- streaming chunk larger than `MAX_HTTP_STREAM_CHUNK_BYTES`

Expected error text should include `http response body exceeds maximum size` or `http stream chunk exceeds maximum size`.

**Step 2: Add checks**

Replace unbounded `read_to_end` with bounded incremental reads. Check decoded chunked body size as chunks are accumulated. In streaming helpers, reject a chunk before calling the handler when it exceeds the stream chunk limit.

**Step 3: Verify**

Run:

```text
cargo test --test http_client_bytes
cargo test --test http_client_headers
```

Expected: all tests pass.

## Task 4: Enforce WebIno Request Limits

**Files:**
- Modify: `src/interpreter/web_ino.rs`
- Test: `tests/web_ino_response_headers.rs`

**Step 1: Write failing test**

Add a WebIno server test that sends a request with `Content-Length` greater than `MAX_WEB_INO_REQUEST_BYTES`.

Expected behavior: WebIno returns a 400 response with body text containing `web.ino request body exceeds maximum size`.

**Step 2: Add checks**

Reject oversized `Content-Length` as soon as headers are available. Also reject when the already-read body bytes exceed the limit, even if the content length is missing or malformed.

**Step 3: Verify**

Run:

```text
cargo test --test web_ino_response_headers
```

Expected: all tests pass.

## Task 5: Full Regression

**Files:**
- All changed files

**Step 1: Format**

Run: `cargo fmt --check`

Expected: no diff.

**Step 2: Full tests**

Run: `cargo test`

Expected: all tests pass, with only the existing ignored WebIno performance test ignored.

**Step 3: Manual examples**

Run:

```text
cargo run -- examples/demo.icoo
cargo run -- examples/coroutines.icoo
cargo run -- examples/modules/main.icoo
```

Expected: examples run successfully.
