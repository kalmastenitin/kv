# kv — Key-Value Store in Rust
 
A Cargo workspace containing three implementations built across Phases 1, 2, and 3 of a 6-month systems engineering roadmap.
 
---
 
## Projects
 
### `cli-kv` — Phase 1
A minimal, persistent CLI key-value store. Data is stored as JSON on disk. No external database. No frameworks. Optimized in Phase 3 using criterion benchmarking and profiling.
 
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
```
 
---
 
## Architecture
 
### cli-kv
- Reads `kv_store.json` on every invocation
- Writes back on every mutation (`set`, `delete`)
- Custom `AppError` enum with `From` conversions
- `?` operator for error propagation throughout
- Async file I/O via tokio
- Zero-copy `get` via string search — no deserialization
 
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
 
## Phase 3 — Optimization Journey
 
All measurements on 1000-key JSON store. Run benchmarks with `cargo bench` from `cli-kv/`.
 
| Approach | Time | vs Baseline |
|---|---|---|
| Baseline — full JSON deserialize | 84µs | 1x |
| String search + owned String | 9.6µs | 8.8x |
| String search + `&str` (zero-copy) | 9.7µs | 8.6x |
| mmap + `&str` | 9.6µs | 8.8x |
| SIMD search (memchr) + mmap | 9.9µs | 8.5x |
| Stack-allocated needle (ArrayString) | 10.2µs | 8.2x |
 
**Key findings:**
- Eliminating deserialization gave 8.8x speedup — the biggest single win
- mmap is slower than `read_to_string` for files under ~1MB — syscall overhead dominates
- SIMD search is 8x faster in isolation but Amdahl's Law limits the full-operation gain — file I/O is the bottleneck
- Stack allocation for needle is 28x faster in isolation, but contributes 0.15% of total operation time
- Profiling (samply flamegraph) showed `accept()` dominates under load — bottleneck is connection rate, not HashMap or mutex
 
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
 
**Phase 3 — performance engineering**
- Criterion benchmarking — statistical, reproducible measurements
- Profiling with samply — flamegraphs on macOS without Xcode
- CPU cache hierarchy — L1/L2/L3/RAM latencies, 64-byte cache lines
- Cache locality — flat layout 3.2x faster than nested vec, 10x row vs col major
- Branch prediction — compiler emits `cmov`, eliminates branches automatically
- SIMD — `memchr` uses hardware vector instructions, 8x faster string search
- Amdahl's Law — optimizing 20% of runtime gives at most 20% total improvement
- Allocator tuning — stack allocation 28x faster than heap for small strings
 
---
 
## Roadmap
 
- [x] CLI KV store with JSON persistence (Phase 1)
- [x] Async I/O with tokio (Phase 1)
- [x] TCP server with thread pool (Phase 2)
- [x] Mutexes and atomics (Phase 2)
- [x] epoll event loop with mio (Phase 2)
- [x] Criterion benchmarking (Phase 3)
- [x] Flamegraph profiling (Phase 3)
- [x] CPU cache + SIMD + allocator optimization (Phase 3)
- [ ] TTL (time-to-live) per key
- [x] Raft consensus layer (Phase 4)
- [ ] LSM tree storage engine (Phase 4)
- [x] Write-ahead log (Phase 4)
- [ ] Full distributed KV store with Raft + WAL (Phase 5 capstone)