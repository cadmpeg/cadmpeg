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
        selected(&records, &selectors, &names)[&55].end.as_raw(),
        &Point3::new(110.0, 0.0, 0.0)
    );
    records.push(record(20, "edge", vec![Token::Ref(-1)]));
    assert_eq!(
        selected(&records, &selectors, &names)[&55].end.as_raw(),
        &Point3::new(110.0, 0.0, 0.0)
    );
    records.push(attribute(
        21,
        20,
        "INV_NMX_MATCHED_ATTRIB",
        &[1, 55, -1, 7, 0],
    ));
    records.push(attribute(22, 20, "INV_NMX_GEN_TAG", &[1, 44, -1, 3, 0, 10]));
    assert!(selected(&records, &selectors, &names).is_empty());
}
