# Fuzz seeds

The repository has two generated seed trees:

- Root [`seeds/`](.) contains minimal inputs for submodule parser targets.
- [`crates/cadmpeg-fuzz/seeds/`](../crates/cadmpeg-fuzz/seeds/) contains container, IR, and integration-target inputs.

All files in both trees are synthesized by generator code in [`crates/cadmpeg-fuzz/src/bin/`](../crates/cadmpeg-fuzz/src/bin/). They contain no bytes carved from CAD application output, vendor samples, customer files, or other third-party CAD files. They are parser inputs, not public CAD corpus files. JSON seeds, malformed inputs, empty inputs, and truncated inputs are not CAD container files at all. Public CAD fixtures enter the repository only through the [corpus donation process](../corpus/README.md).

## Generators

The fuzz crate's seed generator binaries include:

- `generate_submodule_seeds` writes minimal inputs for focused F3D, SolidWorks, CATIA, Creo, and NX parser targets.
- `generate_all_seeds` writes the main container and `ir_from_json` seeds, then creates deterministic mutations of eligible seeds.
- `generate_seeds` writes the focused `f3d_container` set, including pcurve and BinaryFile4 inputs.
- `generate_comprehensive_seeds` writes the expanded SolidWorks, CATIA, Creo, and NX container sets. It does not regenerate F3D seeds.
- `generate_fcstd_seeds`, `generate_rhino_seeds`, and `generate_iges_seeds` write their format-specific seed sets.

Generator output paths are relative to the current working directory. Use the required working directories below.

From the repository root, regenerate root `seeds/`:

```sh
cargo run --manifest-path crates/cadmpeg-fuzz/Cargo.toml --bin generate_submodule_seeds
```

From `crates/cadmpeg-fuzz`, regenerate the fuzz-crate seed sets:

```sh
cd crates/cadmpeg-fuzz
cargo run --bin generate_all_seeds
cargo run --bin generate_seeds
cargo run --bin generate_comprehensive_seeds
```

The fuzz-crate generators overlap for some container targets. Later commands can replace base files written by earlier commands, while target-specific files remain. `generate_all_seeds` owns the IR seeds and creates mutations only from the base files present when that binary runs. It appends:

- `.mut_trunc` for a 50 percent truncation.
- `.mut_flip` for one inverted byte at the midpoint.
- `.mut_lenmax` for four `0xff` bytes at the quarter-length offset.

Files shorter than 32 bytes do not receive generated mutations.

Running `generate_submodule_seeds` from `crates/cadmpeg-fuzz` would create a second submodule seed tree there. Run it from the repository root to reproduce the committed layout.

## Running fuzz targets

The fuzz crate is excluded from the stable workspace because libFuzzer targets require Rust nightly and `cargo-fuzz`. Install the prerequisites:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked
```

The scheduled fuzz smoke workflow performs a compile-only build from the repository root:

```sh
cargo +nightly fuzz build --fuzz-dir crates/cadmpeg-fuzz
```

Run a container target with its fuzz-crate seeds from the repository root:

```sh
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz f3d_container crates/cadmpeg-fuzz/seeds/f3d_container
```

Run a submodule target with the root seeds:

```sh
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz f3d_asm_header seeds/f3d_asm_header
```

Replace the target and seed directory with another matching pair declared in [`crates/cadmpeg-fuzz/Cargo.toml`](../crates/cadmpeg-fuzz/Cargo.toml).
