// SPDX-License-Identifier: Apache-2.0
//! Sphere faces: caps, a band, a sphere with a hole, a lune and whole
//! spheres, with expectations from geometry.

use std::f64::consts::{FRAC_PI_2, PI};

use cadmpeg_ir::ids::{EdgeId, VertexId};
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::{Category, CaveatCode, UnavailableReason};
use crate::report::{FaceMeasures, MeasureReport, RegionRule};
use crate::select::Selection;
use crate::test_support::{
    assert_measured, assert_range, assert_tight_box, measure, Builder, Circle, Frame,
    TEST_ABSOLUTE_FLOOR,
};
use crate::value::{BoxValue, Quantity, Reading, Scalar};

fn faces() -> Selection {
    Selection {
        all_faces: true,
        ..Selection::default()
    }
}

fn only_face(report: &MeasureReport) -> &FaceMeasures {
    let [face] = report.faces().as_slice() else {
        panic!("one face row");
    };
    face
}

/// The value of the one complement candidate of a measured scalar.
fn complement(quantity: &Quantity<Scalar>) -> f64 {
    let measured = quantity.measured().expect("measured");
    assert_eq!(measured.reading(), Some(Reading::Declared));
    let [candidate] = measured.candidates() else {
        panic!("one candidate");
    };
    assert_eq!(candidate.reading(), Reading::Complement);
    candidate.value().value.get()
}

/// A box candidate under the complement reading.
fn complement_box(quantity: &Quantity<BoxValue>) -> BoxValue {
    let measured = quantity.measured().expect("measured box");
    assert_eq!(measured.reading(), Some(Reading::Declared));
    let [candidate] = measured.candidates() else {
        panic!("one candidate");
    };
    *candidate.value()
}

fn assert_box_value(value: &BoxValue, min: [f64; 3], max: [f64; 3], scale: f64) {
    let (low, high) = (value.min.get(), value.max.get());
    for (computed, expected) in [
        (low.x, min[0]),
        (low.y, min[1]),
        (low.z, min[2]),
        (high.x, max[0]),
        (high.y, max[1]),
        (high.z, max[2]),
    ] {
        assert!(
            (computed - expected).abs() <= TEST_ABSOLUTE_FLOOR * scale,
            "computed {computed}, expected {expected}"
        );
    }
}

/// The sphere `R = 10` about the origin with the circle of radius 8 at
/// `z = 6` as its one loop, traversed counterclockwise about `+z` when
/// `sense` is forward.
fn cap(sense: Sense) -> CadIr {
    let mut builder = Builder::new();
    let vertex = builder.vertex([8.0, 0.0, 6.0]);
    let circle = builder.circle(
        [0.0, 0.0, 6.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        8.0,
        &vertex,
    );
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), 10.0);
    builder.face(&sphere, Sense::Forward, &[vec![(circle, sense)]], true);
    builder.finish(BodyKind::Sheet)
}

#[test]
fn a_cap_reads_its_declared_side_with_the_complement() {
    let latitude = 0.6_f64.asin();
    let north = 2.0 * PI * 100.0 * (1.0 - 0.6);
    let south = 2.0 * PI * 100.0 * (1.0 + 0.6);
    for (sense, area, other, range, low, high) in [
        (
            Sense::Forward,
            north,
            south,
            [latitude, FRAC_PI_2],
            [-8.0, -8.0, 6.0],
            [8.0, 8.0, 10.0],
        ),
        (
            Sense::Reversed,
            south,
            north,
            [-FRAC_PI_2, latitude],
            [-10.0, -10.0, -10.0],
            [10.0, 10.0, 6.0],
        ),
    ] {
        let report = measure(&cap(sense), &faces());
        let face = only_face(&report);
        let measures = face.measures();
        assert_eq!(*measures.region(), Some(RegionRule::DeclaredOrientation));
        assert_measured(measures.area(), area, 400.0);
        assert!((complement(measures.area()) - other).abs() <= TEST_ABSOLUTE_FLOOR * 400.0);
        assert_range(measures.latitude_range().as_ref(), range, 1.0);
        let sweep = measures
            .angular_sweep()
            .as_ref()
            .and_then(Quantity::measured)
            .expect("sweep");
        assert!(sweep.value().full);
        let bbox = measures.bbox().measured().expect("box");
        assert_box_value(bbox.value(), low, high, 10.0);
        assert_eq!(*face.orientation().consistent(), None);
        assert!(face
            .caveats()
            .iter()
            .any(|caveat| caveat.code() == CaveatCode::OrientationUnconfirmed));
        let report = measure(&cap(sense), &Selection::default());
        let [body] = report.bodies().as_slice() else {
            panic!("one body row");
        };
        let body_area = body.area().as_ref().expect("body area");
        assert_measured(body_area, area, 400.0);
        assert!((complement(body_area) - other).abs() <= TEST_ABSOLUTE_FLOOR * 400.0);
    }
}

