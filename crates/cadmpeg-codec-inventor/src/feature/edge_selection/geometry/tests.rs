// SPDX-License-Identifier: Apache-2.0
use super::*;

fn record(index: usize, name: &str, tokens: Vec<Token>) -> Record {
    Record {
        index,
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn fixture() -> Vec<Record> {
    vec![
        record(
            1,
            "edge",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(2),
                Token::Double(1.0),
                Token::Ref(3),
                Token::Double(5.0),
                Token::Ref(-1),
                Token::Ref(6),
                Token::False,
            ],
        ),
        record(
            2,
            "vertex",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(1),
                Token::Long(0),
                Token::Ref(4),
            ],
        ),
        record(
            3,
            "vertex",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(1),
                Token::Long(1),
                Token::Ref(5),
            ],
        ),
        record(
            4,
            "point",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position([3.0, 0.0, 0.0]),
            ],
        ),
        record(
            5,
            "point",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position([11.0, 0.0, 0.0]),
            ],
        ),
        record(
            6,
            "straight",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position([1.0, 0.0, 0.0]),
                Token::Vector3([2.0, 0.0, 0.0]),
            ],
        ),
    ]
}

fn sample(records: &[Record]) -> Option<EdgeGeometry> {
    let by_index = records
        .iter()
        .map(|r| (i64::try_from(r.index).expect("index"), r))
        .collect();
    line(&records[0], &by_index)
}

#[test]
fn revision_geometry_uses_vertex_positions_and_scaled_carrier_parameters() {
    let value = sample(&fixture()).expect("line");
    assert_eq!(value.start.as_raw(), &Point3::new(30.0, 0.0, 0.0));
    assert_eq!(value.end.as_raw(), &Point3::new(110.0, 0.0, 0.0));
    assert_eq!(value.interior.as_raw(), &Point3::new(70.0, 0.0, 0.0));
    assert_eq!(value.tangent.as_raw(), &Vector3::new(1.0, 0.0, 0.0));
}

#[test]
fn missing_or_changed_endpoint_revisions_do_not_supply_history_geometry() {
    let mut records = fixture();
    records.pop();
    assert!(sample(&records).is_none());
    let mut records = fixture();
    let tokens = std::sync::Arc::make_mut(&mut records[4].tokens);
    tokens[3] = Token::Position([12.0, 0.0, 0.0]);
    assert!(sample(&records).is_none());
}

#[test]
fn reversed_carrier_uses_its_stored_parameterization() {
    let mut records = fixture();
    let tokens = std::sync::Arc::make_mut(&mut records[0].tokens);
    tokens[4] = Token::Double(-1.0);
    tokens[6] = Token::Double(-5.0);
    tokens[9] = Token::True;
    let value = sample(&records).expect("reversed");
    assert_eq!(value.interior.as_raw(), &Point3::new(70.0, 0.0, 0.0));
    assert_eq!(value.tangent.as_raw(), &Vector3::new(-1.0, 0.0, 0.0));
}

#[test]
fn matched_edge_revision_geometry_is_independent_of_the_active_edge() {
    fn attribute(index: usize, owner: i64, family: &str, values: &[i64]) -> Record {
        let mut tokens = vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(owner),
            Token::Str(family.into()),
            Token::Long(1),
            Token::Long(44),
        ];
        tokens.extend(values.iter().map(|value| Token::Long(*value)));
        record(index, "ATTRIB_CUSTOM-attrib", tokens)
    }
    let mut records = fixture();
    records.push(attribute(
        10,
        1,
        "INV_NMX_MATCHED_ATTRIB",
        &[1, 55, -1, 7, 0],
    ));
    records.push(attribute(11, 1, "INV_NMX_GEN_TAG", &[1, 44, -1, 3, 0, 10]));
    let name = EdgeName {
        generator: 44,
        family: "INV_NMX_GEN_TAG".into(),
        associative_id: 3,
        role: 10,
    };
    let selectors = [Selector {
        source: 55,
        generator: 44,
        key: 7,
    }];
    let names = HashMap::from([(55, Some(name))]);
    assert_eq!(
        selected(&records, &selectors, &names, 44)[&55].end.as_raw(),
        &Point3::new(110.0, 0.0, 0.0)
    );
    records.push(record(20, "edge", vec![Token::Ref(-1)]));
    assert_eq!(
        selected(&records, &selectors, &names, 44)[&55].end.as_raw(),
        &Point3::new(110.0, 0.0, 0.0)
    );
    records.push(attribute(
        21,
        20,
        "INV_NMX_MATCHED_ATTRIB",
        &[1, 55, -1, 7, 0],
    ));
    records.push(attribute(22, 20, "INV_NMX_GEN_TAG", &[1, 44, -1, 3, 0, 10]));
    assert!(selected(&records, &selectors, &names, 44).is_empty());
}

