use super::{
    line_line_angle, line_line_distance, point_line_distance_value, unoriented_line_line_angle,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};

fn line(start: Point2, end: Point2) -> SketchGeometry {
    SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap()
}

#[test]
fn line_metrics_use_geometry_without_entity_metadata() {
    let horizontal = line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let parallel = line(Point2::new(0.0, 5.0), Point2::new(10.0, 5.0));
    let vertical = line(Point2::new(0.0, 0.0), Point2::new(0.0, 10.0));
    let reversed = line(Point2::new(10.0, 0.0), Point2::new(0.0, 0.0));
    assert_eq!(line_line_distance(&horizontal, &parallel), Some(5.0));
    assert_eq!(
        point_line_distance_value(Point2::new(3.0, 5.0), &horizontal),
        Some(5.0)
    );
    assert_eq!(
        line_line_angle(&horizontal, &vertical),
        Some(std::f64::consts::FRAC_PI_2)
    );
    assert_eq!(
        line_line_angle(&horizontal, &reversed),
        Some(std::f64::consts::PI)
    );
    assert_eq!(
        unoriented_line_line_angle(&horizontal, &reversed),
        Some(0.0)
    );
}

#[test]
fn line_metrics_preserve_non_line_and_degenerate_rejection() {
    let line = line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let point = SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(3.0, 5.0),
    })
    .unwrap();
    let degenerate = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(0.0, 0.0),
    })
    .unwrap();
    assert_eq!(line_line_distance(&point, &line), None);
    assert_eq!(line_line_distance(&line, &point), None);
    assert_eq!(line_line_distance(&line, &degenerate), None);
    assert_eq!(line_line_angle(&point, &line), None);
    assert_eq!(line_line_angle(&line, &point), None);
    assert_eq!(line_line_angle(&line, &degenerate), None);
    assert_eq!(unoriented_line_line_angle(&point, &line), None);
    assert_eq!(
        point_line_distance_value(Point2::new(3.0, 5.0), &point),
        None
    );
    assert_eq!(
        point_line_distance_value(Point2::new(3.0, 5.0), &degenerate),
        None
    );
}
