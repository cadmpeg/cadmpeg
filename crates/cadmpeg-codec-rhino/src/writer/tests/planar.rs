// SPDX-License-Identifier: Apache-2.0

use super::add_polygon_hole;
use super::adjacent_quad_sheet;
use super::assert_planar_sheet_round_trip;
use super::planar_tetrahedron;
use super::polygon_sheet;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;

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
    ir.geometry.bodies[0].name = Some("named sheet".into());
    ir.geometry.bodies[0].color = Some(
        cadmpeg_ir::topology::Color::new(64.0 / 255.0, 128.0 / 255.0, 1.0, 192.0 / 255.0)
            .expect("valid color"),
    );
    ir.geometry.bodies[0].visible = Some(false);
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
        let body = &decoded.ir().geometry.bodies[0];
        assert_eq!(body.name.as_deref(), Some("named sheet"), "{version:?}");
        assert_eq!(body.color, ir.geometry.bodies[0].color, "{version:?}");
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
        assert_eq!(decoded.ir().geometry.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().geometry.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Sheet,
            "{version:?}"
        );
        assert_eq!(decoded.ir().geometry.shells.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().geometry.faces.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().geometry.loops.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().geometry.coedges.len(), 8, "{version:?}");
        assert_eq!(decoded.ir().geometry.edges.len(), 7, "{version:?}");
        assert_eq!(decoded.ir().geometry.vertices.len(), 6, "{version:?}");
        assert!(decoded.ir().geometry.edges.iter().all(|edge| edge
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get)
            == Some([2.0, 3.0])));
        let shared = decoded
            .ir()
            .geometry
            .edges
            .iter()
            .find(|edge| {
                decoded
                    .ir()
                    .geometry
                    .coedges
                    .iter()
                    .filter(|coedge| coedge.edge == edge.id)
                    .count()
                    == 2
            })
            .expect("one shared edge");
        let uses = decoded
            .ir()
            .geometry
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == shared.id)
            .collect::<Vec<_>>();
        assert_ne!(uses[0].sense, uses[1].sense);
        assert_eq!(uses[0].radial_next, uses[1].id);
        assert_eq!(uses[1].radial_next, uses[0].id);
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("validate the neutral model")
            .is_ok());
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
        assert_eq!(decoded.ir().geometry.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().geometry.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Solid,
            "{version:?}"
        );
        assert_eq!(decoded.ir().geometry.shells.len(), 1, "{version:?}");
        assert_eq!(decoded.ir().geometry.faces.len(), 4, "{version:?}");
        assert_eq!(decoded.ir().geometry.loops.len(), 4, "{version:?}");
        assert_eq!(decoded.ir().geometry.coedges.len(), 12, "{version:?}");
        assert_eq!(decoded.ir().geometry.edges.len(), 6, "{version:?}");
        assert_eq!(decoded.ir().geometry.vertices.len(), 4, "{version:?}");
        for (actual, expected) in decoded.ir().geometry.edges.iter().zip(&ir.geometry.edges) {
            assert_eq!(actual.param_range(), expected.param_range(), "{version:?}");
            assert_eq!(
                decoded
                    .ir()
                    .geometry
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
            .geometry
            .coedges
            .iter()
            .all(|coedge| coedge.radial_next != coedge.id));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("validate the neutral model")
            .is_ok());
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
    ir.geometry.bodies.append(&mut adjacent.geometry.bodies);
    ir.geometry.regions.append(&mut adjacent.geometry.regions);
    ir.geometry.shells.append(&mut adjacent.geometry.shells);
    ir.geometry.faces.append(&mut adjacent.geometry.faces);
    ir.geometry.loops.append(&mut adjacent.geometry.loops);
    ir.geometry.coedges.append(&mut adjacent.geometry.coedges);
    ir.geometry.edges.append(&mut adjacent.geometry.edges);
    ir.geometry.vertices.append(&mut adjacent.geometry.vertices);
    ir.geometry.points.append(&mut adjacent.geometry.points);
    ir.geometry.surfaces.append(&mut adjacent.geometry.surfaces);
    ir.geometry.curves.append(&mut adjacent.geometry.curves);
    ir.geometry.pcurves.append(&mut adjacent.geometry.pcurves);
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
        assert_eq!(decoded.ir().geometry.bodies.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().geometry.faces.len(), 3, "{version:?}");
        assert_eq!(decoded.ir().geometry.edges.len(), 10, "{version:?}");
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("validate the neutral model")
            .is_ok());
    }
}

