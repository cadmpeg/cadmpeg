// SPDX-License-Identifier: Apache-2.0
//! Bind the tools of an operand-list `Combine` that build in another component.
//!
//! A tool body that a component builds in its own BREP history is not a body
//! of the history that holds the `Combine` state. Its browser node still names
//! the body-map entity of the tool and the scope that creates it, and the
//! body-map pair of that entity is a history-only pair, or a live pair when
//! that history keeps the body unchanged after its creation.

use std::collections::HashMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::selections::{BodySelection, GeneratedBodyRef};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

use crate::history_records::AsmHistory;
use crate::loss::F3dLossCode;
use crate::records::bodies::DesignBodyBinding;
use crate::records::feature::combine::{DesignCombineBodySelection, DesignCombineForm};
use crate::records::feature::scope::DesignParameterScope;

/// The body of one proven tool.
#[derive(PartialEq)]
enum ToolBody {
    /// The local identity of a history-only body: the slot that the creating
    /// state of its generating feature inserts, else its body-map pair
    /// identity.
    Generated(String),
    /// The solved model body of a live pair.
    Model(cadmpeg_ir::ids::BodyId),
}

/// One proven tool: its generating feature and body, the result states that
/// name the body, and the reason that a generated body keeps its body-map pair
/// identity.
struct ResolvedTool {
    generator: FeatureId,
    body: ToolBody,
    results: Vec<ToolResultState>,
    pair_identity: Option<&'static str>,
}

/// The result state of a feature that writes a generated tool body: the
/// feature, its history state, and the body slots that its transition inserts
/// or updates.
pub(crate) struct ToolResultState {
    feature: FeatureId,
    state_id: i64,
    slots: Vec<i64>,
}

/// Resolve one tool to the feature that generates its history-only body, or
/// name the reason.
fn resolve_tool(
    tool: &DesignCombineBodySelection,
    stream: Option<&str>,
    (combine_ordinal, combine_history): (u64, Option<&str>),
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
    if !pair.history_only {
        return Ok(ResolvedTool {
            generator: feature.id.clone(),
            body: ToolBody::Model(live_tool_body(
                pair,
                creator,
                (combine_ordinal, combine_history),
                inputs,
            )?),
            results: Vec::new(),
            pair_identity: None,
        });
    }
    Ok(
        match generated_tool_states(creator, feature, combine_ordinal, inputs) {
            Ok((slot, results)) => ResolvedTool {
                generator: feature.id.clone(),
                body: ToolBody::Generated(slot.to_string()),
                results,
                pair_identity: None,
            },
            Err(reason) => ResolvedTool {
                generator: feature.id.clone(),
                body: ToolBody::Generated(pair.id().clone()),
                results: Vec::new(),
                pair_identity: Some(reason),
            },
        },
    )
}

/// The body slot that the creating state of `creator` inserts in its history,
/// with the result states of `generator` and of each feature between it and
/// the `Combine` whose transition in that history updates the slot.
fn generated_tool_states(
    creator: &DesignParameterScope,
    generator: &Feature,
    combine_ordinal: u64,
    inputs: &Inputs<'_>,
) -> Result<(i64, Vec<ToolResultState>), &'static str> {
    let history =
        super::generated_bodies::scope_history(creator, inputs.scope_histories, inputs.histories)
            .ok_or("its creating scope has no one ASM history")?;
    let state_id = creator
        .history_state_id()
        .ok_or("its creating scope has no history state")?;
    let mut states = history
        .states()
        .iter()
        .filter(|state| state.state_id == state_id);
    let (Some(state), None) = (states.next(), states.next()) else {
        return Err("its creating history state is not unique");
    };
    let [slot] = state.transition.as_ref().map_or(&[][..], |transition| {
        &transition.topology.bodies.inserted[..]
    }) else {
        return Err("its creating state does not insert exactly one body");
    };
    let mut results = vec![ToolResultState {
        feature: generator.id.clone(),
        state_id,
        slots: vec![*slot],
    }];
    for writer in history.states() {
        let Some(transition) = writer.transition.as_ref() else {
            continue;
        };
        let bodies = &transition.topology.bodies;
        if !bodies.updated.contains(slot) {
            continue;
        }
        let Some(feature) = state_feature(history, writer.state_id, inputs) else {
            continue;
        };
        if feature.ordinal <= generator.ordinal || feature.ordinal >= combine_ordinal {
            continue;
        }
        let mut slots = bodies
            .inserted
            .iter()
            .chain(&bodies.updated)
            .copied()
            .collect::<Vec<_>>();
        slots.sort_unstable();
        slots.dedup();
        results.push(ToolResultState {
            feature: feature.id.clone(),
            state_id: writer.state_id,
            slots,
        });
    }
    Ok((*slot, results))
}

