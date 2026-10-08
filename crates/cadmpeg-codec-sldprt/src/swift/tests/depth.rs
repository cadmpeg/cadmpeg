// SPDX-License-Identifier: Apache-2.0
use super::{entity, reference};
use crate::swift::{Entity, RelatedObject, MAX_DEPTH};
use cadmpeg_core::CodecError;
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

#[test]
fn swift_reachability_depth_refuses_instead_of_false() {
    let features = chain();
    let index = features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(
        matches!(crate::swift::feature_reaches(&ctx, "F0", "absent", &index, &mut BTreeSet::new(), 0), Err(CodecError::ResourceLimit(limit)) if limit.operation == "traverse SWIFT feature reachability")
    );
}

#[test]
fn swift_rotational_depth_refuses_instead_of_partial_contributors() {
    let features = chain();
    let index = features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut traversal = crate::swift::FeatureListTraversal::<crate::swift::FiniteReal, _>::new(
        &ctx,
        &index,
        "scan SWIFT rotational features",
        "track SWIFT rotational path",
        "collect SWIFT rotational projections",
        |_| Ok(None),
    )
    .unwrap();
    assert!(matches!(traversal.collect("F0", 0, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan SWIFT rotational features"));
}

#[test]
fn swift_diameter_depth_refuses_instead_of_partial_contributors() {
    let features = chain();
    let index = features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut traversal = crate::swift::FeatureListTraversal::<crate::swift::PositiveReal, _>::new(
        &ctx,
        &index,
        "scan SWIFT diameter features",
        "track SWIFT diameter path",
        "collect SWIFT diameter contributors",
        |_| Ok(None),
    )
    .unwrap();
    assert!(matches!(traversal.collect("F0", 0, &mut Vec::new()),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan SWIFT diameter features"));
}

#[test]
fn swift_measurement_depth_refuses_instead_of_missing_child() {
    let features = chain();
    let index = features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut traversal =
        crate::swift::FeatureMeasurementTraversal::new(&ctx, &index, |_| Ok(None)).unwrap();
    assert!(matches!(traversal.visit("F0", 0),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "measure SWIFT feature geometry"));
}

#[test]
fn swift_pattern_depth_refuses_instead_of_visiting_parent() {
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
        matches!(crate::swift::visit_expanded_feature_ids(&ctx, "F0", &index, 0, &mut |id| {visited.push(id); Ok(true)}), Err(CodecError::ResourceLimit(limit)) if limit.operation == "expand SWIFT target features")
    );
    assert!(visited.is_empty());
}

#[test]
fn swift_cycles_remain_absent_and_pattern_leaves_are_visited() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut visited = BTreeSet::from(["cycle"]);
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

#[test]
fn swift_missing_vector_component_does_not_hide_later_lookup_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    const LOOKUP: &str = "look up SLDPRT ordered key";
    let mut geometry = entity("GeoCylinder");
    geometry.doubles.insert("Other".into(), 0.0);
    let arena = DecodeArena::new();
    let with_work_limit = |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let result = crate::swift::vector(&ctx, &geometry, ["I", "J", "K"]);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    };
    // The first lookup misses; a budget that admits exactly it must still
    // refuse the next component's lookup instead of returning the miss.
    let CodecError::ResourceLimit(first) = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        LOOKUP,
        with_work_limit,
    ) else {
        panic!("expected the first lookup to refuse");
    };
    let first_lookup = first.used + first.additional;
    let CodecError::ResourceLimit(later) = with_work_limit(first_lookup).unwrap_err() else {
        panic!("expected a later lookup to refuse");
    };
    assert_eq!(later.operation, LOOKUP);
    assert_eq!(later.used, first_lookup);
}

#[test]
fn swift_missing_nominal_field_preserves_lookup_refusal() {
    let mut feature = entity("GdtCylinder");
    let mut geometry = entity("GeoCylinder");
    geometry.doubles.insert("Other".into(), 0.0);
    feature.related.push(RelatedObject {
        name: "NomCylinder".into(),
        class: "GeoCylinder".into(),
        entity: geometry,
    });
    assert_work_refusal_at("look up SLDPRT ordered key", |ctx| {
        crate::swift::nominal_measurement(ctx, &feature, "NomCylinder", "R").map(drop)
    });
}

fn assert_work_refusal_at(
    operation: &'static str,
    mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<(), CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let result = run(&ctx);
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == operation
            && limit.additional > 0));
}

#[test]
fn swift_reachability_membership_refusal_reaches_the_caller() {
    assert_work_refusal_at("check SWIFT reachability path", |ctx| {
        let mut visited = BTreeSet::from(["F0"]);
        let result =
            crate::swift::feature_reaches(ctx, "F0", "target", &BTreeMap::new(), &mut visited, 0);
        if matches!(&result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "check SWIFT reachability path")
        {
            assert_eq!(visited, BTreeSet::from(["F0"]));
        }
        result.map(|_| ())
    });
}

