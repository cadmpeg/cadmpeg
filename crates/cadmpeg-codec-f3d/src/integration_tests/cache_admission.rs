// SPDX-License-Identifier: Apache-2.0
//! D9: source cache contradiction cannot establish procedural geometry.

use crate::test_support::smbh_surfaces_test::synthetic_cyl_spl_sur_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::{decode::cache_admission::recover, loss::F3dLossCode, native::F3dNative};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::{geometry::SurfaceGeometry, CadIr};
use cadmpeg_test_support::EditableDecodeResult;

fn original() -> (Vec<u8>, EditableDecodeResult) {
    let bytes = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = crate::F3dCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("original source fixture");
    (bytes, EditableDecodeResult::from(decoded))
}

fn check(ir: &CadIr) {
    let report = cadmpeg_ir::validate_neutral(ir, vec![]).expect("neutral validation");
    assert!(report.is_ok(), "{report:#?}");
}

#[test]
fn original_bad_cache_omits_only_proven_invalid_face_and_preserves_source() {
    let (bytes, result) = original();
    let ir = result.ir();
    check(ir);
    let native_findings =
        crate::validate::validate_native(&cadmpeg_test_support::service_decode_context(), ir)
            .expect("native identity validation");
    assert!(native_findings.is_empty(), "{native_findings:#?}");
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::SurfaceCacheRejected.kind())
        .expect("explicit source cache error");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Error);
    assert!(result
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::ArenaClass::Model));
    assert!(result
        .source_fidelity()
        .retained_records()
        .values()
        .any(|record| record.data() == Some(bytes.as_slice())));
    assert_eq!(
        ir.model
            .faces
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["f3d:brep:entity#4"]
    );
    assert_eq!(
        ir.model
            .loops
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["f3d:brep:entity#6"]
    );
    assert_eq!(
        ir.model
            .coedges
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        [
            "f3d:brep:entity#10",
            "f3d:brep:entity#11",
            "f3d:brep:entity#12"
        ]
    );
    let plane_face = &ir.model.faces[0];
    assert_eq!(plane_face.shell.as_str(), "f3d:brep:entity#3");
    assert_eq!(plane_face.surface.as_str(), "f3d:brep:entity#8");
    assert_eq!(plane_face.sense, cadmpeg_ir::topology::Sense::Forward);
    assert_eq!(
        ir.model
            .surfaces
            .iter()
            .find(|row| row.id == plane_face.surface)
            .unwrap()
            .geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0)
            )
            .unwrap()
        ))
    );
    // The source shared edge16 remains owned by the unaffected plane; removal
    // of its opposite face legitimately closes its surviving radial ring.
    let shared = ir
        .model
        .coedges
        .iter()
        .find(|row| row.id.as_str() == "f3d:brep:entity#10")
        .unwrap();
    assert_eq!(shared.edge.as_str(), "f3d:brep:entity#16");
    assert_eq!(shared.radial_next, shared.id);
    for (id, point) in [
        (25, Point3::new(0.0, 0.0, 0.0)),
        (26, Point3::new(10.0, 0.0, 0.0)),
        (27, Point3::new(0.0, 10.0, 0.0)),
    ] {
        assert_eq!(
            ir.model
                .points
                .iter()
                .find(|row| row.id.as_str() == format!("f3d:brep:entity#{id}"))
                .unwrap()
                .position()
                .get(),
            point
        );
    }
    assert!(!ir
        .model
        .vertices
        .iter()
        .any(|row| row.id.as_str() == "f3d:brep:entity#24"));
    assert!(!ir
        .model
        .edges
        .iter()
        .any(|row| ["f3d:brep:entity#19", "f3d:brep:entity#20"].contains(&row.id.as_str())));
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|row| row.id.as_str() == "f3d:brep:entity#9")
        .unwrap();
    let SurfaceGeometry::Procedural {
        construction,
        cache: None,
    } = &surface.geometry
    else {
        panic!("retain free true construction without contradictory cache")
    };
    let definition = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|row| row.id == *construction)
        .unwrap();
    let ProceduralSurfaceDefinition::Extrusion(payload) = definition.definition() else {
        panic!("original extrusion")
    };
    assert_eq!(payload.parameter_interval().unwrap().get(), [0.25, 0.75]);
    assert_eq!(payload.direction().get(), Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(
        payload.native_position().unwrap().get(),
        Point3::new(40.0, 50.0, 60.0)
    );
    assert!(ir
        .model
        .curves
        .iter()
        .any(|row| row.id == *payload.directrix()));
}

