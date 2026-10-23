# cadmpeg-measure

Geometric measurement of decoded cadmpeg models: carrier parameters, trimmed
areas, boundary lengths, boxes and orientation readings of bodies, faces,
edges and surfaces, in model space.

Every value is measured within its reported error under its stated method,
or unavailable with a pinned reason and a closed category. The output
contract is in `docs/measurement.md` of the repository.

Measurement is an oracle on decoded output. No decoder or encoder depends on
this crate outside its tests.

```rust
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
let arena = cadmpeg_core::decode::DecodeArena::new();
let policy = cadmpeg_core::decode::DecodePolicy::service();
let ctx = cadmpeg_core::decode::DecodeContext::for_loaded_input(&arena, &policy, 0)
    .expect("context");
let input = MeasureInput { ir: &cube, fidelity: None, geometry_transferred: true };
let report = Measurer::new(input, MeasureOptions::default())
    .measure(&ctx, &Selection::default())
    .expect("measured");
ctx.finish_session().expect("session");
let volume = report.bodies()[0].volume().as_ref().and_then(|volume| volume.measured());
assert!((volume.expect("volume").value().value.get() - 1000.0).abs() < 1.0e-9);
```

`cadmpeg_measure::compare::compare` compares two measured documents without
entity IDs. The CLI exposes measurement as `cadmpeg measure` and the
comparison as `cadmpeg diff A B --geometry`.