#[test]
fn a_band_between_two_latitudes_rests_on_its_orientation() {
    let mut builder = Builder::new();
    let low = builder.vertex([8.0, 0.0, -6.0]);
    let high = builder.vertex([8.0, 0.0, 6.0]);
    let lower = builder.circle(
        [0.0, 0.0, -6.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        8.0,
        &low,
    );
    let upper = builder.circle(
        [0.0, 0.0, 6.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        8.0,
        &high,
    );
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), 10.0);
    builder.face(
        &sphere,
        Sense::Forward,
        &[
            vec![(lower, Sense::Forward)],
            vec![(upper, Sense::Reversed)],
        ],
        false,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    assert_measured(measures.area(), 2.0 * PI * 10.0 * 12.0, 400.0);
    let whole = 4.0 * PI * 100.0;
    assert!(
        (complement(measures.area()) - (whole - 240.0 * PI)).abs() <= TEST_ABSOLUTE_FLOOR * whole
    );
    let latitude = 0.6_f64.asin();
    assert_range(
        measures.latitude_range().as_ref(),
        [-latitude, latitude],
        1.0,
    );
    let bbox = measures.bbox().measured().expect("box");
    assert_box_value(bbox.value(), [-10.0, -10.0, -6.0], [10.0, 10.0, 6.0], 10.0);
    assert_box_value(
        &complement_box(measures.bbox()),
        [-8.0, -8.0, -10.0],
        [8.0, 8.0, 10.0],
        10.0,
    );
}

#[test]
fn a_sphere_with_a_hole_keeps_its_poles() {
    let mut builder = Builder::new();
    let vertex = builder.vertex([6.0, 8.0, 0.0]);
    let circle = builder.circle(
        [6.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        8.0,
        &vertex,
    );
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), 10.0);
    builder.face(
        &sphere,
        Sense::Forward,
        &[vec![(circle, Sense::Reversed)]],
        false,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    let hole = 2.0 * PI * 10.0 * 4.0;
    assert_measured(measures.area(), 400.0 * PI - hole, 400.0);
    assert!((complement(measures.area()) - hole).abs() <= TEST_ABSOLUTE_FLOOR * 400.0);
    assert_range(
        measures.latitude_range().as_ref(),
        [-FRAC_PI_2, FRAC_PI_2],
        1.0,
    );
    let bbox = measures.bbox().measured().expect("box");
    assert_box_value(bbox.value(), [-10.0, -10.0, -10.0], [6.0, 10.0, 10.0], 10.0);
    assert_box_value(
        &complement_box(measures.bbox()),
        [6.0, -8.0, -8.0],
        [10.0, 8.0, 8.0],
        10.0,
    );
}

/// The meridian half-circle of the sphere `R` about the origin at angle
/// `u` from `+x`, from the south pole to the north pole.
fn meridian(builder: &mut Builder, radius: f64, u: f64, poles: [&VertexId; 2]) -> EdgeId {
    let (sine, cosine) = u.sin_cos();
    builder.arc(
        Circle {
            frame: Frame {
                origin: [0.0; 3],
                axis: [sine, -cosine, 0.0],
                reference: [cosine, sine, 0.0],
            },
            radius,
        },
        [-FRAC_PI_2, FRAC_PI_2],
        poles[0],
        poles[1],
    )
}

#[test]
fn a_lune_between_two_meridians_and_its_complement() {
    let mut builder = Builder::new();
    let south = builder.vertex([0.0, 0.0, -10.0]);
    let north = builder.vertex([0.0, 0.0, 10.0]);
    let first = meridian(&mut builder, 10.0, 0.0, [&south, &north]);
    let second = meridian(&mut builder, 10.0, FRAC_PI_2, [&south, &north]);
    let north_pole = builder.bare_edge(&north, &north);
    let south_pole = builder.bare_edge(&south, &south);
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), 10.0);
    let face = builder.face(
        &sphere,
        Sense::Forward,
        &[vec![
            (second, Sense::Forward),
            (north_pole, Sense::Forward),
            (first, Sense::Reversed),
            (south_pole, Sense::Forward),
        ]],
        false,
    );
    builder.line_pcurve(
        &face,
        [0, 1],
        [FRAC_PI_2, FRAC_PI_2],
        [-1.0, 0.0],
        [0.0, FRAC_PI_2],
    );
    builder.line_pcurve(
        &face,
        [0, 3],
        [0.0, -FRAC_PI_2],
        [1.0, 0.0],
        [0.0, FRAC_PI_2],
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    let lune = 2.0 * 100.0 * FRAC_PI_2;
    assert_measured(measures.area(), lune, 400.0);
    assert!(
        (complement(measures.area()) - (400.0 * PI - lune)).abs() <= TEST_ABSOLUTE_FLOOR * 400.0
    );
    let sweep = measures
        .angular_sweep()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("sweep");
    assert!(!sweep.value().full);
    assert!((sweep.value().value.get() - FRAC_PI_2).abs() <= TEST_ABSOLUTE_FLOOR);
    let [candidate] = sweep.candidates() else {
        panic!("one sweep candidate");
    };
    assert!((candidate.value().value.get() - 3.0 * FRAC_PI_2).abs() <= TEST_ABSOLUTE_FLOOR);
    let bbox = measures.bbox().measured().expect("box");
    assert_box_value(bbox.value(), [0.0, 0.0, -10.0], [10.0, 10.0, 10.0], 10.0);
    assert_box_value(
        &complement_box(measures.bbox()),
        [-10.0, -10.0, -10.0],
        [10.0, 10.0, 10.0],
        10.0,
    );
}

