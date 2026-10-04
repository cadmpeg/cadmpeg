// SPDX-License-Identifier: Apache-2.0
//! Neutral feature-history state derived from NX body lineage.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::ids::BodyId;

/// Source property on the retained-history input that admits the native
/// primary-body active-state witness.
pub(super) const NATIVE_PRIMARY_BODY_CLOSURE_WITNESS: &str = "native_primary_body_closure_witness";
/// Source property carrying an admitted native primary-body object index.
pub(super) const NATIVE_PRIMARY_BODY_OBJECT_INDEX: &str = "primary_body_object_index";

/// Ordered feature writers indexed by both native history identity and the
/// neutral body identity established by projection.
#[derive(Default)]
pub(super) struct BodyWriterHistory {
    native: BTreeMap<u32, FeatureId>,
    offset_store: BTreeMap<String, FeatureId>,
    outputs: BTreeMap<BodyId, FeatureId>,
}

impl BodyWriterHistory {
    pub(super) fn native_writer(
        &self,
        ctx: &DecodeContext<'_>,
        body: u32,
    ) -> Result<Option<&FeatureId>, CodecError> {
        ctx.get_btree_map(&self.native, &body, "NX native body writer history")
    }

    pub(super) fn offset_store_writer(
        &self,
        ctx: &DecodeContext<'_>,
        data_block: &str,
    ) -> Result<Option<&FeatureId>, CodecError> {
        ctx.get_btree_map(
            &self.offset_store,
            data_block,
            "NX offset-store writer history",
        )
    }

    /// The feature writing one neutral output, unless it is the provisional
    /// retained-history input.
    fn output_writer(
        &self,
        ctx: &DecodeContext<'_>,
        provisional_feature: Option<&FeatureId>,
        output: &BodyId,
    ) -> Result<Option<&FeatureId>, CodecError> {
        let Some(writer) =
            ctx.get_btree_map(&self.outputs, output, "NX neutral body writer history")?
        else {
            return Ok(None);
        };
        if let Some(provisional) = provisional_feature {
            if ctx.equal(writer, provisional, "NX provisional body writer")? {
                return Ok(None);
            }
        }
        Ok(Some(writer))
    }

    /// Return whether a retained history feature already writes one of the
    /// selected bodies. The provisional retained-history input is excluded
    /// because segment-backed body images exist before feature replay but are
    /// not feature writers.
    pub(super) fn has_preceding_writer(
        &self,
        ctx: &DecodeContext<'_>,
        provisional_feature: Option<&FeatureId>,
        native_body: Option<u32>,
        offset_store_body: Option<&str>,
        outputs: &[BodyId],
    ) -> Result<bool, CodecError> {
        if ctx.any_by(
            outputs,
            |output| {
                Ok(self
                    .output_writer(ctx, provisional_feature, output)?
                    .is_some())
            },
            "NX neutral body writer history",
        )? {
            return Ok(true);
        }
        if let Some(body) = native_body {
            if self.native_writer(ctx, body)?.is_some() {
                return Ok(true);
            }
        }
        match offset_store_body {
            Some(body) => Ok(self.offset_store_writer(ctx, body)?.is_some()),
            None => Ok(false),
        }
    }

    pub(super) fn extend_primary_dependencies(
        &self,
        ctx: &DecodeContext<'_>,
        provisional_feature: Option<&FeatureId>,
        native_body: Option<u32>,
        offset_store_body: Option<&str>,
        outputs: &[BodyId],
        dependencies: &mut Vec<FeatureId>,
    ) -> Result<(), CodecError> {
        let mut append = |writer: &FeatureId| -> Result<(), CodecError> {
            if !ctx.contains(dependencies, writer, "NX primary writer dependencies")? {
                ctx.push_vec(
                    dependencies,
                    writer.try_clone_for_decode(ctx, "NX primary writer dependencies")?,
                    "NX primary writer dependencies",
                )?;
            }
            Ok(())
        };
        let mut has_output_writer = false;
        for output in ctx.admit_iter(outputs, "NX neutral body writer history")? {
            if let Some(writer) = self.output_writer(ctx, provisional_feature, output)? {
                has_output_writer = true;
                append(writer)?;
            }
        }
        if has_output_writer {
            return Ok(());
        }
        let native = match native_body {
            Some(body) => self.native_writer(ctx, body)?,
            None => None,
        };
        if let Some(writer) = native {
            append(writer)?;
        } else if let Some(body) = offset_store_body {
            if let Some(writer) = self.offset_store_writer(ctx, body)? {
                append(writer)?;
            }
        }
        Ok(())
    }

