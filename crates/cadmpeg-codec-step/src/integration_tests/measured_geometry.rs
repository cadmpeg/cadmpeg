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
use cadmpeg_measure::report::MeasureReport;
use cadmpeg_measure::test_support::round_trip::{measured, measures_agree};
use cadmpeg_measure::value::Quantity;
use cadmpeg_measure::MeasureOptions;

use crate::export::write_step;
use crate::test_support::box_brep::{box_shell, solid_representation, BoxFaceUse};
use crate::test_support::exchange::decode_inline;
use crate::{StepCodec, StepSchema, StepWriteOptions};

/// Allowance of a measured value against its expression beyond the value's
/// numeric error, per unit of the larger of the expected magnitude and 1.
const TEST_ABSOLUTE_FLOOR: f64 = 1.0e-9;

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
    let (_, _, after) = measures_agree("placed cube", &ir, decoded.ir())
        .expect("the round trip keeps its measured geometry");
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
    let before = measured("solid with a void", decoded.ir()).expect("the document is measured");
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
    let (_, _, after) = measures_agree("solid with a void", decoded.ir(), round_trip.ir())
        .expect("the round trip keeps its measured geometry");
    let (volume, error) = body_volume(&after);
    assert_close(volume, 976.0, error);
}

/// A solid cylinder with radius 10 and height 30, whose closed circle edges
/// store no parameter range, keeps its measured geometry through a STEP
/// write round trip under every written schema.
#[test]
fn solid_cylinder_round_trip_keeps_measured_geometry() {
    let ir = cadmpeg_test_support::solids::solid_cylinder();
    for schema in [StepSchema::Ap214, StepSchema::Ap242Edition3] {
        let mut output = Vec::new();
        write_step(&ir, &mut output, schema, &StepWriteOptions::default())
            .expect("write the cylinder");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("decode the written cylinder");
        assert_eq!(decoded.ir().model.bodies.len(), ir.model.bodies.len());
        measures_agree("cylinder", &ir, decoded.ir())
            .expect("the round trip keeps its measured geometry");
    }
}