/// The feature of the one scope of `history` whose result state is `state_id`.
fn state_feature<'a>(
    history: &AsmHistory,
    state_id: i64,
    inputs: &Inputs<'a>,
) -> Option<&'a Feature> {
    let mut scopes = inputs.scopes.iter().filter(|scope| {
        scope.history_state_id() == Some(state_id)
            && super::generated_bodies::scope_history(
                scope,
                inputs.scope_histories,
                inputs.histories,
            )
            .is_some_and(|candidate| std::ptr::eq(candidate, history))
    });
    let (Some(scope), None) = (scopes.next(), scopes.next()) else {
        return None;
    };
    inputs.by_native.get(scope.id.as_str()).copied()
}

/// The solved body of a live pair whose creating scope reads a history other
/// than `combine_history`, when the creating state inserts one body slot and
/// every later transition of that history that updates or deletes the slot
/// belongs to a feature that precedes the Combine. The solved body is then
/// the body that the Combine reads.
fn live_tool_body(
    pair: &DesignBodyBinding,
    creator: &DesignParameterScope,
    (combine_ordinal, combine_history): (u64, Option<&str>),
    inputs: &Inputs<'_>,
) -> Result<cadmpeg_ir::ids::BodyId, &'static str> {
    let history = super::history_states::bound_scope_history(
        &creator.id,
        inputs.scope_histories,
        inputs.histories,
    )
    .ok_or("its live body-map pair has a creating scope with no bound history")?;
    if combine_history == Some(history.id.as_str()) {
        return Err("its live body-map pair is a body of the Combine history");
    }
    let state_id = creator
        .history_state_id()
        .ok_or("its creating scope has no history state")?;
    let mut states = history
        .states()
        .iter()
        .filter(|state| state.state_id == state_id);
    let (Some(state), None) = (states.next(), states.next()) else {
        return Err("its creating history state is not unique");
    };
    let [slot] = state.transition.as_ref().map_or(&[][..], |transition| {
        &transition.topology.bodies.inserted[..]
    }) else {
        return Err("its creating state does not insert exactly one body");
    };
    let later = super::generated_bodies::later_slot_change_states(history, creator, *slot)
        .ok_or("its creating state has no unique place in the history's state order")?;
    let stream = crate::ids::native_stream(&creator.id);
    for changed in later {
        let mut owners = inputs.scopes.iter().filter(|candidate| {
            candidate.history_state_id() == Some(changed)
                && crate::ids::native_stream(&candidate.id) == stream
                && inputs.scope_histories.get(&candidate.id) == Some(&history.id)
        });
        let owner = match (owners.next(), owners.next()) {
            (Some(owner), None) => owner,
            _ => return Err(
                "no single feature of its history owns a later transition that changes the body",
            ),
        };
        let precedes = inputs
            .by_native
            .get(owner.id.as_str())
            .is_some_and(|feature| feature.ordinal < combine_ordinal);
        if !precedes {
            return Err("a feature that follows the Combine changes the body in its history");
        }
    }
    pair.body
        .clone()
        .ok_or("its live body-map pair names no solved body")
}

/// Resolve every tool body of `scope` by its tool ordinal. Each entry is the
/// proven tool or the reason that it stays native. A tool that names the
/// browser node of an earlier tool is that tool's body and has no entry.
fn resolve_tools(
    operation: &crate::records::feature::combine::DesignCombineOperation,
    stream: Option<&str>,
    (combine_ordinal, combine_history): (u64, Option<&str>),
    inputs: &Inputs<'_>,
) -> Vec<(usize, Result<ResolvedTool, &'static str>)> {
    let mut resolved = Vec::<(usize, Result<ResolvedTool, &'static str>)>::new();
    let mut tool_nodes = std::collections::HashSet::new();
    for (ordinal, tool) in operation.tools.iter().enumerate() {
        if tool
            .body_node
            .is_some_and(|node| !tool_nodes.insert(node.record_index))
        {
            continue;
        }
        let mut entry = resolve_tool(tool, stream, (combine_ordinal, combine_history), inputs);
        if let Ok(candidate) = &entry {
            let repeated = resolved.iter().any(|(_, existing)| {
                existing.as_ref().is_ok_and(|existing| {
                    existing.body == candidate.body
                        && (matches!(existing.body, ToolBody::Model(_))
                            || existing.generator == candidate.generator)
                })
            });
            if repeated {
                entry = Err("another tool names the same body");
            }
        }
        resolved.push((ordinal, entry));
    }
    resolved
}

struct Inputs<'a> {
    scopes: &'a [DesignParameterScope],
    bindings: &'a [DesignBodyBinding],
    histories: &'a [AsmHistory],
    scope_histories: &'a HashMap<String, String>,
    by_native: HashMap<&'a str, &'a Feature>,
}

