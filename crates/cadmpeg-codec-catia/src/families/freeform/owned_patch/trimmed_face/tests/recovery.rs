// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_ir::geometry::{
    ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite,
    RollingBallJetStation, RollingBallJetStations,
};
use cadmpeg_ir::math::Vector3;

fn cylinder_section() -> ProceduralSurfaceDefinition {
    let derivative = |z| RollingBallJetDerivative {
        first_limit: Vector3::new(0.0, 0.0, z),
        second_limit: Vector3::new(0.0, 0.0, z),
        center: Vector3::new(0.0, 0.0, z),
        angle: 0.0,
    };
    let stations = [0.0, 1.0].map(|z| RollingBallJetStation {
        knot: z,
        multiplicity: 6,
        site: RollingBallJetSite {
            first_limit: Point3::new(2.0, 0.0, z),
            second_limit: Point3::new(0.0, 2.0, z),
            center: Point3::new(0.0, 0.0, z),
            angle: std::f64::consts::FRAC_PI_2,
            first_derivative: derivative(1.0),
            second_derivative: derivative(0.0),
        },
    });
    ProceduralSurfaceDefinition::RollingBallJet(
        RollingBallJetStations::try_new(5, stations.to_vec()).expect("quarter-cylinder jet"),
    )
}

fn sheet(definition: &ProceduralSurfaceDefinition, v_min: f64) -> DecodedFace<'_> {
    let corners = [[0.2, v_min], [0.8, v_min], [0.8, 0.5], [0.2, 0.5]];
    let trims = (0..4)
        .map(|i| {
            let j = (i + 1) % 4;
            Trim {
                edge: 10 + i,
                source: 10 + i,
                vertices: [i, j],
                pcurve: line_pcurve([0.0, 1.0], [corners[i], corners[j]]).expect("trim"),
                range: [0.0, 1.0],
                mapped_range: None,
                affine_map: false,
                spline_range: None,
                stations: [Some(0.0), Some(1.0)],
                primary_stations: [None, None],
                sphere_circle: None,
                alternate_line: None,
                inferred_accuracy: None,
            }
        })
        .collect();
    DecodedFace {
        face_pos: 20,
        member: Member {
            body: (0, 1),
            forward: true,
        },
        carrier: Carrier::RollingBall(definition, 1.0),
        outer: Ring {
            trims,
            bounds: None,
        },
        inner: vec![],
    }
}

fn transfer(patch: Patch) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
    use crate::families::FamilyEntityAdmission;
    use cadmpeg_ir::annotations::AnnotationBuilder;
    let mut ir = CadIr::empty();
    let mut notes = LaneRefusals::new();
    crate::test_support::with_service_context(|ctx| {
        super::super::super::transfer::append(
            &mut ir,
            &mut AnnotationBuilder::new(),
            vec![patch],
            &mut notes,
            &mut FamilyEntityAdmission::new(ctx),
        )
    })
    .expect("transfer recovered patch");
    ir.finalize();
    let validation = cadmpeg_ir::validate_neutral(&ir, vec![]).expect("validate cylinder patch");
    assert!(validation.is_ok(), "{:?}", validation.findings);
    (ir, notes.take_notes())
}

#[test]
fn rolling_extension_recovers_and_reports_a_bounded_cylindrical_strip() {
    let definition = cylinder_section();
    let face = sheet(&definition, -0.0015);
    let patch = face
        .recover_rolling_boundary(&CadIr::empty(), None)
        .expect("bounded extension");
    let expected_gap = 4.0 * (0.0015 * std::f64::consts::FRAC_PI_2 / 2.0).sin();
    assert!((patch.recovery.rolling_extension_mm - expected_gap).abs() < 1.0e-9);
    let (ir, notes) = transfer(patch);
    assert_eq!(ir.model.faces.len(), 1);
    assert_eq!(ir.model.edges.len(), 4);
    let note = notes
        .iter()
        .find(|note| note.code.local_code() == "geometry.rolling-sheet-extended")
        .expect("rolling extension loss");
    assert!(note.message.contains("1 emitted sheet faces"));
    assert!(note.message.contains("0.004712388 mm"));
    assert!(notes
        .iter()
        .any(|note| note.code.local_code() == "geometry.sheet-surface-fitted"));
}

#[test]
fn rolling_extension_rejects_physical_and_transverse_overruns() {
    let definition = cylinder_section();
    for v_min in [-0.002, -0.02] {
        assert!(sheet(&definition, v_min)
            .recover_rolling_boundary(&CadIr::empty(), None)
            .is_none());
    }
}

#[test]
fn rolling_recovery_retains_the_active_spline_pcurve_interval() {
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
    let definition = cylinder_section();
    let mut face = sheet(&definition, -0.0015);
    for trim in &mut face.outer.trims {
        let a = trim.uv(0.0).expect("start");
        let b = trim.uv(1.0).expect("end");
        let at = |t: f64| {
            Point2::new(
                (b[0] - a[0]).mul_add(t, a[0]),
                (b[1] - a[1]).mul_add(t, a[1]),
            )
        };
        trim.pcurve = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![-1.0, -1.0, 2.0, 2.0],
                vec![at(-1.0), at(2.0)],
                None,
                false,
            )
            .expect("extended spline basis"),
        };
    }
    let patch = face
        .recover_rolling_boundary(&CadIr::empty(), None)
        .expect("spline boundary extension");
    assert!(patch
        .sides
        .iter()
        .all(|side| matches!(side.pcurve, PcurveGeometry::Trimmed(_))));
    let (ir, _) = transfer(patch);
    assert_eq!(ir.model.faces.len(), 1);
}
