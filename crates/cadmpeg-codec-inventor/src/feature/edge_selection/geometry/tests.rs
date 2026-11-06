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
    revision(&records[0], &by_index)
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