    pub(super) fn record_writer(
        &mut self,
        ctx: &DecodeContext<'_>,
        native_body: Option<u32>,
        offset_store_body: Option<&str>,
        outputs: &[BodyId],
        feature: &FeatureId,
    ) -> Result<(), CodecError> {
        if let Some(body) = native_body {
            ctx.insert_btree_map(
                &mut self.native,
                body,
                feature.try_clone_for_decode(ctx, "NX native body writer history")?,
                "NX native body writer history",
            )?;
        }
        if let Some(data_block) = offset_store_body {
            ctx.insert_btree_map(
                &mut self.offset_store,
                ctx.copy_retained_text(data_block, "NX offset-store writer history")?,
                feature.try_clone_for_decode(ctx, "NX offset-store writer history")?,
                "NX offset-store writer history",
            )?;
        }
        for output in ctx.admit_iter(outputs, "NX neutral body writer history")? {
            ctx.insert_btree_map(
                &mut self.outputs,
                output.try_clone_for_decode(ctx, "NX neutral body writer history")?,
                feature.try_clone_for_decode(ctx, "NX neutral body writer history")?,
                "NX neutral body writer history",
            )?;
        }
        Ok(())
    }

    /// Retract provisional output ownership when a later construction record
    /// proves that the body did not exist at the start of retained replay.
    pub(super) fn retract_outputs(
        &mut self,
        ctx: &DecodeContext<'_>,
        feature: &FeatureId,
        outputs: &[BodyId],
    ) -> Result<(), CodecError> {
        for output in ctx.admit_iter(outputs, "NX retracted body writers")? {
            let Some(writer) =
                ctx.get_btree_map(&self.outputs, output, "NX retracted body writers")?
            else {
                continue;
            };
            if ctx.equal(writer, feature, "NX retracted body writers")? {
                ctx.remove_btree_map(&mut self.outputs, output, "NX retracted body writers")?;
            }
        }
        Ok(())
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
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
        )
    });
    let has_native_body_witness = active_features.values().any(|(_, feature)| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
        ) && feature.evaluation.outputs().len() == active_bodies.len()
            && feature.evaluation.outputs().iter().collect::<BTreeSet<_>>() == active_bodies
            && feature
                .source_properties
                .contains_key(NATIVE_PRIMARY_BODY_CLOSURE_WITNESS)
    });
    let has_retained_history_input = active_features.values().any(|(_, feature)| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
        ) && feature
            .source_properties
            .keys()
            .any(|key| key.as_str().starts_with("segment_body_binding."))
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

