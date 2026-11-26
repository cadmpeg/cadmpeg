use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::math::{Point3, Vector3};

use super::{enrich_circular_patterns, walked_circular, WalkedPatterns};
use crate::carchive::reader::{ParameterRecord, V};
use crate::carchive::PatternSeeds;
use crate::records::{Feature, FeatureHistory, FeatureSource};

fn parameter(class: &str, name: &str, value: f64) -> ParameterRecord {
    ParameterRecord {
        class: Arc::from(class),
        name: V::Str(name.to_owned()),
        value,
        at: 0,
        value_at: 0,
        object_id: None,
    }
}

/// A walked circular pattern `CirPattern1` of seed `Cut-Extrude2` about the
/// axis through (0, 0.5, 0) mm along +Y, with a count `D1` of 4 and an angle
/// `D3` of `angle`, and a second direction `D4`, `D5`.
fn pattern(name: &str, angle: f64) -> PatternSeeds {
    PatternSeeds {
        pattern: Some(name.to_owned()),
        seeds: vec![Some("Cut-Extrude2".to_owned())],
        parameters: vec![
            parameter("moIntegerParameter_c", "D1", 4.0),
            parameter("moIntegerParameter_c", "D4", 0.0),
            parameter("moAngleParameter_c", "D5", 0.25),
            parameter("moAngleParameter_c", "D3", angle),
        ],
        axis: Some(([0.001, 0.0, 0.0005, 0.0, 0.0, 1.0, 0.0], 1)),
        other_sources: false,
    }
}

fn walked(patterns: Vec<PatternSeeds>) -> WalkedPatterns {
    WalkedPatterns::from([("lane".to_owned(), patterns)])
}

fn listed(name: &str) -> bool {
    matches!(name, "D1" | "D3")
}

#[test]
fn the_listed_parameters_give_the_count_and_angle() {
    let found = walked_circular(
        &walked(vec![pattern("CirPattern1", TAU)]),
        "lane",
        "CirPattern1",
        listed,
    )
    .expect("one walked pattern of the name");
    assert_eq!(found.seeds, Some(vec!["Cut-Extrude2".to_owned()]));
    assert_eq!(found.count, Some(("D1".to_owned(), 4)));
    assert_eq!(found.angle, Some(("D3".to_owned(), TAU)));
    let (origin, direction) = found.axis.expect("an axis");
    assert_eq!(origin.get(), Point3::new(0.0, 0.5, 0.0));
    assert_eq!(*direction.as_raw(), Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn a_second_listed_parameter_of_a_class_states_no_value() {
    let every = |name: &str| name.starts_with('D');
    let found = walked_circular(
        &walked(vec![pattern("CirPattern1", TAU)]),
        "lane",
        "CirPattern1",
        every,
    )
    .unwrap();
    assert_eq!((found.count, found.angle), (None, None));
}

#[test]
fn an_unnamed_seed_or_a_repeated_pattern_name_states_no_seeds() {
    let mut unnamed = pattern("CirPattern1", TAU);
    unnamed.seeds.push(None);
    let found = walked_circular(&walked(vec![unnamed]), "lane", "CirPattern1", listed).unwrap();
    assert_eq!(found.seeds, None);
    let twice = walked(vec![
        pattern("CirPattern1", TAU),
        pattern("CirPattern1", TAU),
    ]);
    assert_eq!(walked_circular(&twice, "lane", "CirPattern1", listed), None);
}

#[test]
fn a_direction_of_other_than_unit_length_states_no_axis() {
    for direction in [[0.0; 3], [0.0, 2.0, 0.0]] {
        let mut stretched = pattern("CirPattern1", TAU);
        stretched.axis = Some((
            [0.0, 0.0, 0.0, 0.0, direction[0], direction[1], direction[2]],
            0,
        ));
        let found =
            walked_circular(&walked(vec![stretched]), "lane", "CirPattern1", listed).unwrap();
        assert_eq!(found.axis, None);
    }
}

fn feature(name: &str, source: &str, class: &str, parameters: &[&str]) -> Feature {
    Feature {
        id: name.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(FeatureSource::try_from(source).expect("test feature source id")),
        ordinal: 0,
        name: name.into(),
        kind: String::new(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: parameters
            .iter()
            .map(|key| (NonBlankString::new(*key).unwrap(), "1".to_owned()))
            .collect(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    }
}

/// A history of the seed `Cut-Extrude2` (source 41) and the circular pattern
/// `CirPattern1` that lists `D1` and `D3`.
fn history() -> [FeatureHistory; 1] {
    [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("Cut-Extrude2", "41", "moExtrusion_c", &[]),
            feature("CirPattern1", "86", "moCirPattern_c", &["D1", "D3"]),
        ],
    }]
}

fn property<'a>(history: &'a [FeatureHistory; 1], key: &str) -> Option<&'a str> {
    history[0].features[1]
        .properties
        .get(key)
        .map(String::as_str)
}

