# cadmpeg-step

**Pure-Rust STEP AP214 writing for the cadmpeg CAD pipeline.**

`cadmpeg-step` serializes supported `cadmpeg-ir` B-rep topology and geometry as
ISO 10303-21 STEP AP214. It emits mainstream product and representation
structure and returns an explicit report for IR content STEP cannot represent.

> Export support is partial. The writer reports omitted or reduced content and
> does not fabricate placeholder geometry.

## Install

```sh
cargo add cadmpeg-step cadmpeg-ir
```

cadmpeg requires Rust 1.88 or later.

## Use

```rust
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use cadmpeg_step::{write_step, StepWriteOptions};

let ir = CadIr::empty(Units::default());
let mut output = Vec::new();
let report = write_step(&ir, &mut output, &StepWriteOptions::default())?;

assert_eq!(report.error_count(), 0);
assert!(output.starts_with(b"ISO-10303-21;"));

# Ok::<(), cadmpeg_step::StepError>(())
```

The writer emits product-definition and unit-bearing representation context,
then supported manifold B-rep regions, shells, faces, loops, edges, vertices,
analytic carriers, and B-spline carriers. `StepReport` contains entity counts
and every export loss note.

## Current boundaries

- STEP AP214 output covers supported analytic and B-spline B-rep geometry.
- IR domains without an AP214 representation remain explicit in the report.
- Inputs with unsupported geometry may produce a partial STEP file with loss
  notes; sink I/O failures are returned as errors.

See the [format support profile][support] for the current capability summary.

## Project links

- [API documentation][docs]
- [Format support][support]
- [Architecture and crate map][architecture]
- [Repository][repo]
- [Clean-room and legal policy][legal]

Code is licensed under the Apache License 2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-step
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#step-ap214
