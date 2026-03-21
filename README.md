# kv — CLI Key-Value Store in Rust

A minimal, persistent key-value store built in Rust. Data is stored as JSON on disk. No external database. No frameworks.

Built as the Phase 1 capstone project of a 6-month systems engineering roadmap.

---

## Features

- `set` — insert or update a key-value pair
- `get` — retrieve a value by key
- `delete` — remove a key
- `list` — print all stored pairs
- Persists to `kv_store.json` in the working directory
- Custom error handling — no `unwrap()` in business logic

---

## Build

```bash
git clone https://github.com/kalmastenitin/kv
cd kv
cargo build --release
```

---

## Usage

```bash
# Set a value
cargo run -- set name alice

# Get a value
cargo run -- get name
# alice

# List all keys
cargo run -- list
# key: name val: alice
# key: age val: 30

# Delete a key
cargo run -- delete name
# OK

# Get a deleted key
cargo run -- get name
# Error: Key Not Found: name
```

---

## Error Handling

All errors are typed via a custom `AppError` enum:

| Variant | Cause |
|---|---|
| `Io` | File read/write failure |
| `Json` | Corrupt or empty JSON file |
| `Parse` | Integer parse failure |
| `NotFound` | Missing key or argument |

Errors propagate via the `?` operator. `main` returns `Result<(), AppError>`.

---

## Storage Format

Data is stored as a flat JSON object in `kv_store.json`:

```json
{
  "name": "alice",
  "age": "30"
}
```

The file is read on every command invocation and written after every mutation (`set`, `delete`).

---

## What I Learned Building This

- Ownership boundaries — `&HashMap` for reads, `&mut HashMap` for writes
- `Result<T, E>` propagation with `?` across multiple error types
- `From` trait — automatic error conversion at `?` sites
- `serde_json` deserialization with graceful handling of missing/empty files
- CLI argument parsing without external crates

---

## Roadmap

- [x] Async I/O with tokio
- [ ] TTL (time-to-live) per key
- [ ] TCP server interface (Phase 2)
- [ ] Raft consensus layer (Phase 4)
- [ ] Full distributed KV store (Phase 5 capstone)