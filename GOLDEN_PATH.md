# Golden path: Ladybug primary + one demo

This is the shortest path to run HSM-II with **[LadybugDB](https://github.com/LadybugDB/ladybug)** as the **primary** on-disk graph (checkpoint + typed nodes for Cypher), instead of only the legacy single-file bincode snapshot.

## Prerequisites

- Rust toolchain, CMake, and enough disk for the `lbug` build (or prebuilt `LBUG_*` env per the `lbug` crate docs).
- Build: `cargo build --features lbug`

## One config

```bash
export HSMII_LADYBUG_PATH="$(pwd)/data/hsm_lbug_primary"
export HSMII_LADYBUG_PRIMARY=1
mkdir -p data
```

Optional: keep a bincode mirror alongside Ladybug:

```bash
export HSMII_BINCODE_MIRROR=1
```

## One demo binary

```bash
cargo run --bin hsm_golden --features lbug
```

Ad-hoc Cypher against the same path (power users / debugging):

```bash
cargo run --bin hsm_golden --features lbug -- --cypher "MATCH (b:HsmBelief) RETURN b.bid, b.confidence LIMIT 10;"
```

## Eval slices

Pre-registered suites for `hsm-eval`:

```bash
cargo run --bin hsm-eval -- --suite memory
cargo run --bin hsm-eval -- --suite tool
cargo run --bin hsm-eval -- --suite council
```

For **`hsm_meta_harness`** vs **`hsm_outer_loop`**, artifact locations, promoted-config semantics, and copy-paste smoke commands, see **[`docs/EVAL_AND_META_HARNESS.md`](docs/EVAL_AND_META_HARNESS.md)**.

## Naming

- **`HsmSqliteStore`** (crate export; deprecated alias `LadybugDb`) = bundled **SQLite** for subsystem tables via `DATABASE_URL`.
- **`lbug` / LadybugDB** = embedded **graph** engine (Cypher, future vector + FTS indices aligned with Ladybug extensions).
