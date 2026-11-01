// SPDX-License-Identifier: Apache-2.0
//! Recovery through the public codec interfaces.

#[cfg(all(feature = "step", feature = "iges", feature = "rhino"))]
#[test]
fn planar_pcurve_recovers_a_solid_for_each_geometry_writer() {
    use std::io::Cursor;

    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs};
    use cadmpeg_ir::geometry::{DirectedParameterRange, SolvedSurfaceGeometry};
    use cadmpeg_ir::ids::PcurveId;
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::{BodyKind, EdgeCarrier, PcurveUse, Sense};

    fn round_trip<C: Encoder + Codec>(
        codec: &C,
        ir: &cadmpeg_ir::CadIr,
        original: &cadmpeg_ir::CadIr,
    ) {
        let plan = codec
            .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
            .expect("recover and plan the solid");
        assert!(!plan
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code.local_code() == "geometry.edge-pcurve-sampled-to-bspline" }));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let decoded = codec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().model.faces.len(), 6);
        assert!(decoded
            .ir()
            .model
            .bodies
            .iter()
            .any(|body| body.kind == BodyKind::Solid));
        cadmpeg_measure::test_support::round_trip::measures_agree(
            "recovered cube",
            original,
            decoded.ir(),
        )
        .expect("recovered geometry matches the original cube");
    }

    let mut original = cadmpeg_ir::examples::unit_cube().unwrap();
    for face in &mut original.model.faces {
        face.name = None;
    }
    let mut ir = original.clone();
    let edge = ir.model.edges[0].id.clone();
    let old_curve = ir.model.edges[0].curve().unwrap().clone();
    let coedge = ir
        .model
        .coedges
        .iter()
        .position(|use_| use_.edge == edge)
        .unwrap();
    let lp = ir
        .model
        .loops
        .iter()
        .find(|lp| lp.id == ir.model.coedges[coedge].owner_loop)
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
        panic!("cube face is planar");
    };
    let origin = plane.origin().get();
    let u = *plane.frame().reference().as_raw();
    let v = plane.frame().axis().as_raw().cross(u);
    let uv = |point: Point3| {
        let d = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        Point2::new(d.dot(u), d.dot(v))
    };
    let start = uv(Point3::new(0.0, 0.0, 0.0));
    let end = uv(Point3::new(10.0, 0.0, 0.0));
    let (first, last) = if ir.model.coedges[coedge].sense == Sense::Forward {
        (start, end)
    } else {
        (end, start)
    };
    let pcurve = PcurveId::mint("test:recovery:pcurve#edge").unwrap();
    ir.model.pcurves.push(Pcurve {
        id: pcurve.clone(),
        geometry: PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![first, last],
                None,
                false,
            )
            .unwrap(),
        },
        metadata: PcurveMetadata::general(
            None,
            cadmpeg_ir::units::FiniteVector::new([0.0, 1.0]),
            None,
        ),
    });
    ir.model.coedges[coedge].pcurves.push(PcurveUse {
        pcurve,
        isoparametric: Some(first.u == last.u || first.v == last.v),
        parameter_range: Some(DirectedParameterRange::new([0.0, 1.0]).unwrap()),
    });
    ir.model.edges[0].carrier = EdgeCarrier::Free;
    ir.model.curves.retain(|curve| curve.id != old_curve);

    round_trip(&cadmpeg_codec_step::StepCodec::default(), &ir, &original);
    round_trip(&cadmpeg_codec_iges::IgesCodec, &ir, &original);
    round_trip(&cadmpeg_codec_rhino::RhinoCodec, &ir, &original);
}

mod repro;
