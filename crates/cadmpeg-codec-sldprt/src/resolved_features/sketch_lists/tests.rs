use super::*;
use std::f64::consts::FRAC_PI_2;

/// Agreement of computed lengths and angles.
const EPS_TEST: f64 = 1.0e-12;

/// The 64-byte head of a record (`docs/formats/sldprt.md`, "Sketch objects").
fn head(out: &mut Vec<u8>) {
    out.extend_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
    out.extend_from_slice(&[0xff; 8]);
    out.extend_from_slice(&F32_MINUS_ONE);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&F32_MINUS_ONE);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&[0; 9]);
    out.extend_from_slice(&1.0f64.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
}

fn point(out: &mut Vec<u8>, u: f64, v: f64) -> usize {
    let at = out.len();
    head(out);
    out.extend_from_slice(&0x1eu16.to_le_bytes());
    out.extend_from_slice(&u.to_le_bytes());
    out.extend_from_slice(&v.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&I32_MINUS_TWO);
    out.extend_from_slice(&[0; 36]);
    out.extend_from_slice(&[0; 10]);
    at
}

fn curve(out: &mut Vec<u8>, start: u16, end: u16, kind: u16, center: Option<u16>) -> usize {
    let at = out.len();
    head(out);
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&end.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&F64_MINUS_ONE);
    out.extend_from_slice(&kind.to_le_bytes());
    if let Some(center) = center {
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&center.to_le_bytes());
        for _ in 0..4 {
            out.extend_from_slice(&I32_MINUS_TWO);
        }
    }
    out.extend_from_slice(&[0; 10]);
    at
}

fn header(out: &mut Vec<u8>, count: u32) {
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&1u32.to_le_bytes());
}

/// Points 0 (the centre), 1 and 2, a line 1-2, an arc 1-2 about 0 and a full
/// circle through 1 about 0.
struct Sample {
    payload: Vec<u8>,
    points: [usize; 3],
    line: usize,
    arc: usize,
    circle: usize,
}

fn sample() -> Sample {
    let mut payload = vec![0; 7];
    header(&mut payload, 3);
    let points = [
        point(&mut payload, 0.0, 0.0),
        point(&mut payload, 0.01, 0.0),
        point(&mut payload, 0.0, 0.01),
    ];
    payload.extend_from_slice(&0u32.to_le_bytes());
    let line = curve(&mut payload, 1, 2, 0, None);
    let arc = curve(&mut payload, 1, 2, 1, Some(0));
    let circle = curve(&mut payload, 1, 1, 1, Some(0));
    Sample {
        payload,
        points,
        line,
        arc,
        circle,
    }
}

/// Metres to millimetres, no other change.
fn millimetres(point: [f64; 2]) -> Option<Point2> {
    Some(Point2::new(point[0] * 1000.0, point[1] * 1000.0))
}

/// Metres to millimetres with the v axis mirrored.
fn mirrored(point: [f64; 2]) -> Option<Point2> {
    Some(Point2::new(point[0] * 1000.0, -point[1] * 1000.0))
}

fn geometry(sample: &Sample, offset: usize) -> Option<SketchGeometry> {
    let lists = SketchRecordLists::new(&sample.payload);
    listed_record_geometry(&sample.payload, &lists, offset, &millimetres)
}

fn expected(definition: SketchGeometryDefinition) -> Option<SketchGeometry> {
    SketchGeometry::try_from(definition).ok()
}

/// Centre, radius and the two angles of an arc.
fn arc_parts(geometry: Option<SketchGeometry>) -> (Point2, f64, f64, f64) {
    let geometry = geometry.expect("arc record did not read");
    let SketchGeometryDefinition::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = geometry.definition()
    else {
        panic!("arc record did not read as an arc");
    };
    (
        center.get(),
        radius.get(),
        start_angle.get(),
        end_angle.get(),
    )
}

#[test]
fn point_records_supply_points() {
    let sample = sample();
    assert_eq!(
        geometry(&sample, sample.points[2]),
        expected(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 10.0)
        })
    );
}

#[test]
fn a_line_joins_the_point_records_its_indices_select() {
    let sample = sample();
    assert_eq!(
        geometry(&sample, sample.line),
        expected(SketchGeometryDefinition::Line {
            start: Point2::new(10.0, 0.0),
            end: Point2::new(0.0, 10.0),
        })
    );
}

#[test]
fn an_arc_runs_counterclockwise_from_its_first_index() {
    let sample = sample();
    let (center, radius, start_angle, end_angle) = arc_parts(geometry(&sample, sample.arc));
    assert_eq!(center, Point2::new(0.0, 0.0));
    assert!((radius - 10.0).abs() < EPS_TEST);
    assert!(start_angle.abs() < EPS_TEST);
    assert!((end_angle - FRAC_PI_2).abs() < EPS_TEST);
}

#[test]
fn a_mirroring_map_keeps_the_arc_on_its_own_side() {
    let sample = sample();
    let lists = SketchRecordLists::new(&sample.payload);
    let (_, _, start_angle, end_angle) = arc_parts(listed_record_geometry(
        &sample.payload,
        &lists,
        sample.arc,
        &mirrored,
    ));
    // The mirrored quarter arc runs from -pi/2 to 0.
    assert!((start_angle + FRAC_PI_2).abs() < EPS_TEST);
    assert!(end_angle.abs() < EPS_TEST);
}

#[test]
fn equal_endpoint_indices_make_a_full_circle() {
    let sample = sample();
    let circle = geometry(&sample, sample.circle).expect("circle record did not read");
    let SketchGeometryDefinition::Circle { center, radius } = circle.definition() else {
        panic!("circle record did not read as a circle");
    };
    assert_eq!(center.get(), Point2::new(0.0, 0.0));
    assert!((radius.get() - 10.0).abs() < EPS_TEST);
}

#[test]
fn a_header_whose_count_covers_a_curve_record_frames_no_sketch() {
    let mut payload = vec![0; 7];
    header(&mut payload, 2);
    let first = point(&mut payload, 0.0, 0.0);
    curve(&mut payload, 0, 0, 0, None);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let line = curve(&mut payload, 0, 1, 0, None);
    let lists = SketchRecordLists::new(&payload);
    assert_eq!(lists.record(first), None);
    assert_eq!(
        listed_record_geometry(&payload, &lists, line, &millimetres),
        None
    );
}

#[test]
fn an_unread_curve_type_and_an_index_past_the_points_stay_unresolved() {
    let mut payload = vec![0; 7];
    header(&mut payload, 2);
    point(&mut payload, 0.0, 0.0);
    point(&mut payload, 0.01, 0.0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let reversed = curve(&mut payload, 1, 1, 0xffff, Some(0));
    let past = curve(&mut payload, 0, 5, 0, None);
    let lists = SketchRecordLists::new(&payload);
    assert!(matches!(
        lists.record(reversed),
        Some(ListedRecord::Curve { .. })
    ));
    assert_eq!(
        listed_record_geometry(&payload, &lists, reversed, &millimetres),
        None
    );
    assert_eq!(
        listed_record_geometry(&payload, &lists, past, &millimetres),
        None
    );
}
