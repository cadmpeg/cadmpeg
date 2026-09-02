// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;

use super::*;
use crate::{RhinoArchiveVersion, RhinoCodec};

#[test]
fn planar_triangle_sheet_round_trips_connected_topology() {
    let ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    assert_planar_sheet_round_trip(&ir, 1, 3);
}

#[test]
fn planar_quad_sheet_round_trips_connected_topology() {
    let ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(3.0, 2.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    assert_planar_sheet_round_trip(&ir, 1, 4);
}

#[test]
fn planar_sheet_round_trips_object_attributes() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.model.bodies[0].name = Some("named sheet".into());
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 64.0 / 255.0,
        g: 128.0 / 255.0,
        b: 1.0,
        a: 192.0 / 255.0,
    });
    ir.model.bodies[0].visible = Some(false);
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        let body = &decoded.ir().model.bodies[0];
        assert_eq!(body.name.as_deref(), Some("named sheet"), "{version:?}");
        assert_eq!(body.color, ir.model.bodies[0].color, "{version:?}");
        assert_eq!(body.visible, Some(false), "{version:?}");
    }
}

#[test]
fn planar_sheet_with_hole_round_trips_connected_topology() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(4.0, 0.0, 0.0),
        Point3::new(4.0, 3.0, 0.0),
        Point3::new(0.0, 3.0, 0.0),
    ]);
    add_polygon_hole(
        &mut ir,
        &[
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(3.0, 2.0, 0.0),
            Point3::new(3.0, 1.0, 0.0),
        ],
    );
    assert_planar_sheet_round_trip(&ir, 2, 8);
}

#[test]
fn adjacent_planar_faces_round_trip_shared_edge_and_domains() {
    let ir = adjacent_quad_sheet();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().model.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Sheet,
            "{version:?}"
        );
        assert_eq!(decoded.ir().model.shells.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().model.faces.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().model.loops.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().model.coedges.len(), 8, "{version:?}");
        assert_eq!(decoded.ir().model.edges.len(), 7, "{version:?}");
        assert_eq!(decoded.ir().model.vertices.len(), 6, "{version:?}");
        assert!(decoded
            .ir()
            .model
            .edges
            .iter()
            .all(|edge| edge.param_range == Some([2.0, 3.0])));
        let shared = decoded
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| {
                decoded
                    .ir()
                    .model
                    .coedges
                    .iter()
                    .filter(|coedge| coedge.edge == edge.id)
                    .count()
                    == 2
            })
            .expect("one shared edge");
        let uses = decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == shared.id)
            .collect::<Vec<_>>();
        assert_ne!(uses[0].sense, uses[1].sense);
        assert_eq!(uses[0].radial_next, uses[1].id);
        assert_eq!(uses[1].radial_next, uses[0].id);
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn planar_tetrahedron_round_trips_as_closed_solid() {
    let ir = planar_tetrahedron();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().model.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Solid,
            "{version:?}"
        );
        assert_eq!(decoded.ir().model.shells.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().model.faces.len(), 4, "{version:?}");
        assert_eq!(decoded.ir().model.loops.len(), 4, "{version:?}");
        assert_eq!(decoded.ir().model.coedges.len(), 12, "{version:?}");
        assert_eq!(decoded.ir().model.edges.len(), 6, "{version:?}");
        assert_eq!(decoded.ir().model.vertices.len(), 4, "{version:?}");
        for (actual, expected) in decoded.ir().model.edges.iter().zip(&ir.model.edges) {
            assert_eq!(actual.param_range, expected.param_range, "{version:?}");
            assert_eq!(
                decoded
                    .ir()
                    .model
                    .coedges
                    .iter()
                    .filter(|coedge| coedge.edge == actual.id)
                    .count(),
                2,
                "{version:?}"
            );
        }
        assert!(decoded
            .ir()
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.radial_next != coedge.id));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn multiple_brep_objects_round_trip_in_one_archive() {
    let mut ir = polygon_sheet(&[
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(-1.0, 0.0, 0.0),
        Point3::new(-1.5, 1.0, 0.0),
    ]);
    let mut adjacent = adjacent_quad_sheet();
    ir.model.bodies.append(&mut adjacent.model.bodies);
    ir.model.regions.append(&mut adjacent.model.regions);
    ir.model.shells.append(&mut adjacent.model.shells);
    ir.model.faces.append(&mut adjacent.model.faces);
    ir.model.loops.append(&mut adjacent.model.loops);
    ir.model.coedges.append(&mut adjacent.model.coedges);
    ir.model.edges.append(&mut adjacent.model.edges);
    ir.model.vertices.append(&mut adjacent.model.vertices);
    ir.model.points.append(&mut adjacent.model.points);
    ir.model.surfaces.append(&mut adjacent.model.surfaces);
    ir.model.curves.append(&mut adjacent.model.curves);
    ir.model.pcurves.append(&mut adjacent.model.pcurves);
    ir.finalize();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert!(
            decoded
                .report()
                .losses
                .iter()
                .all(|loss| !loss.message.contains("CRC mismatch")),
            "{version:?}: {:?}",
            decoded.report().losses
        );
        assert_eq!(decoded.ir().model.bodies.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().model.faces.len(), 3, "{version:?}");
        assert_eq!(decoded.ir().model.edges.len(), 10, "{version:?}");
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn brep_and_free_geometry_round_trip_in_one_archive() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::Vector3;

    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.model.points.push(Point {
        id: PointId("cadir:model:point#free".into()),
        position: Point3::new(5.0, 6.0, 7.0),
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId("cadir:model:curve#free".into()),
        geometry: CurveGeometry::Circle {
            center: Point3::new(5.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: SurfaceId("cadir:model:surface#free".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.finalize();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.bodies.len(), 2, "{version:?}");
        assert!(decoded
            .ir()
            .model
            .points
            .iter()
            .any(|point| point.position == Point3::new(5.0, 6.0, 7.0)));
        assert!(decoded
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| matches!(curve.geometry, CurveGeometry::Circle { radius: 2.0, .. })));
        assert!(decoded.ir().model.surfaces.iter().any(|surface| matches!(
            surface.geometry,
            SurfaceGeometry::Plane { origin, .. } if origin.z == 3.0
        )));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn open_planar_solid_is_rejected_before_output() {
    let mut ir = adjacent_quad_sheet();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;
    let mut output = vec![0xaa];
    let error = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("expected error");
    assert!(error.to_string().contains("incidence"));
    assert_eq!(output, [0xaa]);
}