#[test]
fn a_seam_meridian_with_pole_edges_bounds_the_whole_sphere() {
    let radius = 5.0;
    let mut builder = Builder::new();
    let south = builder.vertex([0.0, 0.0, -radius]);
    let north = builder.vertex([0.0, 0.0, radius]);
    let seam = meridian(&mut builder, radius, 0.0, [&south, &north]);
    let north_pole = builder.bare_edge(&north, &north);
    let south_pole = builder.bare_edge(&south, &south);
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), radius);
    let face = builder.face(
        &sphere,
        Sense::Forward,
        &[vec![
            (seam.clone(), Sense::Forward),
            (north_pole, Sense::Forward),
            (seam, Sense::Reversed),
            (south_pole, Sense::Forward),
        ]],
        false,
    );
    builder.line_pcurve(&face, [0, 1], [0.0, FRAC_PI_2], [1.0, 0.0], [0.0, 2.0 * PI]);
    builder.line_pcurve(
        &face,
        [0, 3],
        [0.0, -FRAC_PI_2],
        [1.0, 0.0],
        [0.0, 2.0 * PI],
    );
    let ir = builder.finish(BodyKind::Solid);
    let report = measure(&ir, &Selection::default());
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    assert_measured(
        body.volume().as_ref().expect("volume"),
        4.0 * PI * radius.powi(3) / 3.0,
        radius.powi(3),
    );
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    assert_eq!(*measures.region(), Some(RegionRule::WholeSurface));
    assert_measured(measures.area(), 4.0 * PI * radius * radius, radius * radius);
    assert_measured(measures.seam_length(), PI * radius, radius);
    assert_range(
        measures.latitude_range().as_ref(),
        [-FRAC_PI_2, FRAC_PI_2],
        1.0,
    );
    assert_tight_box(measures.bbox(), [-radius; 3], [radius; 3], radius);
}

