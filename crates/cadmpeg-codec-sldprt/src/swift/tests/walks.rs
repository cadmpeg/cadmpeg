// SPDX-License-Identifier: Apache-2.0
//! Feature walks over graphs whose paths outnumber their features.
use super::{entity, reference};
use crate::swift::{
    approximately_equal, child_feature_ids, nominal_radius, Entity, FeatureIndex, RelatedObject,
    Walk, WalkCaps, MAX_DEPTH,
};
use cadmpeg_ir::scalar::PositiveReal;
use std::collections::{BTreeMap, BTreeSet};

/// Layers whose path count, 2^20, a per-path walk would visit.
const DEEP_LAYERS: usize = 20;
/// Layers small enough for the per-path reference walk.
const SHALLOW_LAYERS: usize = 10;
/// Work a linear walk of the deep graph stays under.
const LINEAR_WORK: usize = 1 << 10;

fn cylinder(radius: f64) -> Entity {
    let mut axis = entity("GdtAxis");
    axis.doubles.insert("R".into(), radius);
    let mut cylinder = entity("GdtCylinder");
    cylinder.related.push(RelatedObject {
        name: "NomCylinder".into(),
        class: axis.class.clone(),
        entity: axis,
    });
    cylinder
}

fn hole(children: &[String]) -> Entity {
    let mut feature = entity("GdtCompoundHole");
    for child in children {
        feature
            .features
            .references
            .push(reference(child, "GdtCompoundHole"));
    }
    feature
}

/// Features `a{i}` and `b{i}` reference both features of layer `i + 1`, so the
/// paths to the bottom double with each layer. The bottom features are
/// cylinders of radius `bottom` and `bottom + spread`. With `side` set, `a{i}`
/// also references a cylinder `c{i}` of radius `bottom + spread * (i + 2)`.
fn layered(layers: usize, bottom: f64, spread: f64, side: bool) -> Vec<(String, Entity)> {
    let mut features = Vec::new();
    for layer in 0..layers {
        let below = layer.checked_add(1).unwrap();
        let (next_a, next_b) = (format!("a{below}"), format!("b{below}"));
        let mut a_children = vec![next_b.clone()];
        if side {
            let side_id = format!("c{layer}");
            a_children.push(side_id.clone());
            let step = f64::from(u32::try_from(below.checked_add(1).unwrap()).unwrap());
            features.push((side_id, cylinder(bottom + spread * step)));
        }
        a_children.push(next_a.clone());
        features.push((format!("a{layer}"), hole(&a_children)));
        features.push((format!("b{layer}"), hole(&[next_a, next_b])));
    }
    features.push((format!("a{layers}"), cylinder(bottom)));
    features.push((format!("b{layers}"), cylinder(bottom + spread)));
    features
}

fn map(features: &[(String, Entity)]) -> BTreeMap<&str, &Entity> {
    features
        .iter()
        .map(|(id, entity)| (id.as_str(), entity))
        .collect()
}

fn diameter(feature: &Entity) -> Option<PositiveReal> {
    (crate::swift::short_class(&feature.class) == "GdtCylinder")
        .then(|| nominal_radius(feature, "NomCylinder"))
        .flatten()
        .and_then(|radius| PositiveReal::new(radius.get() * 2.0))
}

/// The per-path diameter walk the memoized walk replaces.
fn per_path_diameters(
    index: &BTreeMap<&str, &Entity>,
    id: &str,
    visited: &mut BTreeSet<String>,
    depth: usize,
    values: &mut Vec<PositiveReal>,
) {
    if visited.contains(id) {
        return;
    }
    let Some(child_depth) = depth.checked_add(1).filter(|child| *child <= MAX_DEPTH) else {
        return;
    };
    visited.insert(id.to_owned());
    if let Some(feature) = index.get(id) {
        if let Some(value) = diameter(feature) {
            values.push(value);
        } else {
            for child in child_feature_ids(feature) {
                per_path_diameters(index, child, visited, child_depth, values);
            }
        }
    }
    visited.remove(id);
}