const FRAC_1_SQRT_2: f64 = std::f64::consts::FRAC_1_SQRT_2;

struct EllipseSpec {
    center: [f64; 3],
    normal: [f64; 3],
    major: [f64; 3],
    ratio: f64,
    range: [f64; 2],
    reversed: bool,
    start: [f64; 3],
    end: [f64; 3],
}

/// Circle of major radius 2 about (1, 2, 3) with normal +z and reference direction +x.
fn circle(range: [f64; 2], reversed: bool, start: [f64; 3], end: [f64; 3]) -> EllipseSpec {
    EllipseSpec {
        center: [1.0, 2.0, 3.0],
        normal: [0.0, 0.0, 1.0],
        major: [2.0, 0.0, 0.0],
        ratio: 1.0,
        range,
        reversed,
        start,
        end,
    }
}

fn ellipse_records(spec: &EllipseSpec) -> Vec<Record> {
    let vertex = |index, point| {
        record(
            index,
            "vertex",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(1),
                Token::Long(0),
                Token::Ref(point),
            ],
        )
    };
    let point = |index, at: [f64; 3]| {
        record(
            index,
            "point",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position(at),
            ],
        )
    };
    vec![
        record(
            1,
            "edge",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(2),
                Token::Double(spec.range[0]),
                Token::Ref(3),
                Token::Double(spec.range[1]),
                Token::Ref(-1),
                Token::Ref(6),
                if spec.reversed {
                    Token::True
                } else {
                    Token::False
                },
            ],
        ),
        vertex(2, 4),
        vertex(3, 5),
        point(4, spec.start),
        point(5, spec.end),
        record(
            6,
            "ellipse",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position(spec.center),
                Token::Vector3(spec.normal),
                Token::Vector3(spec.major),
                Token::Double(spec.ratio),
                Token::False,
                Token::False,
            ],
        ),
    ]
}

fn same_point(actual: &Point3, x: f64, y: f64, z: f64) -> bool {
    [(actual.x, x), (actual.y, y), (actual.z, z)]
        .iter()
        .all(|(a, b)| (a - b).abs() < 1.0e-9)
}

/// Whether `actual` is the unit vector along (x, y, z).
fn same_direction(actual: &Vector3, x: f64, y: f64, z: f64) -> bool {
    let length = (x * x + y * y + z * z).sqrt();
    [(actual.x, x), (actual.y, y), (actual.z, z)]
        .iter()
        .all(|(a, b)| (a - b / length).abs() < 1.0e-12)
}

fn revision_of(records: &[Record]) -> Option<EdgeGeometry> {
    let by_index = records
        .iter()
        .map(|r| (i64::try_from(r.index).expect("index"), r))
        .collect();
    revision(
        &crate::native::test_ctx(),
        &records[0],
        &by_index,
        &SubtypeTable::from_records(records),
    )
    .expect("no resource refusal")
}

#[test]
fn closed_circle_revision_uses_the_carrier_point_at_the_parameter_midpoint() {
    let pi = std::f64::consts::PI;
    let value = revision_of(&ellipse_records(&circle(
        [-pi, pi],
        false,
        [-1.0, 2.0, 3.0],
        [-1.0, 2.0, 3.0],
    )))
    .expect("circle");
    assert_eq!(value.start.as_raw(), &Point3::new(-10.0, 20.0, 30.0));
    assert_eq!(value.end.as_raw(), &Point3::new(-10.0, 20.0, 30.0));
    assert!(same_point(value.interior.as_raw(), 30.0, 20.0, 30.0));
    assert!(same_direction(value.tangent.as_raw(), 0.0, 1.0, 0.0));
}