/// The unit-square NURBS patch less the `0.5 x 0.5` hole, whose outer
/// boundary is the patch's implicit parameter rectangle, keeps its area
/// `1 - 0.25` through a STEP write round trip in either face sense: the
/// written outer bound winds with the face about its oriented normal.
#[test]
fn implicit_outer_patch_round_trip_keeps_the_face_domain() {
    use crate::test_support::curve_bounded::{implicit_outer_sheet, unit_square_patch, HOLE_SIDE};
    for sense in [
        cadmpeg_ir::topology::Sense::Forward,
        cadmpeg_ir::topology::Sense::Reversed,
    ] {
        let ir = implicit_outer_sheet(unit_square_patch(), sense);
        let mut output = Vec::new();
        write_step(
            &ir,
            &mut output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .expect("write the patch");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("decode the written patch");
        let (_, before, after) = measures_agree("implicit outer patch", &ir, decoded.ir())
            .expect("the round trip keeps its measured geometry");
        for report in [&before, &after] {
            let [body] = report.bodies().as_slice() else {
                panic!("one body row");
            };
            let area = body
                .area()
                .as_ref()
                .and_then(Quantity::measured)
                .expect("measured area");
            assert_close(
                area.value().value.get(),
                1.0 - HOLE_SIDE * HOLE_SIDE,
                area.numeric_error().get(),
            );
        }
    }
}

/// A wedge of a plate and a post joined by a concave fillet on a torus of
/// major radius 6 and minor radius -2. The fillet is the quarter of the tube
/// that faces the axis and the plate.
fn fillet_wedge(angles: [f64; 2]) -> CadIr {
    use crate::test_support::revolved_wedge::{revolved_wedge, Piece};
    revolved_wedge(
        [1.0, 0.0],
        &[
            Piece::Line { to: [12.0, 0.0] },
            Piece::Line { to: [12.0, 2.0] },
            Piece::Line { to: [6.0, 2.0] },
            Piece::Arc {
                to: [4.0, 4.0],
                center: [6.0, 4.0],
                material_inside: false,
            },
            Piece::Line { to: [4.0, 10.0] },
            Piece::Line { to: [1.0, 10.0] },
            Piece::Line { to: [1.0, 0.0] },
        ],
        angles,
    )
}

/// Half-angle of the groove arc of [`groove_wedge`], which is centered on the
/// outward radial direction.
const GROOVE_HALF_ANGLE: f64 = std::f64::consts::FRAC_PI_3;

/// A wedge of a ring wall with a round groove on a torus of major radius 6 and
/// minor radius -2. Under the written minor radius of 2, the groove lies on
/// both sides of the latitude seam.
fn groove_wedge(angles: [f64; 2]) -> CadIr {
    use crate::test_support::revolved_wedge::{revolved_wedge, Piece};
    let wall = 6.0 + 2.0 * GROOVE_HALF_ANGLE.cos();
    let reach = 2.0 * GROOVE_HALF_ANGLE.sin();
    revolved_wedge(
        [wall, 0.0],
        &[
            Piece::Line { to: [12.0, 0.0] },
            Piece::Line { to: [12.0, 10.0] },
            Piece::Line { to: [wall, 10.0] },
            Piece::Line {
                to: [wall, 5.0 + reach],
            },
            Piece::Arc {
                to: [wall, 5.0 - reach],
                center: [6.0, 5.0],
                material_inside: false,
            },
            Piece::Line { to: [wall, 0.0] },
        ],
        angles,
    )
}

/// A wedge of a plate with a spherical dish of radius -13 centered on the axis
/// at `z = 15`, which meets the plate top at `rho = 12`.
fn dish_wedge(angles: [f64; 2]) -> CadIr {
    use crate::test_support::revolved_wedge::{revolved_wedge, Piece};
    revolved_wedge(
        [1.0, 0.0],
        &[
            Piece::Line { to: [14.0, 0.0] },
            Piece::Line { to: [14.0, 10.0] },
            Piece::Line { to: [12.0, 10.0] },
            Piece::Arc {
                to: [1.0, 15.0 - 168.0_f64.sqrt()],
                center: [0.0, 15.0],
                material_inside: false,
            },
            Piece::Line { to: [1.0, 0.0] },
        ],
        angles,
    )
}

/// The angular windows of the wedges: one off the longitude seam, one across it.
const WEDGE_WINDOWS: [[f64; 2]; 2] = [[0.3, 0.3 + std::f64::consts::FRAC_PI_2], [-0.7, 0.7]];

fn written_and_decoded(ir: &CadIr) -> (String, CadIr) {
    let mut output = Vec::new();
    write_step(
        ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write the wedge");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode the written wedge")
        .into_parts()
        .0;
    (text, decoded)
}

/// The `same_sense` of the one written face on the record named `entity`.
fn same_sense_on(text: &str, entity: &str) -> bool {
    let record = |line: &str| line.split(" = ").next().map(str::to_owned);
    let surface = text
        .lines()
        .find(|line| line.contains(entity))
        .and_then(record)
        .expect("written carrier");
    let face = text
        .lines()
        .find(|line| line.contains("ADVANCED_FACE") && line.contains(&format!(",{surface},")))
        .expect("written face");
    face.contains(".T.")
}

/// The volume of the one body of `ir`: its measured value, or else the value
/// its face senses state.
fn volume_by_face_sense(ir: &CadIr) -> (f64, f64) {
    use cadmpeg_measure::value::Reading;
    let report = measured("wedge", ir).expect("measure the wedge");
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    let volume = body.volume().as_ref().expect("a solid has a volume");
    if let Some(value) = volume.measured() {
        return (value.value().value.get(), value.numeric_error().get());
    }
    let candidate = volume
        .unavailable()
        .expect("volume is measured or unavailable")
        .candidates()
        .iter()
        .find(|candidate| candidate.reading() == Reading::FaceSense)
        .expect("a face-sense reading");
    (
        candidate.value().value.get(),
        candidate.numeric_error().get(),
    )
}

/// The report of `ir` with a row for every face.
fn measured_faces(ir: &CadIr) -> MeasureReport {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).expect("context");
    let input = cadmpeg_measure::MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let selection = cadmpeg_measure::Selection {
        all_faces: true,
        ..cadmpeg_measure::Selection::default()
    };
    let report = cadmpeg_measure::Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &selection)
        .expect("measure the faces");
    ctx.finish_session().expect("session stays unrefused");
    report
}