/// Resolve the active feature closure while accounting for decode scratch and
/// the returned identities. The CADIR evaluator uses the context-free form.
pub(crate) fn active_feature_closure_for_decode(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bodies: &[BodyId],
) -> Result<Result<BTreeMap<FeatureId, usize>, ActiveFeatureClosureRejection>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "NX active feature closure scratch")?;
    let mut features = BTreeMap::new();
    for (index, feature) in ctx
        .admit_iter(&ir.model.features, "NX active feature identity index")?
        .enumerate()
    {
        let previous = scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut features,
                &feature.id,
                (index, feature),
                "NX active feature identity index",
            )
        })?;
        if previous.is_some() {
            return Ok(Err(
                ActiveFeatureClosureRejection::DuplicateFeatureIdentity {
                    feature: feature
                        .id
                        .try_clone_for_decode(ctx, "NX active feature closure identity")?,
                },
            ));
        }
    }
    let mut active_bodies = BTreeSet::new();
    for body in ctx.admit_iter(bodies, "NX active bodies")? {
        scratch
            .with_storage(|| ctx.insert_btree_set(&mut active_bodies, body, "NX active bodies"))?;
    }
    let is_active_body =
        |body: &BodyId| ctx.contains_btree_set(&active_bodies, body, "NX active body lookup");
    let mut active_features = BTreeMap::new();
    for (&id, &resolved) in ctx.admit_iter(&features, "NX active body writer lookup")? {
        if ctx.any_by(
            resolved.1.evaluation.outputs(),
            is_active_body,
            "NX active body writer lookup",
        )? {
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut active_features,
                    id,
                    resolved,
                    "NX active feature writers",
                )
            })?;
        }
    }
    let mut has_neutral_body_writer = false;
    let mut has_native_body_witness = false;
    let mut has_retained_history_input = false;
    for &(_, feature) in ctx
        .admit_iter(&active_features, "NX active feature writer roles")?
        .map(|(_, resolved)| resolved)
    {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
        ) {
            has_neutral_body_writer = true;
            continue;
        }
        let outputs = feature.evaluation.outputs();
        has_native_body_witness = has_native_body_witness
            || (outputs.len() == active_bodies.len()
                && ctx.all_by(outputs, is_active_body, "NX active body lookup")?
                && ctx.contains_key_btree_map(
                    &feature.source_properties,
                    NATIVE_PRIMARY_BODY_CLOSURE_WITNESS,
                    "NX native body closure witness",
                )?);
        if !has_retained_history_input {
            for key in ctx
                .admit_iter(&feature.source_properties, "NX retained history input")?
                .map(|(key, _)| key)
            {
                if ctx.starts_with(
                    key.as_str(),
                    "segment_body_binding.",
                    "NX retained history input",
                )? {
                    has_retained_history_input = true;
                    break;
                }
            }
        }
    }
    if !has_neutral_body_writer && has_retained_history_input && !has_native_body_witness {
        return Ok(Err(ActiveFeatureClosureRejection::NoSelectedBodyWriter));
    }
    if !has_neutral_body_writer && has_native_body_witness {
        for (&id, &resolved) in ctx.admit_iter(&features, "NX native active feature witnesses")? {
            let feature = resolved.1;
            if feature.native_ref.is_none() || feature.source_tag.is_none() {
                continue;
            }
            let has_primary_body = ctx
                .get_btree_map(
                    &feature.source_properties,
                    NATIVE_PRIMARY_BODY_OBJECT_INDEX,
                    "NX native active feature witnesses",
                )?
                .is_some_and(|reference| !reference.is_empty());
            if has_primary_body
                && !ctx.contains_key_btree_map(
                    &active_features,
                    id,
                    "NX native active feature witnesses",
                )?
            {
                scratch.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut active_features,
                        id,
                        resolved,
                        "NX native active feature witnesses",
                    )
                })?;
            }
        }
    }
    if active_features.is_empty() {
        return Ok(Err(ActiveFeatureClosureRejection::NoSelectedBodyWriter));
    }
    let mut pending = Vec::new();
    for &resolved in ctx
        .admit_iter(&active_features, "NX pending active features")?
        .map(|(_, resolved)| resolved)
    {
        scratch
            .with_storage(|| ctx.push_vec(&mut pending, resolved, "NX pending active features"))?;
    }
    loop {
        ctx.charge_work(1, "NX active feature closure traversal")?;
        let Some((_, feature)) = pending.pop() else {
            break;
        };
        for dependency in ctx.admit_iter(
            feature.dependencies.as_slice(),
            "NX active feature dependency lookup",
        )? {
            let Some((&dependency_id, &(index, dependency_feature))) = ctx
                .get_key_value_btree_map(
                    &features,
                    dependency,
                    "NX active feature dependency lookup",
                )?
            else {
                return Ok(Err(ActiveFeatureClosureRejection::MissingDependency {
                    feature: feature
                        .id
                        .try_clone_for_decode(ctx, "NX active feature closure identity")?,
                    dependency: dependency
                        .try_clone_for_decode(ctx, "NX active feature closure identity")?,
                }));
            };
            if dependency_feature.ordinal >= feature.ordinal {
                return Ok(Err(ActiveFeatureClosureRejection::DependencyNotEarlier {
                    feature: feature
                        .id
                        .try_clone_for_decode(ctx, "NX active feature closure identity")?,
                    feature_ordinal: feature.ordinal,
                    dependency: dependency
                        .try_clone_for_decode(ctx, "NX active feature closure identity")?,
                    dependency_ordinal: dependency_feature.ordinal,
                }));
            }
            if !ctx.contains_key_btree_map(
                &active_features,
                dependency_id,
                "NX active feature dependencies",
            )? {
                scratch.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut active_features,
                        dependency_id,
                        (index, dependency_feature),
                        "NX active feature dependencies",
                    )?;
                    ctx.push_vec(
                        &mut pending,
                        (index, dependency_feature),
                        "NX pending active features",
                    )
                })?;
            }
        }
    }
    for &(_, feature) in active_features.values() {
        ctx.charge_work(1, "NX suppressed active features")?;
        if feature.suppressed == Some(true) {
            return Ok(Err(ActiveFeatureClosureRejection::ExplicitlySuppressed {
                feature: feature
                    .id
                    .try_clone_for_decode(ctx, "NX active feature closure identity")?,
            }));
        }
    }
    let mut result = BTreeMap::new();
    for (&id, &(index, _)) in
        ctx.admit_iter(&active_features, "NX active feature closure result")?
    {
        ctx.insert_btree_map(
            &mut result,
            id.try_clone_for_decode(ctx, "NX active feature closure result")?,
            index,
            "NX active feature closure result",
        )?;
    }
    Ok(Ok(result))
}

