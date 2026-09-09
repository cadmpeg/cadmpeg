// SPDX-License-Identifier: Apache-2.0
//! Neutral feature-history state derived from NX body lineage.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId};
use cadmpeg_ir::ids::BodyId;

/// Source property on the retained-history input that admits the native
/// primary-body active-state witness.
pub(crate) const NATIVE_PRIMARY_BODY_CLOSURE_WITNESS: &str = "native_primary_body_closure_witness";
/// Source property carrying an admitted native primary-body object index.
pub(crate) const NATIVE_PRIMARY_BODY_OBJECT_INDEX: &str = "primary_body_object_index";

/// Ordered feature writers indexed by both native history identity and the
/// neutral body identity established by projection.
#[derive(Default)]
pub(crate) struct BodyWriterHistory {
    native: BTreeMap<u32, FeatureId>,
    offset_store: BTreeMap<String, FeatureId>,
    outputs: BTreeMap<BodyId, FeatureId>,
}

impl BodyWriterHistory {
    pub(crate) fn native_writer(&self, body: u32) -> Option<&FeatureId> {
        self.native.get(&body)
    }

    pub(crate) fn offset_store_writer(&self, data_block: &str) -> Option<&FeatureId> {
        self.offset_store.get(data_block)
    }

    /// Return whether a retained history feature already writes one of the
    /// selected bodies. The provisional retained-history input is excluded
    /// because segment-backed body images exist before feature replay but are
    /// not feature writers.
    pub(crate) fn has_preceding_writer(
        &self,
        provisional_feature: Option<&FeatureId>,
        native_body: Option<u32>,
        offset_store_body: Option<&str>,
        outputs: &[BodyId],
    ) -> bool {
        outputs.iter().any(|output| {
            self.outputs
                .get(output)
                .is_some_and(|writer| Some(writer) != provisional_feature)
        }) || native_body.is_some_and(|body| self.native.contains_key(&body))
            || offset_store_body.is_some_and(|body| self.offset_store.contains_key(body))
    }

    pub(crate) fn extend_primary_dependencies(
        &self,
        provisional_feature: Option<&FeatureId>,
        native_body: Option<u32>,
        offset_store_body: Option<&str>,
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
            } else if let Some(writer) =
                offset_store_body.and_then(|body| self.offset_store.get(body))
            {
                if !dependencies.contains(writer) {
                    dependencies.push(writer.clone());
                }
            }
        }
    }

    pub(crate) fn record_writer(
        &mut self,
        native_body: Option<u32>,
        offset_store_body: Option<&str>,
        outputs: &[BodyId],
        feature: &FeatureId,
    ) {
        if let Some(body) = native_body {
            self.native.insert(body, feature.clone());
        }
        if let Some(data_block) = offset_store_body {
            self.offset_store
                .insert(data_block.to_string(), feature.clone());
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

/// Reason an active feature closure cannot be formed atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveFeatureClosureRejection {
    /// Two feature records carry the same global identity.
    DuplicateFeatureIdentity { feature: FeatureId },
    /// No feature writes a selected body and no native closure witness applies.
    NoSelectedBodyWriter,
    /// A dependency identity is absent from the feature arena.
    MissingDependency {
        feature: FeatureId,
        dependency: FeatureId,
    },
    /// A dependency is not earlier than its consumer.
    DependencyNotEarlier {
        feature: FeatureId,
        feature_ordinal: u64,
        dependency: FeatureId,
        dependency_ordinal: u64,
    },
    /// A member of the proposed active closure is explicitly suppressed.
    ExplicitlySuppressed { feature: FeatureId },
}

impl ActiveFeatureClosureRejection {
    /// Stable short reason used by decode diagnostics and fleet analysis.
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::DuplicateFeatureIdentity { .. } => "duplicate-feature-identity",
            Self::NoSelectedBodyWriter => "no-selected-body-writer",
            Self::MissingDependency { .. } => "missing-dependency",
            Self::DependencyNotEarlier { .. } => "dependency-not-earlier",
            Self::ExplicitlySuppressed { .. } => "explicitly-suppressed",
        }
    }
}

