// SPDX-License-Identifier: Apache-2.0
//! STEP writer cone chart and apex tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::{curve_point_solved, surface_point_solved};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::analytic::ConeSurface;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

use super::reports::edgeless_doc;

const EPS_ZERO_ANGLE_CONE_POINT: f64 = 1.0e-9;
const ZERO_ANGLE_CONE_SAMPLES: [(f64, f64); 4] = [(0.0, 0.0), (0.3, 0.7), (-1.2, 1.1), (2.5, -0.4)];

/// A unit cube whose first surface is a semi-angle 0 cone over a section of
/// `radius` and `ratio`.
fn zero_angle_cone_cube(radius: f64, ratio: f64) -> (CadIr, SolvedSurfaceGeometry) {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let cone = SolvedSurfaceGeometry::Cone(
        ConeSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
            ratio,
            0.0,
        )
        .unwrap(),
    );
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(cone.clone());
    (ir, cone)
}

/// Writes `ir` and decodes the file back, with the report and the STEP text.
fn zero_angle_cone_round_trip(ir: &CadIr) -> (ExportReport, String, CadIr) {
    let mut output = Vec::new();
    let report = write_step(
        ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("a semi-angle 0 cone over a positive section is writable");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written cone")
        .into_parts()
        .0;
    (report, text, decoded)
}

fn assert_no_cone_reduction(report: &ExportReport, text: &str) {
    assert!(!text.contains("CONICAL_SURFACE"), "{text}");
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::EllipticalConeReduced.kind()),
        "{:?}",
        report.losses
    );
}

fn distance(left: Point3, right: Point3) -> f64 {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
}

#[test]
fn zero_angle_circular_cone_round_trips_as_its_cylinder() {
    let (ir, source) = zero_angle_cone_cube(2.0, 1.0);
    let (report, text, decoded) = zero_angle_cone_round_trip(&ir);

    assert!(text.contains("CYLINDRICAL_SURFACE("), "{text}");
    assert_no_cone_reduction(&report, &text);
    let written = decoded
        .model
        .surfaces
        .iter()
        .find_map(|surface| match surface.geometry.solved() {
            Some(cylinder @ SolvedSurfaceGeometry::Cylinder(_)) => Some(cylinder),
            _ => None,
        })
        .expect("written cylinder surface");
    for (u, v) in ZERO_ANGLE_CONE_SAMPLES {
        let source_point = surface_point_solved(EvaluationAdmission::Standard, &source, u, v)
            .expect("source point")
            .get();
        let written_point = surface_point_solved(EvaluationAdmission::Standard, written, u, v)
            .expect("written point")
            .get();
        assert!(distance(source_point, written_point) < EPS_ZERO_ANGLE_CONE_POINT);
    }
}

#[test]
fn zero_angle_elliptical_cone_round_trips_as_an_extruded_ellipse() {
    let (ir, source) = zero_angle_cone_cube(2.0, 0.4);
    let (report, text, decoded) = zero_angle_cone_round_trip(&ir);

    assert!(text.contains("SURFACE_OF_LINEAR_EXTRUSION("), "{text}");
    assert!(text.contains(",2.,0.8)"), "{text}");
    assert_no_cone_reduction(&report, &text);
    let sweep = decoded
        .model
        .procedural_surfaces
        .iter()
        .find_map(|surface| match surface.definition() {
            ProceduralSurfaceDefinition::LinearSweep(sweep) => Some(sweep),
            _ => None,
        })
        .expect("written linear extrusion");
    let section = decoded
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == sweep.directrix())
        .and_then(|curve| curve.geometry.solved())
        .expect("extruded section curve");
    assert!(matches!(section, SolvedCurveGeometry::Ellipse(_)));
    let extrusion = sweep.direction().get();
    for (u, v) in ZERO_ANGLE_CONE_SAMPLES {
        let source_point = surface_point_solved(EvaluationAdmission::Standard, &source, u, v)
            .expect("source point")
            .get();
        let section_point = curve_point_solved(EvaluationAdmission::Standard, section, u)
            .expect("section point")
            .get();
        let written_point = Point3::new(
            section_point.x + v * extrusion.x,
            section_point.y + v * extrusion.y,
            section_point.z + v * extrusion.z,
        );
        assert!(distance(source_point, written_point) < EPS_ZERO_ANGLE_CONE_POINT);
    }
}

