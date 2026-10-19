//! Tests for the `write_prepare` module.

use super::super::markers::spatial_vertex_coordinates;
use super::{
    append_spatial_vertex, arc_angle_relation_kind, first_marker_offsets_by_id,
    patch_spatial_vertex, solved_tangent,
};
use crate::records::{SketchInputEntity, SketchInputKind, SketchRelationKind};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};

#[test]
fn spatial_vertex_patch_preserves_record_shape_and_order() {
    let first = Point3::new(1.0, 2.0, 3.0);
    let second = Point3::new(4.0, 5.0, 6.0);
    let mut payload = Vec::new();
    append_spatial_vertex(&mut payload, first);
    append_spatial_vertex(&mut payload, second);

    let replacement = Point3::new(-7.5, 8.25, 9.0);
    patch_spatial_vertex(&mut payload, 0, replacement).expect("required invariant");

    assert_eq!(
        spatial_vertex_coordinates(&payload),
        vec![
            cadmpeg_ir::features::FinitePoint3::new(replacement).expect("finite replacement"),
            cadmpeg_ir::features::FinitePoint3::new(second).expect("finite second point"),
        ]
    );
    assert_eq!(payload.len(), 138);
}

#[test]
fn generated_arc_angles_use_only_exact_native_quadrants() {
    assert_eq!(
        arc_angle_relation_kind(std::f64::consts::FRAC_PI_2),
        Some(SketchRelationKind::ArcAngle90)
    );
    assert_eq!(
        arc_angle_relation_kind(std::f64::consts::PI),
        Some(SketchRelationKind::ArcAngle180)
    );
    assert_eq!(
        arc_angle_relation_kind(3.0 * std::f64::consts::FRAC_PI_2),
        Some(SketchRelationKind::ArcAngle270)
    );
    assert_eq!(arc_angle_relation_kind(std::f64::consts::FRAC_PI_3), None);
}

#[test]
fn solved_tangent_treats_arcs_as_bounded_circles() {
    use cadmpeg_ir::scalar::Angle;

    let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(-2.0, 1.0),
        end: Point2::new(2.0, 1.0),
    })
    .unwrap();
    let arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length::new(1.0).unwrap(),
        start_angle: Angle::new(0.0).unwrap(),
        end_angle: Angle::new(std::f64::consts::PI).unwrap(),
    })
    .unwrap();
    let circle = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(2.0, 0.0),
        radius: Length::new(1.0).unwrap(),
    })
    .unwrap();
    assert_eq!(solved_tangent(&line, &arc), Some(true));
    assert_eq!(solved_tangent(&arc, &circle), Some(true));
}

#[test]
fn marker_offset_index_matches_the_per_lane_marker_scan() {
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    for _ in 0..64 {
        let lanes = (0..next(5))
            .map(|_| {
                (0..next(30))
                    .map(|ordinal| {
                        SketchInputEntity::new(
                            format!("marker-{}", next(12)),
                            "lane",
                            u32::try_from(ordinal).unwrap(),
                            next(500),
                            SketchInputKind::Point,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let indexed = first_marker_offsets_by_id(lanes.iter().map(Vec::as_slice));
        for id in (0..13).map(|id| format!("marker-{id}")) {
            let scanned = lanes
                .iter()
                .enumerate()
                .filter_map(|(lane_index, markers)| {
                    let marker = markers.iter().find(|marker| marker.id() == id)?;
                    Some((lane_index, marker.offset()))
                })
                .collect::<Vec<_>>();
            assert_eq!(
                indexed.get(&id).cloned().unwrap_or_default(),
                scanned,
                "{id}"
            );
        }
    }
}