#[test]
fn circle_arc_revision_interior_and_tangent_follow_the_parameter_direction() {
    let half = std::f64::consts::FRAC_PI_2;
    let value = revision_of(&ellipse_records(&circle(
        [0.0, half],
        false,
        [3.0, 2.0, 3.0],
        [1.0, 4.0, 3.0],
    )))
    .expect("arc");
    let interior = Point3::new(
        10.0 * (1.0 + 2.0 * FRAC_1_SQRT_2),
        10.0 * (2.0 + 2.0 * FRAC_1_SQRT_2),
        30.0,
    );
    assert!(same_point(
        value.interior.as_raw(),
        interior.x,
        interior.y,
        interior.z
    ));
    assert!(same_direction(
        value.tangent.as_raw(),
        -FRAC_1_SQRT_2,
        FRAC_1_SQRT_2,
        0.0
    ));
}

#[test]
fn reversed_circle_arc_negates_the_stored_parameters_and_the_tangent() {
    let half = std::f64::consts::FRAC_PI_2;
    let value = revision_of(&ellipse_records(&circle(
        [-half, 0.0],
        true,
        [1.0, 4.0, 3.0],
        [3.0, 2.0, 3.0],
    )))
    .expect("reversed arc");
    assert_eq!(value.start.as_raw(), &Point3::new(10.0, 40.0, 30.0));
    assert_eq!(value.end.as_raw(), &Point3::new(30.0, 20.0, 30.0));
    assert!(same_direction(
        value.tangent.as_raw(),
        FRAC_1_SQRT_2,
        -FRAC_1_SQRT_2,
        0.0
    ));
}

#[test]
fn negative_ratio_flips_the_minor_direction() {
    let half = std::f64::consts::FRAC_PI_2;
    let mut spec = circle([0.0, half], false, [3.0, 2.0, 3.0], [1.0, 0.0, 3.0]);
    spec.ratio = -1.0;
    let value = revision_of(&ellipse_records(&spec)).expect("flipped arc");
    assert_eq!(value.end.as_raw(), &Point3::new(10.0, 0.0, 30.0));
    let mut spec = circle([0.0, half], false, [3.0, 2.0, 3.0], [1.0, 4.0, 3.0]);
    spec.ratio = -1.0;
    assert!(revision_of(&ellipse_records(&spec)).is_none());
}

#[test]
fn elliptical_revision_scales_the_minor_axis_by_the_ratio() {
    let half = std::f64::consts::FRAC_PI_2;
    let mut spec = circle([0.0, half], false, [3.0, 2.0, 3.0], [1.0, 3.0, 3.0]);
    spec.ratio = 0.5;
    let value = revision_of(&ellipse_records(&spec)).expect("ellipse arc");
    assert_eq!(value.end.as_raw(), &Point3::new(10.0, 30.0, 30.0));
    // The derivative is (-sqrt(2), sqrt(2) / 2, 0) before normalization.
    assert!(same_direction(value.tangent.as_raw(), -2.0, 1.0, 0.0));
}

