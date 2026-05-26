# Production Native Libraries Parallel Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move Icoo native libraries toward production-grade runtime support by adding safer host boundaries, path utilities, and logging foundations.

**Architecture:** Split the first production-native-library wave into parallel tracks with explicit file ownership. Worker-owned tasks avoid editing the same high-conflict files at the same time. Shared integration files such as `src/native_modules/mod.rs`, `src/typechecker.rs`, README, and broad native module matrix tests are integrated by the main thread after worker patches are reviewed.

**Tech Stack:** Rust 2021, existing AST interpreter, native module metadata, Cargo integration tests, no new external dependencies in this wave.

---

## Current Baseline

The current native library surface includes:

- `Bytes`, `Buffer`
- `std.math`, `std.time`
- `std.json`, `std.yaml`, `std.toml`
- `std.env`, `std.io`, `std.io.fs`, `std.os`
- `std.net.http.client`, `std.net.http.server`
- `std.web.ino`

Recent work already completed:

- Brace-delimited syntax migration.
- Runtime size limits for bytes, HTTP response/stream chunks, and WebIno request bodies.
- HTTP client bytes streaming APIs.

The next production-level gap is host capability control and operational support.

## Parallel Work Rules

- Workers are not alone in the codebase; they must not revert edits made by others.
- Workers must keep their write sets narrow and listed in final output.
- Workers should run targeted tests only for their scope.
- Main thread owns final integration of shared native registry and typechecker conflicts unless explicitly delegated.
- No worker should edit `Cargo.toml` or add external dependencies in this wave.

## Worker A: Fine-Grained Permission Core

**Goal:** Extend the permission model so future native APIs can express allow-lists for paths, environment keys, and network endpoints while preserving current `AllowAll` / `DenyAll` behavior.

**Files Owned:**
- Modify: `src/runtime/permissions.rs`
- Modify: `tests/permissions_matrix.rs`

**Scope:**
- Extend `PermissionRule` conservatively to support allow lists.
- Preserve existing `RuntimePermissions::allow_all()` and `deny_all()`.
- Add resource-aware check helpers:
  - `check_fs_read_path(path, span)`
  - `check_fs_write_path(path, span)`
  - `check_fs_list_path(path, span)`
  - `check_env_read_key(key, span)`
  - `check_net_connect_endpoint(host, port, span)`
  - `check_net_listen_endpoint(host, port, span)`
- Existing coarse helpers must continue working.
- Add tests for allow-list success and denial messages.

**Verification:**

```text
cargo test --test permissions_matrix
```

## Worker B: `std.path` Native Module

**Goal:** Add a pure path utility native module so scripts stop doing unsafe string path manipulation.

**Files Owned:**
- Create: `src/native_modules/path.rs`
- Create: `tests/path_module.rs`
- Modify if needed for this worker only: `src/native_modules/mod.rs`
- Modify if needed for this worker only: `src/typechecker.rs`

**Scope:**
- Register import path `std.path`, kind `path`, type name `Path`.
- Add methods:
  - `join(base: String, child: String) -> String`
  - `normalize(path: String) -> String`
  - `dirname(path: String) -> String`
  - `basename(path: String) -> String`
  - `extension(path: String) -> String`
  - `is_absolute(path: String) -> Bool`
- Keep the module pure; it should not touch the filesystem or require permissions.
- Use standard library path APIs where possible.
- Add focused tests for Windows-style and normal local paths where behavior is stable.

**Verification:**

```text
cargo test --test path_module
cargo test --test native_modules_matrix
```

## Worker C: Runtime Logging Foundation

**Goal:** Add a logging sink abstraction in runtime so a future `std.log` module can route logs through embedders instead of hardcoding `print`.

**Files Owned:**
- Create: `src/runtime/logging.rs`
- Modify: `src/runtime/mod.rs`
- Create: `tests/logging_runtime.rs`

**Scope:**
- Define `LogLevel` with `Debug`, `Info`, `Warn`, `Error`.
- Define a `RuntimeLogRecord` with level, message, and target.
- Define a `RuntimeLogger` abstraction backed by a callback.
- Provide default behavior that does nothing unless a logger is explicitly installed.
- Keep this independent from `Interpreter` for now.
- Add Rust-level tests for level display, record construction, and callback capture.

**Verification:**

```text
cargo test --test logging_runtime
```

## Worker D: HTTPS/TLS Feasibility and API Design

**Goal:** Produce an implementation-ready design for HTTPS/TLS support without touching code or adding dependencies yet.

**Files Owned:**
- Create: `docs/plans/2026-05-26-https-tls-native-design.md`

**Scope:**
- Compare `rustls`-based and platform-native TLS options.
- Recommend one option for this repository.
- Define API compatibility for existing `std.net.http.client`:
  - `https://` URL support
  - timeout behavior
  - certificate verification
  - redirect handling as explicit future work
- Identify Cargo dependencies and test strategy for local TLS tests.

**Verification:**

```text
No cargo test required. Document must be concrete enough to implement.
```

## Main Thread Integration Plan

After worker results are reviewed:

1. Integrate Worker A permission core first.
2. Integrate Worker B `std.path` module and resolve any registry/typechecker conflicts.
3. Integrate Worker C logging foundation.
4. Keep Worker D as design input for a later HTTPS branch.
5. Run:

```text
cargo fmt --check
cargo test --test permissions_matrix
cargo test --test path_module
cargo test --test logging_runtime
cargo test
```

## Continuation Batch Completed

After the first worker wave was integrated, the main thread continued with two production hardening tasks:

1. Resource-aware permissions are now enforced by native module call sites:
   - `std.io.fs` checks requested file paths for read, write, and list operations.
   - `std.env` and `std.os` check requested environment variable keys.
   - `std.net.http.client`, `std.net.http.server`, and `std.web.ino` check concrete host/port endpoints.
   - `WebInoResponse.download()` checks the concrete file path before reading.
2. `std.log` is now exposed as a native module:
   - `debug(message: String) -> Nil`
   - `info(message: String) -> Nil`
   - `warn(message: String) -> Nil`
   - `error(message: String) -> Nil`
   - By default logs are discarded; embedders can install `RuntimeLogger` to receive structured records.

Additional verification:

```text
cargo test --test permissions_matrix
cargo test --test log_module --test native_modules_matrix
```

## HTTPS/TLS Batch Completed

The HTTPS/TLS design has been implemented for the existing `std.net.http.client` API:

- `http://` behavior remains compatible.
- `https://` is now accepted by regular, bytes, and streaming client methods.
- TLS uses `rustls` with native certificate roots by default.
- Certificate and hostname verification are enabled.
- Unsupported schemes now report `only http:// and https:// URLs are supported`.
- Network permissions check concrete HTTPS endpoints, including default port `443` and explicit custom ports.
- Local HTTPS integration tests use generated certificates and Rust-side root injection; Icoo source code does not expose TLS configuration.
- TLS client configuration is cached per interpreter instance, so native certificate roots are not reloaded for every HTTPS request.

Additional verification:

```text
cargo test --test http_client_https
cargo test --test permissions_matrix --test native_modules_matrix --test http_client_headers --test http_client_bytes
cargo test interpreter_reuses_cached_http_tls_client_config --lib
```

## Non-Goals For This Wave

- Do not implement process spawning.
- Do not add TLS dependencies yet.
- Do not add a package manager.
- Do not convert runtime internals to `Arc<Mutex>`.
- Do not expand WebIno middleware/router APIs.
