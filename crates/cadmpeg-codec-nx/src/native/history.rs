// SPDX-License-Identifier: Apache-2.0
//! Neutral feature-history state derived from NX body lineage.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::BodyId;

/// Ordered feature writers indexed by both native history identity and the
/// neutral body identity established by projection.
#[derive(Default)]
pub(crate) struct BodyWriterHistory {
    native: BTreeMap<u32, FeatureId>,
    outputs: BTreeMap<BodyId, FeatureId>,
}

impl BodyWriterHistory {
    pub(crate) fn native_writer(&self, body: u32) -> Option<&FeatureId> {
        self.native.get(&body)
    }

    /// Return whether a retained history feature already writes one of the
    /// selected bodies. The provisional retained-history input is excluded
    /// because segment-backed body images exist before feature replay but are
    /// not feature writers.
    pub(crate) fn has_preceding_writer(
        &self,
        provisional_feature: Option<&FeatureId>,
        native_body: Option<u32>,
        outputs: &[BodyId],
    ) -> bool {
        outputs.iter().any(|output| {
            self.outputs
                .get(output)
                .is_some_and(|writer| Some(writer) != provisional_feature)
        }) || native_body.is_some_and(|body| self.native.contains_key(&body))
    }

    pub(crate) fn extend_primary_dependencies(
        &self,
        provisional_feature: Option<&FeatureId>,
        native_body: Option<u32>,
        outputs: &[BodyId],
        dependencies: &mut Vec<FeatureId>,
    ) {
        let mut has_output_writer = false;
        for output in outputs {
            if let Some(writer) = self.outputs.get(output) {
                if Some(writer) == provisional_feature {
                    continue;
                }
                has_output_writer = true;
                if !dependencies.contains(writer) {
                    dependencies.push(writer.clone());
                }
            }
        }
        if !has_output_writer {
            if let Some(writer) = native_body.and_then(|body| self.native.get(&body)) {
                if !dependencies.contains(writer) {
                    dependencies.push(writer.clone());
                }
            }
        }
    }

    pub(crate) fn record_writer(
        &mut self,
        native_body: Option<u32>,
        outputs: &[BodyId],
        feature: &FeatureId,
    ) {
        if let Some(body) = native_body {
            self.native.insert(body, feature.clone());
        }
        for output in outputs {
            self.outputs.insert(output.clone(), feature.clone());
        }
    }

    /// Retract provisional output ownership when a later construction record
    /// proves that the body did not exist at the start of retained replay.
    pub(crate) fn retract_outputs(&mut self, feature: &FeatureId, outputs: &[BodyId]) {
        for output in outputs {
            if self.outputs.get(output) == Some(feature) {
                self.outputs.remove(output);
            }
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_output_identity_closes_lineage_across_native_identities() {
        let body = BodyId("body".into());
        let first = FeatureId("first".into());
        let second = FeatureId("second".into());
        let mut history = BodyWriterHistory::default();
        history.record_writer(Some(7), std::slice::from_ref(&body), &first);

        let mut dependencies = Vec::new();
        history.extend_primary_dependencies(
            None,
            Some(8),
            std::slice::from_ref(&body),
            &mut dependencies,
        );

        assert_eq!(dependencies, [first]);
        assert!(history.native_writer(8).is_none());
        history.record_writer(Some(8), std::slice::from_ref(&body), &second);
        assert_eq!(history.native_writer(8), Some(&second));
        dependencies.clear();
        history.extend_primary_dependencies(None, Some(7), &[body], &mut dependencies);
        assert_eq!(dependencies, [second]);

        dependencies.clear();
        history.extend_primary_dependencies(None, Some(7), &[], &mut dependencies);
        assert_eq!(dependencies, [FeatureId("first".into())]);
    }

    #[test]
    fn provisional_output_writer_can_be_retracted_without_affecting_other_writers() {
        let provisional = FeatureId("provisional".into());
        let retained = FeatureId("retained".into());
        let created = BodyId("created".into());
        let existing = BodyId("existing".into());
        let mut history = BodyWriterHistory::default();
        history.record_writer(None, &[created.clone(), existing.clone()], &provisional);
        history.record_writer(Some(7), std::slice::from_ref(&existing), &retained);

        assert!(!history.has_preceding_writer(
            Some(&provisional),
            None,
            std::slice::from_ref(&created)
        ));
        assert!(history.has_preceding_writer(
            Some(&provisional),
            Some(7),
            std::slice::from_ref(&existing)
        ));

        let mut dependencies = Vec::new();
        history.extend_primary_dependencies(
            Some(&provisional),
            Some(7),
            std::slice::from_ref(&existing),
            &mut dependencies,
        );
        assert_eq!(dependencies, [retained]);

        let mut dependencies = Vec::new();
        history.extend_primary_dependencies(
            Some(&provisional),
            Some(7),
            std::slice::from_ref(&created),
            &mut dependencies,
        );
        assert_eq!(dependencies, [FeatureId("retained".into())]);

        history.retract_outputs(&provisional, &[created.clone(), existing.clone()]);

        assert!(!history.has_preceding_writer(Some(&provisional), None, &[created]));
        assert!(history.has_preceding_writer(Some(&provisional), Some(7), &[existing]));
    }
}
