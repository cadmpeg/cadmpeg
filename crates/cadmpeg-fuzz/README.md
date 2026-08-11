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
cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz iges_writer -- -runs=1000
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

Reduce and classify crash artifacts before promoting them into `seeds/`. Seed
files should be small inputs that reach a distinct parser state. Keep each seed
in the directory named for its target.

## Targets

Container and end-to-end codec paths call format detection, inspection, and
decoding:

- `f3d_container`
- `fcstd_container`, `fcstd_decode`, `fcstd_write`
- `sldprt_container`
- `catia_container`
- `creo_container`
- `nx_container`
- `rhino_container`
- `iges_container`

Native writing and replay:

- `f3d_writer` parses IR, generates a source-less archive, inspects it, and
  decodes it.
- `f3d_roundtrip` decodes an archive, replays it through the native writer, and
  decodes the result.
- `iges_writer` selects IGES 5.1, 5.2, or 5.3 from its control byte; exercises
  source-less planning, topology synthesis, unsupported-native rejection,
  inspection, semantic re-decode, validation, and byte-exact replay.

Focused parser coverage:

- Kernel: `acis_header`
- F3D: `f3d_asm_header`, `f3d_sab_frame`, `f3d_nurbs_surfaces`,
  `f3d_nurbs_curves`, `f3d_nurbs_pcurves`
- SolidWorks: `sldprt_parasolid`, `sldprt_container_scan`, `sldprt_entity`,
  `sldprt_topology`, `sldprt_spline_curves`, `sldprt_spline_surfaces`
- CATIA: `catia_geometry_vertices`, `catia_geometry_surfaces`,
  `catia_a8_surfaces`, `catia_a5_surfaces`, `catia_b5`, `catia_e5`,
  `catia_zero_entity`, `catia_container_dir`, `catia_e5_orientation`,
  `catia_value_block`, `catia_catalog`, `catia_object_graph`, `catia_topology`
- Creo: `creo_psb_tokens`, `creo_compact_int`, `creo_short_form_float`,
  `creo_container_scan`, `creo_surface_rows`, `creo_curve_prototypes`,
  `creo_datum`, `creo_scalar`
- NX: `nx_parasolid`, `nx_geometry_points`, `nx_geometry_surfaces`,
  `nx_geometry_curves`, `nx_nurbs_surfaces`, `nx_nurbs_curves`, `nx_om`,
  `nx_topology`, `nx_deltas`, `nx_intersection`
- Inventor and shared containers: `inventor_codec`, `inventor_database`,
  `inventor_rse_meta`, `inventor_rse_records`, `inventor_property_set`,
  `inventor_protein_envelope`, `compound_snapshot`, `protein_decode`
- Rhino: `rhino_chunks`, `rhino_object_record`, `rhino_nurbs`,
  `rhino_mesh_buffer`, `rhino_brep`, `rhino_subd`, `rhino_cage`, `rhino_hatch`,
  `rhino_polyedge`
- FCStd: `fcstd_xml`, `fcstd_gui`, `fcstd_brep`, `fcstd_element_map`,
  `fcstd_auxiliary`

IR and STEP:

- `ir_from_json` parses a `CadIr` document.
- `ir_validate` parses and validates a `CadIr` document.
- `ir_diff` splits the input into two JSON documents and computes their
  structural diff.
- `ir_canonical_roundtrip` serializes parsed IR to canonical JSON and parses it
  again.
- `ir_validate_mutated` uses the first byte to select a semantic mutation,
  parses the remaining bytes as IR, and validates the result.
- `step_writer` parses IR and writes STEP with default options.
- `step_writer_custom` derives STEP header fields from an eight-byte prefix,
  then parses the remaining bytes as IR.
- `step_lexer` tokenizes arbitrary Part 21 bytes.
- `step_parser` parses arbitrary Part 21 exchange structures and resolves
  instance references.
- `step_reader` exercises public STEP inspection on arbitrary bytes.
- `step_decode` exercises public STEP semantic decoding on arbitrary bytes.
- `step_geometry_degenerate` parses IR and exercises STEP export with any
  degenerate geometry present in the document.
- `decode_pipeline_mutated` uses the first byte to mutate the remaining
  container bytes, then runs F3D, Inventor, FreeCAD, SolidWorks, CATIA, Creo,
  NX, and Rhino detection, inspection, and decoding.

Every harness treats a panic, abort, sanitizer finding, or libFuzzer timeout as
a failure. Parse and validation errors are expected results for malformed
input. Harnesses discard successful values and ordinary errors because their
contract is robustness, not input acceptance.

## Seed maintenance

Write submodule seeds from the repository root into the root `seeds/` tree:

```sh
cargo +nightly run --manifest-path crates/cadmpeg-fuzz/Cargo.toml --bin generate_submodule_seeds
```

Write the remaining generators from the fuzz crate into its `seeds/` tree:

```sh
cd crates/cadmpeg-fuzz
cargo +nightly run --bin generate_all_seeds
cargo +nightly run --bin generate_fcstd_seeds
cargo +nightly run --bin generate_rhino_seeds
cargo +nightly run --bin generate_iges_seeds
```

`generate_all_seeds` writes container and IR seeds, then derives deterministic
truncation, byte-flip, and oversized-length mutants. `generate_submodule_seeds`
writes focused parser inputs. `generate_iges_seeds` writes valid IGES 5.3 point
and trimmed-sheet seeds for `iges_container`. `generate_all_seeds` writes
version-selecting IR seeds for `iges_writer`.

Narrower maintenance generators:

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
input cannot reach the operation under test. Leave expected parse errors as
ordinary results. Put reusable structural inputs in `seeds/<target>/`;
libFuzzer can mutate them into malformed cases.

`cargo fuzz list --fuzz-dir crates/cadmpeg-fuzz` also prints the seed
generator binaries because this package declares them as bins. They are Cargo
utilities, not fuzz targets.
