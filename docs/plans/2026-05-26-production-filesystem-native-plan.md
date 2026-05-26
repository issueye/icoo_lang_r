# Production Filesystem Native Library Plan

## Goal

Move `std.io.fs` from small whole-file helpers toward a production runtime filesystem surface while preserving the existing permission model.

## Completed In This Batch

- Directory operations:
  - `mkdir(path)`
  - `mkdir_all(path)`
  - `remove_dir(path)`
  - `remove_dir_all(path)`
- File operations:
  - `remove_file(path)`
  - `rename(from, to)`
  - `copy(from, to) -> Int`
- Metadata and path resolution:
  - `metadata(path) -> Map<String, Any>`
  - `symlink_metadata(path) -> Map<String, Any>`
  - `canonicalize(path) -> String`
- Symlink basics:
  - `read_link(path) -> String`
  - `create_symlink_file(target, link)`
  - `create_symlink_dir(target, link)`
- Safer writing:
  - `write_text_atomic(path, content)`
  - `write_bytes_atomic(path, content)`
- Temporary files:
  - `create_temp_file(dir, prefix) -> String`
- Chunk-oriented file I/O:
  - `read_bytes_range(path, offset, length) -> Bytes`
  - `write_bytes_at(path, offset, bytes)`

## Permission Rules

- Read-like operations use `fs.read`:
  - `exists`, `is_file`, `is_dir`, `read_text`, `read_bytes`, `read_bytes_range`
  - `metadata`, `symlink_metadata`, `canonicalize`, `read_link`
- Write-like operations use `fs.write`:
  - `write_*`, `append_*`, `mkdir*`, `remove_*`, `rename`, `create_temp_file`, symlink creation
- `copy` checks `fs.read` on the source and `fs.write` on the destination.
- `rename` checks `fs.write` on both source and destination.
- `list_dir` remains under `fs.list`.

## Remaining Work

True streaming file handles are intentionally left out of this batch. They need a runtime-owned resource table rather than raw host handles in script values.

The follow-up design should define:

- `File` handle value type or opaque resource id.
- `open(path, mode)` with explicit read/write/append/create modes.
- `read(handle, max_bytes)`, `write(handle, bytes)`, `flush(handle)`, `seek(handle, offset)`, `close(handle)`.
- Automatic cleanup when interpreter execution ends.
- Maximum open file count per interpreter.
- Behavior when user code drops a handle without closing it.
- Interaction with async tasks and concurrent access.

## Verification

Implemented tests:

- `tests/io_fs_production.rs`
- `tests/permissions_matrix.rs`
- `tests/native_modules_matrix.rs`

Expected verification:

```text
cargo fmt --check
cargo test --test io_fs_production
cargo test --test permissions_matrix
cargo test --test native_modules_matrix
cargo test
```
