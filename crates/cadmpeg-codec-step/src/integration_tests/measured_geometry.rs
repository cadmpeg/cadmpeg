// SPDX-License-Identifier: Apache-2.0
//! Measured body geometry and orientation findings of decoded STEP
//! exchanges, and across a STEP write round trip compared without entity
//! identities.

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
use crate::test_support::box_brep::{box_shell, solid_representation, BoxFaceUse};
use crate::test_support::exchange::decode_inline;
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

/// The orientation findings of `ir`.
fn orientation_findings(ir: &CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).expect("context");
    let findings =
        cadmpeg_measure::findings(&ctx, ir, MeasureOptions::default()).expect("findings");
    ctx.finish_session().expect("session stays unrefused");
    findings
        .into_iter()
        .filter(|finding| finding.check == cadmpeg_ir::report::check::Check::Orientation)
        .collect()
}

/// The measured volume of the one body of `report` and its numeric error.
fn body_volume(report: &MeasureReport) -> (f64, f64) {
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    let measured = body
        .volume()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("measured volume");
    (measured.value().value.get(), measured.numeric_error().get())
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

/// Exchanges whose faces are used through `ORIENTED_OPEN_SHELL(.F.)` or
/// `ORIENTED_CLOSED_SHELL(.F.)` decode to faces whose sense and loop winding
/// state one oriented normal.
#[test]
fn reversed_shell_exchanges_have_no_orientation_findings() {
    let sources: [(&str, &[u8]); 16] = [
        (
            "ap214_sheet",
            include_bytes!("../../tests/fixtures/ap214_sheet.p21"),
        ),
        (
            "tp09_curve3d_near_tie",
            include_bytes!("../reader/geometry/tests/data/tp09_curve3d_near_tie.p21"),
        ),
        (
            "tp09_master_s1",
            include_bytes!("../reader/geometry/tests/data/tp09_master_s1.p21"),
        ),
        (
            "tp09_master_s1_reordered",
            include_bytes!("../reader/geometry/tests/data/tp09_master_s1_reordered.p21"),
        ),
        (
            "ap05_independent_styles_first",
            include_bytes!("../reader/presentation/tests/data/ap05_independent_styles_first.p21"),
        ),
        (
            "ap05_independent_styles_reordered",
            include_bytes!(
                "../reader/presentation/tests/data/ap05_independent_styles_reordered.p21"
            ),
        ),
        (
            "ap06_surface_sides_positive_negative_first",
            include_bytes!(
                "../reader/presentation/tests/data/ap06_surface_sides_positive_negative_first.p21"
            ),
        ),
        (
            "ap06_surface_sides_positive_negative_reordered",
            include_bytes!(
                "../reader/presentation/tests/data/ap06_surface_sides_positive_negative_reordered.p21"
            ),
        ),
        (
            "ps03_repeated_mapped_placements",
            include_bytes!("../reader/product/tests/data/ps03_repeated_mapped_placements.p21"),
        ),
        (
            "ps03_repeated_mapped_placements_reordered",
            include_bytes!(
                "../reader/product/tests/data/ps03_repeated_mapped_placements_reordered.p21"
            ),
        ),
        (
            "ps03_repeated_mapped_placements_same_transform",
            include_bytes!(
                "../reader/product/tests/data/ps03_repeated_mapped_placements_same_transform.p21"
            ),
        ),
        (
            "tp09_competing_pcurves",
            include_bytes!("../reader/topology/tests/data/tp09_competing_pcurves.p21"),
        ),
        (
            "tp09_competing_pcurves_crossing",
            include_bytes!("../reader/topology/tests/data/tp09_competing_pcurves_crossing.p21"),
        ),
        (
            "tp09_competing_pcurves_near_tied",
            include_bytes!("../reader/topology/tests/data/tp09_competing_pcurves_near_tied.p21"),
        ),
        (
            "tp09_competing_pcurves_reordered",
            include_bytes!("../reader/topology/tests/data/tp09_competing_pcurves_reordered.p21"),
        ),
        (
            "tp09_divergent_interior",
            include_bytes!("../reader/topology/tests/data/tp09_divergent_interior.p21"),
        ),
    ];
    for (name, source) in sources {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode reversed-shell exchange");
        assert!(!decoded.ir().model.faces.is_empty(), "{name} decodes faces");
        let findings = orientation_findings(decoded.ir());
        assert!(findings.is_empty(), "{name}: {findings:#?}");
    }
}

/// The 10 mm cube at the origin with the `2 x 3 x 4` box void at
/// `(2, 3, 1)` encloses `1000 - 24 = 976`. The void is the outward box shell
/// used through `ORIENTED_CLOSED_SHELL(.F.)`, so its CADIR faces point into
/// the void, and the volume holds after a STEP write round trip.
#[test]
fn reversed_void_shell_measures_the_enclosed_volume_through_a_round_trip() {
    let outer = box_shell(10, [0.0; 3], [10.0; 3], BoxFaceUse::Outward);
    let void = box_shell(
        outer.next,
        [2.0, 3.0, 1.0],
        [4.0, 6.0, 5.0],
        BoxFaceUse::Outward,
    );
    let oriented = void.next;
    let solid = oriented + 1;
    let records = format!(
        "{}{}#{oriented}=ORIENTED_CLOSED_SHELL('',*,#{},.F.);\n\
         #{solid}=BREP_WITH_VOIDS('',#{},(#{oriented}));\n{}",
        outer.records,
        void.records,
        void.shell,
        outer.shell,
        solid_representation(1, solid)
    );
    let decoded = decode_inline(&records);
    assert_eq!(decoded.ir().model.regions.len(), 1);
    assert_eq!(decoded.ir().model.regions[0].shells.len(), 2);
    let findings = orientation_findings(decoded.ir());
    assert!(findings.is_empty(), "{findings:#?}");
    let before = measured(decoded.ir());
    let (volume, error) = body_volume(&before);
    assert_close(volume, 976.0, error);

    let mut output = Vec::new();
    write_step(
        decoded.ir(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write solid with a void");
    let round_trip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written solid with a void");
    let findings = orientation_findings(round_trip.ir());
    assert!(findings.is_empty(), "{findings:#?}");
    let after = measured(round_trip.ir());
    let (volume, error) = body_volume(&after);
    assert_close(volume, 976.0, error);
    let comparison = compare(
        decoded.ir(),
        &before,
        round_trip.ir(),
        &after,
        &CompareOptions::default(),
    )
    .expect("one body pair fits the pair allowance");
    assert_eq!(comparison.verdict(), Verdict::Agree, "{comparison:#?}");
}
