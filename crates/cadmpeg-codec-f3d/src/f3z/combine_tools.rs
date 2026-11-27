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
use crate::records::feature::combine::{
    DesignCombineLegacyExternalBodyIdentity, DesignCombineOperation,
};
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
        let Some(operation) = scopes.get(native).and_then(|scope| {
            scope
                .combine_operation()
                .filter(|operation| {
                    operation
                        .tools
                        .iter()
                        .any(|tool| tool.legacy_external_identity.is_some())
                })
                .map(|operation| (scope, operation))
        }) else {
            continue;
        };
        match joins.scope_tools(operation, bodies) {
            Ok(binding) => rewrites.push((index, native.to_owned(), binding)),
            Err(reason) => losses.push(unbound(feature, &reason)),
        }
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
                edited = operands
                    .try_edit(|_, tools| *tools = selection)
                    .map_err(std::string::ToString::to_string);
            }
        });
        if let Err(error) = edited {
            losses.push(unbound(
                feature,
                &format!(
                    "its operands refuse the tools of occurrence {}: {error}",
                    binding.occurrence.as_str()
                ),
            ));
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

fn unbound(feature: &cadmpeg_ir::features::Feature, reason: &str) -> LossNote {
    F3dLossCode::FeatureCombineToolExternalBodyUnbound.note(format!(
        "Combine {}: {reason}; its legacy cross-document tools stay local",
        feature.id.as_str()
    ))
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
    /// bodies of the tools in tool order, when `bodies` are the local bodies
    /// of the tool selectors.
    fn scope_tools(
        &self,
        (scope, operation): (&DesignParameterScope, &DesignCombineOperation),
        bodies: &[String],
    ) -> Result<ToolBinding, String> {
        let mut identities = Vec::new();
        for tool in operation.tools.iter() {
            match (&tool.external_identity, &tool.legacy_external_identity) {
                (None, Some(identity)) => identities.push(identity),
                _ => {
                    return Err(format!(
                    "tool record {} stores a selector other than a legacy cross-document selector",
                    tool.record_index
                ))
                }
            }
        }
        let expected = identities
            .iter()
            .map(|identity| {
                qualify_like(
                    &crate::ids::neutral_combine_legacy_external_body_id(identity),
                    &scope.id,
                )
            })
            .collect::<Vec<_>>();
        if expected != bodies {
            return Err(
                "its local tools differ from the bodies that its legacy selectors name".to_owned(),
            );
        }
        let mut binding: Option<ToolBinding> = None;
        for identity in identities {
            let (occurrence, body) = self.tool(scope, identity)?;
            match &mut binding {
                None => {
                    binding = Some(ToolBinding {
                        occurrence,
                        bodies: vec![body],
                    });
                }
                Some(found) if found.occurrence != occurrence => {
                    return Err(format!(
                        "its tools name the occurrences {} and {}",
                        found.occurrence.as_str(),
                        occurrence.as_str()
                    ));
                }
                Some(found) if found.bodies.contains(&body) => {
                    return Err(format!("two tools name the body {}", body.as_str()));
                }
                Some(found) => found.bodies.push(body),
            }
        }
        binding.ok_or_else(|| "its scope stores no tool".to_owned())
    }

    fn tool(
        &self,
        scope: &DesignParameterScope,
        identity: &DesignCombineLegacyExternalBodyIdentity,
    ) -> Result<(OccurrenceId, BodyId), String> {
        let archive_scope = qualifier(&scope.id);
        let reference = one(
            self.references.iter().filter(|reference| {
                qualifier(&reference.id) == archive_scope
                    && reference.placement_source.as_ref().is_some_and(|source| {
                        crate::xref::legacy_source_occurrence_identity(&source.frame_bytes)
                            == Some(identity.source_occurrence_identity)
                    })
            }),
            |count| {
                format!(
                    "{count} occurrence placements of its document store the source occurrence identity {}",
                    identity.source_occurrence_identity
                )
            },
        )?;
        let occurrence = one(
            self.model.occurrences.iter().filter(|occurrence| {
                occurrence.native_ref.as_deref() == Some(reference.id.as_str())
            }),
            |count| format!("{count} occurrences project the reference {}", reference.id),
        )?;
        let definition = one(
            self.model
                .product_definitions
                .iter()
                .filter(|definition| definition.id == occurrence.prototype),
            |count| {
                format!(
                    "{count} product definitions have the identity {} of the prototype of occurrence {}",
                    occurrence.prototype.as_str(),
                    occurrence.id.as_str()
                )
            },
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
        let body = one(bodies.into_iter(), |count| {
            format!(
                "{count} bodies of {} have the browser-node entity {}",
                definition.id.as_str(),
                identity.browser_node_entity
            )
        })?;
        Ok((occurrence.id.clone(), body.clone()))
    }
}

/// The sole item of `items`, or the `reason` for their count.
fn one<T>(
    items: impl Iterator<Item = T>,
    reason: impl FnOnce(usize) -> String,
) -> Result<T, String> {
    let mut items = items.collect::<Vec<_>>();
    if items.len() == 1 {
        if let Some(item) = items.pop() {
            return Ok(item);
        }
    }
    Err(reason(items.len()))
}

#[cfg(test)]
mod tests;