#[test]
fn ellipse_revisions_that_the_vertices_do_not_prove_stay_unresolved() {
    let pi = std::f64::consts::PI;
    let half = std::f64::consts::FRAC_PI_2;
    let arc = || circle([0.0, half], false, [3.0, 2.0, 3.0], [1.0, 4.0, 3.0]);
    assert!(revision_of(&ellipse_records(&arc())).is_some());
    // An endpoint off the carrier.
    let mut spec = arc();
    spec.end = [1.0, 4.5, 3.0];
    assert!(revision_of(&ellipse_records(&spec)).is_none());
    // A closed edge on a reversed carrier has no vertex proof of its sense.
    let closed = circle([-pi, pi], true, [-1.0, 2.0, 3.0], [-1.0, 2.0, 3.0]);
    assert!(revision_of(&ellipse_records(&closed)).is_none());
    // An empty or backward parameter interval.
    let mut spec = arc();
    spec.range = [half, half];
    assert!(revision_of(&ellipse_records(&spec)).is_none());
    spec.range = [half, 0.0];
    spec.start = [1.0, 4.0, 3.0];
    spec.end = [3.0, 2.0, 3.0];
    assert!(revision_of(&ellipse_records(&spec)).is_none());
    // A span beyond one turn.
    let mut spec = arc();
    spec.range = [0.0, 3.0 * pi];
    spec.end = [-1.0, 2.0, 3.0];
    assert!(revision_of(&ellipse_records(&spec)).is_none());
    // A frame that is not a unit normal orthogonal to the major axis.
    let mut spec = arc();
    spec.normal = [0.0, 0.0, 2.0];
    assert!(revision_of(&ellipse_records(&spec)).is_none());
    let mut spec = arc();
    spec.major = [2.0, 0.0, 1.0];
    spec.start = [3.0, 2.0, 4.0];
    assert!(revision_of(&ellipse_records(&spec)).is_none());
    // A zero ratio or major axis.
    let mut spec = arc();
    spec.ratio = 0.0;
    assert!(revision_of(&ellipse_records(&spec)).is_none());
    let mut spec = arc();
    spec.major = [0.0, 0.0, 0.0];
    assert!(revision_of(&ellipse_records(&spec)).is_none());
    // A carrier with subset flags or another record family.
    let mut records = ellipse_records(&arc());
    std::sync::Arc::make_mut(&mut records[5].tokens)[7] = Token::True;
    assert!(revision_of(&records).is_none());
    let mut records = ellipse_records(&arc());
    records[5].name = "intcurve".into();
    assert!(revision_of(&records).is_none());
}

/// Quarter-circle arc about (1, 2, 3) from (3, 2, 3) to (1, 4, 3) with the
/// carrier's two optional range endpoints replaced by `tail`.
fn bounded_arc(reversed: bool, tail: &[Token]) -> Vec<Record> {
    let half = std::f64::consts::FRAC_PI_2;
    let mut records = if reversed {
        // The sense negates the parameters: u = -t runs from 0 to pi / 2.
        ellipse_records(&circle(
            [-half, 0.0],
            true,
            [1.0, 4.0, 3.0],
            [3.0, 2.0, 3.0],
        ))
    } else {
        ellipse_records(&circle(
            [0.0, half],
            false,
            [3.0, 2.0, 3.0],
            [1.0, 4.0, 3.0],
        ))
    };
    let mut tokens = records[5].tokens[..7].to_vec();
    tokens.extend_from_slice(tail);
    records[5].tokens = tokens.into();
    records
}

#[test]
fn ellipse_range_endpoints_bound_the_carrier_parameter_of_the_edge() {
    let half = std::f64::consts::FRAC_PI_2;
    let range = |low: f64, high: f64| {
        [
            Token::True,
            Token::Double(low),
            Token::True,
            Token::Double(high),
        ]
    };
    let none = || bounded_arc(false, &[]);
    let expected = revision_of(&none()).expect("unbounded arc");
    // Bounds that contain, or equal, the edge interval leave the geometry unchanged.
    for (low, high) in [(0.0, half), (-1.0, 4.0)] {
        assert_eq!(
            revision_of(&bounded_arc(false, &range(low, high))),
            Some(expected.clone())
        );
    }
    // One absent endpoint leaves that side unbounded.
    let low_only = [Token::True, Token::Double(0.0), Token::False];
    assert_eq!(
        revision_of(&bounded_arc(false, &low_only)),
        Some(expected.clone())
    );
    let high_only = [Token::False, Token::True, Token::Double(half)];
    assert_eq!(
        revision_of(&bounded_arc(false, &high_only)),
        Some(expected.clone())
    );
    // A reversed edge is tested on u = -t.
    let reversed = revision_of(&bounded_arc(true, &range(0.0, half))).expect("reversed arc");
    assert_eq!(reversed.start, expected.end);
    assert_eq!(reversed.end, expected.start);
    // An edge interval outside the bounds does not resolve.
    assert!(revision_of(&bounded_arc(false, &range(0.1, half))).is_none());
    assert!(revision_of(&bounded_arc(false, &range(0.0, half - 0.1))).is_none());
    assert!(revision_of(&bounded_arc(true, &range(-half, 0.0))).is_none());
    assert!(revision_of(&bounded_arc(false, &high_only[..1])).is_none());
    // Unordered bounds, a truncated endpoint, a non-numeric endpoint, and a
    // trailing value do not resolve.
    assert!(revision_of(&bounded_arc(false, &range(half, 0.0))).is_none());
    assert!(revision_of(&bounded_arc(false, &[Token::True])).is_none());
    assert!(revision_of(&bounded_arc(
        false,
        &[Token::True, Token::False, Token::False]
    ))
    .is_none());
    assert!(revision_of(&bounded_arc(
        false,
        &[Token::False, Token::False, Token::False]
    ))
    .is_none());
    assert!(revision_of(&bounded_arc(false, &[Token::Long(0), Token::Long(0)])).is_none());
}

