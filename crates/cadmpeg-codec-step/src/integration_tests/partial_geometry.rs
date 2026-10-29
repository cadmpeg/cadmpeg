// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{DirectedParameterRange, SolvedSurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{BodyKind, EdgeCarrier, PcurveUse};

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

#[test]
fn a_solid_with_unwritable_faces_keeps_the_other_faces_as_a_sheet() {
    let mut ir = unit_cube().expect("unit cube");
    let removed_curve = ir.model.edges[0].curve().expect("curve").clone();
    ir.model.edges[0].carrier = EdgeCarrier::Free;
    ir.model.curves.retain(|curve| curve.id != removed_curve);

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("partial export");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::RegionSolidReducedToSheet.kind()));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("partial STEP decodes");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(decoded.ir().model.faces.len(), 4);
}

#[test]
fn recovered_wire_keeps_its_body_placement() {
    let mut ir = unit_cube().unwrap();
    let retained = ir.model.edges[0].curve().unwrap().clone();
    for edge in ir.model.edges.iter_mut().skip(1) {
        edge.carrier = EdgeCarrier::Free;
    }
    ir.model.curves.retain(|curve| curve.id == retained);
    ir.model.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [0.0, -1.0, 0.0, 15.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 2.0],
        ])
        .unwrap(),
    );
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::RegionEdgesRecovered.kind()));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::RegionVerticesRecovered.kind()));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    let wire = decoded
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.kind == BodyKind::Wire)
        .unwrap();
    assert_eq!(wire.transform, ir.model.bodies[0].transform);
    assert_eq!(decoded.ir().model.edges.len(), 1);
}

#[test]
fn a_mesh_only_export_is_not_reported_as_empty() {
    use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};

    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.tessellations.push(
        Tessellation::new(
            TessellationId::mint("test:recovery:tessellation#triangle").unwrap(),
            TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap(),
    );
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::NoExportableSolids.kind()));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.tessellations.len(), 1);
}

#[test]
fn a_fitting_resource_limit_is_reported_by_the_encoder() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::geometry::pcurve::PlacedPcurve;
    use cadmpeg_ir::geometry::{
        CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralCurveDefinition, SupportPcurve, SurfaceCurveFamily, MAX_GEOMETRY_NESTING,
    };
    use cadmpeg_ir::ids::ProceduralCurveId;

    let mut ir = unit_cube().unwrap();
    let mut pcurve = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
    );
    for _ in 0..MAX_GEOMETRY_NESTING {
        pcurve = PcurveGeometry::Transformed(
            PlacedPcurve::try_new(
                Box::new(pcurve),
                cadmpeg_ir::transform::Transform2::identity(),
            )
            .unwrap(),
        );
    }
    let construction = ProceduralCurveId::mint("test:recovery:construction#deep").unwrap();
    let context = IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: Some(ir.model.faces[0].surface.clone()),
                pcurve: Some(SupportPcurve::new(pcurve, None)),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 10.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.model.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.model.curves[0].geometry = CurveGeometry::Procedural {
        construction,
        cache: None,
    };
    let result = StepCodec::default().plan(EncodeInput::new(&ir, None), TargetRequest::Inherit);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
    ));
}

#[test]
fn a_face_with_an_unwritable_inner_boundary_is_not_filled_in() {
    let mut ir = unit_cube().unwrap();
    let outer = ir.model.faces[0].loops.iter().next().unwrap().clone();
    let inner = cadmpeg_ir::ids::LoopId::mint("test:recovery:loop#missing").unwrap();
    ir.model.faces[0].loops = cadmpeg_ir::topology::FaceLoops::classified(outer, vec![inner]);
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::FaceIncompleteBounds.kind()));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.faces.len(), 5);
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.kind != BodyKind::Solid));
}

#[test]
fn a_pcurve_recovers_a_missing_cube_edge_in_both_directions() {
    for (reverse, fitted, explicit_range) in [
        (false, false, true),
        (true, false, true),
        (false, true, true),
        (true, true, true),
        (false, false, false),
        (true, false, false),
    ] {
        let original = unit_cube().expect("unit cube");
        let mut ir = original.clone();
        let edge = &ir.model.edges[0];
        let edge_id = edge.id.clone();
        let removed_curve = edge.curve().expect("curve").clone();
        let coedge_index = ir
            .model
            .coedges
            .iter()
            .position(|coedge| coedge.edge == edge_id)
            .unwrap();
        let coedge = &ir.model.coedges[coedge_index];
        let lp = ir
            .model
            .loops
            .iter()
            .find(|lp| lp.id == coedge.owner_loop)
            .unwrap();
        let face = ir
            .model
            .faces
            .iter()
            .find(|face| face.id == lp.face)
            .unwrap();
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .unwrap();
        let Some(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved() else {
            panic!("cube plane")
        };
        let origin = plane.origin().get();
        let u = *plane.frame().reference().as_raw();
        let v = plane.frame().axis().as_raw().cross(u);
        let uv = |point: Point3| {
            let d = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
            Point2::new(d.dot(u), d.dot(v))
        };
        let (a, b) = if reverse {
            (
                uv(Point3::new(10.0, 0.0, 0.0)),
                uv(Point3::new(0.0, 0.0, 0.0)),
            )
        } else {
            (
                uv(Point3::new(0.0, 0.0, 0.0)),
                uv(Point3::new(10.0, 0.0, 0.0)),
            )
        };
        let pcurve_id = PcurveId::mint("test:recovery:pcurve#edge").unwrap();
        let direction = Point2::new((b.u - a.u) / 2.0, (b.v - a.v) / 2.0);
        let line_origin = Point2::new(a.u - 2.0 * direction.u, a.v - 2.0 * direction.v);
        let line = PcurveGeometry::Line(LinePcurve::try_new(line_origin, direction).unwrap());
        let geometry = if fitted {
            PcurveGeometry::Transformed(
                cadmpeg_ir::geometry::pcurve::PlacedPcurve::try_new(
                    Box::new(line),
                    cadmpeg_ir::transform::Transform2::identity(),
                )
                .unwrap(),
            )
        } else {
            line
        };
        ir.model.pcurves.push(Pcurve {
            native_ref: None,
            id: pcurve_id.clone(),
            geometry,
            metadata: PcurveMetadata::default(),
        });
        ir.model.coedges[coedge_index].pcurves.push(PcurveUse {
            pcurve: pcurve_id,
            isoparametric: None,
            parameter_range: explicit_range
                .then(|| DirectedParameterRange::new([2.0, 4.0]).unwrap()),
        });
        ir.model.edges[0].carrier = EdgeCarrier::Free;
        ir.model.curves.retain(|curve| curve.id != removed_curve);

        let mut output = Vec::new();
        let report = write_step(
            &ir,
            &mut output,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .unwrap();
        assert!(!report
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::EdgeNo3dCurve.kind()));
        assert!(!report
            .losses
            .iter()
            .any(|loss| loss.code == StepLossCode::PcurveCarrierUnwritable.kind()));
        assert_eq!(
            report
                .losses
                .iter()
                .any(|loss| loss.code == StepLossCode::EdgePcurveSampledToBspline.kind()),
            fitted
        );
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().model.faces.len(), 6);
        assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Solid);
        cadmpeg_measure::test_support::round_trip::measures_agree(
            "recovered cube",
            &original,
            decoded.ir(),
        )
        .unwrap();
    }
}