#[test]
fn brep_and_free_geometry_round_trip_in_one_archive() {
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::Vector3;

    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.geometry.points.push(Point::new(
        PointId::mint("cadir:model:point#free").expect("identity grammar"),
        cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(5.0, 6.0, 7.0))
            .expect("a finite position is a point"),
        None,
    ));
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint("cadir:model:curve#free").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(5.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    });
    ir.geometry.surfaces.push(Surface {
        id: SurfaceId::mint("cadir:model:surface#free").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
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
        assert_eq!(decoded.ir().geometry.bodies.len(), 2, "{version:?}");
        assert!(decoded
            .ir()
            .geometry
            .points
            .iter()
            .any(|point| point.position().get() == Point3::new(5.0, 6.0, 7.0)));
        assert!(decoded
            .ir()
            .geometry
            .curves
            .iter()
            .any(|curve| matches!(curve.geometry, CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) if { circle_curve.radius().get() == 2.0 })));
        assert!(decoded.ir().geometry.surfaces.iter().any(
            |surface| matches!(surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))
            if {
                let origin = plane_surface.origin();
                origin.z == 3.0
            })
        ));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
            .expect("validate the neutral model")
            .is_ok());
    }
}

#[test]
fn open_planar_solid_is_written_as_an_open_brep_and_named() {
    let mut ir = adjacent_quad_sheet();
    ir.geometry.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;
    let (decoded, losses) = super::written(&ir);
    assert_eq!(decoded.geometry.bodies.len(), 1);
    assert_eq!(
        decoded.geometry.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(decoded.geometry.faces.len(), 2);
    assert!(
        losses
            .iter()
            .any(|loss| loss.starts_with("export/record.invalid")
                && loss.contains("written as an open Brep")),
        "{losses:?}"
    );
}

/// A stated tolerance finer than the Rhino Brep write route resolves.
const EPS_BELOW_WRITE_BOUND: f64 = 1.0e-12;

#[test]
fn sub_resolution_edge_tolerance_is_written_unchanged() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.geometry.edges[0].tolerance = Some(
        cadmpeg_ir::scalar::PositiveLength::new(EPS_BELOW_WRITE_BOUND)
            .expect("positive finite tolerance"),
    );
    let (decoded, _) = super::written(&ir);
    assert!(decoded.geometry.edges.iter().any(|edge| edge
        .tolerance
        .is_some_and(|tolerance| tolerance.get() == EPS_BELOW_WRITE_BOUND)));
}

#[test]
fn sub_resolution_face_tolerance_is_written_unchanged() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    ir.geometry.faces[0].tolerance = Some(
        cadmpeg_ir::scalar::PositiveLength::new(EPS_BELOW_WRITE_BOUND)
            .expect("positive finite tolerance"),
    );
    let (decoded, _) = super::written(&ir);
    assert_eq!(decoded.geometry.faces.len(), 1);
}

