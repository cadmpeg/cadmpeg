// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::LossTaxonomy;

use crate::test_support::*;
use crate::SldprtCodec;

use super::*;
use cadmpeg_ir::geometry::{Curve, NurbsSurface, Surface};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Shell, Vertex,
};

fn descriptor(item_size: u32, kind: u32, count: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(item_size.to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(2_u32.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(data);
    out
}

fn table() -> Vec<u8> {
    let mut out = descriptor(4, 8, 1, &3_u32.to_le_bytes());
    let positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    out.extend(descriptor(12, 100, 3, &positions));
    out.extend(descriptor(12, 100, 3, &[0; 36]));
    out.extend(descriptor(4, 8, 4, &[0; 16]));
    out.extend(descriptor(4, 8, 1, &4_u32.to_le_bytes()));
    out.extend(descriptor(1, 8, 4, &[0; 4]));
    out
}

fn class(payload: &mut Vec<u8>, name: &str, sources: &[u32]) {
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
    payload.extend_from_slice(name.as_bytes());
    for source in sources {
        payload.extend_from_slice(SCENE_SOURCE_MARKER);
        payload.extend_from_slice(&source.to_le_bytes());
    }
}

#[test]
fn scene_objects_carry_history_source_identity() {
    let mut payload = Vec::new();
    class(&mut payload, "moAmbientLight_c", &[12]);
    class(&mut payload, "moDirectionLight_c", &[30, 32]);
    class(&mut payload, "moVisualProperties_c", &[99]);
    class(&mut payload, "moPointLight_c", &[21]);
    class(&mut payload, "moSpotLight_c", &[20]);

    assert_eq!(
        scene_classes(&payload),
        vec![
            (12, "moAmbientLight_c".into()),
            (30, "moDirectionLight_c".into()),
            (32, "moDirectionLight_c".into()),
            (21, "moPointLight_c".into()),
            (20, "moSpotLight_c".into()),
        ]
    );
}

#[test]
fn anonymous_scene_object_counts_do_not_create_source_bindings() {
    let mut payload = Vec::new();
    payload.extend_from_slice(CLASS_MARKER);
    let class = b"moDirectionLight_c";
    payload.extend_from_slice(&(class.len() as u16).to_le_bytes());
    payload.extend_from_slice(class);
    for name in ["UnNamed", "Another"] {
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&[0xff, 0xfe, 0xff, 7]);
        for byte in name.bytes() {
            payload.extend_from_slice(&[byte, 0]);
        }
        payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
    }

    assert!(scene_classes(&payload).is_empty());
}

#[test]
fn compact_face_tessellation_header_places_table_at_plus_8() {
    let mut payload = Vec::new();
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());
    assert_eq!(descriptor_table_offset(&payload, 0), 8);
    assert!(parse_table_sequence(&payload, 8, payload.len()).is_some());
}

#[test]
fn extended_face_tessellation_header_places_table_at_plus_40() {
    let mut payload = Vec::new();
    for word in [1_u32, 1, 1, 0, 0, 0x0020_1296, 0, 0, 0, 0] {
        payload.extend(word.to_le_bytes());
    }
    payload.extend(table());
    assert_eq!(descriptor_table_offset(&payload, 0), 40);
    assert!(parse_table_sequence(&payload, 40, payload.len()).is_some());
}

#[test]
fn body_property_class_does_not_end_face_table_sequence() {
    let mut payload = Vec::new();
    class(&mut payload, "uoTempFaceTessData_c", &[]);
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());

    class(&mut payload, "uoBodyPropInfo_c", &[]);
    payload.extend([0x37, 0x80]);
    payload.extend(1_u32.to_le_bytes());
    payload.extend(1_u32.to_le_bytes());
    payload.extend(table());

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));
    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.tessellations.len(), 2);
}

#[test]
fn next_face_class_ends_face_table_sequence() {
    let mut payload = Vec::new();
    for _ in 0..2 {
        class(&mut payload, "uoTempFaceTessData_c", &[]);
        payload.extend(1_u32.to_le_bytes());
        payload.extend(1_u32.to_le_bytes());
        payload.extend(table());
    }

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));
    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.tessellations.len(), 2);
}

#[test]
fn incomplete_extended_header_does_not_shift_the_table() {
    let mut payload = Vec::new();
    for word in [1_u32, 1, 1, 0, 0, 0, 0, 0, 0, 0] {
        payload.extend(word.to_le_bytes());
    }
    payload.extend(table());
    assert_eq!(descriptor_table_offset(&payload, 0), 8);
}

#[test]
fn inconsistent_auxiliary_count_invalidates_the_table() {
    let mut payload = table();
    let list_b_count = 20 + 52 + 52 + 12;
    payload[list_b_count..list_b_count + 4].copy_from_slice(&3_u32.to_le_bytes());
    assert!(parse_table(&payload, 0).is_none());
}

#[test]
fn analytic_surface_residuals_measure_normal_distance() {
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 2.0),
        normal: Vector3::new(0.0, 0.0, 2.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 2.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 3.0,
    };
    let sphere = SurfaceGeometry::Sphere {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0,
    };
    let torus = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 5.0,
        minor_radius: 2.0,
    };
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 3.0,
        ratio: 0.5,
        half_angle: std::f64::consts::FRAC_PI_4,
    };

    for (surface, point, displaced) in [
        (
            &plane,
            Point3::new(3.0, 4.0, 2.0),
            Point3::new(3.0, 4.0, 2.5),
        ),
        (
            &cylinder,
            Point3::new(3.0, 0.0, 7.0),
            Point3::new(3.5, 0.0, 7.0),
        ),
        (
            &sphere,
            Point3::new(5.0, 2.0, 3.0),
            Point3::new(5.5, 2.0, 3.0),
        ),
        (
            &torus,
            Point3::new(7.0, 0.0, 0.0),
            Point3::new(7.5, 0.0, 0.0),
        ),
    ] {
        assert_eq!(analytic_surface_residual(surface, point), Some(0.0));
        assert!(
            analytic_surface_residual(surface, displaced).is_some_and(|residual| residual > 0.0)
        );
    }

    let local_radius = 3.0 + 2.0 * std::f64::consts::FRAC_PI_4.tan();
    let cone_point = Point3::new(local_radius, 0.0, 2.0);
    assert!(analytic_surface_residual(&cone, cone_point)
        .is_some_and(|residual| residual <= f64::EPSILON * 128.0));
    assert!(
        analytic_surface_residual(&cone, Point3::new(local_radius + 0.5, 0.0, 2.0))
            .is_some_and(|residual| residual > 0.0)
    );
}