#[test]
fn swift_reachability_explores_shared_descendants_once() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut features = Vec::new();
    for level in 0..20 {
        for branch in 0..2 {
            let mut feature = entity("GdtCompoundHole");
            if level < 19 {
                for child in 0..2 {
                    feature.features.references.push(reference(
                        &format!("F{}-{child}", level + 1),
                        "GdtCompoundHole",
                    ));
                }
            }
            features.push((format!("F{level}-{branch}"), feature));
        }
    }
    let index = features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 100_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut visited = BTreeSet::new();
    let (reaches, storage) = ctx
        .with_scoped_storage("reachability test workspace", || {
            crate::swift::feature_reaches(&ctx, "F0-0", "absent", &index, &mut visited, 0)
        })
        .unwrap();
    assert!(!reaches);
    assert_eq!(visited.len(), 39);
    assert!(visited.contains("F19-0") && visited.contains("F19-1"));
    drop((visited, storage));
    ctx.finish_session().unwrap();
}

#[test]
fn swift_direct_reachability_defers_subfeature_validation() {
    use cadmpeg_core::decode::{
        refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    let mut feature = entity("GdtCompoundHole");
    feature
        .features
        .references
        .push(reference("target", "GdtCylinder"));
    let mut collection = entity("GdtAppliedFeatureCollection");
    for index in 0..128 {
        let mut applied = entity("GdtAppliedFeature");
        applied
            .features
            .references
            .push(reference(&format!("child-{index}"), "GdtCylinder"));
        collection.related.push(RelatedObject {
            name: format!("child-{index}"),
            class: applied.class.clone(),
            entity: applied,
        });
    }
    feature.related.push(RelatedObject {
        name: "SubFeatures".into(),
        class: collection.class.clone(),
        entity: collection,
    });
    let index = BTreeMap::from([("root", &feature)]);
    {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "validate SWIFT direct subfeatures",
            None,
        );
        assert!(crate::swift::feature_reaches(
            &ctx,
            "root",
            "target",
            &index,
            &mut BTreeSet::new(),
            0
        )
        .unwrap());
        ctx.finish_session().unwrap();
    }
    assert_work_refusal_at("validate SWIFT direct subfeatures", |ctx| {
        crate::swift::feature_reaches(ctx, "root", "absent", &index, &mut BTreeSet::new(), 0)
            .map(drop)
    });
}

fn shared_diamond() -> Vec<(String, Entity)> {
    let mut features = Vec::new();
    for level in 0..20 {
        for branch in 0..2 {
            let mut feature = entity("GdtCompoundHole");
            if level < 19 {
                for child in 0..2 {
                    feature.features.references.push(reference(
                        &format!("F{}-{child}", level + 1),
                        "GdtCompoundHole",
                    ));
                }
            }
            features.push((format!("F{level}-{branch}"), feature));
        }
    }
    features
}

fn feature_index(features: &[(String, Entity)]) -> BTreeMap<&str, &Entity> {
    features
        .iter()
        .map(|(id, feature)| (id.as_str(), feature))
        .collect()
}