#[test]
fn pole_vertex_loops_bound_the_whole_sphere() {
    let radius = 5.0;
    let mut builder = Builder::new();
    let south = builder.vertex([1.0, 2.0, 3.0 - radius]);
    let north = builder.vertex([1.0, 2.0, 3.0 + radius]);
    let sphere = builder.sphere(Frame::z_up([1.0, 2.0, 3.0]), radius);
    builder.vertex_face(&sphere, Sense::Forward, &[south, north]);
    let ir = builder.finish(BodyKind::Solid);
    let report = measure(&ir, &Selection::default());
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    assert_measured(
        body.volume().as_ref().expect("volume"),
        4.0 * PI * radius.powi(3) / 3.0,
        radius.powi(3),
    );
    let centroid = body
        .centroid()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("centroid")
        .value()
        .value
        .get();
    for (computed, expected) in [(centroid.x, 1.0), (centroid.y, 2.0), (centroid.z, 3.0)] {
        assert!((computed - expected).abs() <= TEST_ABSOLUTE_FLOOR * 10.0);
    }
    let report = measure(&ir, &faces());
    let measures = only_face(&report).measures();
    assert_eq!(*measures.region(), Some(RegionRule::WholeSurface));
    assert_measured(measures.area(), 4.0 * PI * radius * radius, radius * radius);
    assert_tight_box(
        measures.bbox(),
        [1.0 - radius, 2.0 - radius, 3.0 - radius],
        [1.0 + radius, 2.0 + radius, 3.0 + radius],
        radius,
    );
}

/// The lune of the sphere of radius 10 about the `z` axis between the
/// meridians at longitude 0 and π, the half `y ≤ 0`: up the meridian at
/// longitude 0, down the meridian at longitude π, with the face on the left
/// of the outward normal. The carrier stores `radius`; a negative radius
/// turns its normal inward, and the face sense is reversed to keep the
/// face. With `pole_edges` the loop meets each pole through a degenerate
/// edge; otherwise the meridians meet at the pole vertices.
fn half_lune(builder: &mut Builder, pole_edges: bool, radius: f64) -> [EdgeId; 2] {
    let south = builder.vertex([0.0, 0.0, -10.0]);
    let north = builder.vertex([0.0, 0.0, 10.0]);
    let first = meridian(builder, 10.0, 0.0, [&south, &north]);
    let second = meridian(builder, 10.0, PI, [&south, &north]);
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), radius);
    let sense = if radius > 0.0 {
        Sense::Forward
    } else {
        Sense::Reversed
    };
    let ring = if pole_edges {
        let north_pole = builder.bare_edge(&north, &north);
        let south_pole = builder.bare_edge(&south, &south);
        vec![
            (first.clone(), Sense::Forward),
            (north_pole, Sense::Forward),
            (second.clone(), Sense::Reversed),
            (south_pole, Sense::Forward),
        ]
    } else {
        vec![
            (first.clone(), Sense::Forward),
            (second.clone(), Sense::Reversed),
        ]
    };
    builder.face(&sphere, sense, &[ring], false);
    [first, second]
}

