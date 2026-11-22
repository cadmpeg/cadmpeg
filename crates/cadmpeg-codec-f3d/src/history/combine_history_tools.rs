// SPDX-License-Identifier: Apache-2.0
//! Bind the tools of an operand-list `Combine` that build in another component.
//!
//! A tool body that a component builds in its own BREP history is not a body
//! of the history that holds the `Combine` state. Its browser node still names
//! the body-map entity of the tool and the scope that creates it, and the
//! body-map pair of that entity is a history-only pair.

use std::collections::HashMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::evaluation::FeatureResultTopology;
use cadmpeg_ir::features::selections::{BodySelection, GeneratedBodyRef};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;
use crate::records::bodies::DesignBodyBinding;
use crate::records::feature::combine::{DesignCombineBodySelection, DesignCombineForm};
use crate::records::feature::scope::DesignParameterScope;

/// One proven tool: its generating feature and body identity.
struct ResolvedTool {
    generator: FeatureId,
    local_id: String,
}

/// Resolve one tool to the feature that generates its history-only body, or
/// name the reason.
fn resolve_tool(
    tool: &DesignCombineBodySelection,
    stream: Option<&str>,
    combine_ordinal: u64,
    inputs: &Inputs<'_>,
) -> Result<ResolvedTool, &'static str> {
    let Some(node) = tool.body_node else {
        return Err("it stores no browser node");
    };
    let mut pairs = inputs.bindings.iter().filter(|binding| {
        crate::ids::native_stream(binding.id()) == stream
            && binding.entity_suffix == u64::from(node.body_entity_suffix)
    });
    let (Some(pair), None) = (pairs.next(), pairs.next()) else {
        return Err("its body entity has no single body-map pair");
    };
    if !pair.history_only {
        return Err("its body-map pair is not history-only");
    }
    let mut creators = inputs.scopes.iter().filter(|candidate| {
        candidate.record_index == node.creating_scope_record_index
            && crate::ids::native_stream(&candidate.id) == stream
    });
    let (Some(creator), None) = (creators.next(), creators.next()) else {
        return Err("its creating scope is not a single scope");
    };
    let Some(feature) = inputs.by_native.get(creator.id.as_str()) else {
        return Err("its creating scope is not a projected feature");
    };
    if feature.ordinal >= combine_ordinal {
        return Err("its generating feature does not precede the Combine");
    }
    Ok(ResolvedTool {
        generator: feature.id.clone(),
        local_id: pair.id().clone(),
    })
}

/// Resolve every tool of `scope`. Each entry is the proven tool or the reason
/// that it stays native.
fn resolve_tools(
    operation: &crate::records::feature::combine::DesignCombineOperation,
    stream: Option<&str>,
    combine_ordinal: u64,
    inputs: &Inputs<'_>,
) -> Vec<Result<ResolvedTool, &'static str>> {
    let mut resolved = Vec::<Result<ResolvedTool, &'static str>>::new();
    for tool in operation.tools.iter() {
        let mut entry = resolve_tool(tool, stream, combine_ordinal, inputs);
        if let Ok(candidate) = &entry {
            let repeated = resolved.iter().any(|existing| {
                existing
                    .as_ref()
                    .is_ok_and(|existing| existing.local_id == candidate.local_id)
            });
            if repeated {
                entry = Err("another tool names the same body");
            }
        }
        resolved.push(entry);
    }
    resolved
}

struct Inputs<'a> {
    scopes: &'a [DesignParameterScope],
    bindings: &'a [DesignBodyBinding],
    by_native: HashMap<&'a str, &'a Feature>,
}

/// Bind the tools of each operand-list `Combine` that name history-only bodies.
///
/// Every proven tool adds its generating feature to the `Combine`
/// dependencies and records `combine:tool:<ordinal>:generator`; the product
/// definition of that feature owns the tool body. The tools become generated
/// bodies only when every tool is proven.
pub(crate) fn bind(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    bindings: &[DesignBodyBinding],
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let mut updates = Vec::<(usize, Vec<Result<ResolvedTool, &'static str>>)>::new();
    {
        let inputs = Inputs {
            scopes,
            bindings,
            by_native: features
                .iter()
                .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature)))
                .collect(),
        };
        for (index, feature) in features.iter().enumerate() {
            let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                feature.evaluation.definition()
            else {
                continue;
            };
            if !matches!(
                operands.tools(),
                BodySelection::Native(_) | BodySelection::NativeSet(_)
            ) {
                continue;
            }
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let mut matching = scopes.iter().filter(|scope| scope.id == native_ref);
            let (Some(scope), None) = (matching.next(), matching.next()) else {
                continue;
            };
            let Some(operation) = scope.combine_operation() else {
                continue;
            };
            if operation.form != DesignCombineForm::OperandLists {
                continue;
            }
            let stream = crate::ids::native_stream(&scope.id);
            let names_history_only_pair = operation.tools.iter().any(|tool| {
                tool.body_node.is_some_and(|node| {
                    bindings.iter().any(|binding| {
                        binding.history_only
                            && crate::ids::native_stream(binding.id()) == stream
                            && binding.entity_suffix == u64::from(node.body_entity_suffix)
                    })
                })
            });
            if !names_history_only_pair {
                continue;
            }
            updates.push((
                index,
                resolve_tools(operation, stream, feature.ordinal, &inputs),
            ));
        }
    }
    for (index, tools) in updates {
        let feature = &mut features[index];
        for (ordinal, tool) in tools.iter().enumerate() {
            let Ok(tool) = tool else {
                continue;
            };
            insert_property(feature, ordinal, "generator", tool.generator.as_str());
            if tool.generator != feature.id {
                feature.dependencies.insert(tool.generator.clone());
            }
        }
        let unproven = tools
            .iter()
            .enumerate()
            .filter_map(|(ordinal, tool)| {
                tool.as_ref()
                    .err()
                    .map(|reason| format!("tool {ordinal}: {reason}"))
            })
            .collect::<Vec<_>>();
        if !unproven.is_empty() {
            losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(format!(
                "feature {} retains its native Combine tools; {}",
                feature.id,
                unproven.join("; ")
            )));
            continue;
        }
        let Some(native) = feature.native_ref.clone() else {
            continue;
        };
        let refs = tools
            .into_iter()
            .flatten()
            .map(|tool| GeneratedBodyRef::new(tool.generator, tool.local_id))
            .collect::<Result<Vec<_>, _>>();
        let Some(selection) = refs
            .ok()
            .and_then(|refs| BodySelection::generated(refs, native).ok())
        else {
            losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(format!(
                "feature {} retains its native Combine tools: the generated-body selection is not admitted",
                feature.id
            )));
            continue;
        };
        let mut refusal = None;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            else {
                return;
            };
            refusal = operands
                .try_edit(|_, operand_tools| *operand_tools = selection)
                .err();
        });
        if let Some(error) = refusal {
            losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(format!(
                "feature {} retains its native Combine tools: {error}",
                feature.id
            )));
        }
    }
    losses
}

