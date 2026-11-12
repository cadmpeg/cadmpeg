# cadmpeg-codec-catia

`cadmpeg-codec-catia` reads CATIA V5 `.CATPart` files into
[`CadIr`](https://docs.rs/cadmpeg-ir). It recognizes the `V5_CFV2` container
layouts used by CATPart files and decodes supported analytic surfaces, NURBS
surfaces, curves, vertices, and B-rep topology.

<!-- generated: capability catia -->

Support: L3 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#catia-v5-catpart)).

<!-- /generated: capability catia -->

Geometry on the standard-nested layout shows as extras.

## Install

```sh
cargo add cadmpeg-codec-catia cadmpeg-ir
```

## Decode

```rust,no_run
use std::fs::File;

use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_ir::{Codec, DecodeOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.CATPart")?;
    let decoded = CatiaCodec.decode(&mut input, &DecodeOptions::default())?;

    println!(
        "{} bodies, {} surfaces",
        decoded.ir().model.bodies.len(),
        decoded.ir().model.surfaces.len()
    );
    for loss in &decoded.report().losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    Ok(())
}
```

The result holds the decoded `CadIr` and a `DecodeReport`. Read
`report.losses` before trusting the IR. Use `CatiaCodec::inspect` to identify
the storage variant and list catalogued logical streams without decoding
entities. Set `DecodeOptions::depth` to `DecodeDepth::Container` when only
source metadata and container diagnostics are needed.

## Storage model

A CATPart starts with an outer `V5_CFV2` container. Most files also contain a
nested `V5_CFV2` directory whose physical extents reconstruct logical streams
such as `MainDataStream` and `SurfacicReps`. The codec identifies the storage
variant before selecting a record decoder.

Standard nested parts have the broadest model coverage. The decoder emits
analytic carrier surfaces and vertices, binds faces when stored senses resolve,
and emits loops, coedges, edges, and endpoint assignments when the trim,
support, and vertex tables form a complete unambiguous graph. Reference-closed
E5 graphs emit connected topology when refs close. Complete float-packed B5
graphs emit connected topology when their reference graph closes. Zero-entity
streams transfer face-local constructions. Inner-without-directory layouts
transfer A8/B2 carriers. Complete scalar formula graphs transfer typed
parameters. Unresolved native bytes stay attached to the IR as unknown records.

## Shared sheet boundaries

Two face occurrences of one native edge can use different curve parameters.
Shared-boundary transfer aligns their pcurves and retries rejected faces after
a neighboring face is admitted. Analytic reprojection retains the seeded
periodic chart branches.

A matching incident edge can corroborate a shared vertex repair. Endpoint
and boundary retry seeds admit discrepancies up to 0.005 mm. Alignment and
reprojection check the recovered pcurve's lift against the selected 3D edge
at 0.001 mm. A repaired outer boundary must contain each inner loop.
The report records these choices with `topology.shared-boundary-reconciled`.
It counts emitted faces admitted by vertex repair, seed alignment, and shared-edge
retry, and prior faces whose shared curves were replaced. These counts can
overlap. The note gives the largest vertex movement or sampled edge discrepancy.

## Sheet surface transfer

A complete rolling-ball jet defines a native procedural surface from its
stored values and derivatives. Bounded sheet transfer uses fitted B-splines
for rolling-ball, ruled, offset, and offset-extrusion carriers that require a
solved surface for STEP export. The decode report records these faces with
`geometry.sheet-surface-fitted`. The fit checks use sampled points.

Rolling-sheet boundary recovery first tries the stored transverse chart. It
can then extend the procedural circular section by at most 0.01 in transverse
parameter and 0.005 mm from its stored boundary. The recovered carrier is a
sample-checked spline fit. `geometry.rolling-sheet-extended` reports emitted
faces and their largest sampled boundary extension.

Two affine-mapped edges can corroborate a vertex when each edge has matching
stations on separate face charts. The selected station can move by at most
0.0005 mm to keep each fitted endpoint within the 0.01 mm coincidence bound.
`topology.affine-vertex-reconciled` reports the choice and the largest change
to an emitted face's vertex positions.

For a planar shared boundary corroborated by an admitted neighboring edge,
the decoder interprets the parameter-map fit tolerance as a model-space bound
in mm and adds the 0.001 mm model gap. This unit interpretation is inferred;
the format model retains the scalar separately. The inferred bound is retained
as edge accuracy. The report records the number of emitted faces and largest
sampled lift gap with
`geometry.parameter-map-accuracy-inferred`. STEP export retains the document
linear tolerance as its global uncertainty. Larger per-entity tolerances remain
subject to the exporter's topology-metadata loss reporting; STEP readback can
therefore reject a recovered boundary at the tighter document tolerance.

For an owned rolling-ball construction, a stored construction radius can
select a small radial correction of the two rail points at each station.
When an emitted face uses moved rail points, the report adds
`geometry.rolling-ball-rails-adjusted` with the largest point movement.
The sampled fit check compares the spline with the construction used for
fitting, after any reported rail correction.

## Reference

- [API documentation][docs]
- [CATIA format model][spec]
- [CATIA coverage contract][coverage]
- [Format support][support]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

CATIA V5 and other product names are trademarks of their respective owners.
cadmpeg uses them only to identify the file formats this codec targets and is
not affiliated with, endorsed by, or sponsored by any CAD vendor. See the
[clean-room and legal policy][legal].

[docs]: https://docs.rs/cadmpeg-codec-catia
[coverage]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia-coverage.md
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[spec]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#catia-v5-catpart