fn assert_half_turn(sweep: &crate::value::Measured<crate::value::Sweep>, start: f64) {
    let value = sweep.value();
    assert!(!value.full);
    assert!((value.value.get() - PI).abs() <= TEST_ABSOLUTE_FLOOR);
    let [[low, high]] = value.intervals.as_slice() else {
        panic!("one interval: {:?}", value.intervals);
    };
    assert!((low.get() - start).abs() <= TEST_ABSOLUTE_FLOOR);
    assert!((high.get() - (start + PI)).abs() <= TEST_ABSOLUTE_FLOOR);
}

#[test]
fn a_lune_half_a_turn_wide_sweeps_half_a_turn() {
    for pole_edges in [false, true] {
        let mut builder = Builder::new();
        half_lune(&mut builder, pole_edges, 10.0);
        let ir = builder.finish(BodyKind::Sheet);
        let report = measure(&ir, &faces());
        let face = only_face(&report);
        let measures = face.measures();
        assert_measured(measures.area(), 200.0 * PI, 400.0);
        let sweep = measures
            .angular_sweep()
            .as_ref()
            .and_then(Quantity::measured)
            .expect("sweep");
        assert_eq!(sweep.reading(), Some(Reading::Declared), "{pole_edges}");
        assert_half_turn(sweep, PI);
        let [candidate] = sweep.candidates() else {
            panic!("one sweep candidate");
        };
        assert_eq!(candidate.reading(), Reading::Complement);
        let intervals = &candidate.value().intervals;
        assert!((candidate.value().value.get() - PI).abs() <= TEST_ABSOLUTE_FLOOR);
        assert!(
            intervals[0][0].get().abs() <= TEST_ABSOLUTE_FLOOR,
            "{intervals:?}"
        );
        assert_eq!(measures.loops()[0].net_shift(), &Some([0, 0]));
    }
}

/// The half ball `y ≤ 0` of radius 10: the lune of [`half_lune`] and the
/// disc in the plane `y = 0` with the outward normal `+y`. The shell sign
/// confirms the lune.
#[test]
fn a_half_ball_cut_through_the_axis_sweeps_half_a_turn() {
    let mut builder = Builder::new();
    let [first, second] = half_lune(&mut builder, false, 10.0);
    let plane = builder.plane([0.0; 3], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
    builder.face(
        &plane,
        Sense::Forward,
        &[vec![(first, Sense::Reversed), (second, Sense::Forward)]],
        false,
    );
    let ir = builder.finish(BodyKind::Solid);
    let report = measure(&ir, &Selection::default());
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    assert_measured(
        body.volume().as_ref().expect("volume"),
        2.0 * PI * 1000.0 / 3.0,
        1000.0,
    );
    let centroid = body
        .centroid()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("centroid")
        .value()
        .value
        .get();
    assert!((centroid.y + 3.75).abs() <= TEST_ABSOLUTE_FLOOR * 10.0);
    let report = measure(&ir, &faces());
    let lune = report
        .faces()
        .iter()
        .find(|face| face.carrier().kind == crate::select::SurfaceKind::Sphere)
        .expect("lune");
    assert_eq!(*lune.orientation().consistent(), Some(true));
    let sweep = lune
        .measures()
        .angular_sweep()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("sweep");
    assert_eq!(sweep.reading(), None);
    assert_half_turn(sweep, PI);
}

/// Two meridian arcs of the sphere `R = 10` from the equator at `u = π` and
/// `u = 2π` to the north pole that run three quarters of a turn through the
/// south pole, joined by the equator arc between them. Each arc passes
/// through the south pole inside its interval, where its parameter image
/// turns by half a period, and no sample lands on the pole.
#[test]
fn an_arc_through_a_pole_inside_its_interval_does_not_invert() {
    let mut builder = Builder::new();
    let north = builder.vertex([0.0, 0.0, 10.0]);
    let start = builder.vertex([-10.0, 0.0, 0.0]);
    let end = builder.vertex([10.0, 0.0, 0.0]);
    let through_south = |builder: &mut Builder, u: f64, from: &VertexId| {
        let (sine, cosine) = u.sin_cos();
        builder.arc(
            Circle {
                frame: Frame {
                    origin: [0.0; 3],
                    axis: [-sine, cosine, 0.0],
                    reference: [cosine, sine, 0.0],
                },
                radius: 10.0,
            },
            [0.0, 1.5 * PI],
            from,
            &north,
        )
    };
    let first = through_south(&mut builder, PI, &start);
    let second = through_south(&mut builder, 2.0 * PI, &end);
    let equator = builder.arc(
        Circle {
            frame: Frame::z_up([0.0; 3]),
            radius: 10.0,
        },
        [PI, 2.0 * PI],
        &start,
        &end,
    );
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), 10.0);
    builder.face(
        &sphere,
        Sense::Forward,
        &[vec![
            (first, Sense::Reversed),
            (equator, Sense::Forward),
            (second, Sense::Forward),
        ]],
        false,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let area = only_face(&report)
        .measures()
        .area()
        .unavailable()
        .expect("unavailable area");
    assert_eq!(area.reason(), UnavailableReason::BoundaryInversionFailed);
    assert_eq!(area.category(), Category::Input);
}

