use super::*;
use std::f64::consts::{FRAC_PI_2, PI};

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

/// A curve record: endpoint indices, a u32, f64 `-1`, the sense word, and for
/// an arc the centre index and four i32 `-2`.
fn curve(out: &mut Vec<u8>, start: u16, end: u16, sense: i32, center: Option<u16>) -> usize {
    let at = out.len();
    head(out);
    out[at + STATE..at + STATE + 2].copy_from_slice(&1u16.to_le_bytes());
    write_curve_fields(out, start, end, sense, center);
    at
}

fn write_curve_fields(out: &mut Vec<u8>, start: u16, end: u16, sense: i32, center: Option<u16>) {
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&end.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&F64_MINUS_ONE);
    out.extend_from_slice(&sense.to_le_bytes());
    if let Some(center) = center {
        out.extend_from_slice(&center.to_le_bytes());
        for _ in 0..4 {
            out.extend_from_slice(&I32_MINUS_TWO);
        }
    }
    out.extend_from_slice(&[0; 10]);
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

/// What [`listed_entity`] reads at `offset` when every point record belongs
/// to the marker's feature object.
fn read(
    payload: &[u8],
    offset: usize,
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<Result<ListedEntity, ChordConflict>> {
    let lists = SketchRecordLists::new(payload);
    listed_entity(payload, &lists, offset, &|_| true, project)
}

/// The geometry [`listed_entity`] reads at `offset`.
fn listed_geometry(
    payload: &[u8],
    offset: usize,
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<SketchGeometry> {
    read(payload, offset, project)?
        .ok()
        .map(|entity| entity.geometry)
}

fn geometry(sample: &Sample, offset: usize) -> Option<SketchGeometry> {
    listed_geometry(&sample.payload, offset, &millimetres)
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

/// A point record is a point, whatever its u32 at +17 (`1` here, a curve
/// code in a marker's roster).
#[test]
fn point_records_supply_points() {
    let sample = sample();
    let at = sample.points[2] + 17;
    assert_eq!(View::u32_le_at(&sample.payload, at), Some(1));
    assert_eq!(
        read(&sample.payload, sample.points[2], &millimetres),
        Some(Ok(ListedEntity {
            geometry: expected(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 10.0)
            })
            .unwrap(),
            endpoints: None,
            construction: None,
            outside_profile: false,
            index_basis_loss: None,
        }))
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
    let (_, _, start_angle, end_angle) =
        arc_parts(listed_geometry(&sample.payload, sample.arc, &mirrored));
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
    assert_eq!(listed_geometry(&payload, line, &millimetres), None);
}

#[test]
fn an_unread_curve_type_and_an_index_past_the_points_stay_unresolved() {
    let mut payload = vec![0; 7];
    header(&mut payload, 2);
    point(&mut payload, 0.0, 0.0);
    point(&mut payload, 0.01, 0.0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let reversed = curve(&mut payload, 1, 1, 0x40, Some(0));
    let past = curve(&mut payload, 0, 5, 0, None);
    let lists = SketchRecordLists::new(&payload);
    assert!(matches!(
        lists.record(reversed),
        Some(ListedRecord::Curve { .. })
    ));
    assert_eq!(listed_geometry(&payload, reversed, &millimetres), None);
    assert_eq!(listed_geometry(&payload, past, &millimetres), None);
}

/// Points 0 (the centre), 1 at 0 degrees and 2 at 90 degrees, then `arc`
/// joining endpoint indices `start` and `end` about point 0 in `sense`.
fn quarter_points_arc(start: u16, end: u16, sense: i32) -> (Vec<u8>, usize) {
    let mut payload = vec![0; 7];
    header(&mut payload, 3);
    point(&mut payload, 0.0, 0.0);
    point(&mut payload, 0.01, 0.0);
    point(&mut payload, 0.0, 0.01);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let arc = curve(&mut payload, start, end, sense, Some(0));
    (payload, arc)
}

/// Sense `1` sweeps counterclockwise from the first index to the second, also
/// past half a turn: from 90 degrees to 0 degrees is three quarters of a turn.
#[test]
fn a_counterclockwise_arc_can_sweep_past_half_a_turn() {
    let (payload, arc) = quarter_points_arc(2, 1, 1);
    for project in [millimetres, mirrored] {
        let (_, _, start_angle, end_angle) = arc_parts(listed_geometry(&payload, arc, &project));
        assert!((end_angle - start_angle - 3.0 * FRAC_PI_2).abs() < EPS_TEST);
    }
    let (_, _, start_angle, _) = arc_parts(listed_geometry(&payload, arc, &millimetres));
    assert!((start_angle - FRAC_PI_2).abs() < EPS_TEST);
}

/// Sense `-1` sweeps clockwise from the first index to the second.
#[test]
fn a_clockwise_arc_runs_from_its_second_index_counterclockwise() {
    let (payload, arc) = quarter_points_arc(2, 1, -1);
    let (center, radius, start_angle, end_angle) =
        arc_parts(listed_geometry(&payload, arc, &millimetres));
    assert_eq!(center, Point2::new(0.0, 0.0));
    assert!((radius - 10.0).abs() < EPS_TEST);
    assert!(start_angle.abs() < EPS_TEST);
    assert!((end_angle - FRAC_PI_2).abs() < EPS_TEST);
    let (payload, arc) = quarter_points_arc(1, 2, -1);
    let (_, _, start_angle, end_angle) = arc_parts(listed_geometry(&payload, arc, &millimetres));
    assert!((start_angle - FRAC_PI_2).abs() < EPS_TEST);
    assert!((end_angle - 2.0 * PI).abs() < EPS_TEST);
}

/// A sense word `-1` whose centre index is not followed by i32 `-2` is not an
/// arc record.
#[test]
fn a_clockwise_word_without_the_minus_two_cells_stays_unresolved() {
    let (mut payload, arc) = quarter_points_arc(2, 1, -1);
    let cells = arc + CURRENT_FIELDS + CENTER + 2;
    payload[cells..cells + 4].copy_from_slice(&3u32.to_le_bytes());
    assert!(curve_stays_unresolved(&payload, arc));
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
        && listed_geometry(payload, offset, &millimetres).is_none()
}

/// Where a current-layout line record stores its trailer u32.
const LINE_TRAILER: usize = 84;
/// Where a current-layout line record stores its identity.
const LINE_IDENTITY: usize = 88;

/// A line's indices select point records whatever its trailer and identity.
#[test]
fn a_line_reads_point_positions_whatever_its_trailer_and_identity() {
    for (trailer, identity) in [(1, 7), (0, 7), (0, u32::MAX)] {
        let mut sample = sample();
        put_u32(&mut sample.payload, sample.line + LINE_TRAILER, trailer);
        put_u32(&mut sample.payload, sample.line + LINE_IDENTITY, identity);
        assert_eq!(
            geometry(&sample, sample.line),
            expected(SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 0.0),
                end: Point2::new(0.0, 10.0),
            })
        );
    }
}

/// Selectors `68` and `132` have bit 0 clear: their indices select point
/// records.
#[test]
fn a_link_bearing_selector_reads_point_positions() {
    for selector in [0x44, 0x84] {
        let mut sample = sample();
        let plain =
            [sample.line, sample.arc, sample.circle].map(|offset| geometry(&sample, offset));
        for offset in [sample.line, sample.arc, sample.circle] {
            sample.payload[offset + SELECTOR] = selector;
        }
        for (offset, plain) in [sample.line, sample.arc, sample.circle]
            .into_iter()
            .zip(plain)
        {
            assert!(plain.is_some());
            assert_eq!(geometry(&sample, offset), plain);
        }
    }
}

/// A curve record in the layout before `swVersion` 11000: the head without
/// its eight zero bytes, so the endpoint indices stand at +56, f64 `-1` at
/// +64 and the sense at +72.
fn compact_curve(
    out: &mut Vec<u8>,
    start: u16,
    end: u16,
    sense: i32,
    center: Option<u16>,
) -> usize {
    let at = out.len();
    head(out);
    out.truncate(out.len() - 8);
    write_curve_fields(out, start, end, sense, center);
    at
}

/// In the layout before `swVersion` 11000 the endpoint and centre indices
/// also select point records.
#[test]
fn a_curve_without_the_eight_zero_bytes_reads_point_positions() {
    let mut payload = vec![0; 7];
    header(&mut payload, 3);
    point(&mut payload, 0.0, 0.0);
    point(&mut payload, 0.01, 0.0);
    point(&mut payload, 0.0, 0.01);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let line = compact_curve(&mut payload, 1, 2, 0, None);
    let arc = compact_curve(&mut payload, 1, 2, 1, Some(0));
    let circle = compact_curve(&mut payload, 1, 1, 1, Some(0));
    assert_eq!(
        listed_geometry(&payload, line, &millimetres),
        expected(SketchGeometryDefinition::Line {
            start: Point2::new(10.0, 0.0),
            end: Point2::new(0.0, 10.0),
        })
    );
    let (center, radius, start_angle, end_angle) =
        arc_parts(listed_geometry(&payload, arc, &millimetres));
    assert_eq!(center, Point2::new(0.0, 0.0));
    assert!((radius - 10.0).abs() < EPS_TEST);
    assert!(start_angle.abs() < EPS_TEST);
    assert!((end_angle - FRAC_PI_2).abs() < EPS_TEST);
    let circle =
        listed_geometry(&payload, circle, &millimetres).expect("circle record did not read");
    assert!(matches!(
        circle.definition(),
        SketchGeometryDefinition::Circle { .. }
    ));
}

#[test]
fn a_curve_after_a_projected_or_unread_record_belongs_to_the_preceding_sketch() {
    for external in [false, true] {
        let mut payload = vec![0; 7];
        header(&mut payload, 2);
        point(&mut payload, 0.0, 0.0);
        point(&mut payload, 0.01, 0.0);
        payload.extend_from_slice(&0u32.to_le_bytes());
        curve(&mut payload, 0, 1, 0, None);
        let separator = payload.len();
        head(&mut payload);
        payload[separator + head::OBJECT_POINTER] = u8::from(external);
        payload.extend_from_slice(&[0; 24]);
        let later = curve(&mut payload, 1, 0, 0, None);
        let lists = SketchRecordLists::new(&payload);
        assert_eq!(
            matches!(lists.record(separator), Some(ListedRecord::Curve { .. })),
            external
        );
        // After a projected record the curve stays in the first curve list;
        // after an unread record it belongs to a later one.
        assert_eq!(lists.listed_curve(&payload, later).is_some(), external);
        assert_eq!(
            listed_geometry(&payload, later, &millimetres),
            expected(SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 0.0),
                end: Point2::new(0.0, 0.0),
            })
        );
    }
}

