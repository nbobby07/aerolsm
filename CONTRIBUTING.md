# Contributing to AeroLSM

Thanks for being here. AeroLSM is built to be contributor-friendly: the engine is
a set of small traits, and almost every meaningful improvement is "write a better
implementation of a trait." This guide explains the architecture and the local
workflow so you can land a PR quickly.

## The mental model: traits are the architecture

Everything pluggable in AeroLSM is a trait defined in `aerolsm-core`. The engine
depends only on the traits, never on a concrete type. To extend AeroLSM you
implement a trait in (or alongside) the relevant crate.

| Trait | Defined in | Implemented in | What you'd build |
| --- | --- | --- | --- |
| [`MemTable`] | `aerolsm-core` | `aerolsm-memtable` | A faster/alternative in-memory write buffer |
| [`StorageBackend`] | `aerolsm-core` | `aerolsm-storage` | A file / `io_uring` / object-store backend |
| [`CompactionPolicy`] | `aerolsm-core` | `aerolsm-compaction` | A merge strategy (size-tiered, leveled, FIFO, ...) |

[`MemTable`]: crates/core/src/traits/memtable.rs
[`StorageBackend`]: crates/core/src/traits/storage.rs
[`CompactionPolicy`]: crates/core/src/traits/compaction.rs

### Design principles

1. **Async & zero-copy first.** Public byte payloads use `aerolsm_core::Bytes`
   (an `Arc<[u8]>`) so clones are cheap and allocations are shared. Async methods
   use native `async fn` in traits - **no `async-trait`**, no boxing on the hot
   path.
2. **Runtime-agnostic core.** `aerolsm-core` and the library crates must not
   depend on Tokio (or any runtime). A runtime is a *test/example* dependency
   only. If your library code needs `tokio::*`, reconsider.
3. **No database crates.** No `sled`, `rocksdb`, `leveldb`. We build the
   primitives ourselves. The default MemTable additionally avoids `crossbeam`
   and `rand` - just `std`.
4. **Pristine docs.** Every crate sets `#![deny(missing_docs)]`. Every public
   item needs a doc comment, and non-trivial ones should carry a runnable
   `# Example`.

## Adding an implementation: a worked example

A new `CompactionPolicy` is the smallest possible PR - the trait is a pure,
synchronous decision function, so it needs no I/O and is trivial to unit test:

```rust
use aerolsm_core::{CompactionPolicy, CompactionTask, SsTableMeta};

/// Merges level 0 whenever it accumulates four or more SSTables.
#[derive(Debug, Default)]
pub struct FourFileL0;

impl CompactionPolicy for FourFileL0 {
    fn name(&self) -> &str {
        "four-file-l0"
    }

    fn pick_compaction(&self, levels: &[Vec<SsTableMeta>]) -> Option<CompactionTask> {
        let l0 = levels.first()?;
        (l0.len() >= 4).then(|| CompactionTask {
            inputs: l0.iter().map(|m| m.id).collect(),
            output_level: 1,
        })
    }
}
```

That's the whole pattern: implement the trait, document it, add tests.

## Local development

```bash
# Build everything
cargo build --workspace

# Run the full test suite (unit, integration, doctests)
cargo test --workspace

# Formatting is enforced in CI
cargo fmt --all
cargo fmt --all --check

# Lint with warnings denied, across all targets
cargo clippy --workspace --all-targets -- -D warnings
```

### Verifying unsafe code

The only `unsafe` in the project lives in the lock-free skiplist
(`crates/memtable/src/skiplist.rs`). Every `unsafe` operation has a `// SAFETY:`
justification. If you touch it, please validate with at least one of:

```bash
# Miri: detects undefined behavior and many data races, no code changes needed.
# Tokio can't run under Miri, so target the std-only `miri` test (small counts).
rustup +nightly component add miri
MIRIFLAGS="-Zmiri-disable-isolation" \
    cargo +nightly miri test -p aerolsm-memtable --test miri

# ThreadSanitizer: detects data races at runtime on the full stress suite.
RUSTFLAGS="-Zsanitizer=thread" cargo +nightly test -p aerolsm-memtable \
    --target x86_64-unknown-linux-gnu
```

## Pull request checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [ ] `cargo test --workspace` passes
- [ ] New public items are documented (the build *denies* missing docs)
- [ ] New `unsafe`, if any, has a `// SAFETY:` comment and is Miri-clean
- [ ] Commit messages explain the *why*

## Toolchain

- **Edition:** 2024
- **MSRV:** Rust 1.85

## License

By contributing, you agree that your contributions will be dual licensed under
the MIT and Apache-2.0 licenses, as described in the [README](README.md#license).
