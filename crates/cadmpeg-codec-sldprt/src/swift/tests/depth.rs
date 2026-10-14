// SPDX-License-Identifier: Apache-2.0
use super::{entity, reference};
use crate::swift::{Entity, RelatedObject, MAX_DEPTH};
use std::collections::{BTreeMap, BTreeSet};

fn chain() -> Vec<(String, Entity)> {
    (0..=MAX_DEPTH + 1)
        .map(|index| {
            let mut feature = entity("GdtCompoundHole");
            if index <= MAX_DEPTH {
                feature.features.references.push(reference(
                    &format!(
                        "F{}",
                        index.checked_add(1).expect("bounded test depth index")
                    ),
                    "GdtCompoundHole",
                ));
            }
            (format!("F{index}"), feature)
        })
        .collect()
}

/// The chain with `F{leaf}` replaced by a cylinder whose axis is +Z through
/// (2, 0, 0) with radius 1.
fn chain_with_cylinder_at(leaf: usize) -> Vec<(String, Entity)> {
    let mut features = chain();
    let mut axis = entity("GdtAxis");
    for (name, value) in [
        ("I", 0.0),
        ("J", 0.0),
        ("K", 1.0),
        ("X", 2.0),
        ("Y", 0.0),
        ("Z", 0.0),
        ("R", 1.0),
    ] {
        axis.doubles.insert(name.into(), value);
    }
    let mut cylinder = entity("GdtCylinder");
    cylinder.related.push(RelatedObject {
        name: "NomCylinder".into(),
        class: axis.class.clone(),
        entity: axis,
    });
    features.get_mut(leaf).expect("leaf in the chain").1 = cylinder;
    features
}

fn feature_index(features: &[(String, Entity)]) -> BTreeMap<&str, &Entity> {
    features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect()
}

#[test]
fn swift_reachability_depth_cap_answers_unreached() {
    let features = chain();
    let index = feature_index(&features);
    let ctx = cadmpeg_test_support::service_decode_context();
    let target_at_cap = format!("F{MAX_DEPTH}");
    assert!(crate::swift::feature_reaches(
        &ctx,
        "F0",
        &target_at_cap,
        &index,
        &mut BTreeSet::new(),
        0
    )
    .unwrap());
    let target_past_cap = format!("F{}", MAX_DEPTH + 1);
    assert!(!crate::swift::feature_reaches(
        &ctx,
        "F0",
        &target_past_cap,
        &index,
        &mut BTreeSet::new(),
        0
    )
    .unwrap());
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_rotational_depth_cap_skips_the_contributor() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let within = chain_with_cylinder_at(MAX_DEPTH - 1);
    let mut projections = Vec::new();
    crate::swift::collect_rotational_projections(
        &ctx,
        "F0",
        &feature_index(&within),
        [1.0, 0.0, 0.0],
        &mut BTreeSet::new(),
        0,
        &mut projections,
    )
    .unwrap();
    assert_eq!(projections.len(), 1);
    let beyond = chain_with_cylinder_at(MAX_DEPTH);
    let mut projections = Vec::new();
    crate::swift::collect_rotational_projections(
        &ctx,
        "F0",
        &feature_index(&beyond),
        [1.0, 0.0, 0.0],
        &mut BTreeSet::new(),
        0,
        &mut projections,
    )
    .unwrap();
    assert!(projections.is_empty());
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_diameter_depth_cap_skips_the_contributor() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let within = chain_with_cylinder_at(MAX_DEPTH - 1);
    let mut values = Vec::new();
    crate::swift::collect_diameter_contributors(
        &ctx,
        "F0",
        &feature_index(&within),
        &mut BTreeSet::new(),
        0,
        &mut values,
    )
    .unwrap();
    assert_eq!(values.len(), 1);
    let beyond = chain_with_cylinder_at(MAX_DEPTH);
    let mut values = Vec::new();
    crate::swift::collect_diameter_contributors(
        &ctx,
        "F0",
        &feature_index(&beyond),
        &mut BTreeSet::new(),
        0,
        &mut values,
    )
    .unwrap();
    assert!(values.is_empty());
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_measurement_depth_cap_skips_the_child() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let measure = |feature: &Entity| {
        (!feature.related.is_empty()).then(|| cadmpeg_ir::scalar::PositiveReal::new(3.0).unwrap())
    };
    let within = chain_with_cylinder_at(MAX_DEPTH - 1);
    assert!(crate::swift::measurement_for_feature(
        &ctx,
        "F0",
        &feature_index(&within),
        &mut BTreeSet::new(),
        0,
        measure
    )
    .unwrap()
    .is_some());
    let beyond = chain_with_cylinder_at(MAX_DEPTH);
    assert!(crate::swift::measurement_for_feature(
        &ctx,
        "F0",
        &feature_index(&beyond),
        &mut BTreeSet::new(),
        0,
        measure
    )
    .unwrap()
    .is_none());
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_pattern_depth_cap_visits_the_truncated_pattern() {
    let mut features = chain();
    for (id, feature) in &mut features {
        let index = id.strip_prefix('F').unwrap().parse::<usize>().unwrap();
        *feature = entity("GdtPattern");
        let mut applied = entity("GdtAppliedFeature");
        applied.features.references.push(reference(
            &format!(
                "F{}",
                index.checked_add(1).expect("bounded test depth index")
            ),
            "GdtPattern",
        ));
        let mut collection = entity("GdtAppliedFeatureCollection");
        collection.related.push(RelatedObject {
            name: "Child".into(),
            class: applied.class.clone(),
            entity: applied,
        });
        feature.related.push(RelatedObject {
            name: "SubFeatures".into(),
            class: collection.class.clone(),
            entity: collection,
        });
    }
    let index = features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut visited = Vec::new();
    assert!(
        crate::swift::visit_expanded_feature_ids(&ctx, "F0", &index, 0, &mut |id| {
            visited.push(id);
            Ok(true)
        })
        .unwrap()
    );
    assert_eq!(visited, [format!("F{MAX_DEPTH}")]);
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_cycles_remain_absent_and_pattern_leaves_are_visited() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut visited = BTreeSet::from(["cycle".to_owned()]);
    assert!(!crate::swift::feature_reaches(
        &ctx,
        "cycle",
        "target",
        &BTreeMap::new(),
        &mut visited,
        MAX_DEPTH
    )
    .unwrap());
    let leaf = entity("GdtCylinder");
    let index = BTreeMap::from([("leaf", &leaf)]);
    let mut visits = Vec::new();
    assert!(
        crate::swift::visit_expanded_feature_ids(&ctx, "leaf", &index, MAX_DEPTH, &mut |id| {
            visits.push(id);
            Ok(true)
        })
        .unwrap()
    );
    assert_eq!(visits, ["leaf"]);
}
