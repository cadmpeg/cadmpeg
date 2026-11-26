use std::f64::consts::TAU;
use std::sync::Arc;

use cadmpeg_ir::math::{Point3, Vector3};

use super::{walked_circular, WalkedPatterns};
use crate::carchive::reader::{ParameterRecord, V};
use crate::carchive::PatternSeeds;

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
/// axis through (0, 0.5, 0) mm along +Y, with an active count `D1` of 4 and
/// angle `D3` of a full turn, and an inactive second direction `D4`, `D5`.
fn pattern(name: &str) -> PatternSeeds {
    PatternSeeds {
        pattern: Some(name.to_owned()),
        seeds: vec![Some("Cut-Extrude2".to_owned())],
        parameters: vec![
            parameter("moIntegerParameter_c", "D1", 4.0),
            parameter("moIntegerParameter_c", "D4", 0.0),
            parameter("moAngleParameter_c", "D5", 0.25),
            parameter("moAngleParameter_c", "D3", TAU),
        ],
        axis: Some(([0.001, 0.0, 0.0005, 0.0, 0.0, 2.0, 0.0], 1)),
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
        &walked(vec![pattern("CirPattern1")]),
        "lane",
        "CirPattern1",
        listed,
    )
    .expect("one walked pattern of the name");
    assert_eq!(found.seeds, Some(vec!["Cut-Extrude2".to_owned()]));
    assert_eq!(found.count, Some(4));
    assert_eq!(found.angle, Some(TAU));
    let (origin, direction) = found.axis.expect("an axis");
    assert_eq!(origin.get(), Point3::new(0.0, 0.5, 0.0));
    assert_eq!(*direction.as_raw(), Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn a_second_listed_parameter_of_a_class_states_no_value() {
    let every = |name: &str| name.starts_with('D');
    let found = walked_circular(
        &walked(vec![pattern("CirPattern1")]),
        "lane",
        "CirPattern1",
        every,
    )
    .unwrap();
    assert_eq!((found.count, found.angle), (None, None));
}

#[test]
fn an_unnamed_seed_or_a_repeated_pattern_name_states_no_seeds() {
    let mut unnamed = pattern("CirPattern1");
    unnamed.seeds.push(None);
    let found = walked_circular(&walked(vec![unnamed]), "lane", "CirPattern1", listed).unwrap();
    assert_eq!(found.seeds, None);
    let twice = walked(vec![pattern("CirPattern1"), pattern("CirPattern1")]);
    assert_eq!(walked_circular(&twice, "lane", "CirPattern1", listed), None);
    assert_eq!(
        walked_circular(
            &walked(vec![pattern("CirPattern1")]),
            "other",
            "CirPattern1",
            listed
        ),
        None
    );
}

#[test]
fn a_degenerate_axis_direction_states_no_axis() {
    let mut flat = pattern("CirPattern1");
    flat.axis = Some(([0.0; 7], 0));
    let found = walked_circular(&walked(vec![flat]), "lane", "CirPattern1", listed).unwrap();
    assert_eq!(found.axis, None);
}