/// Return the exact dependency closure of the features writing `bodies`.
///
/// The closure exists only when feature identities are unique, every
/// dependency names an earlier feature, at least one feature writes a selected
/// body or has an admitted native primary-body relation, and no member is
/// explicitly suppressed.
pub(crate) fn active_feature_closure(
    ir: &CadIr,
    bodies: &[BodyId],
) -> Result<BTreeMap<FeatureId, usize>, ActiveFeatureClosureRejection> {
    let mut features = BTreeMap::new();
    for (index, feature) in ir.model.features.iter().enumerate() {
        if features
            .insert(feature.id.clone(), (index, feature))
            .is_some()
        {
            return Err(ActiveFeatureClosureRejection::DuplicateFeatureIdentity {
                feature: feature.id.clone(),
            });
        }
    }

    let active_bodies = bodies.iter().collect::<BTreeSet<_>>();
    let mut active_features = features
        .iter()
        .filter(|(_, (_, feature))| {
            feature
                .evaluation
                .outputs()
                .iter()
                .any(|output| active_bodies.contains(output))
        })
        .map(|(id, &resolved)| (id.clone(), resolved))
        .collect::<BTreeMap<_, _>>();
    let has_neutral_body_writer = active_features.values().any(|(_, feature)| {
        !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::BaseFeature { .. }
        )
    });
    let has_native_body_witness = active_features.values().any(|(_, feature)| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::BaseFeature { .. }
        ) && feature.evaluation.outputs().len() == active_bodies.len()
            && feature.evaluation.outputs().iter().collect::<BTreeSet<_>>() == active_bodies
            && feature
                .source_properties
                .contains_key(NATIVE_PRIMARY_BODY_CLOSURE_WITNESS)
    });
    let has_retained_history_input = active_features.values().any(|(_, feature)| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::BaseFeature { .. }
        ) && feature
            .source_properties
            .keys()
            .any(|key| key.starts_with("segment_body_binding."))
    });
    if !has_neutral_body_writer && has_retained_history_input && !has_native_body_witness {
        return Err(ActiveFeatureClosureRejection::NoSelectedBodyWriter);
    }
    if !has_neutral_body_writer && has_native_body_witness {
        active_features.extend(
            features
                .iter()
                .filter(|(_, (_, feature))| {
                    feature.native_ref.is_some()
                        && feature.source_tag.is_some()
                        && feature
                            .source_properties
                            .get(NATIVE_PRIMARY_BODY_OBJECT_INDEX)
                            .is_some_and(|reference| !reference.is_empty())
                })
                .map(|(id, &resolved)| (id.clone(), resolved)),
        );
    }
    if active_features.is_empty() {
        return Err(ActiveFeatureClosureRejection::NoSelectedBodyWriter);
    }

    let mut pending = active_features.values().copied().collect::<Vec<_>>();
    while let Some((_, feature)) = pending.pop() {
        for dependency in &feature.dependencies {
            let Some(&(index, dependency_feature)) = features.get(dependency) else {
                return Err(ActiveFeatureClosureRejection::MissingDependency {
                    feature: feature.id.clone(),
                    dependency: dependency.clone(),
                });
            };
            if dependency_feature.ordinal >= feature.ordinal {
                return Err(ActiveFeatureClosureRejection::DependencyNotEarlier {
                    feature: feature.id.clone(),
                    feature_ordinal: feature.ordinal,
                    dependency: dependency.clone(),
                    dependency_ordinal: dependency_feature.ordinal,
                });
            }
            if active_features
                .insert(dependency.clone(), (index, dependency_feature))
                .is_none()
            {
                pending.push((index, dependency_feature));
            }
        }
    }
    if let Some((_, feature)) = active_features
        .values()
        .find(|(_, feature)| feature.suppressed == Some(true))
    {
        return Err(ActiveFeatureClosureRejection::ExplicitlySuppressed {
            feature: feature.id.clone(),
        });
    }
    Ok(active_features
        .into_iter()
        .map(|(id, (index, _))| (id, index))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    use cadmpeg_ir::features::{BodySelection, Feature, FeatureTreeNodeRole};

    fn history_feature(
        id: &str,
        ordinal: u64,
        dependencies: Vec<FeatureId>,
        outputs: Vec<BodyId>,
        source_properties: BTreeMap<String, String>,
        native: bool,
    ) -> Feature {
        Feature {
            id: FeatureId::mint(id).expect("identity grammar"),
            ordinal,
            name: Some(id.into()),
            suppressed: None,
            dependencies: (dependencies).try_into().unwrap(),
            source_properties,
            source_tag: native.then(|| "NX_OPERATION".to_string()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                FeatureDefinition::TreeNode {
                    role: FeatureTreeNodeRole::History,
                    children: cadmpeg_ir::features::TreeChildren::default(),
                },
                outputs,
            )
            .unwrap(),
            native_ref: native.then(|| format!("native:{id}")),
        }
    }

    fn closure_ir(features: Vec<Feature>) -> (CadIr, BodyId) {
        let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
        let mut ir = CadIr::empty();
        ir.model.features = features;
        (ir, body)
    }

    #[test]
    fn active_feature_closure_retains_arena_positions_instead_of_identity_order() {
        let body = BodyId::mint("test:model:entity#body").unwrap();
        let earlier = history_feature(
            "synthetic:test:id#earlier",
            0,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
            false,
        );
        let later = history_feature(
            "synthetic:test:id#later",
            1,
            vec![earlier.id.clone()],
            vec![body.clone()],
            BTreeMap::new(),
            false,
        );
        let expected = BTreeMap::from([(earlier.id.clone(), 1), (later.id.clone(), 0)]);
        let (ir, _) = closure_ir(vec![later, earlier]);
        assert_eq!(active_feature_closure(&ir, &[body]), Ok(expected));
    }

    #[test]
    fn active_feature_closure_reports_each_atomic_rejection() {
        let writer = || {
            history_feature(
                "synthetic:test:id#writer",
                2,
                Vec::new(),
                vec![BodyId::mint("test:model:entity#body").expect("identity grammar")],
                BTreeMap::new(),
                false,
            )
        };

        let (ir, body) = closure_ir(vec![writer(), writer()]);
        assert_eq!(
            active_feature_closure(&ir, &[body]),
            Err(ActiveFeatureClosureRejection::DuplicateFeatureIdentity {
                feature: FeatureId::mint("synthetic:test:id#writer").expect("identity grammar")
            })
        );

        let mut missing = writer();
        missing.dependencies =
            (vec![FeatureId::mint("synthetic:test:id#missing").expect("identity grammar")])
                .try_into()
                .unwrap();
        let (ir, body) = closure_ir(vec![missing]);
        assert_eq!(
            active_feature_closure(&ir, &[body]),
            Err(ActiveFeatureClosureRejection::MissingDependency {
                feature: FeatureId::mint("synthetic:test:id#writer").expect("identity grammar"),
                dependency: FeatureId::mint("synthetic:test:id#missing").expect("identity grammar")
            })
        );

        let mut out_of_order = writer();
        out_of_order.ordinal = 1;
        out_of_order.dependencies =
            (vec![FeatureId::mint("synthetic:test:id#dependency").expect("identity grammar")])
                .try_into()
                .unwrap();
        let dependency = history_feature(
            "synthetic:test:id#dependency",
            2,
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
            false,
        );
        let (ir, body) = closure_ir(vec![dependency, out_of_order]);
        assert_eq!(
            active_feature_closure(&ir, &[body]),
            Err(ActiveFeatureClosureRejection::DependencyNotEarlier {
                feature: FeatureId::mint("synthetic:test:id#writer").expect("identity grammar"),
                feature_ordinal: 1,
                dependency: FeatureId::mint("synthetic:test:id#dependency")
                    .expect("identity grammar"),
                dependency_ordinal: 2
            })
        );

        let mut suppressed = writer();
        suppressed.suppressed = Some(true);
        let (ir, body) = closure_ir(vec![suppressed]);
        let rejection = active_feature_closure(&ir, &[body]);
        assert_eq!(
            rejection,
            Err(ActiveFeatureClosureRejection::ExplicitlySuppressed {
                feature: FeatureId::mint("synthetic:test:id#writer").expect("identity grammar")
            })
        );
        assert_eq!(rejection.unwrap_err().code(), "explicitly-suppressed");
    }

    #[test]
    fn neutral_output_identity_closes_lineage_across_native_identities() {
        let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
        let first = FeatureId::mint("synthetic:test:id#first").expect("identity grammar");
        let second = FeatureId::mint("synthetic:test:id#second").expect("identity grammar");
        let mut history = BodyWriterHistory::default();
        history.record_writer(Some(7), None, std::slice::from_ref(&body), &first);

        let mut dependencies = Vec::new();
        history.extend_primary_dependencies(
            None,
            Some(8),
            None,
            std::slice::from_ref(&body),
            &mut dependencies,
        );

        assert_eq!(dependencies, [first]);
        assert!(history.native_writer(8).is_none());
        history.record_writer(Some(8), None, std::slice::from_ref(&body), &second);
        assert_eq!(history.native_writer(8), Some(&second));
        dependencies.clear();
        history.extend_primary_dependencies(None, Some(7), None, &[body], &mut dependencies);
        assert_eq!(dependencies, [second]);

        dependencies.clear();
        history.extend_primary_dependencies(None, Some(7), None, &[], &mut dependencies);
        assert_eq!(
            dependencies,
            [FeatureId::mint("synthetic:test:id#first").expect("identity grammar")]
        );
    }

    #[test]
    fn provisional_output_writer_can_be_retracted_without_affecting_other_writers() {
        let provisional =
            FeatureId::mint("synthetic:test:id#provisional").expect("identity grammar");
        let retained = FeatureId::mint("synthetic:test:id#retained").expect("identity grammar");
        let created = BodyId::mint("test:model:entity#created").expect("identity grammar");
        let existing = BodyId::mint("test:model:entity#existing").expect("identity grammar");
        let mut history = BodyWriterHistory::default();
        history.record_writer(
            None,
            None,
            &[created.clone(), existing.clone()],
            &provisional,
        );
        history.record_writer(Some(7), None, std::slice::from_ref(&existing), &retained);

        assert!(!history.has_preceding_writer(
            Some(&provisional),
            None,
            None,
            std::slice::from_ref(&created)
        ));
        assert!(history.has_preceding_writer(
            Some(&provisional),
            Some(7),
            None,
            std::slice::from_ref(&existing)
        ));

        let mut dependencies = Vec::new();
        history.extend_primary_dependencies(
            Some(&provisional),
            Some(7),
            None,
            std::slice::from_ref(&existing),
            &mut dependencies,
        );
        assert_eq!(dependencies, [retained]);

        let mut dependencies = Vec::new();
        history.extend_primary_dependencies(
            Some(&provisional),
            Some(7),
            None,
            std::slice::from_ref(&created),
            &mut dependencies,
        );
        assert_eq!(
            dependencies,
            [FeatureId::mint("synthetic:test:id#retained").expect("identity grammar")]
        );

        history.retract_outputs(&provisional, &[created.clone(), existing.clone()]);

        assert!(!history.has_preceding_writer(Some(&provisional), None, None, &[created]));
        assert!(history.has_preceding_writer(Some(&provisional), Some(7), None, &[existing]));
    }

    #[test]
    fn exact_offset_store_identity_orders_writers_without_cross_store_aliases() {
        let first = FeatureId::mint("synthetic:test:id#first").expect("identity grammar");
        let second = FeatureId::mint("synthetic:test:id#second").expect("identity grammar");
        let mut history = BodyWriterHistory::default();
        history.record_writer(None, Some("store-a:block#7"), &[], &first);

        let mut dependencies = Vec::new();
        history.extend_primary_dependencies(
            None,
            None,
            Some("store-a:block#7"),
            &[],
            &mut dependencies,
        );
        assert_eq!(dependencies, [first]);
        dependencies.clear();
        history.extend_primary_dependencies(
            None,
            None,
            Some("store-b:block#7"),
            &[],
            &mut dependencies,
        );
        assert!(dependencies.is_empty());

        history.record_writer(None, Some("store-a:block#7"), &[], &second);
        assert_eq!(
            history.offset_store_writer("store-a:block#7"),
            Some(&second)
        );
        dependencies.clear();
        history.extend_primary_dependencies(
            None,
            None,
            Some("store-a:block#7"),
            &[],
            &mut dependencies,
        );
        assert_eq!(dependencies, [second]);
    }

    #[test]
    fn native_primary_body_witness_closes_history_without_neutral_outputs() {
        let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
        let dependency = FeatureId::mint("synthetic:test:id#dependency").expect("identity grammar");
        let writer = FeatureId::mint("synthetic:test:id#writer").expect("identity grammar");
        let mut ir = CadIr::empty();
        ir.model.features = vec![
            Feature {
                id: FeatureId::mint("synthetic:test:id#base").expect("identity grammar"),
                ordinal: 0,
                name: Some("base".into()),
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                    FeatureDefinition::BaseFeature {
                        bodies: BodySelection::Resolved {
                            bodies: vec![body.clone()],
                            native: "test".into(),
                        },
                    },
                    vec![body.clone()],
                )
                .unwrap(),
                native_ref: None,
            },
            history_feature(
                "synthetic:test:id#dependency",
                1,
                Vec::new(),
                Vec::new(),
                BTreeMap::new(),
                false,
            ),
            history_feature(
                "synthetic:test:id#writer",
                2,
                vec![dependency.clone()],
                Vec::new(),
                BTreeMap::from([
                    (
                        "primary_body_reference".into(),
                        "nx:feature-history:body-reference#writer".into(),
                    ),
                    (NATIVE_PRIMARY_BODY_OBJECT_INDEX.into(), "7".into()),
                ]),
                true,
            ),
            history_feature(
                "synthetic:test:id#unadmitted",
                3,
                Vec::new(),
                Vec::new(),
                BTreeMap::from([(
                    "primary_body_reference".into(),
                    "nx:feature-history:body-reference#unadmitted".into(),
                )]),
                true,
            ),
        ];
        ir.model.features[0].source_properties.insert(
            NATIVE_PRIMARY_BODY_CLOSURE_WITNESS.into(),
            "primary-body-relations".into(),
        );

        assert_eq!(
            active_feature_closure(&ir, &[body]),
            Ok(BTreeMap::from([
                (
                    FeatureId::mint("synthetic:test:id#base").expect("identity grammar"),
                    0
                ),
                (dependency, 1),
                (writer, 2)
            ]))
        );
        assert_eq!(
            active_feature_closure(
                &ir,
                &[BodyId::mint("test:model:entity#other").expect("identity grammar")]
            ),
            Err(ActiveFeatureClosureRejection::NoSelectedBodyWriter)
        );
    }

    #[test]
    fn retained_history_input_alone_is_not_an_active_feature_closure() {
        let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
        let (ir, _) = closure_ir(vec![Feature {
            id: FeatureId::mint("synthetic:test:id#initial").expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::from([(
                "segment_body_binding.0".into(),
                "nx:segment-body-bindings:binding#0".into(),
            )]),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                FeatureDefinition::BaseFeature {
                    bodies: BodySelection::Resolved {
                        bodies: vec![body.clone()],
                        native: "test".into(),
                    },
                },
                vec![body.clone()],
            )
            .unwrap(),
            native_ref: None,
        }]);

        assert_eq!(
            active_feature_closure(&ir, &[body]),
            Err(ActiveFeatureClosureRejection::NoSelectedBodyWriter)
        );
    }
}