#[test]
fn zero_angle_cone_of_zero_radius_omits_the_unsupported_face() {
    let (ir, _) = zero_angle_cone_cube(0.0, 1.0);
    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(StepSchema::Ap214.descriptor().id.as_str()),
        )
        .expect("a line carrier does not refuse the document");
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == StepLossCode::UnknownSurfaceFaceOmitted.kind())
        .expect("unsupported face is reported");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Error);
}

#[test]
fn point_edge_ring_is_written_as_vertex_loop() {
    let mut ir = edgeless_doc();
    ir.model
        .vertices
        .retain(|vertex| vertex.id.as_str() != "test:model:vertex#v1");
    ir.model
        .points
        .retain(|point| point.id.as_str() != "test:model:point#p1");
    let point_curve = CurveId::mint("test:model:curve#point").expect("identity grammar");
    ir.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: point_curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
            cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(Point3::new(0.0, 0.0, 0.0))
                .unwrap(),
        )),
        source_object: None,
    });
    let edge = &mut ir.model.edges[0];
    edge.carrier = cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(point_curve));
    edge.end = edge.start.clone();

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write point loop");
    let text = String::from_utf8(output).expect("STEP output is UTF-8");

    assert_eq!(text.matches("VERTEX_LOOP('',").count(), 1, "{text}");
    assert!(!text.contains("EDGE_LOOP"), "{text}");
    assert!(!text.contains("POLYLINE"), "{text}");
    for code in [
        StepLossCode::EdgeNo3dCurve,
        StepLossCode::CurvelessEdgeOmitted,
        StepLossCode::GeometryCarrierNotWritten,
        StepLossCode::FaceOmittedUnclassifiedLoop,
        StepLossCode::LoopEdgeNotWritable,
    ] {
        assert!(
            !report.losses.iter().any(|loss| loss.code == code.kind()),
            "{}: {:?}",
            code.kind(),
            report.losses
        );
    }
}

#[test]
fn negative_cone_face_pcurve_is_written_in_reversed_chart() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode sheet pcurve")
        .into_parts()
        .0;
    let surface_id = ir.model.faces[0].surface.clone();
    let surface = ir
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .expect("face surface");
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            -0.5,
        )
        .expect("negative cone"),
    ));

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write cone pcurve");
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("PCURVE("));
    // The half-turn of the chart is applied to the pcurve itself.
    assert!(!text.contains("CURVE_REPLICA("));
    assert!(!text.contains("CARTESIAN_TRANSFORMATION_OPERATOR_2D("));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PcurveCarrierUnwritable.kind()));
}

#[test]
fn unmappable_negative_cone_pcurve_loss_names_its_face() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../../../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode sheet pcurve")
        .into_parts()
        .0;
    let face_id = ir.model.faces[0].id.clone();
    let surface_id = ir.model.faces[0].surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .expect("face surface")
        .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            -0.5,
        )
        .expect("negative cone"),
    ));
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::pcurve::PcurveGeometry::Harmonic(
        cadmpeg_ir::geometry::pcurve::HarmonicPcurve::try_new(
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
            cadmpeg_ir::math::Point2::new(0.0, 1.0),
        )
        .expect("harmonic pcurve"),
    );

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write cone without unmappable pcurve");
    assert!(!String::from_utf8(output)
        .expect("STEP output is UTF-8")
        .contains("PCURVE("));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::PcurveCarrierUnwritable.kind()
            && loss.message.contains(face_id.as_str())
            && loss.message.contains(ir.model.pcurves[0].id.as_str())
    }));
}