/// The per-path measurement walk the memoized walk replaces.
fn per_path_measurement(
    index: &BTreeMap<&str, &Entity>,
    id: &str,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<PositiveReal> {
    if visited.contains(id) {
        return None;
    }
    let child_depth = depth.checked_add(1).filter(|child| *child <= MAX_DEPTH)?;
    visited.insert(id.to_owned());
    let result = (|| {
        let feature = index.get(id)?;
        if let Some(value) = diameter(feature) {
            return Some(value);
        }
        let mut first: Option<PositiveReal> = None;
        for child in child_feature_ids(feature) {
            let Some(value) = per_path_measurement(index, child, visited, child_depth) else {
                continue;
            };
            match first {
                Some(prior) if !approximately_equal(value.get(), prior.get()) => return None,
                Some(_) => {}
                None => first = Some(value),
            }
        }
        first
    })();
    visited.remove(id);
    result
}

fn first_occurrences(values: &[PositiveReal]) -> Vec<PositiveReal> {
    let mut unique = Vec::new();
    for value in values {
        if !unique.contains(value) {
            unique.push(*value);
        }
    }
    unique
}

#[test]
fn diameter_walk_keeps_the_per_path_first_visit_order() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let features = layered(SHALLOW_LAYERS, 1.0, 0.5, true);
    let index = FeatureIndex::new(map(&features));
    let Walk::Done(values) =
        crate::swift::collect_diameter_contributors(&ctx, "a0", &index).unwrap()
    else {
        panic!("the shallow walk completes");
    };
    let mut expected = Vec::new();
    per_path_diameters(
        &map(&features),
        "a0",
        &mut BTreeSet::new(),
        0,
        &mut expected,
    );
    assert!(expected.len() > values.len());
    assert_eq!(first_occurrences(&values), first_occurrences(&expected));
}

#[test]
fn diameter_walk_over_doubling_paths_stays_linear() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let features = layered(DEEP_LAYERS, 1.0, 0.0, false);
    let index = FeatureIndex::new(map(&features));
    let two = PositiveReal::new(2.0).unwrap();
    assert_eq!(
        crate::swift::collect_diameter_contributors(&ctx, "a0", &index).unwrap(),
        Walk::Done(vec![two, two])
    );
    assert!(index.work.consumed() < LINEAR_WORK);
    assert_eq!(index.caps(), WalkCaps::default());
}

#[test]
fn reachability_over_doubling_paths_stays_linear() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let features = layered(DEEP_LAYERS, 1.0, 0.0, false);
    let index = FeatureIndex::new(map(&features));
    assert_eq!(
        crate::swift::feature_reaches(&ctx, "a0", "absent", &index).unwrap(),
        Walk::Done(false)
    );
    assert_eq!(
        crate::swift::feature_reaches(&ctx, "a0", &format!("b{DEEP_LAYERS}"), &index).unwrap(),
        Walk::Done(true)
    );
    assert!(index.work.consumed() < LINEAR_WORK);
}

#[test]
fn measurement_walk_matches_the_per_path_walk() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cyclic = layered(SHALLOW_LAYERS, 1.0, 0.0, false);
    cyclic.push((
        "loop".to_owned(),
        hole(&["a1".to_owned(), "loop".to_owned()]),
    ));
    if let Some((_, feature)) = cyclic.iter_mut().find(|(id, _)| id == "a3") {
        feature
            .features
            .references
            .push(reference("a0", "GdtCompoundHole"));
    }
    for features in [
        layered(SHALLOW_LAYERS, 1.0, 0.0, false),
        layered(SHALLOW_LAYERS, 1.0, 0.5, false),
        layered(SHALLOW_LAYERS, 1.0, 0.0, true),
        cyclic,
    ] {
        let index = FeatureIndex::new(map(&features));
        for root in ["a0", "b1", "loop"] {
            assert_eq!(
                crate::swift::measurement_for_feature(&ctx, root, &index, diameter).unwrap(),
                Walk::Done(per_path_measurement(
                    &map(&features),
                    root,
                    &mut BTreeSet::new(),
                    0
                )),
                "root {root}"
            );
        }
    }
}

