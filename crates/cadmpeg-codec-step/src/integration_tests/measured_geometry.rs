// SPDX-License-Identifier: Apache-2.0
//! Measured body geometry across a STEP write round trip, compared without
//! entity identities.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use cadmpeg_measure::compare::{compare, CompareOptions, Verdict};
use cadmpeg_measure::report::MeasureReport;
use cadmpeg_measure::value::Quantity;
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

/// Allowance of a measured value against its expression beyond the value's
/// numeric error, per unit of the larger of the expected magnitude and 1.
const TEST_ABSOLUTE_FLOOR: f64 = 1.0e-9;

/// The body rows of `ir`, measured in model space.
fn measured(ir: &CadIr) -> MeasureReport {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).expect("context");
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &Selection::default())
        .expect("measured");
    ctx.finish_session().expect("session stays unrefused");
    report
}

fn assert_close(computed: f64, expected: f64, numeric: f64) {
    let allowed = numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(1.0);
    assert!(
        (computed - expected).abs() <= allowed,
        "computed {computed}, expected {expected}, allowed {allowed}"
    );
}

/// The 10 mm cube under a rigid placement that turns `+x` into `+y` and
/// moves the origin to `(15, 4, 2)`: volume 1000, area 600, and the centroid
/// `(5, 5, 5)` placed at `(10, 9, 7)`.
#[test]
fn writer_round_trip_keeps_measured_body_geometry() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].transform = Some(
        Transform::affine([
            [0.0, -1.0, 0.0, 15.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 2.0],
        ])
        .expect("affine transform"),
    );
    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write placed body");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode placed body");
    let before = measured(&ir);
    let after = measured(decoded.ir());
    let comparison = compare(
        &ir,
        &before,
        decoded.ir(),
        &after,
        &CompareOptions::default(),
    )
    .expect("one body pair fits the pair allowance");
    assert_eq!(comparison.verdict(), Verdict::Agree, "{comparison:#?}");
    let [body] = after.bodies().as_slice() else {
        panic!("one body row");
    };
    let scalar = |quantity: Option<&Quantity<cadmpeg_measure::value::Scalar>>| {
        let measured = quantity.and_then(Quantity::measured).expect("measured");
        (measured.value().value.get(), measured.numeric_error().get())
    };
    let (volume, volume_error) = scalar(body.volume().as_ref());
    assert_close(volume, 1000.0, volume_error);
    let (area, area_error) = scalar(body.area().as_ref());
    assert_close(area, 600.0, area_error);
    let centroid = body
        .centroid()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("centroid");
    let point = centroid.value().value.get();
    let error = centroid.numeric_error().get();
    assert_close(point.x, 10.0, error);
    assert_close(point.y, 9.0, error);
    assert_close(point.z, 7.0, error);
}
