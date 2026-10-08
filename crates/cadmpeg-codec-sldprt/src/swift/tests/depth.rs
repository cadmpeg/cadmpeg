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
    assert!(
        matches!(crate::swift::collect_rotational_projections(&ctx, "F0", &index, [1.,0.,0.], &mut BTreeSet::new(), 0, &mut Vec::new()), Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan SWIFT rotational features")
    );
}

#[test]
fn swift_diameter_depth_refuses_instead_of_partial_contributors() {
    let features = chain();
    let index = features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(
        matches!(crate::swift::collect_diameter_contributors(&ctx, "F0", &index, &mut BTreeSet::new(), 0, &mut Vec::new()), Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan SWIFT diameter features")
    );
}

#[test]
fn swift_measurement_depth_refuses_instead_of_missing_child() {
    let features = chain();
    let index = features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(
        matches!(crate::swift::measurement_for_feature(&ctx, "F0", &index, &mut BTreeSet::new(), 0, |_| Ok(None)), Err(CodecError::ResourceLimit(limit)) if limit.operation == "measure SWIFT feature geometry")
    );
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
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
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
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "validate SWIFT direct subfeatures",
            None,
        );
        let ctx = cadmpeg_test_support::service_decode_context();
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