/// Bind the tools of each operand-list `Combine` that name history-only bodies
/// or bodies whose creating scope reads another history.
///
/// Every proven tool adds its generating feature to the `Combine`
/// dependencies and records `combine:tool:<ordinal>:generator`; the product
/// definition of that feature owns the tool body. The tools become generated
/// bodies when every tool is a proven history-only body, and model bodies
/// when every tool is a proven live body. Return the losses, the native tools
/// that each bound `Combine` replaces, and the result states that name the
/// generated tool bodies.
pub(crate) fn bind(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    bindings: &[DesignBodyBinding],
    (histories, scope_histories): (&[AsmHistory], &HashMap<String, String>),
) -> (Vec<LossNote>, Vec<ReplacedTools>, Vec<ToolResultState>) {
    let mut losses = Vec::new();
    let mut replaced = Vec::new();
    let mut results = Vec::new();
    let mut updates = Vec::<(usize, Vec<(usize, Result<ResolvedTool, &'static str>)>)>::new();
    {
        let inputs = Inputs {
            scopes,
            bindings,
            histories,
            scope_histories,
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
            let combine_history = combine_history(scope, &inputs);
            let names_other_history = operation.tools.iter().any(|tool| {
                tool.body_node.is_some_and(|node| {
                    scopes.iter().any(|candidate| {
                        candidate.record_index == node.creating_scope_record_index
                            && crate::ids::native_stream(&candidate.id) == stream
                            && scope_histories
                                .get(&candidate.id)
                                .is_some_and(|bound| Some(bound.as_str()) != combine_history)
                    })
                })
            });
            if !names_history_only_pair && !names_other_history {
                continue;
            }
            updates.push((
                index,
                resolve_tools(
                    operation,
                    stream,
                    (feature.ordinal, combine_history),
                    &inputs,
                ),
            ));
        }
    }
    for (index, tools) in updates {
        let feature = &mut features[index];
        for (ordinal, tool) in &tools {
            let Ok(tool) = tool else {
                continue;
            };
            insert_property(feature, *ordinal, "generator", tool.generator.as_str());
            if tool.generator != feature.id {
                feature.dependencies.insert(tool.generator.clone());
            }
        }
        let unproven = tools
            .iter()
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
        let mut tool_results = Vec::new();
        let mut pair_identities = Vec::new();
        for (ordinal, tool) in &tools {
            if let Ok(tool) = tool {
                if let Some(reason) = tool.pair_identity {
                    pair_identities.push(format!("tool {ordinal}: {reason}"));
                }
            }
        }
        let tools = tools
            .into_iter()
            .map(|(ordinal, tool)| {
                (
                    ordinal,
                    tool.map(|mut tool| {
                        tool_results.append(&mut tool.results);
                        tool
                    }),
                )
            })
            .collect::<Vec<_>>();
        let Some(selection) = tool_selection(tools, native) else {
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
            let mut previous = None;
            refusal = operands
                .try_edit(|_, operand_tools| {
                    previous = Some(std::mem::replace(operand_tools, selection));
                })
                .err();
            if refusal.is_none() {
                replaced.extend(previous.map(|tools| ReplacedTools {
                    feature: feature.id.clone(),
                    tools,
                }));
            }
        });
        if let Some(error) = refusal {
            losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(format!(
                "feature {} retains its native Combine tools: {error}",
                feature.id
            )));
            continue;
        }
        if !pair_identities.is_empty() {
            losses.push(
                F3dLossCode::FeatureCombineToolResultStateUnbound.note(format!(
                    "feature {} names generated tool bodies by their body-map pairs; {}",
                    feature.id,
                    pair_identities.join("; ")
                )),
            );
        }
        results.extend(tool_results);
    }
    (losses, replaced, results)
}

/// Align the replaced Combine tools with the result states that `states`
/// holds, then add the tool result states. Alignment reads only the result
/// states that exist before this pass, so a writer row of this pass never
/// renames or reverts a tool.
pub(crate) fn apply_result_states(
    features: &mut [Feature],
    states: &mut Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
    (results, replaced): (Vec<ToolResultState>, Vec<ReplacedTools>),
) -> Vec<LossNote> {
    let losses = align_with_result_states(features, states, replaced);
    add_result_states(states, results);
    losses
}

