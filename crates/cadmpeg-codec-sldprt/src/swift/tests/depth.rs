// SPDX-License-Identifier: Apache-2.0
use super::{entity, reference};
use crate::swift::{Entity, RelatedObject, MAX_DEPTH};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

fn chain() -> Vec<(String, Entity)> {
    (0..=MAX_DEPTH + 1).map(|index| {
        let mut feature = entity("GdtCompoundHole");
        if index <= MAX_DEPTH { feature.features.references.push(reference(&format!("F{}", index+1), "GdtCompoundHole")); }
        (format!("F{index}"), feature)
    }).collect()
}

#[test]
fn swift_reachability_depth_refuses_instead_of_false() {
    let features = chain(); let index = features.iter().map(|(id, entity)| (id.as_str(), entity)).collect::<BTreeMap<_,_>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(matches!(crate::swift::feature_reaches(&ctx, "F0", "absent", &index, &mut BTreeSet::new(), 0), Err(CodecError::ResourceLimit(limit)) if limit.operation == "traverse SWIFT feature reachability"));
}

#[test]
fn swift_rotational_depth_refuses_instead_of_partial_contributors() {
    let features = chain(); let index = features.iter().map(|(id, entity)| (id.as_str(), entity)).collect::<BTreeMap<_,_>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(matches!(crate::swift::collect_rotational_projections(&ctx, "F0", &index, [1.,0.,0.], &mut BTreeSet::new(), 0, &mut Vec::new()), Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan SWIFT rotational features"));
}

#[test]
fn swift_diameter_depth_refuses_instead_of_partial_contributors() {
    let features = chain(); let index = features.iter().map(|(id, entity)| (id.as_str(), entity)).collect::<BTreeMap<_,_>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(matches!(crate::swift::collect_diameter_contributors(&ctx, "F0", &index, &mut BTreeSet::new(), 0, &mut Vec::new()), Err(CodecError::ResourceLimit(limit)) if limit.operation == "scan SWIFT diameter features"));
}

#[test]
fn swift_measurement_depth_refuses_instead_of_missing_child() {
    let features = chain(); let index = features.iter().map(|(id, entity)| (id.as_str(), entity)).collect::<BTreeMap<_,_>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(matches!(crate::swift::measurement_for_feature(&ctx, "F0", &index, &mut BTreeSet::new(), 0, |_| None), Err(CodecError::ResourceLimit(limit)) if limit.operation == "measure SWIFT feature geometry"));
}

#[test]
fn swift_pattern_depth_refuses_instead_of_visiting_parent() {
    let mut features = chain();
    for (id, feature) in &mut features {
        let index = id.strip_prefix('F').unwrap().parse::<usize>().unwrap();
        *feature = entity("GdtPattern");
        let mut applied = entity("GdtAppliedFeature");
        applied.features.references.push(reference(&format!("F{}", index+1), "GdtPattern"));
        let mut collection = entity("GdtAppliedFeatureCollection");
        collection.related.push(RelatedObject { name: "Child".into(), class: applied.class.clone(), entity: applied });
        feature.related.push(RelatedObject { name: "SubFeatures".into(), class: collection.class.clone(), entity: collection });
    }
    let index = features.iter().map(|(id, entity)| (id.as_str(), entity)).collect::<BTreeMap<_,_>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut visited = Vec::new();
    assert!(matches!(crate::swift::visit_expanded_feature_ids(&ctx, "F0", &index, 0, &mut |id| {visited.push(id); Ok(true)}), Err(CodecError::ResourceLimit(limit)) if limit.operation == "expand SWIFT target features"));
    assert!(visited.is_empty());
}

#[test]
fn swift_cycles_remain_absent_and_pattern_leaves_are_visited() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut visited = BTreeSet::from(["cycle".to_owned()]);
    assert!(!crate::swift::feature_reaches(&ctx, "cycle", "target", &BTreeMap::new(), &mut visited, MAX_DEPTH).unwrap());
    let leaf = entity("GdtCylinder"); let index = BTreeMap::from([("leaf", &leaf)]);
    let mut visits = Vec::new();
    assert!(crate::swift::visit_expanded_feature_ids(&ctx, "leaf", &index, MAX_DEPTH, &mut |id| { visits.push(id); Ok(true) }).unwrap());
    assert_eq!(visits, ["leaf"]);
}
