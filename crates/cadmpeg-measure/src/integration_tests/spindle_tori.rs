// SPDX-License-Identifier: Apache-2.0
//! Faces on the selected sheets of the spindle torus `R = 1`, `r = 5` about
//! `+z`: whole sheets, bands and caps cut by planes `z = c`, lunes between
//! meridians and the lemon solid, with expectations from the surface of
//! revolution.
//!
//! A sheet's pinch points are the poles `z = ±h`, `h = √(r² − R²)`, where
//! `cos v = −R/r`. The apple sheet is `v ∈ [−a, a]` and the lemon sheet is
//! `v ∈ [a, 2π − a]` with `a = acos(−R/r)`. A sheet's area between `v0` and
//! `v1` is `2π·∫ r·|R + r cos v| dv`. The plane `z = c` meets the lemon at
//! `v = π − asin(c/r)`, on the circle of radius `√(r² − c²) − R`, and the
//! apple at `v = asin(c/r)`, on the circle of radius `R + √(r² − c²)` for
//! `|c| < h`.

use std::f64::consts::{PI, TAU};

use cadmpeg_ir::ids::{EdgeId, SurfaceId, VertexId};
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::UnavailableReason;
use crate::report::{FaceMeasures, MeasureReport, RegionRule};
use crate::select::Selection;
use crate::test_support::builders::{assert_measured, measure, Builder, Circle, Frame};
use crate::value::Unavailable;

const MAJOR: f64 = 1.0;
const MINOR: f64 = 5.0;

/// The area scale of the tests' allowances.
const AREA_SCALE: f64 = MINOR * MINOR;

/// The volume scale of the tests' allowances.
const VOLUME_SCALE: f64 = MINOR * MINOR * MINOR;

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

/// The pole height `h`.
fn height() -> f64 {
    MINOR.mul_add(MINOR, -MAJOR * MAJOR).sqrt()
}

/// The pole angle `a = acos(−R/r)`.
fn pole_angle() -> f64 {
    (-MAJOR / MINOR).acos()
}

/// `∫ r·(R + r cos v) dv` from `v0` to `v1`.
fn profile_integral(v0: f64, v1: f64) -> f64 {
    MINOR * (MAJOR * (v1 - v0) + MINOR * (v1.sin() - v0.sin()))
}

/// The area of the apple sheet between `v0 ≤ v1`, where `R + r cos v ≥ 0`.
fn apple_area(v0: f64, v1: f64) -> f64 {
    TAU * profile_integral(v0, v1)
}

/// The area of the lemon sheet between `v0 ≤ v1`, where `R + r cos v ≤ 0`.
fn lemon_area(v0: f64, v1: f64) -> f64 {
    -TAU * profile_integral(v0, v1)
}

/// The lemon's `v` at the plane `z = c`.
fn lemon_v(c: f64) -> f64 {
    PI - (c / MINOR).asin()
}

/// `∫ √(r² − z²) dz` from 0 to `z`.
fn half_chord_integral(z: f64) -> f64 {
    0.5 * (z * MINOR.mul_add(MINOR, -z * z).sqrt() + MINOR * MINOR * (z / MINOR).asin())
}

/// The volume of the lemon solid above `z = c`:
/// `π·∫_c^h (√(r² − z²) − R)² dz`.
fn lemon_volume_above(c: f64) -> f64 {
    let h = height();
    PI * ((MINOR * MINOR + MAJOR * MAJOR) * (h - c)
        - (h.powi(3) - c.powi(3)) / 3.0
        - 2.0 * MAJOR * (half_chord_integral(h) - half_chord_integral(c)))
}

/// The circle of the plane `z = c` with `radius`, counterclockwise about
/// `+z`, through a new vertex on `+x`.
fn parallel(builder: &mut Builder, c: f64, radius: f64) -> EdgeId {
    let vertex = builder.vertex([radius, 0.0, c]);
    builder.circle(
        [0.0, 0.0, c],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        radius,
        &vertex,
    )
}

/// The lemon's circle at `z = c`.
fn lemon_parallel(builder: &mut Builder, c: f64) -> EdgeId {
    let radius = MINOR.mul_add(MINOR, -c * c).sqrt() - MAJOR;
    parallel(builder, c, radius)
}

fn sheet(builder: &mut Builder, outer: bool) -> SurfaceId {
    builder.degenerate_torus(Frame::z_up([0.0; 3]), MAJOR, MINOR, outer)
}

