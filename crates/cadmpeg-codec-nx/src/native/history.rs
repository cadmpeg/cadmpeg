// SPDX-License-Identifier: Apache-2.0
//! Neutral feature-history state derived from NX body lineage.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::BodyId;

/// Return the exact dependency closure of the features writing `bodies`.
///
/// The closure exists only when feature identities are unique, every
/// dependency names an earlier feature, at least one feature writes a selected
/// body, and no member is explicitly suppressed.
pub(crate) fn active_feature_closure(ir: &CadIr, bodies: &[BodyId]) -> Option<BTreeSet<FeatureId>> {
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.clone(), feature))
        .collect::<BTreeMap<_, _>>();
    if features.len() != ir.model.features.len() {
        return None;
    }

    let active_bodies = bodies.iter().collect::<BTreeSet<_>>();
    let mut active_features = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            feature
                .outputs
                .iter()
                .any(|output| active_bodies.contains(output))
        })
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    if active_features.is_empty() {
        return None;
    }

    let mut pending = active_features.iter().cloned().collect::<Vec<_>>();
    while let Some(feature_id) = pending.pop() {
        let feature = features.get(&feature_id)?;
        for dependency in &feature.dependencies {
            let dependency_feature = features.get(dependency)?;
            (dependency_feature.ordinal < feature.ordinal).then_some(())?;
            if active_features.insert(dependency.clone()) {
                pending.push(dependency.clone());
            }
        }
    }
    if active_features
        .iter()
        .any(|id| features[id].suppressed == Some(true))
    {
        return None;
    }
    Some(active_features)
}