fn add_face(
    model: &mut cadmpeg_ir::document::Model,
    name: &str,
    geometry: SurfaceGeometry,
    corners: [Point3; 4],
) -> FaceId {
    let face_id = FaceId(format!("face-{name}"));
    let loop_id = LoopId(format!("loop-{name}"));
    let surface_id = SurfaceId(format!("surface-{name}"));
    model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry,
        source_object: None,
    });

    let coedge_ids = (0..4)
        .map(|index| CoedgeId(format!("coedge-{name}-{index}")))
        .collect::<Vec<_>>();
    for (index, corner) in corners.iter().copied().enumerate() {
        let point_id = PointId(format!("point-{name}-{index}"));
        let vertex_id = VertexId(format!("vertex-{name}-{index}"));
        model.points.push(Point {
            id: point_id.clone(),
            position: corner,
            source_object: None,
        });
        model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (index, origin) in corners.iter().copied().enumerate() {
        let next = (index + 1) % 4;
        let curve_id = CurveId(format!("curve-{name}-{index}"));
        let edge_id = EdgeId(format!("edge-{name}-{index}"));
        let direction = corners[next].vector_from(origin).unit().unwrap();
        model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Line { origin, direction },
            source_object: None,
        });
        model.edges.push(Edge {
            id: edge_id.clone(),
            curve: Some(curve_id),
            start: VertexId(format!("vertex-{name}-{index}")),
            end: VertexId(format!("vertex-{name}-{next}")),
            param_range: None,
            tolerance: None,
        });
        model.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            next: coedge_ids[next].clone(),
            previous: coedge_ids[(index + 3) % 4].clone(),
            radial_next: coedge_ids[index].clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
        });
    }
    model.loops.push(Loop {
        id: loop_id.clone(),
        face: face_id.clone(),
        boundary_role: LoopBoundaryRole::Outer,
        coedges: coedge_ids,
        vertex_uses: Vec::new(),
    });
    model.faces.push(Face {
        id: face_id.clone(),
        shell: ShellId("shell".into()),
        surface: surface_id,
        sense: Sense::Forward,
        loops: vec![loop_id],
        name: None,
        color: None,
        tolerance: None,
    });
    face_id
}

fn add_square_face(model: &mut cadmpeg_ir::document::Model, name: &str, x: f64) -> FaceId {
    add_face(
        model,
        name,
        SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        [
            Point3::new(x, -1.0, 0.0),
            Point3::new(x + 2.0, -1.0, 0.0),
            Point3::new(x + 2.0, 1.0, 0.0),
            Point3::new(x, 1.0, 0.0),
        ],
    )
}

fn test_nurbs_surface() -> NurbsSurface {
    let heights = [0.0, 0.25, 0.0, 0.25, 0.9, 0.25, 0.0, 0.25, 0.0];
    let control_points = (0..3)
        .flat_map(|u| (0..3).map(move |v| Point3::new(u as f64, v as f64, heights[u * 3 + v])))
        .collect();
    NurbsSurface {
        u_degree: 2,
        v_degree: 2,
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        u_count: 3,
        v_count: 3,
        control_points,
        weights: None,
        normal_reversed: false,
        u_periodic: false,
        v_periodic: false,
    }
}

fn flat_test_nurbs_surface() -> NurbsSurface {
    let mut surface = test_nurbs_surface();
    for point in &mut surface.control_points {
        point.z = 0.0;
    }
    surface
}