/// A line or an arc names the point records it runs between, the first at
/// the start of its geometry; a full circle names none.
#[test]
fn a_curve_names_its_endpoint_point_records_from_its_start() {
    let sample = sample();
    let endpoints = |offset, project: &dyn Fn([f64; 2]) -> Option<Point2>| {
        read(&sample.payload, offset, &project)
            .and_then(Result::ok)
            .map(|entity| entity.endpoints)
    };
    let forward = Some(Some([sample.points[1], sample.points[2]]));
    assert_eq!(endpoints(sample.line, &millimetres), forward);
    assert_eq!(endpoints(sample.arc, &millimetres), forward);
    // The mirrored quarter arc starts at the mirrored second point record.
    assert_eq!(
        endpoints(sample.arc, &mirrored),
        Some(Some([sample.points[2], sample.points[1]]))
    );
    assert_eq!(endpoints(sample.circle, &millimetres), Some(None));
}

/// Selector bit 3 (`12`, `8`) marks a construction curve.
#[test]
fn selector_bit_three_marks_construction() {
    for (selector, construction) in [(4, false), (12, true), (8, true)] {
        let mut sample = sample();
        for offset in [sample.line, sample.arc] {
            sample.payload[offset + SELECTOR] = selector;
            let entity = read(&sample.payload, offset, &millimetres)
                .and_then(Result::ok)
                .expect("curve record reads");
            assert_eq!(entity.construction, Some(construction));
        }
    }
}

