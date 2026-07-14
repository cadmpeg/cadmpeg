# cadmpeg-step

`cadmpeg-step` serializes a [`CadIr`] document as an ISO 10303-21 exchange file
using the STEP AP214 `AUTOMOTIVE_DESIGN` schema. It is the library interface for
STEP export; the cadmpeg CLI uses the same model and writer.

## Export a document

Add the writer and IR crates:

```sh
cargo add cadmpeg-step cadmpeg-ir
```

Pass any [`std::io::Write`] sink to [`write_step`]:

```rust
use std::fs::File;
use std::io::BufWriter;

use cadmpeg_ir::examples::unit_cube;
use cadmpeg_step::{
    write_step, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

let ir = unit_cube();
let file = File::create("cube.step")?;
let mut output = BufWriter::new(file);
let options = StepWriteOptions {
    schema: StepSchema::Ap242Edition3,
    unsupported: StepUnsupportedPolicy::Reject,
    product_name: "cube".into(),
    author: "Example Author".into(),
    organization: "Example Organization".into(),
    timestamp: "2026-07-11T09:00:00Z".into(),
    originating_system: "example-exporter".into(),
};

let report = write_step(&ir, &mut output, &options)?;
if !report.losses.is_empty() {
    for loss in &report.losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
}

# Ok::<(), Box<dyn std::error::Error>>(())
```

`write_step` emits the complete Part 21 envelope, product-definition records,
representation context, and reachable boundary-representation geometry. Each IR
region becomes a `MANIFOLD_SOLID_BREP`; regions with additional shells become
`BREP_WITH_VOIDS`. The topology walk continues through shells, faces, loops,
coedges, edges, and vertices.

Supported surface carriers are planes, cylinders, cones, spheres, tori, and
rational or non-rational NURBS surfaces. Supported curve carriers are lines,
circles, ellipses, parabolas, hyperbolas, and rational or non-rational NURBS
curves. The writer preserves shared carriers by reusing STEP instances.

## Units and metadata

Coordinates are written without rescaling and the representation context
declares millimetres. Supply geometry in millimetres before export. The context
uses the IR linear tolerance as its uncertainty value; plane and solid angles
use radians and steradians.

[`StepWriteOptions`] controls `FILE_NAME` metadata. An empty timestamp produces
`1970-01-01T00:00:00`, which keeps default output deterministic. The first body
name, when present, supplies the STEP product name. `product_name` supplies the
`FILE_NAME` name instead.

## Losses and errors

The writer exports representable geometry and records reductions in
[`StepReport::losses`]. Review these notes before accepting the file. In
particular:

- faces on unknown surfaces and edges without typed 3D curves are omitted;
- body transforms are not applied, so affected coordinates remain in body-local
  space;
- coedge pcurves, colors, appearances, source attributes, passthrough records,
  and parametric history are not emitted;
- procedural geometry is reduced to its solved curve or surface carrier;
- signed sphere radii and nonstandard torus minor radii are normalized where
  required by the emitted STEP entity.

An empty or fully unrepresentable model still produces a syntactically complete
file with an empty geometric representation and a warning. [`StepError`] reports
only failures from the output sink. Because output is streamed, an I/O failure
can leave a partial file.

[`StepReport::entity_counts`] groups DATA instances by entity keyword, and
[`StepReport::total_entities`] gives the complete DATA instance count.
[`StepReport::error_count`] counts loss notes whose severity is at least
`Error`; lower-severity losses still require caller review.

## References

- [API documentation][docs]
- [Format support][support]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-step
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#step-ap214
[`cadIr`]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/struct.CadIr.html
[`stepError`]: https://docs.rs/cadmpeg-step/latest/cadmpeg_step/enum.StepError.html
[`stepReport::entity_counts`]: https://docs.rs/cadmpeg-step/latest/cadmpeg_step/struct.StepReport.html#structfield.entity_counts
[`stepReport::error_count`]: https://docs.rs/cadmpeg-step/latest/cadmpeg_step/struct.StepReport.html#method.error_count
[`stepReport::losses`]: https://docs.rs/cadmpeg-step/latest/cadmpeg_step/struct.StepReport.html#structfield.losses
[`stepReport::total_entities`]: https://docs.rs/cadmpeg-step/latest/cadmpeg_step/struct.StepReport.html#structfield.total_entities
[`stepWriteOptions`]: https://docs.rs/cadmpeg-step/latest/cadmpeg_step/struct.StepWriteOptions.html
[`std::io::Write`]: https://doc.rust-lang.org/std/io/trait.Write.html
[`write_step`]: https://docs.rs/cadmpeg-step/latest/cadmpeg_step/fn.write_step.html