/// A separate exact XY extrusion control, using the source fixture's untouched
/// surviving planar triangle. Its real construction admits every boundary.
fn planar_control(ir: &mut CadIr, cache_z: f64) {
    let curve_id =
        cadmpeg_ir::ids::CurveId::mint("synthetic:cache-control:curve#directrix").unwrap();
    let construction = cadmpeg_ir::ids::ProceduralSurfaceId::mint(
        "synthetic:cache-control:procedural-surface#extrusion",
    )
    .unwrap();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        )),
        parameter_range: None,
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Extrusion(ExtrusionSurfaceConstruction::legacy(
            curve_id,
            None,
            cadmpeg_ir::features::FiniteVector3::new(Vector3::new(0.0, 10.0, 0.0)).unwrap(),
            None,
            None,
        )),
        None,
    ));
    let cache = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![
                    Point3::new(0.0, 0.0, cache_z),
                    Point3::new(0.0, 10.0, cache_z),
                ],
                vec![
                    Point3::new(10.0, 0.0, cache_z),
                    Point3::new(10.0, 10.0, cache_z),
                ],
            ],
            None,
        ),
        false,
    )
    .unwrap();
    ir.model
        .surfaces
        .iter_mut()
        .find(|row| row.id.as_str() == "f3d:brep:entity#8")
        .unwrap()
        .geometry = SurfaceGeometry::Procedural {
        construction,
        cache: Some(SolvedSurfaceGeometry::Nurbs(cache)),
    };
}

#[test]
fn exact_new_cache_control_is_retained() {
    let (_, mut result) = original();
    let mut ir = result.ir_mut();
    planar_control(&mut ir, 0.0);
    let before = ir.model.clone();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut losses = vec![];
    assert!(!recover(&ctx, &mut ir, &mut F3dNative::default(), &mut losses).unwrap());
    assert_eq!(ir.model, before);
    assert!(losses.is_empty());
    check(&ir);
}

#[test]
fn rejecting_bad_cache_preserves_independently_valid_face() {
    let (_, mut result) = original();
    let mut ir = result.ir_mut();
    planar_control(&mut ir, 100.0);
    let mut empty_body = ir.model.bodies[0].clone();
    empty_body.id = cadmpeg_ir::ids::BodyId::mint("synthetic:cache-control:body#empty").unwrap();
    let region_id =
        cadmpeg_ir::ids::RegionId::mint("synthetic:cache-control:region#empty").unwrap();
    empty_body.regions = vec![region_id.clone()];
    ir.model.regions.push(cadmpeg_ir::topology::Region {
        id: region_id,
        body: empty_body.id.clone(),
        shells: vec![],
    });
    ir.model.bodies.push(empty_body);
    let bodies = ir.model.bodies.clone();
    let regions = ir.model.regions.clone();
    let shells = ir.model.shells.clone();
    let faces = ir.model.faces.clone();
    let edges = ir.model.edges.clone();
    let coedges = ir.model.coedges.clone();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut losses = vec![];
    assert!(recover(&ctx, &mut ir, &mut F3dNative::default(), &mut losses).unwrap());
    assert_eq!(ir.model.bodies, bodies);
    assert_eq!(ir.model.regions, regions);
    assert_eq!(ir.model.shells, shells);
    assert_eq!(ir.model.faces, faces);
    assert_eq!(ir.model.edges, edges);
    assert_eq!(ir.model.coedges, coedges);
    assert_eq!(losses.len(), 1);
    assert!(matches!(
        ir.model
            .surfaces
            .iter()
            .find(|row| row.id.as_str() == "f3d:brep:entity#8")
            .unwrap()
            .geometry,
        SurfaceGeometry::Procedural { cache: None, .. }
    ));
    check(&ir);
}