/// A curve record of state `0` (+29) stands outside the sketch's profile, and
/// one of state `1` or `2` belongs to it.
#[test]
fn a_curve_record_of_state_zero_stands_outside_the_profile() {
    for (state, outside_profile) in [(1u16, false), (2, false), (0, true)] {
        let mut sample = sample();
        for offset in [sample.line, sample.arc, sample.circle] {
            sample.payload[offset + STATE..offset + STATE + 2]
                .copy_from_slice(&state.to_le_bytes());
            let entity = read(&sample.payload, offset, &millimetres)
                .and_then(Result::ok)
                .expect("curve record reads");
            assert_eq!(entity.outside_profile, outside_profile);
        }
    }
}

/// A curve record that indexes a point record outside the marker's feature
/// object gives no entity; the marker keeps its other readings.
#[test]
fn a_curve_indexing_a_point_record_of_another_object_gives_no_entity() {
    let sample = sample();
    let lists = SketchRecordLists::new(&sample.payload);
    let outside = sample.points[2];
    let owns = |at: usize| at != outside;
    for offset in [sample.line, sample.arc] {
        assert_eq!(
            listed_entity(&sample.payload, &lists, offset, &owns, &millimetres),
            None
        );
    }
    assert!(
        listed_entity(&sample.payload, &lists, sample.circle, &owns, &millimetres)
            .is_some_and(|read| read.is_ok())
    );
}