/// The opposite of `sense`.
fn flip(sense: Sense) -> Sense {
    match sense {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    }
}

#[test]
fn whole_spindle_sheets_have_their_closed_form_areas() {
    let a = pole_angle();
    for (outer, expected) in [(true, apple_area(-a, a)), (false, lemon_area(a, TAU - a))] {
        let mut builder = Builder::new();
        let surface = sheet(&mut builder, outer);
        builder.face(&surface, Sense::Forward, &[], false);
        let report = measure(&builder.finish(BodyKind::Sheet), &faces());
        let measures = only_face(&report).measures();
        assert_eq!(*measures.region(), Some(RegionRule::WholeSurface));
        assert_measured(measures.area(), expected, AREA_SCALE);
    }
}

/// The lemon band between `z = −2` and `z = 3`, and the apple band between
/// `z = −1` and `z = 2`, on faces of either sense.
#[test]
fn bands_between_two_planes_have_the_area_of_their_v_range() {
    for face_sense in [Sense::Forward, Sense::Reversed] {
        let keep = |sense: Sense| match face_sense {
            Sense::Forward => sense,
            Sense::Reversed => flip(sense),
        };
        let (low, high) = (-2.0, 3.0);
        let mut builder = Builder::new();
        let lower = lemon_parallel(&mut builder, low);
        let upper = lemon_parallel(&mut builder, high);
        let surface = sheet(&mut builder, false);
        builder.face(
            &surface,
            face_sense,
            &[
                vec![(upper, keep(Sense::Forward))],
                vec![(lower, keep(Sense::Reversed))],
            ],
            false,
        );
        let report = measure(&builder.finish(BodyKind::Sheet), &faces());
        let measures = only_face(&report).measures();
        assert_eq!(*measures.region(), Some(RegionRule::DeclaredOrientation));
        assert_measured(
            measures.area(),
            lemon_area(lemon_v(high), lemon_v(low)),
            AREA_SCALE,
        );

        let (low, high) = (-1.0, 2.0);
        let radius = |c: f64| MAJOR + MINOR.mul_add(MINOR, -c * c).sqrt();
        let mut builder = Builder::new();
        let lower = parallel(&mut builder, low, radius(low));
        let upper = parallel(&mut builder, high, radius(high));
        let surface = sheet(&mut builder, true);
        builder.face(
            &surface,
            face_sense,
            &[
                vec![(lower, keep(Sense::Forward))],
                vec![(upper, keep(Sense::Reversed))],
            ],
            false,
        );
        let report = measure(&builder.finish(BodyKind::Sheet), &faces());
        let measures = only_face(&report).measures();
        assert_measured(
            measures.area(),
            apple_area((low / MINOR).asin(), (high / MINOR).asin()),
            AREA_SCALE,
        );
    }
}

/// The lemon cap above `z = c` holds the upper pole inside its region; the
/// face with the opposite loop holds the lower pole.
#[test]
fn lemon_caps_reach_the_pole_beyond_their_loop() {
    let c = 1.5;
    let a = pole_angle();
    for (sense, expected) in [
        (Sense::Reversed, lemon_area(a, lemon_v(c))),
        (Sense::Forward, lemon_area(lemon_v(c), TAU - a)),
    ] {
        let mut builder = Builder::new();
        let circle = lemon_parallel(&mut builder, c);
        let surface = sheet(&mut builder, false);
        builder.face(&surface, Sense::Forward, &[vec![(circle, sense)]], false);
        let report = measure(&builder.finish(BodyKind::Sheet), &faces());
        let measures = only_face(&report).measures();
        assert_measured(measures.area(), expected, AREA_SCALE);
    }
}

/// The lemon meridian at the angle `u` about `+z`, from the lower pole to
/// the upper pole: the arc of the circle of radius `r` about `−R` along
/// `(cos u, sin u, 0)` where its distance from the axis is not negative.
fn lemon_meridian(builder: &mut Builder, u: f64, poles: [&VertexId; 2]) -> EdgeId {
    let (sine, cosine) = u.sin_cos();
    let half = (MAJOR / MINOR).acos();
    builder.arc(
        Circle {
            frame: Frame {
                origin: [-MAJOR * cosine, -MAJOR * sine, 0.0],
                axis: [sine, -cosine, 0.0],
                reference: [cosine, sine, 0.0],
            },
            radius: MINOR,
        },
        [-half, half],
        poles[0],
        poles[1],
    )
}