#[test]
fn list_traversals_cache_empty_diamond_results_per_query() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    const UNIQUE_FEATURES: usize = 39; // F0-0 plus two features at each of levels 1..19.
    let features = shared_diamond();
    let index = feature_index(&features);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let rotational_visits = std::cell::Cell::new(0_usize);
    let (values, output_storage) = ctx
        .with_scoped_storage("diamond output", || {
            let mut values = Vec::new();
            let mut traversal =
                crate::swift::FeatureListTraversal::<crate::swift::FiniteReal, _>::new(
                    &ctx,
                    &index,
                    "scan SWIFT rotational features",
                    "track SWIFT rotational path",
                    "collect SWIFT rotational projections",
                    |_| {
                        let visits = rotational_visits.get().checked_add(1).expect("bounded visits");
                        assert!(visits <= UNIQUE_FEATURES, "a completed feature result was evaluated twice");
                        rotational_visits.set(visits);
                        Ok(None)
                    },
                )?;
            traversal.collect("F0-0", 0, &mut values)?;
            Ok::<_, CodecError>(values)
        })
        .unwrap();
    assert!(values.is_empty());
    assert_eq!(rotational_visits.get(), UNIQUE_FEATURES);
    drop((values, output_storage));
    ctx.finish_session().unwrap();

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let diameter_visits = std::cell::Cell::new(0_usize);
    let (values, output_storage) = ctx
        .with_scoped_storage("diamond output", || {
            let mut values = Vec::new();
            let mut traversal =
                crate::swift::FeatureListTraversal::<crate::swift::PositiveReal, _>::new(
                    &ctx,
                    &index,
                    "scan SWIFT diameter features",
                    "track SWIFT diameter path",
                    "collect SWIFT diameter contributors",
                    |_| {
                        let visits = diameter_visits.get().checked_add(1).expect("bounded visits");
                        assert!(visits <= UNIQUE_FEATURES, "a completed feature result was evaluated twice");
                        diameter_visits.set(visits);
                        Ok(None)
                    },
                )?;
            traversal.collect("F0-0", 0, &mut values)?;
            Ok::<_, CodecError>(values)
        })
        .unwrap();
    assert!(values.is_empty());
    assert_eq!(diameter_visits.get(), UNIQUE_FEATURES);
    drop((values, output_storage));
    ctx.finish_session().unwrap();

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let measurement_visits = std::cell::Cell::new(0_usize);
    let mut traversal = crate::swift::FeatureMeasurementTraversal::new(&ctx, &index, |_| {
        let visits = measurement_visits.get().checked_add(1).expect("bounded visits");
        assert!(visits <= UNIQUE_FEATURES, "a completed feature result was evaluated twice");
        measurement_visits.set(visits);
        Ok(None)
    })
    .unwrap();
    assert_eq!(traversal.visit("F0-0", 0).unwrap().value, None);
    assert_eq!(measurement_visits.get(), UNIQUE_FEATURES);
    drop(traversal);
    ctx.finish_session().unwrap();
}

fn marker(value: f64) -> Entity {
    let mut feature = entity("Leaf");
    feature.doubles.insert("value".into(), value);
    feature
}

#[test]
fn cached_feature_lists_preserve_path_order_and_repeated_contributors() {
    let mut root = entity("GdtCompoundHole");
    root.features.references = vec![
        reference("left", "GdtCompoundHole"),
        reference("right", "GdtCompoundHole"),
    ];
    let mut left = entity("GdtCompoundHole");
    left.features.references = vec![reference("one", "Leaf"), reference("two", "Leaf")];
    let mut right = entity("GdtCompoundHole");
    right.features.references = vec![reference("two", "Leaf"), reference("one", "Leaf")];
    let features = vec![
        ("root".to_owned(), root),
        ("left".to_owned(), left),
        ("right".to_owned(), right),
        ("one".to_owned(), marker(1.0)),
        ("two".to_owned(), marker(2.0)),
    ];
    let index = feature_index(&features);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut traversal = crate::swift::FeatureListTraversal::<crate::swift::FiniteReal, _>::new(
        &ctx,
        &index,
        "scan SWIFT rotational features",
        "track SWIFT rotational path",
        "collect SWIFT rotational projections",
        |feature| {
            Ok(ctx
                .get_btree_map(&feature.doubles, "value", "test value")?
                .copied()
                .and_then(crate::swift::FiniteReal::new))
        },
    )
    .unwrap();
    let mut values = Vec::new();
    traversal.collect("root", 0, &mut values).unwrap();
    assert_eq!(
        values.iter().map(|value| value.get()).collect::<Vec<_>>(),
        [1.0, 2.0, 2.0, 1.0]
    );
}

#[test]
fn cycle_dependent_list_results_are_recomputed_for_each_path() {
    let mut root = entity("GdtCompoundHole");
    root.features.references = vec![
        reference("a", "GdtCompoundHole"),
        reference("b", "GdtCompoundHole"),
    ];
    let mut a = entity("GdtCompoundHole");
    a.features.references = vec![
        reference("shared", "GdtCompoundHole"),
        reference("two", "Leaf"),
    ];
    let mut b = entity("GdtCompoundHole");
    b.features
        .references
        .push(reference("shared", "GdtCompoundHole"));
    let mut shared = entity("GdtCompoundHole");
    shared.features.references = vec![reference("a", "GdtCompoundHole"), reference("one", "Leaf")];
    let features = vec![
        ("root".to_owned(), root),
        ("a".to_owned(), a),
        ("b".to_owned(), b),
        ("shared".to_owned(), shared),
        ("one".to_owned(), marker(1.0)),
        ("two".to_owned(), marker(2.0)),
    ];
    let index = feature_index(&features);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut traversal = crate::swift::FeatureListTraversal::<crate::swift::FiniteReal, _>::new(
        &ctx,
        &index,
        "scan SWIFT rotational features",
        "track SWIFT rotational path",
        "collect SWIFT rotational projections",
        |feature| {
            Ok(ctx
                .get_btree_map(&feature.doubles, "value", "test value")?
                .copied()
                .and_then(crate::swift::FiniteReal::new))
        },
    )
    .unwrap();
    let mut values = Vec::new();
    traversal.collect("root", 0, &mut values).unwrap();
    assert_eq!(
        values.iter().map(|value| value.get()).collect::<Vec<_>>(),
        [1.0, 2.0, 2.0, 1.0]
    );
}

