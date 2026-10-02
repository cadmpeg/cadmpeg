// SPDX-License-Identifier: Apache-2.0
//! Structural ownership and predecessor ordering for feature relations.

use std::collections::HashMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::Model;
use crate::features::{FeatureDefinition, FeatureId, FeatureOperation};
use crate::index::{identity_hash, public_result, DecodeStorage, IndexStorage, PublicStorage};

/// A rejected parent relation expressed through borrowed model identities.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ParentError<'a> {
    #[error("feature identity `{owner}` is repeated")]
    Repeated { owner: &'a FeatureId },
    #[error("feature `{owner}` has two tree parents `{first}` and `{second}`")]
    TreeParents { owner: &'a FeatureId, first: &'a FeatureId, second: &'a FeatureId },
    #[error("feature `{owner}` has two regeneration parent entries `{first}` and `{second}`")]
    RegenerationParents { owner: &'a FeatureId, first: &'a FeatureId, second: &'a FeatureId },
    #[error("regeneration relation names missing child feature `{owner}`")]
    MissingChild { owner: &'a FeatureId },
    #[error("tree child `{owner}` is owned by `{parent}` and states no regeneration parent")]
    TreeChild { owner: &'a FeatureId, parent: &'a FeatureId },
    #[error("feature `{owner}` names missing regeneration parent `{parent}`")]
    MissingParent { owner: &'a FeatureId, parent: &'a FeatureId },
    #[error("regeneration parent `{parent}` does not precede child `{owner}`")]
    Order { owner: &'a FeatureId, parent: &'a FeatureId },
}

impl ParentError<'_> {
    pub(crate) fn owner(&self) -> &FeatureId {
        match self {
            Self::Repeated { owner } | Self::TreeParents { owner, .. } | Self::RegenerationParents { owner, .. }
            | Self::MissingChild { owner } | Self::TreeChild { owner, .. } | Self::MissingParent { owner, .. }
            | Self::Order { owner, .. } => owner,
        }
    }
}

/// Check supplied models as one graph under the caller's allocation policy.
/// Serde reconstruction passes no context and uses standard allocation.
pub(crate) fn validate<'a>(
    ctx: Option<&DecodeContext<'_>>,
    models: &[&'a Model],
) -> Result<Result<(), ParentError<'a>>, CodecError> {
    match ctx {
        Some(ctx) => {
            let (result, _storage) = ctx.with_scoped_storage("feature parent validation storage", || {
                validate_graph(models, &DecodeStorage(ctx)).map_err(CodecError::from)
            })?;
            Ok(result)
        }
        None => Ok(public_result(validate_graph(models, &PublicStorage))),
    }
}

/// Scalar hashes select borrowed facts; full identity comparison resolves collisions.
struct ParentFacts<'a, T> {
    entries: HashMap<u64, Vec<(&'a FeatureId, T)>>,
}

impl<'a, T: Copy> ParentFacts<'a, T> {
    fn new() -> Self { Self { entries: HashMap::new() } }

    fn hash<S: IndexStorage>(id: &FeatureId, storage: &S) -> Result<u64, S::Error> {
        storage.work(id.as_str().len(), "feature parent identity hash")?;
        storage.work(1, "feature parent identity hash")?;
        Ok(identity_hash(id.as_str()))
    }

    fn find<S: IndexStorage>(&self, hash: u64, id: &FeatureId, storage: &S) -> Result<Option<T>, S::Error> {
        storage.work(1, "feature parent fact lookup")?;
        for (candidate, value) in self.entries.get(&hash).into_iter().flatten() {
            storage.work(1, "feature parent collision scan")?;
            storage.work(candidate.as_str().len().min(id.as_str().len()), "feature parent identity comparison")?;
            if candidate == &id { return Ok(Some(*value)); }
        }
        Ok(None)
    }

    fn get<S: IndexStorage>(&self, id: &FeatureId, storage: &S) -> Result<Option<T>, S::Error> {
        self.find(Self::hash(id, storage)?, id, storage)
    }

    fn insert<S: IndexStorage>(&mut self, id: &'a FeatureId, value: T, storage: &S) -> Result<Option<T>, S::Error> {
        let hash = Self::hash(id, storage)?;
        if let Some(previous) = self.find(hash, id, storage)? { return Ok(Some(previous)); }
        storage.work(1, "feature parent fact insertion")?;
        storage.entry(&mut self.entries, &hash, "feature parent identities")?;
        storage.push(self.entries.entry(hash).or_default(), (id, value), "feature parent fact slots")?;
        Ok(None)
    }
}

fn validate_graph<'a, S: IndexStorage>(models: &[&'a Model], storage: &S) -> Result<Result<(), ParentError<'a>>, S::Error> {
    let mut features = ParentFacts::new();
    for feature in models.iter().flat_map(|model| &model.features) {
        storage.work(1, "feature parent validation scan")?;
        if features.insert(&feature.id, feature, storage)?.is_some() {
            return Ok(Err(ParentError::Repeated { owner: &feature.id }));
        }
    }
    let mut tree_parents = ParentFacts::new();
    for parent in models.iter().flat_map(|model| &model.features) {
        storage.work(1, "feature tree definition scan")?;
        let FeatureDefinition::Operation(FeatureOperation::TreeNode { children, .. }) = parent.evaluation.definition() else { continue; };
        for child in children {
            storage.work(1, "feature tree parent scan")?;
            if let Some(previous) = tree_parents.insert(child, &parent.id, storage)? {
                return Ok(Err(ParentError::TreeParents { owner: child, first: previous, second: &parent.id }));
            }
        }
    }
    let mut regeneration_parents = ParentFacts::new();
    for (child, parent) in models.iter().flat_map(|model| &model.feature_regeneration_parents.0) {
        storage.work(1, "feature regeneration parent scan")?;
        if let Some(previous) = regeneration_parents.insert(child, parent, storage)? {
            return Ok(Err(ParentError::RegenerationParents { owner: child, first: previous, second: parent }));
        }
        let Some(child_feature) = features.get(child, storage)? else { return Ok(Err(ParentError::MissingChild { owner: child })); };
        if let Some(existing) = tree_parents.get(child, storage)? { return Ok(Err(ParentError::TreeChild { owner: child, parent: existing })); }
        let Some(parent_feature) = features.get(parent, storage)? else { return Ok(Err(ParentError::MissingParent { owner: child, parent })); };
        if parent_feature.ordinal >= child_feature.ordinal { return Ok(Err(ParentError::Order { owner: child, parent })); }
    }
    Ok(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::{validate, FeatureDefinition, FeatureOperation, Model, ParentFacts};
    use crate::index::{identity_hash, DecodeStorage};
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::CodecError;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    fn model() -> Model {
        let mut model = Model::default();
        model.features.push(crate::features::Feature {
            id: "test:parents:feature#one".try_into().unwrap(), ordinal: 0,
            name: None, suppressed: None, dependencies: Default::default(), source_properties: Default::default(),
            source_tag: None, source_text: None, source_content: Default::default(),
            evaluation: crate::features::FeatureEvaluation::from_definition(FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})), native_ref: None,
        });
        model
    }

    #[test]
    fn feature_parent_validation_preserves_caller_refusals_and_serde_reconstruction() {
        let model = model();
        let wire = serde_json::to_value(&model).unwrap();
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let Err(CodecError::ResourceLimit(limit)) = validate(Some(&ctx), &[&model]) else { panic!("parent graph must retain the caller refusal"); };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(serde_json::from_value::<Model>(wire.clone()).unwrap(), model);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
    }

    #[test]
    fn feature_parent_validation_releases_indexes_without_retaining_text() {
        let model = model();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4096;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        validate(Some(&ctx), &[&model]).unwrap().unwrap();
        let storage = ctx.reserve_scoped(4096, "feature parent indexes released").unwrap();
        drop(storage);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn feature_parent_hash_collision_admits_full_identity_comparison() {
        let first: crate::features::FeatureId = "test:parents:feature#first".try_into().unwrap();
        let second: crate::features::FeatureId = "test:parents:feature#other".try_into().unwrap();
        let hash = identity_hash(second.as_str());
        let facts = ParentFacts { entries: std::collections::HashMap::from([(hash, vec![(&first, 7)])]) };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(limit) = facts.find(hash, &second, &DecodeStorage(&ctx)) else { panic!("collision comparison must refuse"); };
        assert_eq!(limit.operation, "feature parent identity comparison");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }

}