fn test_nurbs_corners(surface: &NurbsSurface) -> [Point3; 4] {
    [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
        .map(|(u, v)| cadmpeg_ir::eval::nurbs_surface_point(surface, u, v).unwrap())
}

fn test_nurbs_point_normal(surface: &NurbsSurface, u: f64, v: f64) -> (Point3, Vector3) {
    let partials = cadmpeg_ir::eval::nurbs_surface_partials(surface, u, v).unwrap();
    (
        partials.point,
        partials.du.cross(partials.dv).unit().unwrap(),
    )
}

fn add_cylindrical_patch_face(
    model: &mut cadmpeg_ir::document::Model,
    name: &str,
    min_z: f64,
    max_z: f64,
) -> FaceId {
    let radius = 5.0;
    let angles = [0.0, std::f64::consts::FRAC_PI_2];
    let point_at = |angle: f64, z: f64| Point3::new(radius * angle.cos(), radius * angle.sin(), z);
    let corners = [
        point_at(angles[0], min_z),
        point_at(angles[1], min_z),
        point_at(angles[1], max_z),
        point_at(angles[0], max_z),
    ];
    let face_id = FaceId(format!("face-{name}"));
    let loop_id = LoopId(format!("loop-{name}"));
    let surface_id = SurfaceId(format!("surface-{name}"));
    model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        },
        source_object: None,
    });

    let vertex_ids = corners
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let point_id = PointId(format!("point-{name}-{index}"));
            let vertex_id = VertexId(format!("vertex-{name}-{index}"));
            model.points.push(Point {
                id: point_id.clone(),
                position: *point,
                source_object: None,
            });
            model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            vertex_id
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..4)
        .map(|index| CoedgeId(format!("coedge-{name}-{index}")))
        .collect::<Vec<_>>();
    let curve_geometries = [
        CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, min_z),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        },
        CurveGeometry::Line {
            origin: corners[1],
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, max_z),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        },
        CurveGeometry::Line {
            origin: corners[3],
            direction: Vector3::new(0.0, 0.0, -1.0),
        },
    ];
    for (index, geometry) in curve_geometries.into_iter().enumerate() {
        let next = (index + 1) % 4;
        let curve_id = CurveId(format!("curve-{name}-{index}"));
        let edge_id = EdgeId(format!("edge-{name}-{index}"));
        model.curves.push(Curve {
            id: curve_id.clone(),
            geometry,
            source_object: None,
        });
        model.edges.push(Edge {
            id: edge_id.clone(),
            curve: Some(curve_id),
            start: vertex_ids[index].clone(),
            end: vertex_ids[next].clone(),
            param_range: None,
            tolerance: None,
        });
        model.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            next: coedge_ids[next].clone(),
            previous: coedge_ids[(index + 3) % 4].clone(),
            radial_next: coedge_ids[index].clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
        });
    }
    model.loops.push(Loop {
        id: loop_id.clone(),
        face: face_id.clone(),
        boundary_role: LoopBoundaryRole::Outer,
        coedges: coedge_ids,
        vertex_uses: Vec::new(),
    });
    model.faces.push(Face {
        id: face_id.clone(),
        shell: ShellId("shell".into()),
        surface: surface_id,
        sense: Sense::Forward,
        loops: vec![loop_id],
        name: None,
        color: None,
        tolerance: None,
    });
    face_id
}

fn model_with_body() -> cadmpeg_ir::document::Model {
    cadmpeg_ir::document::Model {
        bodies: vec![Body {
            id: BodyId("body".into()),
            kind: BodyKind::Solid,
            regions: vec![RegionId("region".into())],
            transform: None,
            name: None,
            color: None,
            visible: None,
        }],
        regions: vec![Region {
            id: RegionId("region".into()),
            body: BodyId("body".into()),
            shells: vec![ShellId("shell".into())],
        }],
        shells: vec![Shell {
            id: ShellId("shell".into()),
            region: RegionId("region".into()),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        }],
        ..Default::default()
    }
}

fn persistent_mesh(id: &str) -> Tessellation {
    Tessellation {
        id: id.into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    }
}

fn persistent_identity(source: u32, local: u32, trailing_fields: &[u32]) -> PersistentFaceIdentity {
    PersistentFaceIdentity {
        feature_source_id: source,
        local_id: local,
        trailing_fields: trailing_fields.to_vec(),
    }
}

fn framed_surface_reference(text: &str) -> Vec<u8> {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut payload = vec![0xff, 0xfe, 0xff, units.len().try_into().unwrap()];
    payload.extend(units.into_iter().flat_map(u16::to_le_bytes));
    payload
}

#[test]
fn persistent_surface_reference_decodes_signed_tail() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let references = persistent_surface_references(
        &payload,
        ByteRange {
            start: 0,
            end: payload.len(),
        },
    );
    assert_eq!(
        references,
        vec![PersistentSurfaceReference::Complete(persistent_identity(
            300,
            4,
            &[u32::MAX, 0],
        ))]
    );
}

#[test]
fn opaque_surface_suffix_remains_source_only() {
    let payload = framed_surface_reference("moFromSktEntSurfIdRep_c,7,3,opaque");
    let references = persistent_surface_references(
        &payload,
        ByteRange {
            start: 0,
            end: payload.len(),
        },
    );
    assert_eq!(
        references,
        vec![PersistentSurfaceReference::SourceOnly {
            feature_source_id: 7,
            local_surface_id: 3,
        }]
    );
    let face = DisplayFace {
        mesh: Mesh::default(),
        table_index: 0,
        table: ByteRange { start: 0, end: 1 },
        metadata: ByteRange { start: 1, end: 2 },
        surface_references: references,
    };
    assert_eq!(face.feature_source_id(), Some(7));
    assert_eq!(face.persistent_surface_identity(), None);
}

#[test]
fn persistent_surface_identity_requires_agreeing_duplicates() {
    let face = DisplayFace {
        mesh: Mesh::default(),
        table_index: 0,
        table: ByteRange { start: 0, end: 1 },
        metadata: ByteRange { start: 1, end: 2 },
        surface_references: vec![
            PersistentSurfaceReference::Complete(persistent_identity(7, 3, &[])),
            PersistentSurfaceReference::Complete(persistent_identity(7, 3, &[])),
        ],
    };
    assert_eq!(face.feature_source_id(), Some(7));
    assert_eq!(
        face.persistent_surface_identity(),
        Some(persistent_identity(7, 3, &[]))
    );

    let mut conflicting = face;
    if let PersistentSurfaceReference::Complete(identity) = &mut conflicting.surface_references[1] {
        identity.local_id = 4;
    }
    assert_eq!(conflicting.feature_source_id(), Some(7));
    assert_eq!(conflicting.persistent_surface_identity(), None);
}