#[cfg(test)]
mod tests {
    use super::{
        active_feature_closure, active_feature_closure_for_decode, ActiveFeatureClosureRejection,
        BodyWriterHistory, NATIVE_PRIMARY_BODY_CLOSURE_WITNESS, NATIVE_PRIMARY_BODY_OBJECT_INDEX,
    };
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::features::FeatureId;
    use cadmpeg_ir::features::FeatureOperation;
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    use cadmpeg_ir::features::{BodySelection, Feature, FeatureTreeNodeRole};

    #[test]
    fn body_writer_history_refuses_collection_limit() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let feature = FeatureId::mint("synthetic:test:id#writer").unwrap();
                let mut history = BodyWriterHistory::default();
                let error = history
                    .record_writer(ctx, Some(7), None, &[], &feature)
                    .unwrap_err();
                assert!(
                    matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
                );
            },
        );
    }

    #[test]
    fn body_writer_history_refuses_retained_limit() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                let feature = FeatureId::mint("synthetic:test:id#writer").unwrap();
                let mut history = BodyWriterHistory::default();
                let error = history
                    .record_writer(ctx, Some(7), None, &[], &feature)
                    .unwrap_err();
                assert!(
                    matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
                );
            },
        );
    }

    #[test]
    fn primary_writer_dependency_refuses_collection_limit() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 1;
            },
            |ctx| {
                let feature = FeatureId::mint("synthetic:test:id#writer").unwrap();
                let mut history = BodyWriterHistory::default();
                history
                    .record_writer(ctx, Some(7), None, &[], &feature)
                    .unwrap();
                let mut dependencies = Vec::new();
                let error = history
                    .extend_primary_dependencies(ctx, None, Some(7), None, &[], &mut dependencies)
                    .unwrap_err();
                assert!(
                    matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
                );
            },
        );
    }

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
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(
                dependencies,
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
            source_properties: cadmpeg_core::text::named_entries(id, source_properties)
                .expect("the fixture states named properties"),
            source_tag: native.then(|| "NX_OPERATION".to_string()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    role: FeatureTreeNodeRole::History,
                    children: cadmpeg_ir::features::TreeChildren::default(),
                }),
                cadmpeg_ir::features::DistinctMembers::try_from(
                    outputs,
                    &cadmpeg_test_support::service_decode_context(),
                )
                .unwrap(),
            ),
            native_ref: native.then(|| format!("native:{id}")),
        }
    }

    fn closure_ir(features: Vec<Feature>) -> (CadIr, BodyId) {
        let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
        let mut ir = CadIr::empty();
        ir.model.features = features;
        (ir, body)
    }

    fn closure_refusal_for_limit(dimension: ResourceDimension) -> CodecError {
        let (ir, body) = closure_ir(vec![history_feature(
            "synthetic:test:id#writer",
            0,
            Vec::new(),
            vec![BodyId::mint("test:model:entity#body").expect("identity grammar")],
            BTreeMap::new(),
            false,
        )]);

        let adjust: fn(&mut cadmpeg_core::decode::DecodePolicy) = match dimension {
            ResourceDimension::CollectionItems => |policy| {
                policy.limits.max_collection_items = 0;
            },
            ResourceDimension::RetainedBytes => |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            ResourceDimension::MaterializedBytes => |policy| {
                policy.limits.max_materialized_bytes = 0;
            },
            ResourceDimension::WorkUnits => |policy| {
                policy.limits.max_work_units = 0;
            },
            _ => unreachable!("test only covers four closure dimensions"),
        };
        crate::test_support::with_decode_context_over(&[], adjust, |ctx| {
            active_feature_closure_for_decode(ctx, &ir, &[body]).expect_err("closure must refuse")
        })
    }

    #[test]
    fn active_feature_closure_refuses_collection_limit() {
        assert!(
            matches!(closure_refusal_for_limit(ResourceDimension::CollectionItems), CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn active_feature_closure_refuses_retained_limit() {
        assert!(
            matches!(closure_refusal_for_limit(ResourceDimension::RetainedBytes), CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn active_feature_closure_refuses_scoped_limit() {
        assert!(
            matches!(closure_refusal_for_limit(ResourceDimension::MaterializedBytes), CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
    }

    #[test]
    fn active_feature_closure_refuses_work_limit() {
        assert!(
            matches!(closure_refusal_for_limit(ResourceDimension::WorkUnits), CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits)
        );
    }

    #[test]
    fn active_feature_closure_refuses_work_limit_at_pending_pop() {
        let body = BodyId::mint("test:model:entity#body").unwrap();
        let mut ir = CadIr::empty();
        ir.model.features.push(history_feature(
            "synthetic:test:id#writer",
            0,
            Vec::new(),
            vec![body.clone()],
            BTreeMap::new(),
            false,
        ));
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "NX active feature closure traversal",
            |ctx| active_feature_closure_for_decode(ctx, &ir, std::slice::from_ref(&body)),
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "NX active feature closure traversal"
                    && limit.used == limit.limit
                    && limit.additional == 1
        ));
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
        missing.dependencies = cadmpeg_ir::features::DistinctMembers::try_from(
            vec![FeatureId::mint("synthetic:test:id#missing").expect("identity grammar")],
            &cadmpeg_test_support::service_decode_context(),
        )
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
        out_of_order.dependencies = cadmpeg_ir::features::DistinctMembers::try_from(
            vec![FeatureId::mint("synthetic:test:id#dependency").expect("identity grammar")],
            &cadmpeg_test_support::service_decode_context(),
        )
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
        crate::test_support::with_decode_context(|ctx| {
            let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
            let first = FeatureId::mint("synthetic:test:id#first").expect("identity grammar");
            let second = FeatureId::mint("synthetic:test:id#second").expect("identity grammar");
            let mut history = BodyWriterHistory::default();
            history
                .record_writer(ctx, Some(7), None, std::slice::from_ref(&body), &first)
                .expect("admitted writer history");

            let mut dependencies = Vec::new();
            history
                .extend_primary_dependencies(
                    ctx,
                    None,
                    Some(8),
                    None,
                    std::slice::from_ref(&body),
                    &mut dependencies,
                )
                .expect("admitted writer history");

            assert_eq!(dependencies, [first]);
            assert!(history
                .native_writer(ctx, 8)
                .expect("admitted writer lookup")
                .is_none());
            history
                .record_writer(ctx, Some(8), None, std::slice::from_ref(&body), &second)
                .expect("admitted writer history");
            assert_eq!(
                history
                    .native_writer(ctx, 8)
                    .expect("admitted writer lookup"),
                Some(&second)
            );
            dependencies.clear();
            history
                .extend_primary_dependencies(ctx, None, Some(7), None, &[body], &mut dependencies)
                .expect("admitted writer history");
            assert_eq!(dependencies, [second]);

            dependencies.clear();
            history
                .extend_primary_dependencies(ctx, None, Some(7), None, &[], &mut dependencies)
                .expect("admitted writer history");
            assert_eq!(
                dependencies,
                [FeatureId::mint("synthetic:test:id#first").expect("identity grammar")]
            );
        });
    }

    #[test]
    fn provisional_output_writer_can_be_retracted_without_affecting_other_writers() {
        crate::test_support::with_decode_context(|ctx| {
            let provisional =
                FeatureId::mint("synthetic:test:id#provisional").expect("identity grammar");
            let retained = FeatureId::mint("synthetic:test:id#retained").expect("identity grammar");
            let created = BodyId::mint("test:model:entity#created").expect("identity grammar");
            let existing = BodyId::mint("test:model:entity#existing").expect("identity grammar");
            let mut history = BodyWriterHistory::default();
            history
                .record_writer(
                    ctx,
                    None,
                    None,
                    &[created.clone(), existing.clone()],
                    &provisional,
                )
                .expect("admitted writer history");
            history
                .record_writer(
                    ctx,
                    Some(7),
                    None,
                    std::slice::from_ref(&existing),
                    &retained,
                )
                .expect("admitted writer history");

            assert!(!history
                .has_preceding_writer(
                    ctx,
                    Some(&provisional),
                    None,
                    None,
                    std::slice::from_ref(&created)
                )
                .expect("admitted writer lookup"));
            assert!(history
                .has_preceding_writer(
                    ctx,
                    Some(&provisional),
                    Some(7),
                    None,
                    std::slice::from_ref(&existing)
                )
                .expect("admitted writer lookup"));

            let mut dependencies = Vec::new();
            history
                .extend_primary_dependencies(
                    ctx,
                    Some(&provisional),
                    Some(7),
                    None,
                    std::slice::from_ref(&existing),
                    &mut dependencies,
                )
                .expect("admitted writer history");
            assert_eq!(dependencies, [retained]);

            let mut dependencies = Vec::new();
            history
                .extend_primary_dependencies(
                    ctx,
                    Some(&provisional),
                    Some(7),
                    None,
                    std::slice::from_ref(&created),
                    &mut dependencies,
                )
                .expect("admitted writer history");
            assert_eq!(
                dependencies,
                [FeatureId::mint("synthetic:test:id#retained").expect("identity grammar")]
            );

            history
                .retract_outputs(ctx, &provisional, &[created.clone(), existing.clone()])
                .expect("admitted writer retraction");

            assert!(!history
                .has_preceding_writer(ctx, Some(&provisional), None, None, &[created])
                .expect("admitted writer lookup"));
            assert!(history
                .has_preceding_writer(ctx, Some(&provisional), Some(7), None, &[existing])
                .expect("admitted writer lookup"));
        });
    }

    #[test]
    fn exact_offset_store_identity_orders_writers_without_cross_store_aliases() {
        crate::test_support::with_decode_context(|ctx| {
            let first = FeatureId::mint("synthetic:test:id#first").expect("identity grammar");
            let second = FeatureId::mint("synthetic:test:id#second").expect("identity grammar");
            let mut history = BodyWriterHistory::default();
            history
                .record_writer(ctx, None, Some("store-a:block#7"), &[], &first)
                .expect("admitted writer history");

            let mut dependencies = Vec::new();
            history
                .extend_primary_dependencies(
                    ctx,
                    None,
                    None,
                    Some("store-a:block#7"),
                    &[],
                    &mut dependencies,
                )
                .expect("admitted writer history");
            assert_eq!(dependencies, [first]);
            dependencies.clear();
            history
                .extend_primary_dependencies(
                    ctx,
                    None,
                    None,
                    Some("store-b:block#7"),
                    &[],
                    &mut dependencies,
                )
                .expect("admitted writer history");
            assert!(dependencies.is_empty());

            history
                .record_writer(ctx, None, Some("store-a:block#7"), &[], &second)
                .expect("admitted writer history");
            assert_eq!(
                history
                    .offset_store_writer(ctx, "store-a:block#7")
                    .expect("admitted writer lookup"),
                Some(&second)
            );
            dependencies.clear();
            history
                .extend_primary_dependencies(
                    ctx,
                    None,
                    None,
                    Some("store-a:block#7"),
                    &[],
                    &mut dependencies,
                )
                .expect("admitted writer history");
            assert_eq!(dependencies, [second]);
        });
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
                    FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                        bodies: BodySelection::Resolved {
                            bodies: cadmpeg_ir::features::DistinctMembers::try_from(
                                vec![body.clone()],
                                &cadmpeg_test_support::service_decode_context(),
                            )
                            .expect("distinct bodies"),
                            native: "test".into(),
                        },
                    }),
                    cadmpeg_ir::features::DistinctMembers::try_from(
                        vec![body.clone()],
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .unwrap(),
                ),
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
            cadmpeg_core::nonblank_const!(NATIVE_PRIMARY_BODY_CLOSURE_WITNESS),
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
                cadmpeg_core::nonblank_literal!("segment_body_binding.0"),
                "nx:segment-body-bindings:binding#0".into(),
            )]),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                    bodies: BodySelection::Resolved {
                        bodies: cadmpeg_ir::features::DistinctMembers::try_from(
                            vec![body.clone()],
                            &cadmpeg_test_support::service_decode_context(),
                        )
                        .expect("distinct bodies"),
                        native: "test".into(),
                    },
                }),
                cadmpeg_ir::features::DistinctMembers::try_from(
                    vec![body.clone()],
                    &cadmpeg_test_support::service_decode_context(),
                )
                .unwrap(),
            ),
            native_ref: None,
        }]);

        assert_eq!(
            active_feature_closure(&ir, &[body]),
            Err(ActiveFeatureClosureRejection::NoSelectedBodyWriter)
        );
    }
}