#[test]
fn a_full_turn_walked_pattern_fills_the_record() {
    let mut histories = history();
    enrich_circular_patterns(&mut histories, &walked(vec![pattern("CirPattern1", TAU)]));
    assert_eq!(property(&histories, "AxisOrigin"), Some("0mm,0.5mm,0mm"));
    assert_eq!(property(&histories, "AxisDirection"), Some("0,1,0"));
    assert_eq!(property(&histories, "SeedSources"), Some("41"));
    assert_eq!(property(&histories, "CountDimension"), Some("D1"));
    assert_eq!(property(&histories, "AngleDimension"), Some("D3"));
}

#[test]
fn a_partial_turn_disagreeing_lanes_or_a_stated_group_leave_the_record() {
    let mut partial = history();
    enrich_circular_patterns(&mut partial, &walked(vec![pattern("CirPattern1", PI)]));
    assert_eq!(property(&partial, "SeedSources"), None);

    let mut other = pattern("CirPattern1", TAU);
    other.parameters[0].value = 6.0;
    let lanes = WalkedPatterns::from([
        ("lane".to_owned(), vec![pattern("CirPattern1", TAU)]),
        ("other".to_owned(), vec![other]),
    ]);
    let mut disagreeing = history();
    enrich_circular_patterns(&mut disagreeing, &lanes);
    assert_eq!(property(&disagreeing, "SeedSources"), None);

    let mut stated = history();
    stated[0].features[1].properties.insert(
        NonBlankString::new("AxisOrigin").unwrap(),
        "1mm,2mm,3mm".to_owned(),
    );
    enrich_circular_patterns(&mut stated, &walked(vec![pattern("CirPattern1", TAU)]));
    assert_eq!(property(&stated, "SeedSources"), None);
    assert_eq!(property(&stated, "AxisOrigin"), Some("1mm,2mm,3mm"));
}

#[test]
fn an_ambiguous_lane_a_carrier_a_repeated_seed_name_or_another_class_leave_the_record() {
    let lanes = WalkedPatterns::from([
        ("lane".to_owned(), vec![pattern("CirPattern1", TAU)]),
        (
            "other".to_owned(),
            vec![pattern("CirPattern1", TAU), pattern("CirPattern1", TAU)],
        ),
    ]);
    let mut ambiguous = history();
    enrich_circular_patterns(&mut ambiguous, &lanes);
    assert_eq!(property(&ambiguous, "SeedSources"), None);

    let mut carried = pattern("CirPattern1", TAU);
    carried.other_sources = true;
    let mut with_carrier = history();
    enrich_circular_patterns(&mut with_carrier, &walked(vec![carried]));
    assert_eq!(property(&with_carrier, "SeedSources"), None);

    let mut repeated = history();
    repeated[0]
        .features
        .push(feature("Cut-Extrude2", "42", "moExtrusion_c", &[]));
    enrich_circular_patterns(&mut repeated, &walked(vec![pattern("CirPattern1", TAU)]));
    assert_eq!(property(&repeated, "SeedSources"), None);

    let mut linear = history();
    linear[0].features[1].input_class = Some("moLPattern_c".to_owned());
    enrich_circular_patterns(&mut linear, &walked(vec![pattern("CirPattern1", TAU)]));
    assert_eq!(property(&linear, "SeedSources"), None);
}
