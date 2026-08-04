//! Tests for the `write_prepare` module.

use super::super::markers::spatial_vertex_coordinates;
use super::{append_spatial_vertex, arc_angle_relation_kind, patch_spatial_vertex, solved_tangent};
use crate::records::SketchRelationKind;
use cadmpeg_ir::features::Length;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::sketches::SketchGeometry;

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
        vec![replacement, second]
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
    use cadmpeg_ir::features::Angle;

    let line = SketchGeometry::Line {
        start: Point2::new(-2.0, 1.0),
        end: Point2::new(2.0, 1.0),
    };
    let arc = SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(1.0),
        start_angle: Angle(0.0),
        end_angle: Angle(std::f64::consts::PI),
    };
    let circle = SketchGeometry::Circle {
        center: Point2::new(2.0, 0.0),
        radius: Length(1.0),
    };
    assert_eq!(solved_tangent(&line, &arc), Some(true));
    assert_eq!(solved_tangent(&arc, &circle), Some(true));
}