#[test]
fn measurement_cache_preserves_child_disagreement() {
    use std::cell::Cell;
    let mut root = entity("GdtCompoundHole");
    root.features.references = vec![reference("low", "Leaf"), reference("high", "Leaf")];
    let low = marker(4.0);
    let high = marker(9.0);
    let features = vec![
        ("root".to_owned(), root),
        ("low".to_owned(), low),
        ("high".to_owned(), high),
    ];
    let index = feature_index(&features);
    let ctx = cadmpeg_test_support::service_decode_context();
    let calls = Cell::new(0usize);
    let mut traversal = crate::swift::FeatureMeasurementTraversal::new(&ctx, &index, |feature| {
        calls.set(calls.get() + 1);
        Ok(ctx
            .get_btree_map(&feature.doubles, "value", "test value")?
            .copied()
            .and_then(crate::swift::PositiveReal::new))
    })
    .unwrap();
    assert_eq!(traversal.visit("root", 0).unwrap().value, None);
    assert_eq!(traversal.visit("root", 0).unwrap().value, None);
    assert_eq!(
        calls.get(),
        3,
        "the second query reuses the cached disagreement"
    );
}

#[test]
fn measurement_results_with_cycle_dependence_are_not_reused() {
    let mut a = entity("GdtCompoundHole");
    a.features.references = vec![
        reference("shared", "GdtCompoundHole"),
        reference("nine", "Leaf"),
    ];
    let mut shared = entity("GdtCompoundHole");
    shared.features.references = vec![reference("a", "GdtCompoundHole"), reference("five", "Leaf")];
    let mut b = entity("GdtCompoundHole");
    b.features
        .references
        .push(reference("shared", "GdtCompoundHole"));
    let features = vec![
        ("a".to_owned(), a),
        ("b".to_owned(), b),
        ("shared".to_owned(), shared),
        ("nine".to_owned(), marker(9.0)),
        ("five".to_owned(), marker(5.0)),
    ];
    let index = feature_index(&features);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut traversal = crate::swift::FeatureMeasurementTraversal::new(&ctx, &index, |feature| {
        Ok(ctx
            .get_btree_map(&feature.doubles, "value", "test value")?
            .copied()
            .and_then(crate::swift::PositiveReal::new))
    })
    .unwrap();
    assert_eq!(traversal.visit("a", 0).unwrap().value, None);
    assert_eq!(traversal.visit("b", 0).unwrap().value, None);
}

#[test]
fn cached_list_depth_does_not_bypass_policy_nesting() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut root = entity("GdtCompoundHole");
    root.features.references = vec![
        reference("short", "GdtCompoundHole"),
        reference("long0", "GdtCompoundHole"),
    ];
    let mut short = entity("GdtCompoundHole");
    short
        .features
        .references
        .push(reference("shared", "GdtCompoundHole"));
    let mut long0 = entity("GdtCompoundHole");
    long0
        .features
        .references
        .push(reference("long1", "GdtCompoundHole"));
    let mut long1 = entity("GdtCompoundHole");
    long1
        .features
        .references
        .push(reference("shared", "GdtCompoundHole"));
    let mut shared = entity("GdtCompoundHole");
    shared.features.references.push(reference("leaf", "Leaf"));
    let features = vec![
        ("root".to_owned(), root),
        ("short".to_owned(), short),
        ("long0".to_owned(), long0),
        ("long1".to_owned(), long1),
        ("shared".to_owned(), shared),
        ("leaf".to_owned(), marker(1.0)),
    ];
    let index = feature_index(&features);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut traversal = crate::swift::FeatureListTraversal::<crate::swift::FiniteReal, _>::new(
        &ctx,
        &index,
        "scan SWIFT rotational features",
        "track SWIFT rotational path",
        "collect SWIFT rotational projections",
        |feature| {
            Ok(ctx
                .get_btree_map(&feature.doubles, "value", "test value")?
                .copied()
                .and_then(crate::swift::FiniteReal::new))
        },
    )
    .unwrap();
    let refusal = traversal.collect("root", 0, &mut Vec::new())
        .expect_err("the deeper contributor path must hit the live nesting limit");
    assert!(matches!(&refusal, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth));
    drop(traversal);
    let repeated = ctx.finish_session().expect_err("the depth refusal remains fused");
    assert!(matches!((refusal, repeated),
        (CodecError::ResourceLimit(first), CodecError::ResourceLimit(repeated))
            if first == repeated));
}
