// SPDX-License-Identifier: Apache-2.0
//! Measured body geometry across an IGES write round trip, compared without
//! entity identities.

use std::f64::consts::PI;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;
use cadmpeg_measure::compare::{compare, CompareOptions, Verdict};
use cadmpeg_measure::report::MeasureReport;
use cadmpeg_measure::value::Quantity;
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

use crate::test_support::plan_at;
use crate::test_support::test_closed_cylinder::{solid_cylinder, CYLINDER_HEIGHT, CYLINDER_RADIUS};
use crate::test_support::test_drawing_and_trimming::trimmed_plane_with_inner_loop_file;
use crate::test_support::test_owned::explicit_void_solid_file;
use crate::{IgesCodec, IgesVersion};

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

/// A solid cylinder whose circle edges store no parameter range: volume
/// `π r² h` and area `2π r h + 2π r²`, which are `3000π` and `800π` for
/// `r = 10` and `h = 30`.
#[test]
fn closed_circle_edges_without_range_keep_measured_cylinder_geometry() {
    let ir = solid_cylinder();
    let plan = plan_at(IgesVersion::V5_3, &ir, None).expect("plan the cylinder");
    let mut output = Vec::new();
    plan.write_to(&mut output).expect("write the cylinder");
    let decoded = IgesCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode the written cylinder");
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
    let (r, h) = (CYLINDER_RADIUS, CYLINDER_HEIGHT);
    let (volume, volume_error) = scalar(body.volume().as_ref());
    assert_close(volume, PI * r * r * h, volume_error);
    let (area, area_error) = scalar(body.area().as_ref());
    assert_close(area, 2.0 * PI * r * h + 2.0 * PI * r * r, area_error);
}

fn decoded(bytes: Vec<u8>) -> CadIr {
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the fixture")
        .ir()
        .clone()
}

/// `ir` written as IGES 5.3 and decoded again.
fn round_tripped(ir: &CadIr) -> CadIr {
    let plan = plan_at(IgesVersion::V5_3, ir, None).expect("plan the model");
    let mut output = Vec::new();
    plan.write_to(&mut output).expect("write the model");
    decoded(output)
}

/// The face senses and the loop windings of every body of `report` give one
/// orientation reading.
fn assert_one_orientation_reading(report: &MeasureReport) {
    for body in report.bodies() {
        let orientation = body.orientation();
        assert_eq!(orientation.consistent(), &Some(true));
        assert!(orientation.inconsistent().is_empty());
        assert!(orientation.mixed().is_empty());
    }
}

/// The right tetrahedron with legs 4 less the void right tetrahedron with
/// legs 0.5: `32/3 − 1/48 = 511/48`.
#[test]
fn void_solid_measures_one_orientation_reading() {
    let (bytes, ..) = explicit_void_solid_file();
    let report = measured(&decoded(bytes));
    assert_one_orientation_reading(&report);
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    let volume = body
        .volume()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("measured volume");
    assert_close(
        volume.value().value.get(),
        511.0 / 48.0,
        volume.numeric_error().get(),
    );
}

#[test]
fn void_solid_write_round_trip_keeps_measured_geometry() {
    let (bytes, ..) = explicit_void_solid_file();
    let ir = decoded(bytes);
    let written = round_tripped(&ir);
    let before = measured(&ir);
    let after = measured(&written);
    assert_one_orientation_reading(&after);
    let comparison = compare(&ir, &before, &written, &after, &CompareOptions::default())
        .expect("one body pair fits the pair allowance");
    assert_eq!(comparison.verdict(), Verdict::Agree);
}

#[test]
fn trimmed_inner_loop_write_round_trip_keeps_measured_geometry() {
    let ir = decoded(trimmed_plane_with_inner_loop_file());
    let written = round_tripped(&ir);
    let before = measured(&ir);
    let after = measured(&written);
    assert_one_orientation_reading(&before);
    assert_one_orientation_reading(&after);
    let comparison = compare(&ir, &before, &written, &after, &CompareOptions::default())
        .expect("one body pair fits the pair allowance");
    assert_eq!(comparison.verdict(), Verdict::Agree);
}