/// The integral of `rho` over the fillet profile by Pappus: two rectangles and
/// the corner square, less the quarter disk of radius 2 about `rho = 6`.
#[test]
fn concave_fillet_on_a_negative_minor_torus_keeps_its_volume() {
    let moment = 143.0 + 60.0 + 20.0 - (6.0 * std::f64::consts::PI - 8.0 / 3.0);
    for angles in WEDGE_WINDOWS {
        let ir = fillet_wedge(angles);
        let (text, decoded) = written_and_decoded(&ir);
        assert!(!same_sense_on(&text, "TOROIDAL_SURFACE"), "{text}");
        for model in [&ir, &decoded] {
            let (volume, numeric) = volume_by_face_sense(model);
            assert_close(volume, moment * (angles[1] - angles[0]), numeric);
        }
    }
}

/// The wall's moment by Pappus less the groove's circular segment of minor
/// radius 2. The complement reading adds the tube's whole volume, which is how
/// a reader that takes the region off the other side of the loops reads it.
#[test]
fn groove_across_the_written_seams_keeps_its_volume_and_area() {
    let radius = 2.0_f64;
    let major = 6.0_f64;
    let (sine, cosine) = GROOVE_HALF_ANGLE.sin_cos();
    let wall = major + radius * cosine;
    let wall_moment = 10.0 * (144.0 - wall * wall) / 2.0;
    let segment_moment = major * radius * radius * (GROOVE_HALF_ANGLE - sine * cosine)
        + 2.0 / 3.0 * radius.powi(3) * sine.powi(3);
    for angles in WEDGE_WINDOWS {
        let span = angles[1] - angles[0];
        let ir = groove_wedge(angles);
        let (text, decoded) = written_and_decoded(&ir);
        assert!(!same_sense_on(&text, "TOROIDAL_SURFACE"), "{text}");
        let (volume, numeric) = volume_by_face_sense(&decoded);
        assert_close(volume, (wall_moment - segment_moment) * span, numeric);

        let report = measured_faces(&decoded);
        let face_area = report
            .faces()
            .iter()
            .find(|face| face.name().as_deref() == Some("swept-4"))
            .and_then(|face| face.measures().area().measured())
            .expect("the groove face has a measured area");
        assert_close(
            face_area.value().value.get(),
            span * radius * (major * 2.0 * GROOVE_HALF_ANGLE + radius * 2.0 * sine),
            face_area.numeric_error().get(),
        );
    }
}

/// The plate's moment less the dish's, by integrating `rho * depth` from
/// `rho = 1` to `rho = 12`, where the depth is `sqrt(169 - rho^2) - 5`.
#[test]
fn concave_dish_on_a_negative_radius_sphere_keeps_its_volume() {
    let plate = 10.0 * (196.0 - 1.0) / 2.0;
    let dish = (168.0_f64.powf(1.5) - 125.0) / 3.0 - 5.0 * (144.0 - 1.0) / 2.0;
    for angles in WEDGE_WINDOWS {
        let ir = dish_wedge(angles);
        let (text, decoded) = written_and_decoded(&ir);
        assert!(!same_sense_on(&text, "SPHERICAL_SURFACE"), "{text}");
        let (volume, numeric) = volume_by_face_sense(&decoded);
        assert_close(volume, (plate - dish) * (angles[1] - angles[0]), numeric);
    }
}