/// The arc of `bounded_arc` with its start vertex moved by `offset_cm` along x
/// and tolerant with these three tolerance slots in centimetres.
fn tolerant_start(offset_cm: f64, slots: Option<[f64; 3]>) -> Vec<Record> {
    let mut records = bounded_arc(false, &[Token::False, Token::False]);
    let tokens = std::sync::Arc::make_mut(&mut records[3].tokens);
    tokens[3] = Token::Position([3.0 + offset_cm, 2.0, 3.0]);
    if let Some(slots) = slots {
        records[1].name = "tvertex".into();
        let mut tokens = records[1].tokens.to_vec();
        tokens.extend(slots.map(Token::Double));
        records[1].tokens = tokens.into();
    }
    records
}

#[test]
fn tolerant_vertex_tolerance_widens_the_endpoint_proof() {
    let off = 2.0e-5; // 2e-4 mm
    let value = revision_of(&tolerant_start(off, Some([-1.0, 3.0e-5, 3.0e-5]))).expect("tolerant");
    // The stored vertex stays the edge start.
    assert!(same_point(
        value.start.as_raw(),
        30.0 + 10.0 * off,
        20.0,
        30.0
    ));
    // The last slot is the tolerance; an earlier slot does not widen it.
    assert!(revision_of(&tolerant_start(off, Some([-1.0, 3.0e-5, 1.0e-5]))).is_none());
    assert!(revision_of(&tolerant_start(off, Some([3.0e-5, 3.0e-5, -1.0]))).is_none());
    // A plain vertex keeps the kernel linear tolerance.
    assert!(revision_of(&tolerant_start(off, None)).is_none());
    assert!(revision_of(&tolerant_start(0.0, None)).is_some());
    // The tolerance does not admit a vertex that is farther away.
    assert!(revision_of(&tolerant_start(1.0e-3, Some([-1.0, 3.0e-5, 3.0e-5]))).is_none());
}

/// A stored `intcurve` carrier: the cache poles in centimetres with optional
/// weights, the unique knots with their stored multiplicities, and the record
/// sense.
struct SplineSpec {
    degree: i64,
    knots: Vec<(f64, i64)>,
    poles: Vec<[f64; 3]>,
    weights: Option<Vec<f64>>,
    carrier_reversed: bool,
    construction: &'static str,
    fit: f64,
    closure: i64,
    edge_reversed: bool,
    range: [f64; 2],
    start: [f64; 3],
    end: [f64; 3],
}