#[test]
fn planar_solid_orientation_is_translation_and_scale_invariant() {
    use crate::writer::{model::WritableModel, planar_solid_orientation};
    let ir = planar_tetrahedron();
    for (scale, translation) in [(1.0, 0.0), (1.0, 1e8), (1e200, 0.0), (1e-200, 0.0)] {
        let mut model = WritableModel::try_new(&ir).unwrap();
        // The sign owner consumes the oriented vertex rings; other writer fields
        // do not enter this calculation.
        for vertex in &mut model.vertices {
            vertex.point = Point3::new(
                2.0 * scale * vertex.point.x + translation,
                scale * vertex.point.y + translation,
                scale * vertex.point.z + translation,
            );
        }
        assert_eq!(planar_solid_orientation(&model), 1);
        for loop_ in &mut model.loops {
            loop_.coedges.reverse();
        }
        assert_eq!(planar_solid_orientation(&model), 2);
    }
}

/// `adjacent_quad_sheet` with its second face on a plane whose normal is
/// `-z` and with the face reversed: the face keeps its `+z` side.
fn adjacent_sheet_with_reversed_face() -> cadmpeg_ir::document::CadIr {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let mut ir = adjacent_quad_sheet();
    ir.geometry.surfaces[1].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("a plane frame"),
    ));
    ir.geometry.faces[1].sense = cadmpeg_ir::topology::Sense::Reversed;
    ir
}

/// The senses of the two uses of the edge two decoded faces share.
fn shared_edge_senses(ir: &cadmpeg_ir::document::CadIr) -> Vec<cadmpeg_ir::topology::Sense> {
    let shared = ir
        .geometry
        .edges
        .iter()
        .find(|edge| {
            ir.geometry
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == edge.id)
                .count()
                == 2
        })
        .expect("the faces share an edge");
    ir.geometry
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == shared.id)
        .map(|coedge| coedge.sense)
        .collect()
}

#[test]
fn reversed_face_of_a_consistent_shell_keeps_its_shared_edge() {
    let ir = adjacent_sheet_with_reversed_face();
    let (decoded, losses) = super::written(&ir);
    assert_eq!(decoded.geometry.bodies.len(), 1, "{losses:?}");
    assert_eq!(decoded.geometry.faces.len(), 2);
    assert_eq!(decoded.geometry.edges.len(), 7);
    assert_eq!(
        decoded
            .geometry
            .faces
            .iter()
            .map(|face| face.sense)
            .collect::<Vec<_>>(),
        [
            cadmpeg_ir::topology::Sense::Forward,
            cadmpeg_ir::topology::Sense::Reversed
        ]
    );
    let senses = shared_edge_senses(&decoded);
    assert_ne!(senses[0], senses[1]);
    let validation =
        cadmpeg_ir::validate_neutral(&decoded, Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:?}", validation.findings);
    assert!(
        losses
            .iter()
            .all(|loss| !loss.contains("writer.record-omitted")
                && !loss.contains("writer.edge-unjoined")),
        "{losses:?}"
    );
}

#[test]
fn shared_edge_with_equal_uses_is_written_as_two_edges() {
    use cadmpeg_ir::topology::Sense;

    let mut ir = adjacent_sheet_with_reversed_face();
    let ring = ir.geometry.loops[1]
        .coedges()
        .iter()
        .rev()
        .cloned()
        .collect();
    ir.geometry.loops[1]
        .replace_ring(ring, Vec::new())
        .expect("the reversed ring");
    for coedge in &mut ir.geometry.coedges[4..8] {
        coedge.sense = match coedge.sense {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        };
    }
    let (decoded, losses) = super::written(&ir);
    assert_eq!(decoded.geometry.bodies.len(), 2, "{losses:?}");
    assert_eq!(decoded.geometry.faces.len(), 2);
    assert_eq!(decoded.geometry.edges.len(), 8);
    assert!(
        losses
            .iter()
            .any(|loss| loss.contains("writer.edge-unjoined")
                && loss.contains("cadir:model:edge#adjacent.1")),
        "{losses:?}"
    );
    assert!(
        losses.iter().any(|loss| loss.contains("writer.body-split")),
        "{losses:?}"
    );
    assert!(
        losses
            .iter()
            .all(|loss| !loss.contains("writer.record-omitted")),
        "{losses:?}"
    );
}
