// SPDX-License-Identifier: Apache-2.0
//! Bind a `Combine` tool that names a body of an inserted design to that
//! design's body in the inserting occurrence's frame
//! ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::{BodyId, OccurrenceId};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;
use crate::records::bodies::BodyVisibility;
use crate::records::feature::combine::DesignCombineLegacyExternalBodyIdentity;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::xref::XrefReference;

/// The occurrence and the bodies, in tool order, that the tool selectors name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolBinding {
    occurrence: OccurrenceId,
    bodies: Vec<BodyId>,
}

/// Rewrite each `Combine` tool selection that its legacy cross-document
/// selectors bind to one body of one placed occurrence's prototype.
pub(super) fn bind(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let candidates = local_tool_scopes(&ir.model.features);
    if candidates.is_empty() {
        return Ok(());
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let mut scopes = BTreeMap::new();
    for scope in
        namespace.arena_iter_as_for_decode::<DesignParameterScope>(ctx, "design_parameter_scopes")
    {
        let scope = scope?;
        if candidates.contains(scope.id.as_str()) {
            scopes.insert(scope.id.clone(), scope);
        }
    }
    let references: Vec<XrefReference> = namespace.arena_as_for_decode(ctx, "xref_references")?;
    let visibilities: Vec<BodyVisibility> =
        namespace.arena_as_for_decode(ctx, "body_visibilities")?;
    let joins = Joins {
        model: &ir.model,
        references: &references,
        visibilities: &visibilities,
    };
    let mut rewrites = Vec::new();
    for (index, feature) in ir.model.features.iter().enumerate() {
        let Some((native, bodies)) = local_tools(feature.evaluation.definition()) else {
            continue;
        };
        let Some(scope) = scopes.get(native) else {
            continue;
        };
        let Some(binding) = joins.scope_tools(scope) else {
            continue;
        };
        let expected = scope_tool_bodies(scope);
        if expected.as_deref() != Some(bodies) {
            continue;
        }
        rewrites.push((index, native.to_owned(), binding));
    }
    for (index, native, binding) in rewrites {
        let feature = &mut ir.model.features[index];
        let selection = BodySelection::Occurrence {
            occurrence: binding.occurrence.clone(),
            occurrence_path: Vec::new(),
            bodies: binding
                .bodies
                .clone()
                .try_into()
                .map_err(|_| CodecError::malformed("Combine tool body repeats"))?,
            native,
        };
        let mut edited = Ok(());
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            {
                edited = operands.try_edit(|_, tools| *tools = selection);
            }
        });
        if edited.is_err() {
            continue;
        }
        losses.push(F3dLossCode::FeatureCombineToolExternalFinalStateAssumed.note(format!(
            "Combine {}: the tool body is the final body {} of the design that occurrence {} inserts",
            feature.id.as_str(),
            binding
                .bodies
                .iter()
                .map(BodyId::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            binding.occurrence.as_str(),
        )));
    }
    Ok(())
}

/// The native scope identity of every `Combine` whose tools are
/// feature-input-local bodies.
fn local_tool_scopes(features: &[cadmpeg_ir::features::Feature]) -> BTreeSet<&str> {
    features
        .iter()
        .filter_map(|feature| local_tools(feature.evaluation.definition()))
        .map(|(native, _)| native)
        .collect()
}

fn local_tools(definition: &FeatureDefinition) -> Option<(&str, &[String])> {
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = definition
    else {
        return None;
    };
    match operands.tools() {
        BodySelection::Local { bodies, native } => Some((native.as_str(), bodies.as_slice())),
        _ => None,
    }
}

/// The feature-input-local bodies that the legacy selectors of `scope` give.
fn scope_tool_bodies(scope: &DesignParameterScope) -> Option<Vec<String>> {
    scope
        .combine_operation()?
        .tools
        .iter()
        .map(|tool| {
            tool.legacy_external_identity
                .as_ref()
                .filter(|_| tool.external_identity.is_none())
                .map(|identity| {
                    qualify_like(
                        &crate::ids::neutral_combine_legacy_external_body_id(identity),
                        &scope.id,
                    )
                })
        })
        .collect()
}

/// The archive scope that qualifies an `f3d:` identity, if any.
fn qualifier(id: &str) -> Option<&str> {
    let local = id.strip_prefix(crate::ids::SCHEME_PREFIX)?;
    let end = local.find(['@', ':'])?;
    local[end..].starts_with('@').then(|| &local[..end])
}

/// `id` in the archive scope of `like`.
fn qualify_like(id: &str, like: &str) -> String {
    qualifier(like)
        .and_then(|scope| crate::ids::qualify(id, scope))
        .unwrap_or_else(|| id.to_owned())
}

/// The model and native records that join a legacy selector to its body.
struct Joins<'a> {
    model: &'a cadmpeg_ir::document::Model,
    references: &'a [XrefReference],
    visibilities: &'a [BodyVisibility],
}

impl Joins<'_> {
    /// The one occurrence that every tool of `scope` names, and the distinct
    /// bodies of the tools in tool order.
    fn scope_tools(&self, scope: &DesignParameterScope) -> Option<ToolBinding> {
        let mut binding: Option<ToolBinding> = None;
        for tool in scope.combine_operation()?.tools.iter() {
            if tool.external_identity.is_some() {
                return None;
            }
            let (occurrence, body) = self.tool(scope, tool.legacy_external_identity.as_ref()?)?;
            match &mut binding {
                None => {
                    binding = Some(ToolBinding {
                        occurrence,
                        bodies: vec![body],
                    });
                }
                Some(found) if found.occurrence == occurrence && !found.bodies.contains(&body) => {
                    found.bodies.push(body);
                }
                Some(_) => return None,
            }
        }
        binding
    }

    fn tool(
        &self,
        scope: &DesignParameterScope,
        identity: &DesignCombineLegacyExternalBodyIdentity,
    ) -> Option<(OccurrenceId, BodyId)> {
        let archive_scope = qualifier(&scope.id);
        let reference = unique(self.references.iter().filter(|reference| {
            qualifier(&reference.id) == archive_scope
                && reference
                    .placement_source
                    .as_ref()
                    .and_then(|source| crate::xref::legacy_placement_target(&source.frame_bytes))
                    .is_some_and(|(_, source_occurrence)| {
                        source_occurrence == identity.source_occurrence_identity
                    })
        }))?;
        let occurrence =
            unique(self.model.occurrences.iter().filter(|occurrence| {
                occurrence.native_ref.as_deref() == Some(reference.id.as_str())
            }))?;
        let definition = unique(
            self.model
                .product_definitions
                .iter()
                .filter(|definition| definition.id == occurrence.prototype),
        )?;
        let bodies = self
            .visibilities
            .iter()
            .filter(|visibility| {
                visibility.browser_node_entity == identity.browser_node_entity
                    && definition.bodies.contains(&visibility.body)
            })
            .map(|visibility| &visibility.body)
            .collect::<BTreeSet<_>>();
        let body = unique(bodies.into_iter())?;
        Some((occurrence.id.clone(), body.clone()))
    }
}

fn unique<T>(mut items: impl Iterator<Item = T>) -> Option<T> {
    let first = items.next()?;
    items.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests;