fn spline_records(spec: &SplineSpec) -> Vec<Record> {
    let mut records = ellipse_records(&circle(
        spec.range,
        spec.edge_reversed,
        spec.start,
        spec.end,
    ));
    let mut tokens = vec![
        Token::Ref(-1),
        Token::Long(-1),
        Token::Ref(-1),
        if spec.carrier_reversed {
            Token::True
        } else {
            Token::False
        },
        Token::SubtypeOpen,
        Token::Ident(spec.construction.into()),
        Token::Long(23_100),
        Token::Enum(0),
        Token::Ident(
            if spec.weights.is_some() {
                "nurbs"
            } else {
                "nubs"
            }
            .into(),
        ),
        Token::Long(spec.degree),
        Token::Enum(spec.closure),
        Token::Long(i64::try_from(spec.knots.len()).expect("knot count")),
    ];
    for (knot, multiplicity) in &spec.knots {
        tokens.push(Token::Double(*knot));
        tokens.push(Token::Long(*multiplicity));
    }
    for (index, pole) in spec.poles.iter().enumerate() {
        tokens.extend(pole.iter().map(|value| Token::Double(*value)));
        if let Some(weights) = &spec.weights {
            tokens.push(Token::Double(weights[index]));
        }
    }
    tokens.push(Token::Double(spec.fit));
    tokens.push(Token::SubtypeClose);
    records[5] = record(6, "intcurve", tokens);
    records
}

/// Quadratic polynomial curve over knots [0, 2]: the Bernstein form in
/// `s = t / 2` with these poles.
const QUADRATIC_POLES: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 2.0, 0.5], [3.0, 0.0, 1.0]];

fn quadratic_point(t: f64) -> [f64; 3] {
    let s = t / 2.0;
    let basis = [(1.0 - s) * (1.0 - s), 2.0 * s * (1.0 - s), s * s];
    [0, 1, 2].map(|axis| (0..3).map(|k| basis[k] * QUADRATIC_POLES[k][axis]).sum())
}

fn quadratic_derivative(t: f64) -> [f64; 3] {
    let s = t / 2.0;
    [0, 1, 2].map(|axis| {
        let d0 = QUADRATIC_POLES[1][axis] - QUADRATIC_POLES[0][axis];
        let d1 = QUADRATIC_POLES[2][axis] - QUADRATIC_POLES[1][axis];
        (1.0 - s) * d0 + s * d1
    })
}

fn quadratic(range: [f64; 2]) -> SplineSpec {
    SplineSpec {
        degree: 2,
        knots: vec![(0.0, 2), (2.0, 2)],
        poles: QUADRATIC_POLES.to_vec(),
        weights: None,
        carrier_reversed: false,
        construction: "exact_int_cur",
        fit: 0.0,
        closure: 0,
        edge_reversed: false,
        range,
        start: quadratic_point(range[0]),
        end: quadratic_point(range[1]),
    }
}

fn assert_spline_geometry(
    value: &EdgeGeometry,
    start: [f64; 3],
    end: [f64; 3],
    middle: [f64; 3],
    direction: [f64; 3],
) {
    let millimetres = |p: [f64; 3]| p.map(|v| v * CM_TO_MM);
    let [x, y, z] = millimetres(start);
    assert!(same_point(value.start.as_raw(), x, y, z));
    let [x, y, z] = millimetres(end);
    assert!(same_point(value.end.as_raw(), x, y, z));
    let [x, y, z] = millimetres(middle);
    assert!(same_point(value.interior.as_raw(), x, y, z));
    let [x, y, z] = direction;
    assert!(same_direction(value.tangent.as_raw(), x, y, z));
}

#[test]
fn polynomial_spline_revision_evaluates_the_stored_bspline() {
    let spec = quadratic([0.5, 1.5]);
    let value = revision_of(&spline_records(&spec)).expect("spline revision");
    assert_spline_geometry(
        &value,
        quadratic_point(0.5),
        quadratic_point(1.5),
        quadratic_point(1.0),
        quadratic_derivative(1.0),
    );
}

