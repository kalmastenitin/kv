# kv — Key-Value Store in Rust

A Cargo workspace containing three implementations built across Phases 1 and 2 of a 6-month systems engineering roadmap.

---

## Projects

### `cli-kv` — Phase 1
A minimal, persistent CLI key-value store. Data is stored as JSON on disk. No external database. No frameworks.

### `tcp-kv` — Phase 2
An in-memory key-value store exposed over a raw TCP socket. Multi-threaded with a fixed thread pool. No frameworks.

### `epoll-server` — Phase 2
A single-threaded event-driven TCP server using `mio` (wraps epoll on Linux, kqueue on macOS). Handles thousands of connections with no thread pool.

---

## Build

```bash
git clone https://github.com/kalmastenitin/kv
cd kv
cargo build --release
```

---

## cli-kv Usage

```bash
cd cli-kv

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

## tcp-kv Usage

```bash
cd tcp-kv
cargo run
# Listening on port 8080

# In another terminal — use nc to send commands
echo "set name alice" | nc 127.0.0.1 8080
# HTTP/1.1 200 OK
# OK

echo "get name" | nc 127.0.0.1 8080
# HTTP/1.1 200 OK
# alice

echo "list" | nc 127.0.0.1 8080
# HTTP/1.1 200 OK
# name=alice

echo "get missing" | nc 127.0.0.1 8080
# HTTP/1.1 200 OK
# missing Not Found
```

---

## epoll-server Usage

```bash
cd epoll-server
cargo run
# Listening on 127.0.0.1:8080

# In another terminal
echo "hello" | nc 127.0.0.1 8080
# Hello

echo "get missing" | nc 127.0.0.1 8080
# Hello
```

---

## Architecture

### cli-kv
- Reads `kv_store.json` on every invocation
- Writes back on every mutation (`set`, `delete`)
- Custom `AppError` enum with `From` conversions
- `?` operator for error propagation throughout
- Async file I/O via tokio

### tcp-kv
- Fixed thread pool (4 workers) — no unbounded thread spawning
- `Arc<Mutex<Receiver>>` — shared channel across worker threads
- `Arc<Mutex<HashMap>>` — shared in-memory store across connections
- Raw TCP socket — `bind` → `listen` → `accept` → `read` → `write`
- In-memory only — data lives as long as the server runs

### epoll-server
- Single thread, event loop — no thread pool
- `mio::Poll` wraps epoll (Linux) and kqueue (macOS) transparently
- `Token` system — each connection gets a unique integer ID
- `HashMap<Token, TcpStream>` — look up connection by token when event fires
- Kernel does all the waiting — thread only runs when I/O is ready

---

## Thread Pool vs Event Loop

| | tcp-kv (thread pool) | epoll-server (event loop) |
|---|---|---|
| Concurrency model | 4 OS threads | 1 thread, kernel events |
| Memory per connection | ~8MB stack | ~KB |
| Max connections | bounded by thread count | tens of thousands |
| CPU parallelism | yes | no (single threaded) |
| Best for | CPU-heavy work | I/O-heavy, many connections |

Real servers (nginx, tokio) combine both: event loop per core, work distributed across threads.

---

## Error Handling

All projects use a custom `AppError` enum:

| Variant | Cause |
|---|---|
| `Io` | File read/write failure |
| `Json` | Corrupt or empty JSON file |
| `Parse` | Integer parse failure |
| `NotFound` | Missing key or argument |

---

## What I Learned

**Phase 1 — cli-kv**
- Ownership boundaries — `&HashMap` for reads, `&mut HashMap` for writes
- `Result<T, E>` propagation with `?` across multiple error types
- `From` trait — automatic error conversion at `?` sites
- `serde_json` deserialization with graceful handling of missing/empty files
- CLI argument parsing without external crates
- Async file I/O with tokio

**Phase 2 — tcp-kv + epoll-server**
- Syscall chain — `socket` → `bind` → `listen` → `accept`
- Thread pool — fixed workers, `Arc<Mutex<Receiver>>` channel distribution
- `Arc<Mutex<T>>` — shared mutable state across OS threads
- Mutex vs Atomic — when to use each, LOAD/ADD/STORE data races
- Memory ordering — `Release`/`Acquire` pair, why CPUs reorder instructions
- epoll/kqueue — event-driven I/O, token-based connection tracking
- Why blocking I/O doesn't scale — stacks, context switching, kernel scheduling

---

## Roadmap

- [x] CLI KV store with JSON persistence (Phase 1)
- [x] Async I/O with tokio (Phase 1)
- [x] TCP server with thread pool (Phase 2)
- [x] Mutexes and atomics (Phase 2)
- [x] epoll event loop with mio (Phase 2)
- [ ] TTL (time-to-live) per key
- [ ] CPU cache optimization (Phase 3)
- [ ] Flamegraph profiling + criterion benchmarking (Phase 3)
- [ ] 10x KV store optimization (Phase 3 project)
- [ ] Raft consensus layer (Phase 4)
- [ ] Full distributed KV store with WAL (Phase 5 capstone)