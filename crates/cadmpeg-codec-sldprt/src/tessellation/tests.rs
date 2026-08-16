// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::LossTaxonomy;

use crate::test_support::*;
use crate::SldprtCodec;

use super::*;
use cadmpeg_ir::geometry::{Curve, Surface};
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
    assert!(parse_table_sequence(&payload, 8).is_some());
}

#[test]
fn extended_face_tessellation_header_places_table_at_plus_40() {
    let mut payload = Vec::new();
    for word in [1_u32, 1, 1, 0, 0, 0x0020_1296, 0, 0, 0, 0] {
        payload.extend(word.to_le_bytes());
    }
    payload.extend(table());
    assert_eq!(descriptor_table_offset(&payload, 0), 40);
    assert!(parse_table_sequence(&payload, 40).is_some());
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

    assert_eq!(assign_unique_analytic_owners(&mut model), vec!["mesh"]);
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
    assert!(assign_unique_analytic_owners(&mut model).is_empty());
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

    assert_eq!(
        assign_unique_analytic_owners(&mut model),
        vec!["lower-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![lower]);
    assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));
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

    assert_eq!(assign_unique_analytic_owners(&mut model), vec!["cone-mesh"]);
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(model.tessellations[0].body, Some(BodyId("body".into())));
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
        outer: vec![
            Point2::new(-3.0, -3.0),
            Point2::new(3.0, -3.0),
            Point2::new(3.0, 3.0),
            Point2::new(-3.0, 3.0),
        ],
        holes: vec![CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 1.0,
        }],
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
                    .is_some_and(|source| source.data.is_some())
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