#[test]
fn spline_sense_negates_the_parameters_once_per_reversal() {
    // Edge sense and carrier sense each negate the parameter. The knot domain
    // is [0, 2], so a reversed edge reads the stored cache at -t and needs a
    // negative stored range to stay inside it.
    for (carrier_reversed, edge_reversed) in [(false, true), (true, false)] {
        let mut spec = quadratic([-1.5, -0.5]);
        spec.carrier_reversed = carrier_reversed;
        spec.edge_reversed = edge_reversed;
        spec.start = quadratic_point(1.5);
        spec.end = quadratic_point(0.5);
        let value = revision_of(&spline_records(&spec)).expect("one reversal");
        let tangent = quadratic_derivative(1.0).map(|v| -v);
        assert_spline_geometry(
            &value,
            quadratic_point(1.5),
            quadratic_point(0.5),
            quadratic_point(1.0),
            tangent,
        );
    }
    // Two reversals cancel.
    let mut spec = quadratic([0.5, 1.5]);
    spec.carrier_reversed = true;
    spec.edge_reversed = true;
    let value = revision_of(&spline_records(&spec)).expect("two reversals");
    assert_spline_geometry(
        &value,
        quadratic_point(0.5),
        quadratic_point(1.5),
        quadratic_point(1.0),
        quadratic_derivative(1.0),
    );
}

#[test]
fn rational_spline_revision_evaluates_the_weighted_bspline() {
    // A rational quadratic quarter circle of radius 3 about (1, 1, 0) over
    // knots [0, 1] with middle weight sqrt(1/2).
    let w = FRAC_1_SQRT_2;
    let poles = [[4.0, 1.0, 0.0], [4.0, 4.0, 0.0], [1.0, 4.0, 0.0]];
    let point = |s: f64| {
        let basis = [(1.0 - s) * (1.0 - s), 2.0 * s * (1.0 - s) * w, s * s];
        let sum = (1.0 - s) * (1.0 - s) + 2.0 * s * (1.0 - s) * w + s * s;
        [0, 1, 2].map(|axis| (0..3).map(|k| basis[k] * poles[k][axis]).sum::<f64>() / sum)
    };
    let spec = SplineSpec {
        degree: 2,
        knots: vec![(0.0, 2), (1.0, 2)],
        poles: poles.to_vec(),
        weights: Some(vec![1.0, w, 1.0]),
        carrier_reversed: false,
        construction: "exact_int_cur",
        fit: 0.0,
        closure: 0,
        edge_reversed: false,
        range: [0.25, 0.75],
        start: point(0.25),
        end: point(0.75),
    };
    let value = revision_of(&spline_records(&spec)).expect("rational revision");
    // At s = 1/2 the arc point is at 45 degrees and the tangent is
    // perpendicular to the radius.
    let middle = [1.0 + 3.0 * FRAC_1_SQRT_2, 1.0 + 3.0 * FRAC_1_SQRT_2, 0.0];
    assert_spline_geometry(
        &value,
        point(0.25),
        point(0.75),
        middle,
        [-FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0],
    );
}

#[test]
fn spline_revision_refuses_unproven_carriers() {
    let refuses = |change: &dyn Fn(&mut SplineSpec)| {
        let mut spec = quadratic([0.5, 1.5]);
        change(&mut spec);
        assert!(revision_of(&spline_records(&spec)).is_none());
    };
    // Control: the unmodified form resolves.
    assert!(revision_of(&spline_records(&quadratic([0.5, 1.5]))).is_some());
    // A vertex off the carrier.
    refuses(&|spec| spec.end = [3.0, 0.0, 1.0]);
    refuses(&|spec| spec.start[0] += 1.0e-3);
    // A parameter outside the knot domain, and an empty or backward range.
    refuses(&|spec| {
        spec.range = [0.5, 2.5];
        spec.end = quadratic_point(2.5);
    });
    refuses(&|spec| {
        spec.range = [-0.5, 1.5];
        spec.start = quadratic_point(-0.5);
    });
    refuses(&|spec| spec.range = [1.5, 0.5]);
    refuses(&|spec| spec.range = [1.0, 1.0]);
    // A reversed edge whose negated range leaves the domain.
    refuses(&|spec| spec.edge_reversed = true);
    // A procedural construction or an approximate cache.
    refuses(&|spec| spec.construction = "off_int_cur");
    refuses(&|spec| spec.fit = 1.0e-4);
    // A periodic spline, and a closed edge.
    refuses(&|spec| spec.closure = 2);
    refuses(&|spec| {
        spec.end = spec.start;
    });
    // A cache whose poles do not match its knots.
    refuses(&|spec| {
        spec.poles.pop();
    });
}
