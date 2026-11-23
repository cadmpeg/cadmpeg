// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use super::marker_sketch_profiles;

/// Corner positions of a unit square, by endpoint reference.
const CORNERS: [(&str, [f64; 2]); 5] = [
    ("a", [0.0, 0.0]),
    ("b", [1.0, 0.0]),
    ("c", [1.0, 1.0]),
    ("d", [0.0, 1.0]),
    ("e", [0.0, -1.0]),
];

fn id(name: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("synthetic:test:id#line-{name}")).unwrap()
}

/// The line `name` from corner `start` to corner `end`.
fn line(name: &str, start: &str, end: &str) -> SketchEntity {
    let at = |corner: &str| {
        let [u, v] = CORNERS
            .iter()
            .find(|(name, _)| *name == corner)
            .map(|(_, position)| *position)
            .unwrap();
        Point2::new(u, v)
    };
    SketchEntity::new(
        id(name),
        SketchId::mint("synthetic:test:id#sketch").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: at(start),
            end: at(end),
        })
        .unwrap(),
    )
    .with_endpoint_refs(vec![start.to_owned(), end.to_owned()])
}

/// The four sides of the square, each read from its `sgSketch` record.
fn square() -> Vec<SketchEntity> {
    vec![
        line("ab", "a", "b"),
        line("bc", "b", "c"),
        line("cd", "c", "d"),
        line("da", "d", "a"),
    ]
}

/// The member names of each profile, sorted.
fn members(profiles: &[Vec<cadmpeg_ir::sketches::SketchEntityUse>]) -> Vec<Vec<String>> {
    profiles
        .iter()
        .map(|profile| {
            let mut names = profile
                .iter()
                .map(|entity_use| {
                    entity_use
                        .entity
                        .as_str()
                        .rsplit('-')
                        .next()
                        .unwrap()
                        .to_owned()
                })
                .collect::<Vec<_>>();
            names.sort();
            names
        })
        .collect()
}

/// The square's sides, read from their records, close a profile alone. A
/// line of another marker reading branches it at a corner, so the sketch's
/// profiles are those of the record-read curves.
#[test]
fn record_read_curves_that_close_more_profiles_alone_give_the_profiles() {
    let mut entities = square();
    entities.push(line("ae", "a", "e"));
    let record_read = square()
        .iter()
        .map(|entity| entity.id().clone())
        .collect::<HashSet<_>>();
    let profiles =
        marker_sketch_profiles(&entities, |id| record_read.contains(id), &HashSet::new());
    assert_eq!(members(&profiles), [["ab", "bc", "cd", "da"]]);
}

/// A diagonal read from a curve record of state `0` joins no profile; read as
/// a profile curve it would branch the square at two corners.
#[test]
fn a_curve_outside_the_profile_joins_none() {
    let mut entities = square();
    entities.push(line("ac", "a", "c"));
    let profiles = marker_sketch_profiles(&entities, |_| true, &HashSet::new());
    assert!(profiles.is_empty());
    let profiles = marker_sketch_profiles(&entities, |_| true, &HashSet::from([id("ac")]));
    assert_eq!(members(&profiles), [["ab", "bc", "cd", "da"]]);
}