#[test]
fn measurement_walk_over_doubling_paths_stays_linear() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let features = layered(DEEP_LAYERS, 1.0, 0.0, false);
    let index = FeatureIndex::new(map(&features));
    assert_eq!(
        crate::swift::measurement_for_feature(&ctx, "a0", &index, diameter).unwrap(),
        Walk::Done(PositiveReal::new(2.0))
    );
    assert!(index.work.consumed() < LINEAR_WORK);
}

#[test]
fn pattern_expansion_over_doubling_paths_stays_linear() {
    let mut features = Vec::new();
    for layer in 0..DEEP_LAYERS {
        for side in ["a", "b"] {
            let mut collection = entity("GdtAppliedFeatureCollection");
            for next in ["a", "b"] {
                let mut applied = entity("GdtAppliedFeature");
                let below = layer.checked_add(1).unwrap();
                applied
                    .features
                    .references
                    .push(reference(&format!("{next}{below}"), "GdtPattern"));
                collection.related.push(RelatedObject {
                    name: "Child".into(),
                    class: applied.class.clone(),
                    entity: applied,
                });
            }
            let mut pattern = entity("GdtPattern");
            pattern.related.push(RelatedObject {
                name: "SubFeatures".into(),
                class: collection.class.clone(),
                entity: collection,
            });
            features.push((format!("{side}{layer}"), pattern));
        }
    }
    features.push((format!("a{DEEP_LAYERS}"), cylinder(1.0)));
    features.push((format!("b{DEEP_LAYERS}"), cylinder(1.0)));
    let index = FeatureIndex::new(map(&features));
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut annotation = entity("GdtPosition");
    annotation
        .features
        .references
        .push(reference("a0", "GdtPattern"));
    let targets = crate::swift::targets(&ctx, &annotation, &index, None)
        .unwrap()
        .unwrap();
    let expected = [format!("a{DEEP_LAYERS}"), format!("b{DEEP_LAYERS}")]
        .map(|id| crate::swift::shape_aspect_target(&id).unwrap());
    assert_eq!(targets, expected);
    assert!(index.work.consumed() < LINEAR_WORK);
}

#[test]
fn walk_work_cap_reports_the_unknown() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let features = layered(DEEP_LAYERS, 1.0, 0.0, false);
    let index = FeatureIndex::with_work_limit(map(&features), 4);
    assert_eq!(
        crate::swift::measurement_for_feature(&ctx, "a0", &index, diameter).unwrap(),
        Walk::Capped
    );
    assert_eq!(index.caps(), WalkCaps { depth: 0, work: 1 });
    let [loss] = index.caps().losses(index.work_limit).try_into().unwrap();
    assert_eq!(
        loss.message,
        "1 SWIFT feature walk(s) stopped at the local work cap SWIFT_FEATURE_WALK_WORK \
         (4); the targets or nominal values they feed were left unresolved."
    );
}

#[test]
fn feature_walk_work_scales_with_rows_above_the_floor() {
    use crate::swift::{
        swift_feature_walk_work_limit, SWIFT_FEATURE_WALK_WORK, SWIFT_FEATURE_WALK_WORK_PER_ROW,
    };
    let past_floor = SWIFT_FEATURE_WALK_WORK / SWIFT_FEATURE_WALK_WORK_PER_ROW * 4;
    assert_eq!(
        swift_feature_walk_work_limit(Some(10)),
        SWIFT_FEATURE_WALK_WORK
    );
    assert_eq!(swift_feature_walk_work_limit(None), SWIFT_FEATURE_WALK_WORK);
    assert_eq!(
        swift_feature_walk_work_limit(Some(past_floor)),
        past_floor * SWIFT_FEATURE_WALK_WORK_PER_ROW
    );
    assert_eq!(
        swift_feature_walk_work_limit(Some(usize::MAX)),
        SWIFT_FEATURE_WALK_WORK
    );
}
