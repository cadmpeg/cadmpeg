# cadmpeg-fuzz

`cadmpeg-fuzz` contains libFuzzer harnesses for cadmpeg codecs, intermediate
representation (IR) operations, and STEP export. It is an unpublished,
standalone Cargo workspace because `cargo-fuzz` requires nightly Rust and
fuzz-specific build settings.

Run commands from the repository root:

```sh
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz f3d_container
```

The campaign runs until it finds a failure or receives a libFuzzer limit.
Bound a local check by placing libFuzzer options after `--`:

```sh
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz f3d_container -- -runs=1000
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz f3d_container -- -max_total_time=60
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz f3d_writer -- -runs=1000
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz f3d_roundtrip -- -runs=1000
```

Pass one or more corpus directories between the target and `--`. The checked-in
seeds use one directory per target:

```sh
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz \
  f3d_container crates/cadmpeg-fuzz/seeds/f3d_container
```

`cargo-fuzz` stores discovered failures under
`crates/cadmpeg-fuzz/artifacts/<target>/`. Reproduce one by passing its path as
a corpus argument:

```sh
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz \
  f3d_container crates/cadmpeg-fuzz/artifacts/f3d_container/<artifact>
```

Do not add crash artifacts to `seeds/` without reducing and classifying them.
Seed files should be small inputs that reach a distinct parser state. Keep each
seed in the directory named for its target.

## Choosing a target

Start with a container target when testing an end-to-end codec path. These
harnesses call format detection, inspection, and decoding:

- `f3d_container`
- `fcstd_container`, `fcstd_decode`, `fcstd_write`
- `sldprt_container`
- `catia_container`
- `creo_container`
- `nx_container`
- `iges_container`

Use the F3D semantic targets for native writing and replay:

- `f3d_writer` parses IR, generates a source-less archive, inspects it, and
  decodes it.
- `f3d_roundtrip` decodes an archive, replays it through the native writer, and
  decodes the result.

Use a parser target for focused binary-format coverage:

- F3D: `f3d_asm_header`, `f3d_sab_frame`, `f3d_nurbs_surfaces`,
  `f3d_nurbs_curves`, `f3d_nurbs_pcurves`
- SolidWorks: `sldprt_parasolid`, `sldprt_container_scan`
- CATIA: `catia_geometry_vertices`, `catia_geometry_surfaces`,
  `catia_a8_surfaces`, `catia_a5_surfaces`, `catia_b5`, `catia_e5`,
  `catia_zero_entity`, `catia_container_dir`, `catia_e5_orientation`
- Creo: `creo_psb_tokens`, `creo_compact_int`, `creo_short_form_float`,
  `creo_container_scan`, `creo_surface_rows`, `creo_curve_prototypes`
- NX: `nx_parasolid`, `nx_geometry_points`, `nx_geometry_surfaces`,
  `nx_geometry_curves`, `nx_nurbs_surfaces`, `nx_nurbs_curves`
- FCStd: `fcstd_xml`, `fcstd_gui`, `fcstd_brep`, `fcstd_element_map`,
  `fcstd_auxiliary`
- IGES uses `iges_container` for bounded representation detection, physical-card parsing,
  Global and Directory fields, Parameter tokens, reference graphs, semantic geometry, and
  topology projection. Generate its valid 5.3 point and trimmed-sheet
  seeds with `cargo run --bin generate_iges_seeds` from `crates/cadmpeg-fuzz`.

Use an IR or STEP target when the input is JSON or the behavior under test is
format-independent:

- `ir_from_json` parses a `CadIr` document.
- `ir_validate` parses and validates a `CadIr` document.
- `ir_diff` splits the input into two JSON documents and computes their
  structural diff.
- `ir_canonical_roundtrip` serializes parsed IR to canonical JSON and parses it
  again.
- `step_writer` parses IR and writes STEP with default options.
- `step_writer_custom` derives STEP header fields from an eight-byte prefix,
  then parses the remaining bytes as IR.
- `step_lexer` tokenizes arbitrary Part 21 bytes.
- `step_parser` parses arbitrary Part 21 exchange structures and resolves
  instance references.
- `step_reader` exercises public STEP inspection on arbitrary bytes.
- `step_geometry_degenerate` parses IR and exercises STEP export with any
  degenerate geometry present in the document.
- `ir_validate_mutated` uses the first byte to select a semantic mutation,
  parses the remaining bytes as IR, and validates the result.
- `decode_pipeline_mutated` uses the first byte to mutate the remaining
  container bytes, then runs all five codecs.

Every harness treats a panic, abort, sanitizer finding, or libFuzzer timeout as
a failure. Parse and validation errors are expected results for malformed
input. Harnesses discard successful values and ordinary errors because their
contract is robustness, not input acceptance.

## Seed maintenance

Run seed generators from this crate so their relative `seeds/` paths resolve
correctly:

```sh
cd crates/cadmpeg-fuzz
cargo +nightly run --bin generate_all_seeds
cargo +nightly run --bin generate_submodule_seeds
cargo +nightly run --bin generate_synthetic_fixtures
cargo +nightly run --bin generate_fcstd_seeds
```

`generate_all_seeds` writes container and IR seeds, then derives deterministic
truncation, byte-flip, and oversized-length mutants. `generate_submodule_seeds`
writes focused parser inputs. `generate_synthetic_fixtures` writes deterministic
IR JSON fixtures for IR and STEP targets.

Two older generators remain available for narrower maintenance:

```sh
cargo +nightly run --bin generate_seeds
cargo +nightly run --bin generate_comprehensive_seeds
```

`generate_seeds` writes only the F3D container corpus.
`generate_comprehensive_seeds` leaves F3D unchanged and writes deeper
SolidWorks, CATIA, Creo, and NX container fixtures.

Seed generation overwrites files with matching names and may add deterministic
mutants. Review the resulting diff before keeping regenerated data.

## Adding or changing a harness

Place the harness in `fuzz_targets/` and add a matching `[[bin]]` entry to
`Cargo.toml` with `test`, `doc`, and `bench` disabled. A source file without a
manifest entry is not runnable through `cargo fuzz run`.

Keep the harness deterministic for a given byte sequence. Return early when an
input cannot reach the operation under test. Do not convert expected parse
errors into crashes. Put reusable structural inputs in `seeds/<target>/`;
libFuzzer can mutate them into malformed cases.

`cargo fuzz list --fuzz-dir crates/cadmpeg-fuzz` also prints the five seed
generator binaries because this package declares them as bins. They are Cargo
utilities, not fuzz targets.

Four harness sources are present without manifest entries:
`sldprt_entity.rs`, `sldprt_topology.rs`, `sldprt_spline_curves.rs`, and
`sldprt_spline_surfaces.rs`. Register a harness before treating it as an active
target.