/// Add each tool result state to the result state of its feature. A feature
/// with a result state for the same history state gains the missing slots.
fn add_result_states(
    states: &mut Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
    results: Vec<ToolResultState>,
) {
    use cadmpeg_ir::features::evaluation::{FeatureResultMembers, FeatureResultTopology};
    for ToolResultState {
        feature,
        state_id,
        slots,
    } in results
    {
        let id = crate::ids::history_result_state_id(&feature, state_id);
        let slots = slots
            .iter()
            .filter_map(|slot| NonBlankString::new(slot.to_string()))
            .collect::<Vec<_>>();
        if let Some(state) = states.iter_mut().find(|state| state.output_of == feature) {
            if state.id != id {
                continue;
            }
            let mut bodies = state.bodies().to_vec();
            for slot in slots {
                if !bodies.contains(&slot) {
                    bodies.push(slot);
                }
            }
            if let Ok(members) = FeatureResultMembers::new(
                bodies,
                state.faces().to_vec(),
                state.edges().to_vec(),
                state.vertices().to_vec(),
            ) {
                *state = FeatureResultTopology::new(id, feature, members, state.native_ref.clone());
            }
            continue;
        }
        if let Ok(members) = FeatureResultMembers::new(slots, Vec::new(), Vec::new(), Vec::new()) {
            states.push(FeatureResultTopology::new(id, feature, members, None));
        }
    }
}

/// The history that holds the state of the `Combine` scope: its bound
/// history, else the one history that holds its state pair.
fn combine_history<'a>(scope: &DesignParameterScope, inputs: &Inputs<'a>) -> Option<&'a str> {
    if let Some(bound) = inputs.scope_histories.get(&scope.id) {
        return Some(bound.as_str());
    }
    let state = scope.history_state_id()?;
    let previous =
        super::history_states::effective_scope_previous_history_state_id(scope, inputs.histories)?;
    super::history_states::unique_history_state_pair(inputs.histories, state, previous)
        .map(|(history, _, _)| history.id.as_str())
}

/// The selection of proven tools: generated bodies when every tool is a
/// history-only body, model bodies when every tool is a live body.
fn tool_selection(
    tools: Vec<(usize, Result<ResolvedTool, &'static str>)>,
    native: String,
) -> Option<BodySelection> {
    let tools = tools
        .into_iter()
        .map(|(_, tool)| tool.ok())
        .collect::<Option<Vec<_>>>()?;
    if tools
        .iter()
        .all(|tool| matches!(tool.body, ToolBody::Model(_)))
    {
        let bodies = tools
            .into_iter()
            .filter_map(|tool| match tool.body {
                ToolBody::Model(body) => Some(body),
                ToolBody::Generated(_) => None,
            })
            .collect::<Vec<_>>();
        return Some(BodySelection::Resolved {
            bodies: cadmpeg_ir::features::selections::DistinctMembers::try_from(bodies).ok()?,
            native,
        });
    }
    let refs = tools
        .into_iter()
        .map(|tool| match tool.body {
            ToolBody::Generated(local_id) => GeneratedBodyRef::new(tool.generator, local_id).ok(),
            ToolBody::Model(_) => None,
        })
        .collect::<Option<Vec<_>>>()?;
    BodySelection::generated(refs, native).ok()
}

/// The native tools of a `Combine` that [`bind`] replaced with generated
/// bodies.
pub(crate) struct ReplacedTools {
    feature: FeatureId,
    tools: BodySelection,
}

/// Name each generated tool body by the identity that the result state of its
/// generating feature lists. `results` holds the result states that the body
/// selections produce, before the tool result states of this pass. A tool whose identity that result state lists
/// keeps it. A generating feature whose result state lists one body generates
/// that body, so another tool that it generates is that body. A `Combine` with
/// a tool that neither rule names, or whose tools then name one body twice,
/// takes back its native tools.
fn align_with_result_states(
    features: &mut [Feature],
    results: &[cadmpeg_ir::features::evaluation::FeatureResultTopology],
    replaced: Vec<ReplacedTools>,
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for ReplacedTools { feature: id, tools } in replaced {
        let Some(feature) = features.iter_mut().find(|feature| feature.id == id) else {
            continue;
        };
        let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let BodySelection::Generated { bodies, native } = operands.tools() else {
            continue;
        };
        let aligned = bodies
            .iter()
            .map(|body| {
                let Some(state) = results.iter().find(|state| state.output_of == body.feature)
                else {
                    return Some(body.clone());
                };
                if state
                    .bodies()
                    .iter()
                    .any(|id| id.as_str() == body.local_id.as_str())
                {
                    return Some(body.clone());
                }
                let [only] = state.bodies() else {
                    return None;
                };
                GeneratedBodyRef::new(body.feature.clone(), only.as_str().to_owned()).ok()
            })
            .collect::<Option<Vec<_>>>()
            .and_then(|refs| BodySelection::generated(refs, native.as_str().to_owned()).ok());
        let mut reverted = false;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            else {
                return;
            };
            let aligned = aligned.is_some_and(|selection| {
                operands
                    .try_edit(|_, operand_tools| *operand_tools = selection)
                    .is_ok()
            });
            if !aligned {
                reverted = operands
                    .try_edit(|_, operand_tools| *operand_tools = tools)
                    .is_ok();
            }
        });
        if reverted {
            losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(format!(
                "feature {} retains its native Combine tools: the result state of a generating feature lists several bodies and not the tool body, or two tools name one body",
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
