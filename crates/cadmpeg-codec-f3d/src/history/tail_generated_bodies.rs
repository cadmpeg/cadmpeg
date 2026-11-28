// SPDX-License-Identifier: Apache-2.0
//! Remove Body selections whose tail nodes name history-only bodies.
//!
//! A body that a feature creates and a later feature deletes has a Design
//! body-map pair without a solved body. A whole-body reference tail that names
//! the browser node of such a body selects the body that the creating
//! feature generates.

use std::collections::HashMap;

use super::body_selections::FeatureBodySelectionInputs;
use super::tail_bodies::group_nodes;
use cadmpeg_ir::features::evaluation::FeatureResultTopology;
use cadmpeg_ir::features::selections::{BodySelection, GeneratedBodyRef};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

/// The generated-body selection that the tail nodes of the native body group
/// `group_id` of a Remove Body feature name, with the generating features.
fn generated_selection(
    group_id: &str,
    ordinal: u64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'_>,
    by_native: &HashMap<&str, (FeatureId, u64)>,
    results: &[FeatureResultTopology],
) -> Option<(BodySelection, Vec<FeatureId>)> {
    let (native, nodes) = group_nodes(group_id, scope, inputs)?;
    let stream = crate::ids::native_stream(&scope.id)?;
    let mut refs = Vec::new();
    let mut generators = Vec::new();
    for node in nodes {
        let mut pairs = inputs.body_bindings.iter().filter(|pair| {
            pair.entity_suffix == u64::from(node.body_entity_suffix)
                && crate::ids::native_stream(pair.id()) == Some(stream)
        });
        let (Some(pair), None) = (pairs.next(), pairs.next()) else {
            return None;
        };
        if !pair.history_only || pair.body.is_some() {
            return None;
        }
        let mut creators = inputs.scopes.iter().filter(|candidate| {
            candidate.record_index == node.creating_scope_record_index
                && crate::ids::native_stream(&candidate.id) == Some(stream)
        });
        let (Some(creator), None) = (creators.next(), creators.next()) else {
            return None;
        };
        let (generator, generator_ordinal) = by_native.get(creator.id.as_str())?;
        if *generator_ordinal >= ordinal {
            return None;
        }
        // A generating feature whose result state lists one body generates
        // that body; one that lists several names none of them by the pair.
        let local_id = match results.iter().find(|state| state.output_of == *generator) {
            Some(state) => match state.bodies() {
                [only] => only.as_str().to_owned(),
                _ => return None,
            },
            None => pair.id().clone(),
        };
        refs.push(GeneratedBodyRef::new(generator.clone(), local_id).ok()?);
        generators.push(generator.clone());
    }
    Some((BodySelection::generated(refs, native).ok()?, generators))
}

/// Bind the Remove Body selections that name history-only bodies, after the
/// result states of the generating features are known.
pub(crate) fn bind(
    features: &mut [Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
    results: &[FeatureResultTopology],
) {
    let by_native = features
        .iter()
        .filter_map(|feature| {
            Some((
                feature.native_ref.as_deref()?,
                (feature.id.clone(), feature.ordinal),
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut bound = Vec::new();
    for (index, feature) in features.iter().enumerate() {
        let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(group),
            ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut scopes = inputs.scopes.iter().filter(|scope| scope.id == native_ref);
        let (Some(scope), None) = (scopes.next(), scopes.next()) else {
            continue;
        };
        if let Some(selection) =
            generated_selection(group, feature.ordinal, scope, inputs, &by_native, results)
        {
            bound.push((index, selection));
        }
    }
    for (index, (selection, generators)) in bound {
        let feature = &mut features[index];
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) =
                definition
            {
                *bodies = selection;
            }
        });
        for generator in generators {
            feature.dependencies.insert(generator);
        }
    }
}