/// A projection that stretches the sketch's v axis by `stretch` moves the
/// record's end point record off the circle through its start, so the arc
/// reads only when the stretch moves that end by at most the linear
/// resolution.
#[test]
fn a_sensed_record_whose_arc_ends_off_its_point_records_conflicts() {
    let sample = sample();
    let reading = |stretch: f64| {
        let project = |[u, v]: [f64; 2]| Some(Point2::new(u * 1000.0, v * 1000.0 * stretch));
        read(&sample.payload, sample.arc, &project)
    };
    let within = 1.0 + 0.5 * LINEAR_RESOLUTION_MM / 10.0;
    assert!(matches!(reading(within), Some(Ok(_))));
    for beyond in [1.0 + 2.0 * LINEAR_RESOLUTION_MM / 10.0, 2.0] {
        assert_eq!(reading(beyond), Some(Err(ChordConflict::MissesChord)));
    }
    assert!(ChordConflict::MissesChord
        .to_string()
        .contains("ends off its endpoint point records"));
}

/// The arc that [`listed_entity`] refuses for ending off its point records
/// reads through [`off_chord_arc`]: about its centre point record from its
/// start, to the direction of its end, without endpoints. Another record
/// reads as [`listed_entity`] reads it.
#[test]
fn an_arc_that_ends_off_its_point_records_reads_off_the_chord() {
    let sample = sample();
    let lists = SketchRecordLists::new(&sample.payload);
    let stretched = |[u, v]: [f64; 2]| Some(Point2::new(u * 1000.0, v * 2000.0));
    let entity = off_chord_arc(&sample.payload, &lists, sample.arc, &stretched).unwrap();
    assert_eq!(entity.endpoints, None);
    let (center, radius, start, end) = arc_parts(Some(entity.geometry));
    assert_eq!(center, Point2::new(0.0, 0.0));
    assert!((radius - 10.0).abs() < EPS_TEST);
    assert!(start.abs() < EPS_TEST);
    assert!((end - FRAC_PI_2).abs() < EPS_TEST);
    assert_eq!(
        off_chord_arc(&sample.payload, &lists, sample.line, &millimetres),
        read(&sample.payload, sample.line, &millimetres).and_then(Result::ok)
    );
}