/// Name the generated tools of each operand-list `Combine` in the body
/// identities of the result topology of their generating feature.
///
/// A generated tool initially names the body-map pair of its body. A
/// generating feature with a result topology owns bodies by history body slot
/// instead. The tool then names the one slot of that topology, or the slot
/// that the body-map pair keys when the topology has several. A tool whose
/// slot is not proven, or two tools that share a slot, return the selection
/// to its native form.
pub(crate) fn reconcile_result_topologies(
    features: &mut [Feature],
    topologies: &[FeatureResultTopology],
    bindings: &[DesignBodyBinding],
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for feature in features.iter_mut() {
        let Some(native) = feature.native_ref.clone() else {
            continue;
        };
        let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let BodySelection::Generated { bodies, .. } = operands.tools() else {
            continue;
        };
        let mut renamed = Vec::with_capacity(bodies.len());
        let mut changed = false;
        let mut failure = None;
        for (ordinal, body) in bodies.iter().enumerate() {
            let Some(topology) = topologies
                .iter()
                .find(|topology| topology.output_of == body.feature)
            else {
                renamed.push(body.local_id.as_str().to_owned());
                continue;
            };
            let slots = topology.bodies();
            if slots
                .iter()
                .any(|slot| slot.as_str() == body.local_id.as_str())
            {
                renamed.push(body.local_id.as_str().to_owned());
                continue;
            }
            let keyed = bindings
                .iter()
                .find(|binding| binding.id().as_str() == body.local_id.as_str())
                .map(|binding| binding.asm_body_key.to_string());
            let slot = match (slots, keyed) {
                ([only], _) => Some(only.as_str().to_owned()),
                (_, Some(key)) => slots
                    .iter()
                    .find(|slot| slot.as_str() == key)
                    .map(|slot| slot.as_str().to_owned()),
                _ => None,
            };
            let Some(slot) = slot else {
                failure = Some(format!(
                    "tool {ordinal} names a body of {} that its result topology does not identify",
                    body.feature
                ));
                break;
            };
            changed = true;
            renamed.push(slot);
        }
        if failure.is_none() && changed {
            let distinct = renamed
                .iter()
                .zip(bodies.iter())
                .map(|(slot, body)| (&body.feature, slot))
                .collect::<std::collections::BTreeSet<_>>();
            if distinct.len() != renamed.len() {
                failure = Some("two tools name the same body".to_owned());
            }
        }
        if let Some(reason) = failure {
            let mut refusal = None;
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Combine {
                    operands, ..
                }) = definition
                {
                    refusal = operands
                        .try_edit(|_, tools| *tools = BodySelection::Native(native.clone()))
                        .err();
                }
            });
            losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(format!(
                "feature {} retains its native Combine tools; {reason}{}",
                feature.id,
                refusal.map_or_else(String::new, |error| format!("; {error}"))
            )));
            continue;
        }
        if !changed {
            continue;
        }
        let refs = bodies
            .iter()
            .zip(renamed)
            .map(|(body, slot)| GeneratedBodyRef::new(body.feature.clone(), slot))
            .collect::<Result<Vec<_>, _>>();
        let Some(selection) = refs
            .ok()
            .and_then(|refs| BodySelection::generated(refs, native.clone()).ok())
        else {
            continue;
        };
        let mut refusal = None;
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            {
                refusal = operands.try_edit(|_, tools| *tools = selection).err();
            }
        });
        if let Some(error) = refusal {
            losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(format!(
                "feature {} retains its pair-named Combine tools: {error}",
                feature.id
            )));
        }
    }
    losses
}

/// Record one fact about a proven tool as `combine:tool:<ordinal>:<field>`.
fn insert_property(feature: &mut Feature, ordinal: usize, field: &str, value: &str) {
    if let Some(key) = NonBlankString::new(format!("combine:tool:{ordinal}:{field}")) {
        feature.source_properties.insert(key, value.to_owned());
    }
}

#[cfg(test)]
mod tests;
