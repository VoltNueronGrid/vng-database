# S8 Memory Allocator Strategy

**Sprint:** S8 — Scale and Performance Proof Phase 1
**Status:** Draft / Under Review
**Owner:** VoltNueronGrid Platform Team

---

## Current State

VoltNueronGrid uses the **default system allocator** provided by the Rust standard
library, which delegates to the OS-level allocator (`ptmalloc2` on Linux/glibc,
`libmalloc` on macOS).

### Observed characteristics

| Property | System allocator (ptmalloc2) |
|---|---|
| Long-lived service fragmentation | High — arenas can grow unboundedly |
| Per-thread caching | Limited; contention on global lock under high thread counts |
| Profiling hooks | None built-in |
| Binary size impact | None (zero-cost, part of libc) |

For short-lived CLI tools and tests the system allocator is adequate. For the
long-lived `voltnuerongridd` daemon processing millions of rows per day,
fragmentation and lock contention become measurable.

---

## Recommended Allocator: jemalloc

[jemalloc](http://jemalloc.net/) is a general-purpose allocator designed for
**fragmentation reduction** and **multi-threaded scalability**. It is used in
production by Firefox, Redis, MySQL, Cassandra, and many Rust services.

### Key advantages for `voltnuerongridd`

| Property | jemalloc |
|---|---|
| Fragmentation | Significantly reduced via size-class bins and background GC |
| Thread-local caches | Per-thread arenas avoid global lock contention |
| Memory profiling | Built-in epoch-based stats (heap bytes, fragmentation ratio) |
| Compatibility | Crate `tikv-jemallocator` ships a ready-to-use Rust binding |

---

## Migration Path

### Step 1 — Add the crate as an optional feature

In `services/voltnuerongridd/Cargo.toml`:

```toml
[features]
default = []
jemalloc = ["dep:tikv-jemallocator"]

[dependencies]
tikv-jemallocator = { version = "0.6", optional = true }
```

### Step 2 — Register the global allocator behind the feature flag

In `services/voltnuerongridd/src/main.rs`:

```rust
#[cfg(feature = "jemalloc")]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
```

### Step 3 — Enable in production builds

```bash
cargo build --release --features jemalloc
```

Keep the feature **optional** so that:
- Developer builds remain zero-dep (no C compilation overhead).
- Tests continue to use the system allocator.
- Feature can be toggled via environment-specific CI profiles.

### Step 4 — Validate with profiling (see below)

---

## Profiling Approach

### heaptrack (Linux, recommended)

```bash
# Install
sudo apt-get install heaptrack heaptrack-gui

# Profile a 1M-row ingest run
heaptrack ./target/release/voltnuerongridd --ingest large_dataset.csv

# Visualise
heaptrack_gui heaptrack.voltnuerongridd.*.gz
```

Captures per-allocation stack traces and produces a flame-graph annotated with
peak heap usage.

### Valgrind Massif (Linux/macOS)

```bash
valgrind --tool=massif --pages-as-heap=yes \
  ./target/release/voltnuerongridd --ingest large_dataset.csv

ms_print massif.out.<pid> | head -60
```

Useful when heaptrack is unavailable; slower (10-20x overhead) but requires no
recompilation.

### jemalloc epoch stats (in-process)

When compiled with jemalloc, the `tikv-jemalloc-ctl` crate exposes epoch-based
counters without external tooling:

```rust
use tikv_jemalloc_ctl::{epoch, stats};

epoch::mib().unwrap().advance().unwrap();
let allocated = stats::allocated::mib().unwrap().read().unwrap();
let resident  = stats::resident::mib().unwrap().read().unwrap();
println!("allocated={allocated} resident={resident}");
```

This is the hook that `MemoryProfiler` (see `tests/benchmarks/src/memory_profile.rs`)
will call in a follow-on sprint once the jemalloc feature is stabilised.

---

## Decision Log

| Date | Decision | Rationale |
|---|---|---|
| S8 | Keep system allocator as default | No fragmentation data yet; change carries risk |
| S8 | Add `jemalloc` as opt-in feature | Enables A/B comparison without forcing all users |
| Post-S8 | Re-evaluate after 30-day soak | Collect heaptrack data under production-representative load |