#[test]
fn unequal_radii_and_an_index_past_the_points_conflict() {
    let mut payload = vec![0; 7];
    header(&mut payload, 3);
    point(&mut payload, 0.0, 0.0);
    point(&mut payload, 0.01, 0.0);
    point(&mut payload, 0.0, 0.02);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let unequal = curve(&mut payload, 1, 2, -1, Some(0));
    let past = curve(&mut payload, 1, 5, 1, Some(0));
    assert_eq!(
        read(&payload, unequal, &millimetres),
        Some(Err(ChordConflict::NoArc))
    );
    assert_eq!(
        read(&payload, past, &millimetres),
        Some(Err(ChordConflict::PointUnread))
    );
}

#[test]
fn an_arc_sense_with_selector_bit_zero_set_conflicts() {
    let mut sample = sample();
    sample.payload[sample.arc + SELECTOR] = 5;
    let conflict = ChordConflict::OpenSelector;
    assert_eq!(
        read(&sample.payload, sample.arc, &millimetres),
        Some(Err(conflict))
    );
    assert!(conflict.to_string().contains("EV-27"));
}

#[test]
fn an_arc_sense_without_the_minus_two_cells_is_an_ellipse_that_conflicts() {
    let (mut payload, arc) = quarter_points_arc(2, 1, -1);
    let cells = arc + CURRENT_FIELDS + CENTER + 2;
    payload[cells..cells + 4].copy_from_slice(&3u32.to_le_bytes());
    let conflict = ChordConflict::Ellipse;
    assert_eq!(read(&payload, arc, &millimetres), Some(Err(conflict)));
    assert!(conflict.to_string().contains("ellipse"));
}

#[test]
fn a_sense_word_that_is_neither_a_line_nor_an_arc_conflicts() {
    let mut sample = sample();
    let sense = sample.arc + CURRENT_FIELDS + SENSE;
    sample.payload[sense..sense + 4].copy_from_slice(&2i32.to_le_bytes());
    let conflict = ChordConflict::OtherSense(2);
    assert_eq!(
        read(&sample.payload, sample.arc, &millimetres),
        Some(Err(conflict))
    );
    assert!(conflict.to_string().contains("sense word 2"));
}

/// A line that a reading other than [`listed_entity`] gives conflicts with
/// every arc record, the full circle included, and with no line record.
#[test]
fn an_arc_record_conflicts_with_a_line_no_chord_test_compared() {
    let sample = sample();
    let lists = SketchRecordLists::new(&sample.payload);
    let conflict = |offset| unchecked_record_conflict(&sample.payload, &lists, offset);
    assert_eq!(conflict(sample.arc), Some(ChordConflict::Unchecked));
    assert_eq!(conflict(sample.circle), Some(ChordConflict::Unchecked));
    assert_eq!(conflict(sample.line), None);
    assert_eq!(conflict(sample.points[0]), None);
    assert!(ChordConflict::Unchecked
        .to_string()
        .contains("does not compare the record"));
}

