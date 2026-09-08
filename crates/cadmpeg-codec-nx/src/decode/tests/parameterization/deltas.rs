// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::framing::node_kind::NodeKind;

const EPS_CONE_ANGLE: f64 = 1.0e-12;

#[test]
fn decode_reports_status_framed_deltas_records_and_tombstones() {
    let stream = status_framed_deltas_stream();
    assert_eq!(
        crate::deltas::walk(&stream).bytes_decoded,
        stream.len() - DELTAS_PREAMBLE.len()
    );
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result
        .ir()
        .source
        .as_ref()
        .expect("source metadata")
        .attributes;

    assert_eq!(
        attributes.get("deltas.0.full.FACE").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes
            .get("deltas.0.tombstone.EDGE")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes.get("deltas.0.grammar").map(String::as_str),
        Some("typed_status_framed_records")
    );
}

#[test]
fn decode_accepts_exact_loop_and_rejects_incomplete_fin_deltas() {
    let stream = variable_status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result
        .ir()
        .source
        .as_ref()
        .expect("source metadata")
        .attributes;

    assert!(!attributes.contains_key("deltas.0.full.FIN"));
    assert_eq!(
        attributes.get("deltas.0.full.LOOP").map(String::as_str),
        Some("1")
    );
}

#[test]
fn decode_emits_point_added_by_deltas_stream() {
    let mut cur = Cursor::new(prt_with_partition(&deltas_point_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 12.5);
    assert_eq!(result.ir().model.points[0].position.y, -2.0);
    assert_eq!(result.ir().model.points[0].position.z, 4.0);
}

#[test]
fn decode_replaces_partition_point_with_same_xmt_deltas_point() {
    let partition = topology_partition_stream();
    let mut deltas = deltas_point_partition_stream();
    let record = deltas
        .windows(2)
        .rposition(|window| window == 29u16.to_be_bytes())
        .expect("deltas POINT");
    deltas[record + 2..record + 4].copy_from_slice(&11u16.to_be_bytes());
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 12.5);
    assert_eq!(result.ir().model.points[0].position.y, -2.0);
    assert_eq!(result.ir().model.points[0].position.z, 4.0);
}

