// SPDX-License-Identifier: Apache-2.0
//! Separate structural feature references from derived expression edges.

use crate::native::PropertyRecord;
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    features::{FeatureId, FeatureOperation},
    schema::rewrite::typed::RewriteIdentities,
    CadIr,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn structural(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    properties: &[PropertyRecord],
) -> Result<(), CodecError> {
    let known: BTreeSet<_> = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect();
    let owners: BTreeMap<_, _> = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .native_ref
                .as_deref()
                .map(|native| (native.to_owned(), feature.id.clone()))
        })
        .collect();
    let parameter_owners: BTreeMap<_, _> = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect();
    let mut formula_edges = BTreeMap::<FeatureId, BTreeSet<FeatureId>>::new();
    for parameter in &ir.model.parameters {
        let Some(owner) = &parameter.owner else {
            continue;
        };
        for dependency in parameter
            .expression
            .dependencies()
            .filter_map(|id| parameter_owners.get(id).and_then(Option::as_ref))
        {
            formula_edges
                .entry(owner.clone())
                .or_default()
                .insert(dependency.clone());
        }
    }
    for feature in &mut ir.model.features {
        let mut explicit = BTreeSet::new();
        feature
            .evaluation
            .definition()
            .visit_identity_references(ctx, &mut |reference| {
                if let Some(id) = known.iter().find(|id| id.as_str() == reference) {
                    explicit.insert(id.clone());
                }
                if let Some(property) = properties.iter().find(|property| property.id == reference)
                {
                    for target in property
                        .links()
                        .iter()
                        .flatten()
                        .filter_map(|link| link.object())
                    {
                        if let Some(id) = owners.get(target) {
                            explicit.insert(id.clone());
                        }
                    }
                }
                Ok(())
            })?;
        let native = matches!(
            feature.evaluation.definition().operation(),
            FeatureOperation::Native { .. }
        );
        // A native definition's referenced typed object owns its structural
        // native links. Expression-only dependencies are still derived once.
        if native {
            if let Some(object) = &feature.native_ref {
                for property in properties
                    .iter()
                    .filter(|property| &property.owner == object)
                {
                    for target in property
                        .links()
                        .iter()
                        .flatten()
                        .filter_map(|link| link.object())
                    {
                        if let Some(id) = owners.get(target) {
                            explicit.insert(id.clone());
                        }
                    }
                }
            }
        }
        let retained = feature
            .dependencies
            .iter()
            .filter(|dependency| {
                explicit.contains(*dependency)
                    || (native
                        && !formula_edges
                            .get(&feature.id)
                            .is_some_and(|edges| edges.contains(*dependency)))
            })
            .cloned()
            .collect::<Vec<_>>();
        feature.dependencies = retained.try_into().map_err(CodecError::malformed)?;
    }
    Ok(())
}