#[test]
fn geometry_locus_arcs_and_circles_keep_point_record_readings_with_a_loss() {
    let mut sample = sample();
    for (offset, end) in [(sample.arc, 2), (sample.circle, 1)] {
        sample.payload[offset + head::LOCUS..offset + head::LOCUS + 4]
            .copy_from_slice(&OBJECT_INDEX_LOCUS);
        assert_eq!(
            curve_form(&sample.payload, offset),
            Some(CurveForm::Arc {
                start: 1,
                end,
                center: 0,
                counterclockwise: true,
            })
        );
        let loss = point_record_index_basis_loss(&sample.payload, offset).unwrap();
        assert_eq!(
            loss.code,
            SldprtLossCode::SketchPointRecordIndexBasisAssumed.kind()
        );
        assert!(loss.message.contains("zero-based point-record positions"));
        assert_eq!(
            read(&sample.payload, offset, &millimetres)
                .and_then(Result::ok)
                .and_then(|entity| entity.index_basis_loss),
            Some(loss)
        );
    }
    let (center, radius, start, end) = arc_parts(geometry(&sample, sample.arc));
    assert_eq!(center, Point2::new(0.0, 0.0));
    assert!((radius - 10.0).abs() < EPS_TEST);
    assert!(start.abs() < EPS_TEST);
    assert!((end - FRAC_PI_2).abs() < EPS_TEST);
    assert_eq!(
        geometry(&sample, sample.circle),
        expected(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(10.0).unwrap()
        })
    );
    assert!(point_record_index_basis_loss(&sample.payload, sample.line).is_none());
}

#[test]
fn current_prefix_92_byte_selector_four_geometry_locus_line_uses_object_indices() {
    let mut sample = sample();
    sample.payload.drain(sample.line + 92..sample.line + 94);
    sample.payload[sample.line + head::LOCUS..sample.line + head::LOCUS + 4]
        .copy_from_slice(&OBJECT_INDEX_LOCUS);
    assert!(
        super::super::endpoints::wide_indexed_curve_endpoint_indices(&sample.payload, sample.line)
            .is_some()
    );
    assert!(curve_stays_unresolved(&sample.payload, sample.line));
    assert!(geometry(&sample, sample.line).is_none());
    assert!(point_record_index_basis_loss(&sample.payload, sample.line).is_none());
    sample.payload.truncate(sample.line + 92);
    assert_eq!(
        curve_form(&sample.payload, sample.line),
        Some(CurveForm::Line { start: 1, end: 2 })
    );
    assert_eq!(
        point_record_index_basis_loss(&sample.payload, sample.line)
            .unwrap()
            .code,
        SldprtLossCode::SketchPointRecordIndexBasisAssumed.kind()
    );
}

#[test]
fn geometry_locus_lines_rejected_by_object_index_gate_keep_point_record_readings() {
    for (field, value) in [
        (48, 2.0f64.to_le_bytes().to_vec()),
        (27, 2u16.to_le_bytes().to_vec()),
        (68, 2u32.to_le_bytes().to_vec()),
    ] {
        let mut sample = sample();
        sample.payload.drain(sample.line + 92..sample.line + 94);
        sample.payload[sample.line + head::LOCUS..sample.line + head::LOCUS + 4]
            .copy_from_slice(&OBJECT_INDEX_LOCUS);
        sample.payload[sample.line + field..sample.line + field + value.len()]
            .copy_from_slice(&value);
        assert!(
            super::super::endpoints::wide_indexed_curve_endpoint_indices(
                &sample.payload,
                sample.line
            )
            .is_none()
        );
        assert_eq!(
            curve_form(&sample.payload, sample.line),
            Some(CurveForm::Line { start: 1, end: 2 })
        );
        assert_eq!(
            geometry(&sample, sample.line),
            expected(SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 0.0),
                end: Point2::new(0.0, 10.0),
            })
        );
        assert_eq!(
            point_record_index_basis_loss(&sample.payload, sample.line)
                .unwrap()
                .code,
            SldprtLossCode::SketchPointRecordIndexBasisAssumed.kind()
        );
    }
}