/// The cap `z ≥ 6` of [`cap`] on a carrier that stores the radius −10 with
/// the face sense reversed: the same face, whose chart latitude is the
/// negated latitude about the stored axis.
#[test]
fn a_negative_radius_cap_reads_its_latitude_about_the_stored_axis() {
    let mut builder = Builder::new();
    let vertex = builder.vertex([8.0, 0.0, 6.0]);
    let circle = builder.circle(
        [0.0, 0.0, 6.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        8.0,
        &vertex,
    );
    let sphere = builder.sphere(Frame::z_up([0.0; 3]), -10.0);
    builder.face(
        &sphere,
        Sense::Reversed,
        &[vec![(circle, Sense::Forward)]],
        true,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    assert_measured(measures.area(), 2.0 * PI * 100.0 * 0.4, 400.0);
    assert_range(
        measures.latitude_range().as_ref(),
        [0.6_f64.asin(), FRAC_PI_2],
        1.0,
    );
    let crate::report::CarrierParameters::Sphere {
        axis_direction,
        ref_direction,
        ..
    } = face.carrier().parameters()
    else {
        panic!("sphere parameters");
    };
    let direction = |quantity: &Quantity<crate::value::Point>| {
        quantity.measured().expect("direction").value().value.get()
    };
    assert_eq!(
        direction(axis_direction),
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0)
    );
    assert_eq!(
        direction(ref_direction),
        cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)
    );
}

/// The half lune `y ≤ 0` on carriers that store the radius 10 and −10:
/// longitudes from the stored reference direction about the stored axis,
/// `[π, 2π]` for the face and `[0, π]` for its complement, for both.
#[test]
fn a_negative_radius_lune_sweeps_from_the_stored_reference_direction() {
    for radius in [10.0, -10.0] {
        let mut builder = Builder::new();
        half_lune(&mut builder, false, radius);
        let ir = builder.finish(BodyKind::Sheet);
        let report = measure(&ir, &faces());
        let measures = only_face(&report).measures();
        assert_measured(measures.area(), 200.0 * PI, 400.0);
        let sweep = measures
            .angular_sweep()
            .as_ref()
            .and_then(Quantity::measured)
            .expect("sweep");
        assert_half_turn(sweep, PI);
        let [candidate] = sweep.candidates() else {
            panic!("one sweep candidate");
        };
        let [[low, high]] = candidate.value().intervals.as_slice() else {
            panic!("one interval");
        };
        assert!(low.get().abs() <= TEST_ABSOLUTE_FLOOR, "{radius}");
        assert!((high.get() - PI).abs() <= TEST_ABSOLUTE_FLOOR, "{radius}");
        assert_range(
            measures.latitude_range().as_ref(),
            [-FRAC_PI_2, FRAC_PI_2],
            1.0,
        );
    }
}