#[test]
fn decode_preserves_partition_edge_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_edge_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir().model.edges[0].curve.as_ref(),
        Some(&result.ir().model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_face_and_vertex_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_face_vertex_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.faces[0].tolerance, Some(0.2));
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert_eq!(result.ir().model.vertices[0].tolerance, Some(0.1));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_loop_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_loop_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(NodeKind::Loop, 5)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_shell_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_shell_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(NodeKind::Shell, 3)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_fin_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_fin_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.coedges.len(), 1);
    assert_eq!(
        result.ir().model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_line_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_line_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let CurveGeometry::Line(line_curve) = result.ir().model.curves[0].geometry else {
        panic!("line");
    };
    let (&origin, &direction) = line_curve.parts();
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0));
    assert_eq!(direction, Vector3::new(0.0, 1.0, 0.0));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_plane_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_plane_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(
        matches!(result.ir().model.surfaces[0].geometry, SurfaceGeometry::Plane(plane_surface)
        if {
            let (origin, normal, u_axis) = plane_surface.parts();
            *origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && *normal == Vector3::new(0.0, 1.0, 0.0)
                && *u_axis == Vector3::new(1.0, 0.0, 0.0)
        })
    );
    assert_eq!(
        result.ir().model.faces[0].surface,
        result.ir().model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_offset_surface_from_status_framed_deltas() {
    let partition = offset_surface_topology_partition_stream();
    let deltas = deltas_offset_surface_partition_stream();
    let census = crate::deltas::walk(&deltas);
    assert_eq!(census.full_counts().get("OFFSET_SURF"), Some(&1));
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::offset_surfaces(&merged)
            .iter()
            .map(|surface| surface.state.distance())
            .collect::<Vec<_>>(),
        [4.5]
    );
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let [procedural] = result.ir().model.procedural_surfaces.as_slice() else {
        panic!("one offset surface");
    };
    let ProceduralSurfaceDefinition::Offset { distance, .. } = procedural.definition() else {
        panic!("offset surface");
    };
    assert_eq!(*distance, 4.5);
    assert_eq!(
        result.ir().model.faces[0].surface,
        *result
            .ir()
            .model
            .procedural_surface_owner(&procedural.id)
            .expect("offset owner")
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_blend_surface_from_status_framed_deltas() {
    let partition = blend_surface_topology_partition_stream();
    let deltas = deltas_blend_surface_partition_stream();
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let ProceduralSurfaceDefinition::Blend { radius, .. } =
        &result.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("blend surface");
    };
    assert_eq!(
        *radius,
        BlendRadiusLaw::Constant {
            signed_radius: -4.0
        }
    );
    assert_eq!(
        result.ir().model.faces[0].surface,
        *result
            .ir()
            .model
            .procedural_surface_owner(&result.ir().model.procedural_surfaces[0].id)
            .expect("blend owner")
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_trimmed_curve_from_status_framed_deltas() {
    let partition = trimmed_topology_partition_stream();
    let deltas = deltas_trimmed_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::trimmed_curves(&merged)[0]
            .state
            .parameters(),
        [0.000_3, 0.000_7]
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.edges[0].param_range, Some([0.3, 0.7]));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_surface_curve_from_status_framed_deltas() {
    let partition = surface_curve_topology_partition_stream();
    let deltas = deltas_surface_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::surface_curves(&merged)[0]
            .state
            .tolerance(),
        0.000_02
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir().model.edges[0].curve.as_ref(),
        Some(&result.ir().model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_circle_from_status_framed_deltas() {
    let partition = circle_topology_partition_stream();
    let deltas = deltas_circle_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.curves.iter().any(
        |curve| matches!(curve.geometry, CurveGeometry::Circle(circle_curve)
        if {
            let (center, axis, ref_direction, radius) = circle_curve.parts();
            *center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && *axis == Vector3::new(0.0, 1.0, 0.0)
                && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && *radius == 25.0
        })
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_ellipse_from_status_framed_deltas() {
    let partition = ellipse_topology_partition_stream();
    let deltas = deltas_ellipse_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.curves.iter().any(
        |curve| matches!(curve.geometry, CurveGeometry::Ellipse(ellipse_curve)
        if {
            let (center, axis, major_direction, major_radius, minor_radius) =
                ellipse_curve.parts();
            *center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && *axis == Vector3::new(0.0, 1.0, 0.0)
                && *major_direction == Vector3::new(1.0, 0.0, 0.0)
                && *major_radius == 30.0
                && *minor_radius == 12.0
        })
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cylinder_from_status_framed_deltas() {
    let partition = cylinder_topology_partition_stream();
    let deltas = deltas_cylinder_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.surfaces.iter().any(
        |surface| matches!(surface.geometry, SurfaceGeometry::Cylinder(cylinder_surface)
        if {
            let (origin, axis, ref_direction, radius) = cylinder_surface.parts();
            *origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && *axis == Vector3::new(0.0, 1.0, 0.0)
                && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && *radius == 25.0
        })
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cone_from_status_framed_deltas() {
    let partition = cone_topology_partition_stream();
    let deltas = deltas_cone_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.surfaces.iter().any(
        |surface| matches!(surface.geometry, SurfaceGeometry::Cone(cone_surface)
        if {
            let (origin, axis, ref_direction, radius, ratio, half_angle) =
                cone_surface.parts();
            *origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && *axis == Vector3::new(0.0, 1.0, 0.0)
                && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && *radius == 25.0
                && *ratio == 1.0
                && (half_angle - std::f64::consts::FRAC_PI_6).abs() < EPS_CONE_ANGLE
        })
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_sphere_from_status_framed_deltas() {
    let partition = sphere_topology_partition_stream();
    let deltas = deltas_sphere_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.surfaces.iter().any(
        |surface| matches!(surface.geometry, SurfaceGeometry::Sphere(sphere_surface)
        if {
            let (center, axis, ref_direction, radius) = sphere_surface.parts();
            *center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && *axis == Vector3::new(0.0, 1.0, 0.0)
                && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && *radius == 25.0
        })
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_torus_from_status_framed_deltas() {
    let partition = torus_topology_partition_stream();
    let deltas = deltas_torus_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.surfaces.iter().any(
        |surface| matches!(surface.geometry, SurfaceGeometry::Torus(torus_surface)
        if {
            let (center, axis, ref_direction, major_radius, minor_radius) =
                torus_surface.parts();
            *center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && *axis == Vector3::new(0.0, 1.0, 0.0)
                && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && *major_radius == 40.0
                && *minor_radius == 15.0
        })
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_emits_ext11_deltas_intersection_chart() {
    let stream = ext11_charted_intersection_curve_stream();
    let partition = charted_intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let curve_id = result
        .ir()
        .model
        .procedural_curve_owner(&result.ir().model.procedural_curves[0].id)
        .expect("intersection owner");
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("intersection cache");
    let Some(CurveGeometry::Nurbs(nurbs)) = curve.geometry.solved_cache() else {
        panic!("NURBS chart cache");
    };
    assert_eq!(nurbs.control_points()[1].x, 10.0);
    assert_eq!(nurbs.knots(), [2.0, 2.0, 5.0, 5.0]);
}
