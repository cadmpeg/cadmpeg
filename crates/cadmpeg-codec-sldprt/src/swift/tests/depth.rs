// SPDX-License-Identifier: Apache-2.0
use super::{entity, reference};
use crate::swift::{Entity, FeatureIndex, RelatedObject, Walk, WalkCaps, WalkDepths, MAX_DEPTH};
use std::collections::BTreeMap;

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

fn feature_index(features: &[(String, Entity)]) -> FeatureIndex<'_> {
    FeatureIndex::new(
        features
            .iter()
            .map(|(id, entity)| (id.as_str(), entity))
            .collect(),
    )
}

const DEPTH_CAPPED: WalkCaps = WalkCaps { depth: 1, work: 0 };

#[test]
fn swift_reachability_depth_cap_reports_the_unknown() {
    let features = chain();
    let index = feature_index(&features);
    let ctx = cadmpeg_test_support::service_decode_context();
    let target_at_cap = format!("F{MAX_DEPTH}");
    assert_eq!(
        crate::swift::feature_reaches(&ctx, "F0", &target_at_cap, &index).unwrap(),
        Walk::Done(true)
    );
    assert_eq!(index.caps(), WalkCaps::default());
    let target_past_cap = format!("F{}", MAX_DEPTH + 1);
    assert_eq!(
        crate::swift::feature_reaches(&ctx, "F0", &target_past_cap, &index).unwrap(),
        Walk::Capped
    );
    assert_eq!(index.caps(), DEPTH_CAPPED);
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_rotational_depth_cap_reports_the_unknown() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let within = chain_with_cylinder_at(MAX_DEPTH - 1);
    let index = feature_index(&within);
    let Walk::Done(projections) =
        crate::swift::collect_rotational_projections(&ctx, "F0", &index, [1.0, 0.0, 0.0]).unwrap()
    else {
        panic!("a contributor within the cap is found");
    };
    assert_eq!(projections.len(), 1);
    assert_eq!(index.caps(), WalkCaps::default());
    let beyond = chain_with_cylinder_at(MAX_DEPTH);
    let index = feature_index(&beyond);
    assert_eq!(
        crate::swift::collect_rotational_projections(&ctx, "F0", &index, [1.0, 0.0, 0.0]).unwrap(),
        Walk::Capped
    );
    assert_eq!(index.caps(), DEPTH_CAPPED);
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_diameter_depth_cap_reports_the_unknown() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let within = chain_with_cylinder_at(MAX_DEPTH - 1);
    let index = feature_index(&within);
    let Walk::Done(values) =
        crate::swift::collect_diameter_contributors(&ctx, "F0", &index).unwrap()
    else {
        panic!("a contributor within the cap is found");
    };
    assert_eq!(values.len(), 1);
    let beyond = chain_with_cylinder_at(MAX_DEPTH);
    let index = feature_index(&beyond);
    assert_eq!(
        crate::swift::collect_diameter_contributors(&ctx, "F0", &index).unwrap(),
        Walk::Capped
    );
    assert_eq!(index.caps(), DEPTH_CAPPED);
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_measurement_depth_cap_reports_the_unknown() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let measure = |feature: &Entity| {
        (!feature.related.is_empty()).then(|| cadmpeg_ir::scalar::PositiveReal::new(3.0).unwrap())
    };
    let within = chain_with_cylinder_at(MAX_DEPTH - 1);
    let index = feature_index(&within);
    assert!(matches!(
        crate::swift::measurement_for_feature(&ctx, "F0", &index, measure).unwrap(),
        Walk::Done(Some(_))
    ));
    let beyond = chain_with_cylinder_at(MAX_DEPTH);
    let index = feature_index(&beyond);
    assert_eq!(
        crate::swift::measurement_for_feature(&ctx, "F0", &index, measure).unwrap(),
        Walk::Capped
    );
    assert_eq!(index.caps(), DEPTH_CAPPED);
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_pattern_depth_cap_drops_the_targets() {
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
    let index = feature_index(&features);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut annotation = entity("GdtPosition");
    annotation
        .features
        .references
        .push(reference("F0", "GdtPattern"));
    assert_eq!(
        crate::swift::targets(&ctx, &annotation, &index, None).unwrap(),
        None
    );
    assert_eq!(index.caps(), DEPTH_CAPPED);
    let [loss] = index.caps().losses(index.work_limit).try_into().unwrap();
    assert_eq!(
        loss.code,
        crate::loss::SldprtLossCode::PmiSwiftFeatureWalkCapped.kind()
    );
    assert_eq!(
        loss.message,
        "1 SWIFT feature walk(s) reached a feature only past MAX_DEPTH (32); the targets or \
         nominal values they feed were left unresolved."
    );
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn swift_cycles_remain_absent_and_pattern_leaves_are_visited() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cycle = entity("GdtCompoundHole");
    cycle
        .features
        .references
        .push(reference("cycle", "GdtCompoundHole"));
    let features = vec![("cycle".to_owned(), cycle)];
    let index = feature_index(&features);
    assert_eq!(
        crate::swift::feature_reaches(&ctx, "cycle", "target", &index).unwrap(),
        Walk::Done(false)
    );
    assert_eq!(index.caps(), WalkCaps::default());
    let leaf = entity("GdtCylinder");
    let index = FeatureIndex::new(BTreeMap::from([("leaf", &leaf)]));
    let mut visits = Vec::new();
    assert_eq!(
        crate::swift::visit_expanded_feature_ids(
            &ctx,
            "leaf",
            &index,
            &mut WalkDepths::default(),
            MAX_DEPTH,
            &mut |id| {
                visits.push(id);
                Ok(true)
            }
        )
        .unwrap(),
        Some(true)
    );
    assert_eq!(visits, ["leaf"]);
}
