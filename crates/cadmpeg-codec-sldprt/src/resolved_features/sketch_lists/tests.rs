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
    scaled(point, 1.0)
}

/// Metres to millimetres with the v axis mirrored.
fn mirrored(point: [f64; 2]) -> Option<Point2> {
    scaled(point, -1.0)
}

/// Metres to millimetres with the v axis scaled by `v_sign`; `None` for a
/// coordinate that does not stay finite.
fn scaled(point: [f64; 2], v_sign: f64) -> Option<Point2> {
    let (u, v) = (point[0] * 1000.0, v_sign * point[1] * 1000.0);
    (u.is_finite() && v.is_finite()).then_some(Point2::new(u, v))
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

/// `docs/formats/sldprt.md` §2: distinct endpoint indices define the minor
/// arc, so endpoints 270 degrees apart counterclockwise from the first index
/// give the 90 degree arc from the second index to the first.
#[test]
fn distinct_endpoint_indices_select_the_minor_arc() {
    let mut payload = vec![0; 7];
    header(&mut payload, 3);
    point(&mut payload, 0.0, 0.0);
    point(&mut payload, 0.01, 0.0);
    point(&mut payload, 0.0, 0.01);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let arc = curve(&mut payload, 2, 1, 1, Some(0));
    let lists = SketchRecordLists::new(&payload);
    for project in [millimetres, mirrored] {
        let (_, _, start_angle, end_angle) =
            arc_parts(listed_record_geometry(&payload, &lists, arc, &project));
        assert!((end_angle - start_angle - FRAC_PI_2).abs() < EPS_TEST);
    }
    let (_, _, start_angle, _) =
        arc_parts(listed_record_geometry(&payload, &lists, arc, &millimetres));
    assert!(start_angle.abs() < EPS_TEST);
}

#[test]
fn a_curve_whose_selector_has_bit_zero_set_stays_unresolved() {
    let mut sample = sample();
    for offset in [sample.line, sample.circle] {
        sample.payload[offset + SELECTOR] = 5;
    }
    let lists = SketchRecordLists::new(&sample.payload);
    assert!(matches!(
        lists.record(sample.circle),
        Some(ListedRecord::Curve { .. })
    ));
    assert_eq!(geometry(&sample, sample.line), None);
    assert_eq!(geometry(&sample, sample.circle), None);
    assert!(geometry(&sample, sample.arc).is_some());
}

/// Write `value` as a little-endian u32 at `at`.
fn put_u32(payload: &mut [u8], at: usize, value: u32) {
    payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// Whether the record at `offset` stays a listed curve record with no
/// geometry.
fn curve_stays_unresolved(payload: &[u8], offset: usize) -> bool {
    let lists = SketchRecordLists::new(payload);
    matches!(lists.record(offset), Some(ListedRecord::Curve { .. }))
        && listed_record_geometry(payload, &lists, offset, &millimetres).is_none()
}

/// `docs/formats/sldprt.md` §2: the 92-byte direct profile line stores u32
/// `1` at +84 and point-object identifiers at +64 and +66.
#[test]
fn a_line_with_the_direct_identifier_trailer_stays_unresolved() {
    let mut sample = sample();
    put_u32(&mut sample.payload, sample.line + LINE_TRAILER, 1);
    put_u32(&mut sample.payload, sample.line + LINE_IDENTITY, 7);
    assert!(curve_stays_unresolved(&sample.payload, sample.line));
}

/// `docs/formats/sldprt.md` §2: a profile-locus 92-byte record with a
/// nonzero, non-null identity at +88 and no trailer `1` at +84 uses one-based
/// ordinals; a null identity selects no such reading.
#[test]
fn a_line_with_an_ordinal_identity_stays_unresolved() {
    let mut sample = sample();
    put_u32(&mut sample.payload, sample.line + LINE_IDENTITY, 7);
    assert!(curve_stays_unresolved(&sample.payload, sample.line));
    put_u32(&mut sample.payload, sample.line + LINE_IDENTITY, u32::MAX);
    assert_eq!(
        geometry(&sample, sample.line),
        expected(SketchGeometryDefinition::Line {
            start: Point2::new(10.0, 0.0),
            end: Point2::new(0.0, 10.0),
        })
    );
}

/// `docs/formats/sldprt.md` §2: selectors `68` and `132` mark the compact
/// legacy link-bearing forms, whose endpoint values are not point-record
/// positions.
#[test]
fn a_link_bearing_selector_leaves_the_curve_unresolved() {
    for selector in [0x44, 0x84] {
        let mut sample = sample();
        for offset in [sample.line, sample.arc, sample.circle] {
            sample.payload[offset + SELECTOR] = selector;
        }
        for offset in [sample.line, sample.arc, sample.circle] {
            assert!(curve_stays_unresolved(&sample.payload, offset));
        }
    }
}

/// A curve record in the layout before `swVersion` 11000: the head without
/// its eight zero bytes, so the endpoint indices stand at +56, f64 `-1` at
/// +64 and the curve type at +72.
fn compact_curve(out: &mut Vec<u8>, start: u16, end: u16, kind: u16, center: Option<u16>) -> usize {
    let at = out.len();
    head(out);
    out.truncate(out.len() - 8);
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

/// `docs/formats/sldprt.md` §2: in the layout before `swVersion` 11000 the
/// endpoint and centre values follow the coordinate-roster, object-index and
/// minor-arc rules of the compact indexed forms, not point-record positions.
#[test]
fn a_curve_without_the_eight_zero_bytes_stays_unresolved() {
    let mut payload = vec![0; 7];
    header(&mut payload, 3);
    point(&mut payload, 0.0, 0.0);
    point(&mut payload, 0.01, 0.0);
    point(&mut payload, 0.0, 0.01);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let line = compact_curve(&mut payload, 1, 2, 0, None);
    let arc = compact_curve(&mut payload, 2, 1, 1, Some(0));
    let circle = compact_curve(&mut payload, 1, 1, 1, Some(0));
    for offset in [line, arc, circle] {
        assert!(curve_stays_unresolved(&payload, offset));
    }
}
