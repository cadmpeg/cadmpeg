// SPDX-License-Identifier: Apache-2.0
use super::{definitions, nominal_diameter};
use crate::native::features::holes::FeatureSymbolicThread;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation, FinitePoint3};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
    Point, Region, Sense, Shell, Vertex,
};

fn thread() -> FeatureSymbolicThread {
    serde_json::from_value(serde_json::json!({
        "id":"thread","operation_label":"operation","operation_record":"record","source_offset":0,
        "text_frames":[{"id":"profile","symbolic_thread":"thread","ordinal":0,"marker":3,"value":"M Profile","source_offset":10},{"id":"designation","symbolic_thread":"thread","ordinal":1,"marker":3,"value":"M3_x_0.5","source_offset":22}],
        "construction":{"parameter_class_index":3,"coordinates":[{"value":0.0,"raw":"00"},{"value":0.0,"raw":"00"},{"value":0.1,"raw":"2fb999999999999a"}],"tail_references":[{"object_index":0,"raw_object_index":"f000"},{"object_index":255,"raw_object_index":"f0ff"},{"object_index":256,"raw_object_index":"f10100"}],"tail_sentinel":true,"source_offset":100,"byte_len":30}
    })).unwrap()
}

fn fixture(cylinders: &[(&str, &str, f64)]) -> CadIr {
    let mut ir = CadIr::empty();
    for &(name, owner, radius) in cylinders {
        let body = BodyId::mint(format!("test:thread:body#{owner}")).unwrap();
        let region = RegionId::mint(format!("test:thread:region#{owner}")).unwrap();
        let shell = ShellId::mint(format!("test:thread:shell#{owner}")).unwrap();
        let face = FaceId::mint(format!("test:thread:face#{name}")).unwrap();
        let surface = SurfaceId::mint(format!("test:thread:surface#{name}")).unwrap();
        let mut loops = Vec::new();
        for (ordinal, z) in [0., 50.].into_iter().enumerate() {
            let key = format!("{name}-{ordinal}");
            let point = PointId::mint(format!("test:thread:point#{key}")).unwrap();
            let vertex = VertexId::mint(format!("test:thread:vertex#{key}")).unwrap();
            let edge = EdgeId::mint(format!("test:thread:edge#{key}")).unwrap();
            let coedge = CoedgeId::mint(format!("test:thread:coedge#{key}")).unwrap();
            let boundary = LoopId::mint(format!("test:thread:loop#{key}")).unwrap();
            ir.model.points.push(Point::new(
                point.clone(),
                FinitePoint3::new(Point3::new(radius, 0., z)).unwrap(),
                None,
            ));
            ir.model.vertices.push(Vertex {
                id: vertex.clone(),
                point,
                tolerance: None,
            });
            ir.model.edges.push(Edge {
                id: edge.clone(),
                start: vertex.clone(),
                end: vertex,
                carrier: EdgeCarrier::Free,
                tolerance: None,
            });
            ir.model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: boundary.clone(),
                edge,
                radial_next: coedge.clone(),
                sense: Sense::Forward,
                pcurves: Vec::new(),
                use_curve: None,
            });
            ir.model.loops.push(Loop {
                id: boundary.clone(),
                face: face.clone(),
                boundary: LoopBoundary::Ring(LoopRing::single(coedge)),
            });
            loops.push(boundary);
        }
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(0., 0., 0.),
                    Vector3::new(0., 0., 1.),
                    Vector3::new(1., 0., 0.),
                    radius,
                )
                .unwrap(),
            )),
            source_object: None,
        });
        ir.model.faces.push(Face {
            id: face.clone(),
            shell: shell.clone(),
            surface,
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(loops),
            name: None,
            color: None,
            tolerance: None,
        });
        if let Some(existing) = ir.model.shells.iter_mut().find(|s| s.id == shell) {
            existing.add_face(face);
        } else {
            ir.model.shells.push(
                Shell::new(
                    shell.clone(),
                    region.clone(),
                    vec![face],
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap(),
            );
            ir.model.regions.push(Region {
                id: region.clone(),
                body: body.clone(),
                shells: vec![shell],
            });
            ir.model.bodies.push(Body {
                id: body,
                kind: BodyKind::Sheet,
                regions: vec![region],
                transform: None,
                name: None,
                color: None,
                visible: None,
            });
        }
    }
    ir
}

fn selected(ir: &CadIr) -> FaceSelection {
    crate::test_support::with_decode_context(|ctx| {
        let FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            face,
            diameter,
            extent,
        }) = definitions(ctx, ir, &[thread()])
            .unwrap()
            .remove("operation")
            .unwrap()
        else {
            panic!("cosmetic thread")
        };
        assert_eq!(diameter.unwrap().get(), 3.);
        assert!(extent.is_none());
        face
    })
}

#[test]
fn symbolic_thread_nominal_designation_is_independent_of_brep_diameter() {
    for (text, value) in [
        ("M12_x_1.75", 12.),
        ("M3_X_0.5", 3.),
        ("1/2-13_UNC", 1.0 / 2.0 * 25.4),
        ("3/8-16_UNC", 3.0 / 8.0 * 25.4),
    ] {
        assert_eq!(nominal_diameter(text).unwrap().get(), value);
    }
    for text in [
        "M0_x_1",
        "M3_x_0",
        "M3e1_x_0.5",
        "M3_x_nan",
        "1/0-13_UNC",
        "1/2-0_UNC",
        "1/2-13_UNX",
        "0.75",
    ] {
        assert!(nominal_diameter(text).is_none(), "{text}");
    }
}

#[test]
fn symbolic_thread_selection_abstains_for_zero_multiple_or_incomplete_ownership() {
    assert_eq!(selected(&CadIr::empty()), FaceSelection::Unresolved);
    let ir = fixture(&[("small", "a", 1.), ("large", "a", 4.)]);
    let FaceSelection::Resolved { faces, native } = selected(&ir) else {
        panic!("unique radius-bounded candidate")
    };
    assert_eq!(faces, vec![ir.model.faces[0].id.clone()]);
    assert_eq!(native, "thread");
    assert_eq!(
        selected(&fixture(&[("first", "a", 1.), ("second", "b", 1.)])),
        FaceSelection::Unresolved
    );
    let seam = fixture(&[("first", "a", 1.), ("second", "a", 1.)]);
    let FaceSelection::Resolved { faces, .. } = selected(&seam) else {
        panic!("one physical cylinder")
    };
    assert_eq!(
        faces,
        seam.model
            .faces
            .iter()
            .map(|face| face.id.clone())
            .collect::<Vec<_>>()
    );
    let mut duplicate = fixture(&[("first", "a", 1.)]);
    duplicate.model.shells[0].add_face(duplicate.model.faces[0].id.clone());
    assert_eq!(selected(&duplicate), FaceSelection::Unresolved);
    let mut placed = fixture(&[("first", "a", 1.)]);
    placed.model.bodies[0].transform = Some(cadmpeg_ir::transform::Transform::identity());
    assert_eq!(selected(&placed), FaceSelection::Unresolved);
    let mut missing = fixture(&[("first", "a", 1.)]);
    missing.model.points.pop();
    assert_eq!(selected(&missing), FaceSelection::Unresolved);
}