#[test]
fn geometry_locus_lines_of_other_sizes_selectors_and_prefixes_keep_their_reading() {
    for (short, selector, slot) in [(false, 4u16, 3), (true, 12, 3), (true, 4, 1)] {
        let mut sample = sample();
        if short {
            sample.payload.drain(sample.line + 92..sample.line + 94);
        }
        sample.payload[sample.line + head::LOCUS..sample.line + head::LOCUS + 4]
            .copy_from_slice(&OBJECT_INDEX_LOCUS);
        sample.payload[sample.line + SELECTOR..sample.line + SELECTOR + 2]
            .copy_from_slice(&selector.to_le_bytes());
        sample.payload[sample.line + 4] = slot;
        assert_eq!(
            curve_form(&sample.payload, sample.line),
            Some(CurveForm::Line { start: 1, end: 2 })
        );
        assert_eq!(
            geometry(&sample, sample.line),
            expected(SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 0.0),
                end: Point2::new(0.0, 10.0),
            })
        );
        assert!(point_record_index_basis_loss(&sample.payload, sample.line).is_some());
    }
}

#[test]
fn listed_lines_and_arcs_are_listed_curves_and_points_are_not() {
    let sample = sample();
    let lists = SketchRecordLists::new(&sample.payload);
    for offset in [sample.line, sample.arc, sample.circle] {
        assert!(lists.listed_curve(&sample.payload, offset).is_some());
    }
    for offset in sample.points {
        assert!(lists.listed_curve(&sample.payload, offset).is_none());
    }
}

#[test]
fn an_unread_curve_form_or_an_unlisted_record_is_not_a_listed_curve() {
    let mut sample = sample();
    sample.payload[sample.line + SELECTOR] = 5;
    let lists = SketchRecordLists::new(&sample.payload);
    assert!(lists.listed_curve(&sample.payload, sample.line).is_none());
    assert!(lists.listed_curve(&sample.payload, sample.arc).is_some());

    let mut payload = vec![0; 7];
    let unlisted = curve(&mut payload, 0, 1, 0, None);
    let lists = SketchRecordLists::new(&payload);
    assert!(lists.listed_curve(&payload, unlisted).is_none());
}

#[test]
fn selector_bit_three_marks_a_construction_record() {
    let mut sample = sample();
    sample.payload[sample.line + SELECTOR] = 12;
    assert!(construction_record(&sample.payload, sample.line));
    assert!(!construction_record(&sample.payload, sample.arc));
    let lists = SketchRecordLists::new(&sample.payload);
    assert!(lists.listed_curve(&sample.payload, sample.line).is_some());
}

/// A record projected from outside the sketch: an object pointer at +35.
fn projected(out: &mut Vec<u8>) -> usize {
    let at = out.len();
    head(out);
    out[at + 35..at + 37].copy_from_slice(&0x8001u16.to_le_bytes());
    out.extend_from_slice(&[0; 40]);
    at
}

#[test]
fn a_projected_record_leading_the_curve_run_keeps_the_list() {
    let mut payload = vec![0; 7];
    header(&mut payload, 2);
    let first = point(&mut payload, 0.0, 0.0);
    point(&mut payload, 0.01, 0.0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let outside = projected(&mut payload);
    let line = curve(&mut payload, 0, 1, 0, None);
    let trailing = projected(&mut payload);
    let lists = SketchRecordLists::new(&payload);
    assert_eq!(lists.record(first), Some(ListedRecord::Point));
    assert!(lists.listed_curve(&payload, line).is_some());
    assert!(lists.listed_curve(&payload, outside).is_none());
    assert_eq!(lists.record(trailing), None);
    assert_eq!(
        listed_geometry(&payload, line, &millimetres),
        expected(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
    );
}

#[test]
fn projected_records_alone_frame_no_list() {
    let mut payload = vec![0; 7];
    header(&mut payload, 1);
    let first = point(&mut payload, 0.0, 0.0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    projected(&mut payload);
    projected(&mut payload);
    let lists = SketchRecordLists::new(&payload);
    assert_eq!(lists.record(first), None);
}