/// The lemon lune between the meridians at the angles 0 and 1 about `+z`,
/// which meet at both poles, with degenerate pole edges and with bare pole
/// vertices. The lemon's `u` is the angle plus `π` and its `v` grows toward
/// `−z`, so the loop runs down the meridian at 1 and up the meridian at 0.
#[test]
fn a_lemon_lune_between_two_meridians_covers_its_share_of_the_sheet() {
    let a = pole_angle();
    let expected = lemon_area(a, TAU - a) / TAU;
    for pole_edges in [true, false] {
        let mut builder = Builder::new();
        let south = builder.vertex([0.0, 0.0, -height()]);
        let north = builder.vertex([0.0, 0.0, height()]);
        let first = lemon_meridian(&mut builder, 0.0, [&south, &north]);
        let second = lemon_meridian(&mut builder, 1.0, [&south, &north]);
        let surface = sheet(&mut builder, false);
        let ring = if pole_edges {
            let south_pole = builder.bare_edge(&south, &south);
            let north_pole = builder.bare_edge(&north, &north);
            vec![
                (second, Sense::Reversed),
                (south_pole, Sense::Forward),
                (first, Sense::Forward),
                (north_pole, Sense::Forward),
            ]
        } else {
            vec![(second, Sense::Reversed), (first, Sense::Forward)]
        };
        let face = builder.face(&surface, Sense::Forward, &[ring], false);
        if pole_edges {
            builder.line_pcurve(&face, [0, 1], [PI + 1.0, TAU - a], [-1.0, 0.0], [0.0, 1.0]);
            builder.line_pcurve(&face, [0, 3], [PI, a], [1.0, 0.0], [0.0, 1.0]);
        }
        let report = measure(&builder.finish(BodyKind::Sheet), &faces());
        let measures = only_face(&report).measures();
        assert_measured(measures.area(), expected, AREA_SCALE);
    }
}

/// On the lemon `S_u × S_v` points toward the axis, so a lemon solid's
/// faces are reversed. The whole lemon encloses
/// `π·∫_{−h}^{h} (√(r² − z²) − R)² dz`; the cap above `z = c`, closed by
/// the disk at `z = c`, encloses the part above the plane.
#[test]
fn lemon_solids_enclose_their_solids_of_revolution() {
    let mut builder = Builder::new();
    let surface = sheet(&mut builder, false);
    builder.face(&surface, Sense::Reversed, &[], false);
    let report = measure(&builder.finish(BodyKind::Solid), &Selection::default());
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    assert_measured(
        body.volume().as_ref().expect("volume"),
        lemon_volume_above(-height()),
        VOLUME_SCALE,
    );

    let c = 1.5;
    let mut builder = Builder::new();
    let circle = lemon_parallel(&mut builder, c);
    let surface = sheet(&mut builder, false);
    builder.face(
        &surface,
        Sense::Reversed,
        &[vec![(circle.clone(), Sense::Forward)]],
        false,
    );
    let floor = builder.plane([0.0, 0.0, c], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.face(
        &floor,
        Sense::Reversed,
        &[vec![(circle, Sense::Reversed)]],
        false,
    );
    let report = measure(&builder.finish(BodyKind::Solid), &Selection::default());
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    assert_measured(
        body.volume().as_ref().expect("volume"),
        lemon_volume_above(c),
        VOLUME_SCALE,
    );
}

/// A trimmed face on a horn torus or on a spindle torus without a selected
/// sheet has no region.
fn unselected_band(major: f64) -> CadIr {
    let mut builder = Builder::new();
    let lower = parallel(
        &mut builder,
        -1.0,
        major + MINOR.mul_add(MINOR, -1.0).sqrt(),
    );
    let upper = parallel(&mut builder, 1.0, major + MINOR.mul_add(MINOR, -1.0).sqrt());
    let surface = builder.torus(Frame::z_up([0.0; 3]), major, MINOR);
    builder.face(
        &surface,
        Sense::Forward,
        &[
            vec![(lower, Sense::Forward)],
            vec![(upper, Sense::Reversed)],
        ],
        false,
    );
    builder.finish(BodyKind::Sheet)
}

#[test]
fn horn_tori_and_unselected_spindle_tori_stay_unclassified() {
    for major in [MINOR, MAJOR] {
        let report = measure(&unselected_band(major), &faces());
        let measures = only_face(&report).measures();
        assert_eq!(
            measures.area().unavailable().map(Unavailable::reason),
            Some(UnavailableReason::RegionDomainUnsupported),
            "{major}"
        );
    }
}