#[test]
fn persistent_surface_identity_binds_one_face_and_body() {
    let mut model = model_with_body();
    let face = add_square_face(&mut model, "persistent", 0.0);
    model.shells[0].faces.push(face.clone());
    model.tessellations.push(persistent_mesh("mesh"));

    let face_identities = vec![(face.0.clone(), persistent_identity(7, 3, &[]))];
    let bindings = vec![PersistentFaceBinding {
        tessellation: "mesh".into(),
        identity: persistent_identity(7, 3, &[]),
    }];

    assert_eq!(
        assign_persistent_owners(&mut model, &face_identities, &bindings),
        vec!["mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));
}

#[test]
fn persistent_surface_identity_rejects_ambiguous_face_or_mesh_keys() {
    let mut model = model_with_body();
    let first = add_square_face(&mut model, "first-persistent", 0.0);
    let second = add_square_face(&mut model, "second-persistent", 3.0);
    model.shells[0].faces = vec![first.clone(), second.clone()];
    model.tessellations.push(persistent_mesh("mesh"));
    let face_identities = vec![
        (first.0.clone(), persistent_identity(7, 3, &[])),
        (second.0.clone(), persistent_identity(7, 3, &[])),
    ];
    let binding = PersistentFaceBinding {
        tessellation: "mesh".into(),
        identity: persistent_identity(7, 3, &[]),
    };
    assert!(assign_persistent_owners(&mut model, &face_identities, &[binding]).is_empty());
    assert!(model.tessellations[0].faces.is_empty());

    let mut model = model_with_body();
    let first = add_square_face(&mut model, "first-mesh", 0.0);
    let second = add_square_face(&mut model, "second-mesh", 3.0);
    model.shells[0].faces = vec![first.clone(), second.clone()];
    model.tessellations.push(persistent_mesh("mesh"));
    let face_identities = vec![
        (first.0.clone(), persistent_identity(7, 3, &[])),
        (second.0.clone(), persistent_identity(8, 4, &[])),
    ];
    let bindings = vec![
        PersistentFaceBinding {
            tessellation: "mesh".into(),
            identity: persistent_identity(7, 3, &[]),
        },
        PersistentFaceBinding {
            tessellation: "mesh".into(),
            identity: persistent_identity(8, 4, &[]),
        },
    ];
    assert!(assign_persistent_owners(&mut model, &face_identities, &bindings).is_empty());
    assert!(model.tessellations[0].faces.is_empty());
}

#[test]
fn persistent_surface_identity_distinguishes_trailing_path_fields() {
    let mut model = model_with_body();
    let first = add_square_face(&mut model, "first-tail", 0.0);
    let second = add_square_face(&mut model, "second-tail", 3.0);
    model.shells[0].faces = vec![first.clone(), second.clone()];
    model.tessellations.push(persistent_mesh("mesh"));
    let face_identities = vec![
        (first.0.clone(), persistent_identity(266, 2, &[0])),
        (second.0.clone(), persistent_identity(266, 2, &[1])),
    ];
    let binding = PersistentFaceBinding {
        tessellation: "mesh".into(),
        identity: persistent_identity(266, 2, &[1]),
    };

    assert_eq!(
        assign_persistent_owners(&mut model, &face_identities, &[binding]),
        vec!["mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![second]);
}

#[test]
fn bounded_planar_trim_selects_between_coincident_supports() {
    let mut model = model_with_body();
    let first = add_square_face(&mut model, "first", -4.0);
    let second = add_square_face(&mut model, "second", 2.0);
    model.shells[0].faces = vec![first.clone(), second.clone()];
    model.tessellations.push(Tessellation {
        id: "mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(2.25, -0.75, 0.0),
            Point3::new(3.75, -0.75, 0.0),
            Point3::new(3.0, 0.75, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert_eq!(assign_unique_surface_owners(&mut model), vec!["mesh"]);
    assert_eq!(model.tessellations[0].faces, vec![second]);
    assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));

    model
        .faces
        .iter_mut()
        .find(|face| face.id == first)
        .unwrap()
        .loops
        .clear();
    model.tessellations[0].body = None;
    model.tessellations[0].faces.clear();
    assert!(assign_unique_surface_owners(&mut model).is_empty());
    assert!(model.tessellations[0].faces.is_empty());
}

#[test]
fn bounded_cylindrical_trim_selects_between_coincident_supports() {
    let mut model = model_with_body();
    let lower = add_cylindrical_patch_face(&mut model, "lower", 0.0, 1.0);
    let upper = add_cylindrical_patch_face(&mut model, "upper", 2.0, 3.0);
    model.shells[0].faces = vec![lower.clone(), upper.clone()];
    model.tessellations.push(Tessellation {
        id: "lower-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(5.0, 0.0, 0.25),
            Point3::new(0.0, 5.0, 0.25),
            Point3::new(5.0, 0.0, 0.75),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert_eq!(assign_unique_surface_owners(&mut model), vec!["lower-mesh"]);
    assert_eq!(model.tessellations[0].faces, vec![lower]);
    assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));
}

#[test]
fn chordal_cylindrical_mesh_records_measured_support_deflection() {
    let mut model = model_with_body();
    let face = add_cylindrical_patch_face(&mut model, "chordal", 0.0, 1.0);
    model.shells[0].faces.push(face.clone());
    let deflection = 0.1;
    model.tessellations.push(Tessellation {
        id: "chordal-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(5.0 - deflection, 0.0, 0.25),
            Point3::new(0.0, 5.0 - deflection, 0.25),
            Point3::new(5.0 - deflection, 0.0, 0.75),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: vec![
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ],
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert_eq!(
        assign_unique_surface_owners(&mut model),
        vec!["chordal-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert!(model.tessellations[0]
        .chordal_deflection
        .is_some_and(|value| (value - deflection).abs() <= f64::EPSILON * 128.0));
}

#[test]
fn chordal_cylindrical_mesh_uses_unique_trim_when_normals_disagree() {
    let mut model = model_with_body();
    let face = add_cylindrical_patch_face(&mut model, "inconsistent-normals", 0.0, 1.0);
    model.shells[0].faces.push(face.clone());
    let deflection = 0.1;
    model.tessellations.push(Tessellation {
        id: "inconsistent-normals-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(5.0 - deflection, 0.0, 0.25),
            Point3::new(0.0, 5.0 - deflection, 0.25),
            Point3::new(5.0 - deflection, 0.0, 0.75),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: vec![
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, 1.0),
        ],
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert_eq!(
        assign_unique_surface_owners(&mut model),
        vec!["inconsistent-normals-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert!(model.tessellations[0]
        .chordal_deflection
        .is_some_and(|value| (value - deflection).abs() <= f64::EPSILON * 128.0));
}

#[test]
fn off_surface_planar_mesh_does_not_become_a_chordal_cache() {
    let mut model = model_with_body();
    let face = add_square_face(&mut model, "off-surface", 0.0);
    model.shells[0].faces.push(face);
    model.tessellations.push(Tessellation {
        id: "off-surface-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(0.25, -0.75, 0.1),
            Point3::new(1.75, -0.75, 0.1),
            Point3::new(1.0, 0.75, 0.1),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: vec![
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, 1.0),
        ],
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert!(assign_unique_surface_owners(&mut model).is_empty());
    assert!(model.tessellations[0].body.is_none());
    assert!(model.tessellations[0].faces.is_empty());
}

#[test]
fn cylindrical_trim_uses_the_short_boundary_arc() {
    let (start, span) = circular_interval(&[0.0, std::f64::consts::FRAC_PI_2]).unwrap();
    assert_eq!(start, 0.0);
    assert_eq!(span, std::f64::consts::FRAC_PI_2);
    assert!(circular_interval_contains(
        start,
        span,
        std::f64::consts::FRAC_PI_4,
        0.0
    ));
    assert!(!circular_interval_contains(
        start,
        span,
        std::f64::consts::PI,
        0.0
    ));
}

#[test]
fn cylindrical_trim_accepts_quantized_points_within_boundary_tolerance() {
    let start = 1.0;
    let span = 0.5;
    let tolerance = f64::EPSILON * 4096.0;

    assert!(circular_interval_contains(
        start,
        span,
        start - tolerance * 0.5,
        tolerance,
    ));
    assert!(circular_interval_contains(
        start,
        span,
        start + span + tolerance * 0.5,
        tolerance,
    ));
    assert!(!circular_interval_contains(
        start,
        span,
        start - tolerance * 2.0,
        tolerance,
    ));
}

#[test]
fn cone_support_binds_display_list_face() {
    let mut model = model_with_body();
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 3.0,
        ratio: 0.5,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    let v = 2.0;
    let local_radius = 3.0 + v * std::f64::consts::FRAC_PI_4.tan();
    let face = add_face(
        &mut model,
        "cone",
        cone,
        [
            Point3::new(local_radius, 0.0, v),
            Point3::new(0.0, local_radius * 0.5, v),
            Point3::new(-local_radius, 0.0, v),
            Point3::new(0.0, -local_radius * 0.5, v),
        ],
    );
    model.shells[0].faces.push(face.clone());
    model.tessellations.push(Tessellation {
        id: "cone-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(local_radius, 0.0, v),
            Point3::new(0.0, local_radius * 0.5, v),
            Point3::new(-local_radius, 0.0, v),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert_eq!(assign_unique_surface_owners(&mut model), vec!["cone-mesh"]);
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));
}

#[test]
fn cone_chordal_display_list_uses_analytic_normal_for_ownership() {
    let mut model = model_with_body();
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 3.0,
        ratio: 0.5,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    let axial = 2.0;
    let surface_radius = 3.0 + axial * std::f64::consts::FRAC_PI_4.tan();
    let face = add_face(
        &mut model,
        "cone-cache",
        cone.clone(),
        [
            Point3::new(surface_radius, 0.0, axial),
            Point3::new(0.0, surface_radius * 0.5, axial),
            Point3::new(-surface_radius, 0.0, axial),
            Point3::new(0.0, -surface_radius * 0.5, axial),
        ],
    );
    model.shells[0].faces.push(face.clone());
    let cache_radius = surface_radius - 0.1;
    let vertices = vec![
        Point3::new(cache_radius, 0.0, axial),
        Point3::new(0.0, cache_radius * 0.5, axial),
        Point3::new(-cache_radius, 0.0, axial),
    ];
    let normals = vertices
        .iter()
        .map(|point| analytic_surface_normal(&cone, *point).unwrap())
        .collect();
    model.tessellations.push(Tessellation {
        id: "cone-cache-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices,
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals,
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert_eq!(
        assign_unique_surface_owners(&mut model),
        vec!["cone-cache-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));
    assert!(model.tessellations[0]
        .chordal_deflection
        .is_some_and(|deflection| deflection > 0.09 && deflection < 0.11));
}

#[test]
fn conical_trim_uses_scaled_angular_coordinate() {
    let trim = ConicalTrim {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 3.0,
        ratio: 0.5,
        slope: 1.0,
        min_axial: 0.0,
        max_axial: 2.0,
        angular_start: 0.0,
        angular_span: std::f64::consts::FRAC_PI_2,
    };
    let mesh = |point: Point3, id: &str| Tessellation {
        id: id.into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![point],
        triangles: Vec::new(),
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    };
    let point_at = |angle: f64| {
        let local_radius = 4.0;
        Point3::new(
            local_radius * angle.cos(),
            local_radius * trim.ratio * angle.sin(),
            1.0,
        )
    };

    assert!(trim.contains_mesh(
        &mesh(point_at(std::f64::consts::FRAC_PI_4), "inside"),
        cadmpeg_ir::transform::Transform::identity(),
        0.0,
    ));
    assert!(!trim.contains_mesh(
        &mesh(point_at(3.0 * std::f64::consts::FRAC_PI_4), "outside"),
        cadmpeg_ir::transform::Transform::identity(),
        0.0,
    ));
}

#[test]
fn unique_nurbs_support_binds_exact_display_list_face() {
    let mut model = model_with_body();
    let surface = test_nurbs_surface();
    let face = add_face(
        &mut model,
        "nurbs-exact",
        SurfaceGeometry::Nurbs(surface.clone()),
        test_nurbs_corners(&surface),
    );
    model.shells[0].faces.push(face.clone());
    let vertices = [(0.15, 0.2), (0.8, 0.2), (0.5, 0.8)]
        .map(|(u, v)| cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v).unwrap())
        .to_vec();
    model.tessellations.push(Tessellation {
        id: "nurbs-exact-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices,
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert_eq!(
        assign_unique_surface_owners(&mut model),
        vec!["nurbs-exact-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));
    assert!(model.tessellations[0].chordal_deflection.is_none());
}

#[test]
fn non_exact_nurbs_support_does_not_use_an_unbounded_cache_fit() {
    let mut model = model_with_body();
    let surface = test_nurbs_surface();
    let face = add_face(
        &mut model,
        "nurbs-cache",
        SurfaceGeometry::Nurbs(surface.clone()),
        test_nurbs_corners(&surface),
    );
    model.shells[0].faces.push(face);
    let samples =
        [(0.15, 0.2), (0.8, 0.2), (0.5, 0.8)].map(|(u, v)| test_nurbs_point_normal(&surface, u, v));
    let deflection = 0.02;
    model.tessellations.push(Tessellation {
        id: "nurbs-cache-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: samples
            .iter()
            .map(|(point, normal)| point.translated(*normal, deflection))
            .collect(),
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: samples.iter().map(|(_, normal)| *normal).collect(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert!(assign_unique_surface_owners(&mut model).is_empty());
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
    assert!(model.tessellations[0].chordal_deflection.is_none());
}

#[test]
fn coincident_nurbs_supports_do_not_choose_a_display_list_face() {
    let mut model = model_with_body();
    let surface = test_nurbs_surface();
    let corners = test_nurbs_corners(&surface);
    let first = add_face(
        &mut model,
        "nurbs-coincident-first",
        SurfaceGeometry::Nurbs(surface.clone()),
        corners,
    );
    let second = add_face(
        &mut model,
        "nurbs-coincident-second",
        SurfaceGeometry::Nurbs(surface.clone()),
        corners,
    );
    model.shells[0].faces.extend([first, second]);
    model.tessellations.push(Tessellation {
        id: "nurbs-ambiguous-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: [(0.15, 0.2), (0.8, 0.2), (0.5, 0.8)]
            .map(|(u, v)| cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v).unwrap())
            .to_vec(),
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert!(assign_unique_surface_owners(&mut model).is_empty());
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
}

#[test]
fn coincident_nurbs_and_analytic_supports_do_not_fall_through_to_analytic_fit() {
    let mut model = model_with_body();
    let surface = flat_test_nurbs_surface();
    let corners = test_nurbs_corners(&surface);
    let nurbs_face = add_face(
        &mut model,
        "nurbs-plane-coincident",
        SurfaceGeometry::Nurbs(surface.clone()),
        corners,
    );
    let plane_face = add_face(
        &mut model,
        "plane-coincident",
        SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        corners,
    );
    model.shells[0].faces.extend([nurbs_face, plane_face]);
    model.tessellations.push(Tessellation {
        id: "nurbs-plane-ambiguous-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: [(0.15, 0.2), (0.8, 0.2), (0.5, 0.8)]
            .map(|(u, v)| cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v).unwrap())
            .to_vec(),
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: vec![Vector3::new(0.0, 0.0, 1.0); 3],
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert!(assign_unique_surface_owners(&mut model).is_empty());
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
}

#[test]
fn circular_hole_excludes_crossing_triangles_but_allows_boundary_chords() {
    let trim = PlanarTrim {
        frame: PlaneFrame {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            v_axis: Vector3::new(0.0, 1.0, 0.0),
        },
        outer: Some(PlanarOuter::Polygon(vec![
            Point2::new(-3.0, -3.0),
            Point2::new(3.0, -3.0),
            Point2::new(3.0, 3.0),
            Point2::new(-3.0, 3.0),
        ])),
        holes: vec![PlanarHole::Circle(CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 1.0,
        })],
        boundary_tolerance: 0.0,
    };
    let mesh = |vertices, triangle| Tessellation {
        id: "mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices,
        triangles: vec![triangle],
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        feature_edges: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    };
    let boundary_chord = mesh(
        vec![
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(2.0, 2.0, 0.0),
        ],
        [0, 1, 2],
    );
    let crossing = mesh(
        vec![
            Point3::new(-2.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ],
        [0, 1, 2],
    );

    assert!(trim.contains_mesh(
        &boundary_chord,
        cadmpeg_ir::transform::Transform::identity(),
        1.0e-9
    ));
    assert!(!trim.contains_mesh(
        &crossing,
        cadmpeg_ir::transform::Transform::identity(),
        1.0e-9
    ));
}

#[test]
fn polygonal_planar_hole_excludes_inner_face_mesh() {
    let trim = PlanarTrim {
        frame: PlaneFrame {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            v_axis: Vector3::new(0.0, 1.0, 0.0),
        },
        outer: Some(PlanarOuter::Polygon(vec![
            Point2::new(-4.0, -4.0),
            Point2::new(4.0, -4.0),
            Point2::new(4.0, 4.0),
            Point2::new(-4.0, 4.0),
        ])),
        holes: vec![PlanarHole::polygon(
            vec![
                Point2::new(-2.0, -2.0),
                Point2::new(2.0, -2.0),
                Point2::new(2.0, 2.0),
                Point2::new(-2.0, 2.0),
            ],
            EPS_DISPLAY_QUANTIZATION,
        )
        .unwrap()],
        boundary_tolerance: 0.0,
    };
    let mesh = |vertices, triangles| Tessellation {
        id: "mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices,
        triangles,
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    };
    let inner_face = mesh(
        vec![
            Point3::new(-2.0, -2.0, 0.0),
            Point3::new(2.0, -2.0, 0.0),
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(-2.0, 2.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    );
    let outer_face = mesh(
        vec![
            Point3::new(-4.0, -4.0, 0.0),
            Point3::new(-3.0, -4.0, 0.0),
            Point3::new(-4.0, -3.0, 0.0),
        ],
        vec![[0, 1, 2]],
    );
    let exterior_boundary_chord = mesh(
        vec![
            Point3::new(-2.0, -2.0, 0.0),
            Point3::new(2.0, -2.0, 0.0),
            Point3::new(0.0, -4.0, 0.0),
        ],
        vec![[0, 1, 2]],
    );
    let interior_boundary_chord = mesh(
        vec![
            Point3::new(-2.0, -2.0, 0.0),
            Point3::new(2.0, -2.0, 0.0),
            Point3::new(4.0, 4.0, 0.0),
        ],
        vec![[0, 1, 2]],
    );

    assert!(!trim.contains_mesh(
        &inner_face,
        cadmpeg_ir::transform::Transform::identity(),
        EPS_DISPLAY_QUANTIZATION
    ));
    assert!(trim.contains_mesh(
        &outer_face,
        cadmpeg_ir::transform::Transform::identity(),
        EPS_DISPLAY_QUANTIZATION
    ));
    assert!(trim.contains_mesh(
        &exterior_boundary_chord,
        cadmpeg_ir::transform::Transform::identity(),
        EPS_DISPLAY_QUANTIZATION
    ));
    assert!(!trim.contains_mesh(
        &interior_boundary_chord,
        cadmpeg_ir::transform::Transform::identity(),
        EPS_DISPLAY_QUANTIZATION
    ));
}

#[test]
fn mixed_planar_holes_reject_overlap() {
    let polygon = vec![
        Point2::new(-2.0, -2.0),
        Point2::new(2.0, -2.0),
        Point2::new(2.0, 2.0),
        Point2::new(-2.0, 2.0),
    ];
    assert!(circle_overlaps_polygon(
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 1.0,
        },
        &polygon,
        EPS_DISPLAY_QUANTIZATION
    ));
    assert!(!circle_overlaps_polygon(
        CircularHole {
            center: Point2::new(4.0, 0.0),
            radius: 1.0,
        },
        &polygon,
        EPS_DISPLAY_QUANTIZATION
    ));
}

#[test]
fn chordal_hole_constraint_uses_the_boundary_sampling_sagitta() {
    let hole = CircularHole {
        center: Point2::new(0.0, 0.0),
        radius: 1.0,
    };
    let boundary = (0..6)
        .map(|index| {
            let angle = f64::from(index) * std::f64::consts::TAU / 6.0;
            Point2::new(angle.cos(), angle.sin())
        })
        .collect::<Vec<_>>();
    let mut chordal = boundary.clone();
    let angle = std::f64::consts::PI / 6.0;
    chordal.push(Point2::new(0.9 * angle.cos(), 0.9 * angle.sin()));
    let (exclusion, boundary_circle) =
        chordal_hole_constraint(hole, &chordal, EPS_DISPLAY_QUANTIZATION).unwrap();
    assert_eq!(boundary_circle.radius, hole.radius);
    assert!(exclusion.radius < hole.radius);
    assert!(exclusion.radius > 0.8);

    let mut deep = boundary;
    deep.push(Point2::new(0.7 * angle.cos(), 0.7 * angle.sin()));
    assert!(chordal_hole_constraint(hole, &deep, EPS_DISPLAY_QUANTIZATION).is_none());

    let interior = vec![Point2::new(0.5, 0.0), Point2::new(0.0, 0.5)];
    assert!(chordal_hole_constraint(hole, &interior, EPS_DISPLAY_QUANTIZATION).is_none());
}

#[test]
fn circular_planar_bounds_choose_one_enclosing_outer() {
    let circles = vec![
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 10.0,
        },
        CircularHole {
            center: Point2::new(6.0, 0.0),
            radius: 2.0,
        },
        CircularHole {
            center: Point2::new(-6.0, 0.0),
            radius: 2.0,
        },
    ];
    let (outer, holes) = circular_outer_and_holes(&circles, EPS_DISPLAY_QUANTIZATION).unwrap();
    assert_eq!(outer.radius, 10.0);
    assert_eq!(holes.len(), 2);

    let ambiguous = vec![
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 10.0,
        },
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 10.0,
        },
    ];
    assert!(circular_outer_and_holes(&ambiguous, EPS_DISPLAY_QUANTIZATION).is_none());
}

#[test]
fn decode_reports_display_list_geometry() {
    let f = sldprt_with_body_and_display_list(&triangle_body());
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let source = result.ir().source.as_ref().expect("source metadata");

    assert_eq!(
        source
            .attributes
            .get("displaylist_vertices")
            .map(String::as_str),
        Some("3")
    );
    assert_eq!(
        source
            .attributes
            .get("displaylist_triangles")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].vertices.len(), 3);
    assert_eq!(result.ir().model.tessellations[0].vertices[1].x, 1000.0);
    assert_eq!(
        result.ir().model.tessellations[0].triangles,
        vec![[0, 1, 2]]
    );
    assert_eq!(result.ir().model.tessellations[0].strip_lengths, vec![3]);
    assert_eq!(result.ir().model.tessellations[0].normals.len(), 3);
    assert_eq!(result.ir().model.tessellations[0].channels.len(), 6);
    assert_eq!(
        result.ir().model.tessellations[0].faces,
        [result.ir().model.faces[0].id.clone()]
    );
    assert_eq!(
        result.ir().model.tessellations[0].body.as_ref(),
        Some(&result.ir().model.bodies[0].id)
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code.taxonomy() == LossTaxonomy::ReferenceGraphNotClosed
            && loss.message.contains("DisplayLists tessellation")
    }));
    assert!(result
        .ir()
        .native_unknowns("sldprt")
        .unwrap()
        .iter()
        .any(|record| {
            result
                .source_fidelity()
                .annotations
                .provenance
                .get(&record.id.0)
                .and_then(|note| note.tag.as_deref())
                == Some("displaylist_tessellation")
                && result
                    .source_fidelity()
                    .retained_record(&record.id.0)
                    .is_some_and(|source| source.data().is_some())
        }));
}

#[test]
fn decode_reports_extended_header_display_list_geometry() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x41,
        "Contents/DisplayLists",
        &extended_display_list_payload(),
    ));
    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].triangles, [[0, 1, 2]]);
}

#[test]
fn decode_rejects_incoherent_display_list_header_counts() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let header = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .expect("face tessellation class")
        + marker.len();
    payload[header..header + 4].copy_from_slice(&2_u32.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.tessellations.is_empty());
}

#[test]
fn decode_rejects_inconsistent_display_list_table() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let at = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len()
        + 8
        + 16;
    payload[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.tessellations.is_empty());
    assert!(!result
        .ir()
        .source
        .as_ref()
        .unwrap()
        .attributes
        .contains_key("displaylist_vertices"));
}

#[test]
fn decode_rejects_nonfinite_display_list_values() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let position_data = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len()
        + 8
        + 16
        + 4
        + 16;
    payload[position_data..position_data + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.tessellations.is_empty());
}

#[test]
fn planar_boundary_accepts_bounded_ellipse_arcs() {
    const SAMPLE_TOLERANCE: f64 = 1.0e-4;
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let frame = plane_frame(&surface).unwrap();
    let curve = CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 2.0,
        minor_radius: 1.0,
    };
    let (samples, boundary_tolerance) = planar_boundary_samples(
        &curve,
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        &surface,
        frame,
        EPS_DISPLAY_QUANTIZATION,
        SAMPLE_TOLERANCE,
    )
    .unwrap();

    assert!(samples.len() > 1);
    assert!(boundary_tolerance <= SAMPLE_TOLERANCE);
    assert_eq!(samples.first(), Some(&Point2::new(2.0, 0.0)));
    assert!(shortest_arc_span(0.0, std::f64::consts::PI).is_none());
}

#[test]
fn planar_boundary_accepts_bounded_circle_arcs() {
    const SAMPLE_TOLERANCE: f64 = 1.0e-4;
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let frame = plane_frame(&surface).unwrap();
    let curve = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let (samples, boundary_tolerance) = planar_boundary_samples(
        &curve,
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
        &surface,
        frame,
        EPS_DISPLAY_QUANTIZATION,
        SAMPLE_TOLERANCE,
    )
    .unwrap();

    assert!(samples.len() > 1);
    assert!(boundary_tolerance <= SAMPLE_TOLERANCE);
    assert_eq!(samples.first(), Some(&Point2::new(2.0, 0.0)));
}

#[test]
fn circular_arc_trim_disambiguates_coincident_planar_supports() {
    let mut model = model_with_body();
    let target = add_square_face(&mut model, "arc-target", 0.0);
    let competitor = add_face(
        &mut model,
        "arc-competitor",
        SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        [
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
    );
    for (curve_id, radius) in [
        ("curve-arc-competitor-0", 2.0),
        ("curve-arc-competitor-2", 1.0),
    ] {
        model
            .curves
            .iter_mut()
            .find(|curve| curve.id.0 == curve_id)
            .unwrap()
            .geometry = CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        };
    }
    model.shells[0].faces = vec![target.clone(), competitor];
    model.tessellations.push(Tessellation {
        id: "arc-trim-mesh".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(0.25, -0.75, 0.0),
            Point3::new(1.75, -0.75, 0.0),
            Point3::new(1.0, -0.25, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: Vec::new(),
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        corner_normals: Vec::new(),
        triangle_groups: Vec::new(),
        texture_assignments: Vec::new(),
        channels: Vec::new(),
    });

    assert_eq!(
        assign_unique_surface_owners(&mut model),
        vec!["arc-trim-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![target]);
    assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));
}

#[test]
fn planar_trim_accepts_concave_simple_loops_and_rejects_crossings() {
    const CONTAINMENT_TOLERANCE: f64 = 1.0e-9;
    let concave = vec![
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 0.0),
        Point2::new(4.0, 4.0),
        Point2::new(2.0, 4.0),
        Point2::new(2.0, 2.0),
        Point2::new(0.0, 2.0),
    ];
    assert!(is_simple_polygon(&concave, CONTAINMENT_TOLERANCE));
    assert!(PlanarHole::polygon(concave.clone(), CONTAINMENT_TOLERANCE).is_some());
    assert!(polygon_contains(
        &concave,
        Point2::new(1.0, 1.0),
        CONTAINMENT_TOLERANCE
    ));
    assert!(polygon_contains(
        &concave,
        Point2::new(3.0, 3.0),
        CONTAINMENT_TOLERANCE
    ));
    assert!(!polygon_contains(
        &concave,
        Point2::new(1.0, 3.0),
        CONTAINMENT_TOLERANCE
    ));

    let crossing = vec![
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 4.0),
        Point2::new(0.0, 4.0),
        Point2::new(4.0, 0.0),
    ];
    assert!(!is_simple_polygon(&crossing, CONTAINMENT_TOLERANCE));
}
