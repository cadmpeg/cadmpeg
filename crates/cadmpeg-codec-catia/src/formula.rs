// SPDX-License-Identifier: Apache-2.0
//! Transfer of complete, typed CATIA formula programs to neutral parameters.

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_core::convert::{f64_from_i64, truncate_f64_to_i32, truncate_f64_to_i64};

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::{
    features::{DesignParameter, ParameterId, ParameterValue},
    scalar::{Angle, Length},
};
use cadmpeg_ir::{AnnotationBuilder, Annotations};

use crate::native::CatiaNative;
use crate::resource;

const FORMULA_INTEGER_LOWER: f64 = -9_223_372_036_854_775_808.0;
const FORMULA_INTEGER_UPPER: f64 = 9_223_372_036_854_775_808.0;

pub(crate) fn transfer_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    annotations: &mut Annotations,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<FormulaTransfer, cadmpeg_core::CodecError> {
    let entities = ctx.collect_hash_map(
        native
            .entity_records
            .iter()
            .map(|entity| (entity.id.as_str(), entity)),
        "catia_formula_entity_index",
    )?;
    let object_records = ctx.collect_hash_map(
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .map(|record| (record.id.as_str(), record)),
        "catia_formula_object_index",
    )?;
    let mut candidates = BTreeMap::<ParameterId, FormulaParameterCandidate>::new();
    let mut conflicting_inputs = BTreeSet::<ParameterId>::new();
    collect_definition_chain_parameters(
        ctx,
        native,
        graph_scope,
        &mut candidates,
        &mut conflicting_inputs,
    )?;
    let mut programs = Vec::<FormulaProgramCandidate>::new();
    let mut formula_definition_counts = HashMap::<ParameterId, usize>::new();
    for entity in native
        .entity_records
        .iter()
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        let outputs = entity
            .formula_relation()
            .and_then(|relation| relation.output_entity.reference.entity())
            .into_iter()
            .chain(
                entity
                    .relation_program_instance()
                    .and_then(|instance| instance.output_entity())
                    .and_then(|output| output.entity()),
            );
        for output in outputs {
            let output_id = neutral_parameter_id(ctx, output)?;
            ctx.admit_hash_map_entry(
                &mut formula_definition_counts,
                &output_id,
                "catia_formula_definition_counts",
            )?;
            *formula_definition_counts.entry(output_id).or_default() += 1;
        }
    }
    let legacy_scope = if graph_scope.is_unscoped() {
        LegacyModelingScope::Unbounded
    } else {
        native
            .object_graphs
            .iter()
            .find(|graph| graph_scope.contains(graph.id.as_str()))
            .and_then(|graph| graph.outer_container.as_ref())
            .map_or(
                LegacyModelingScope::Unresolved,
                LegacyModelingScope::Container,
            )
    };
    let legacy_transfer = collect_legacy_parameters(ctx, native, &mut candidates, legacy_scope)?;
    let mut relation_program_parameters =
        BTreeMap::<ParameterId, Option<(DesignParameter, FormulaParameterType)>>::new();
    for program_entity in native
        .entity_records
        .iter()
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        let Some(inputs) = program_entity
            .relation_program_instance()
            .and_then(|instance| instance.inputs.as_ref())
        else {
            continue;
        };
        for input in inputs {
            let Some(entity) = input
                .entity
                .entity()
                .and_then(|entity| entities.get(entity))
            else {
                continue;
            };
            let Some(candidate) = typed_entity_parameter_candidate_for_source(
                ctx,
                entity,
                input.value_type.as_str(),
            )?
            else {
                continue;
            };
            if let Some(existing) = relation_program_parameters.get_mut(&candidate.parameter.id) {
                if existing.as_ref().is_some_and(|input| {
                    input.1 != candidate.parameter_type || input.0 != candidate.parameter
                }) {
                    *existing = None;
                }
            } else {
                let id = candidate
                    .parameter
                    .id
                    .try_clone_for_decode(ctx, "catia_relation_program_index_id")?;
                let parameter = copy_design_parameter(ctx, &candidate.parameter)?;
                ctx.insert_btree_map(
                    &mut relation_program_parameters,
                    id,
                    Some((parameter, candidate.parameter_type)),
                    "catia_relation_program_parameter_index",
                )?;
            }
            match candidates.get(&candidate.parameter.id) {
                Some(existing) if !formula_parameter_candidates_agree(existing, &candidate) => {
                    ctx.insert_btree_set(
                        &mut conflicting_inputs,
                        candidate.parameter.id,
                        "catia_formula_conflicting_inputs",
                    )?;
                }
                Some(_) => {}
                None => {
                    let id = candidate
                        .parameter
                        .id
                        .try_clone_for_decode(ctx, "catia_formula_candidate_index_id")?;
                    ctx.insert_btree_map(
                        &mut candidates,
                        id,
                        candidate,
                        "catia_formula_candidates",
                    )?;
                }
            }
        }
    }

    for formula_entity in native
        .entity_records
        .iter()
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        let Some(formula) = &formula_entity.formula_relation() else {
            continue;
        };
        let Some(expression_entity) = formula
            .expression_entity
            .reference
            .entity()
            .and_then(|expression| entities.get(expression))
        else {
            continue;
        };
        let Some(expression) = expression_entity.relation_expression() else {
            continue;
        };
        let Some(signature) = expression.signature_charged(ctx)? else {
            continue;
        };
        let mut transferred = Vec::new();
        let mut dependencies = Vec::new();
        let mut used_inputs = BTreeSet::new();
        let mut expression_bindings = BTreeMap::new();
        // E7 inputs have a declared type but no value. Keep a separate typed
        // binding set so their defining expression can still be retained as
        // an unset result after syntax and type validation.
        let mut type_bindings = BTreeMap::new();
        let mut all_inputs_complete = true;
        let mut all_inputs_typed = true;
        for dependency in &formula.parameter_dependencies {
            let Some(input) = signature
                .inputs
                .iter()
                .find(|input| crate::native::dependency_matches_input(dependency, input))
            else {
                all_inputs_complete = false;
                all_inputs_typed = false;
                continue;
            };
            let [parameter] = dependency.candidates.as_slice() else {
                all_inputs_complete = false;
                all_inputs_typed = false;
                continue;
            };
            let Some(entity) = parameter
                .entity()
                .and_then(|parameter| entities.get(parameter))
            else {
                all_inputs_complete = false;
                all_inputs_typed = false;
                continue;
            };
            let Some(candidate) =
                typed_entity_parameter_candidate_for_source(ctx, entity, &input.input_type)?
            else {
                all_inputs_complete = false;
                all_inputs_typed = false;
                continue;
            };
            ctx.insert_btree_set(
                &mut used_inputs,
                input.parameter.as_str(),
                "catia_formula_used_inputs",
            )?;
            if dependencies.contains(&candidate.parameter.id) {
                continue;
            }
            let id = candidate
                .parameter
                .id
                .try_clone_for_decode(ctx, "catia_formula_dependency_id")?;
            ctx.push_vec(&mut dependencies, id, "catia_formula_dependencies")?;
            let type_value = static_formula_value(candidate.parameter_type);
            ctx.insert_btree_map(
                &mut type_bindings,
                input.parameter.as_str(),
                type_value,
                "catia_formula_type_bindings",
            )?;
            match candidate.parameter.value.as_ref() {
                None => {
                    all_inputs_complete = false;
                }
                Some(value) => {
                    ctx.insert_btree_map(
                        &mut expression_bindings,
                        input.parameter.as_str(),
                        EvaluatedFormulaValue::from_parameter_value_charged(ctx, value)?,
                        "catia_formula_expression_bindings",
                    )?;
                }
            }
            ctx.push_vec(
                &mut transferred,
                candidate,
                "catia_formula_transferred_inputs",
            )?;
        }
        let formula_complete = all_inputs_complete
            && used_inputs.len() == signature.inputs.len()
            && dependencies.len() == signature.inputs.len();
        let formula_type_complete = all_inputs_typed
            && used_inputs.len() == signature.inputs.len()
            && dependencies.len() == signature.inputs.len();
        let type_checked_expression = (if formula_type_complete {
            evaluate_formula_expression_with_mode_charged(
                ctx,
                &expression.expression.value,
                &type_bindings,
                false,
            )?
        } else {
            None
        })
        .filter(|value| {
            canonical_parameter_type(&signature.result_type)
                .is_some_and(|source_type| value.satisfies_source_type(source_type))
        });
        let evaluated_expression = (if formula_complete {
            evaluate_formula_expression_charged(
                ctx,
                &expression.expression.value,
                &expression_bindings,
            )?
        } else {
            None
        })
        .filter(|value| {
            canonical_parameter_type(&signature.result_type)
                .is_some_and(|source_type| value.satisfies_source_type(source_type))
        });
        let transferable_expression = if formula_complete {
            evaluated_expression.as_ref()
        } else {
            type_checked_expression.as_ref()
        };
        let input_parameters = ctx.try_collect_vec(
            transferred
                .iter()
                .map(|candidate| -> Result<_, cadmpeg_core::CodecError> {
                    Ok((
                        copy_design_parameter(ctx, &candidate.parameter)?,
                        candidate.parameter_type,
                    ))
                }),
            "catia_formula_input_parameters",
        )?;
        if let Some(output) = formula
            .output_entity
            .reference
            .entity()
            .filter(|_| transferable_expression.is_some())
            .and_then(|id| entities.get(id))
        {
            if let Some(output_value) = output.parameter_value() {
                let output_id = neutral_parameter_id(ctx, &output.id)?;
                if !dependencies.contains(&output_id) {
                    if let Some((parameter_type, value)) =
                        typed_parameter_evaluation(&signature.result_type, &output_value.evaluation)
                    {
                        let accepted = match &value {
                            TypedParameterEvaluation::Unset => transferable_expression.is_some(),
                            TypedParameterEvaluation::Value(value) => {
                                if let Some(evaluated) = evaluated_expression.as_ref() {
                                    evaluated.agrees_with(&TypedParameterEvaluation::Value(
                                        (value).try_clone_for_decode(
                                            ctx,
                                            "catia_formula_parameter_value_copy",
                                        )?,
                                    ))
                                } else {
                                    false
                                }
                            }
                        };
                        if accepted {
                            let relation_entity = ctx.copy_retained_text(
                                &formula_entity.id,
                                "catia_formula_program_relation",
                            )?;
                            let expression_entity_id = ctx.copy_retained_text(
                                &expression_entity.id,
                                "catia_formula_program_expression",
                            )?;
                            let program_output = output_id
                                .try_clone_for_decode(ctx, "catia_formula_program_output")?;
                            let program_inputs = ctx.try_collect_vec(
                                dependencies.iter().map(|id| {
                                    id.try_clone_for_decode(ctx, "catia_formula_program_inputs")
                                }),
                                "catia_formula_program_inputs",
                            )?;
                            ctx.push_vec(
                                &mut programs,
                                FormulaProgramCandidate {
                                    relation_entity,
                                    expression_entity: expression_entity_id,
                                    output: program_output,
                                    inputs: program_inputs,
                                    input_parameters,
                                },
                                "catia_formula_programs",
                            )?;
                            ctx.charge_entities(1, "admit CATIA formula candidate")?;
                            let mut output_dependencies =
                                cadmpeg_ir::features::DistinctMembers::default();
                            for dependency in dependencies {
                                output_dependencies.insert(
                                    ctx,
                                    dependency,
                                    "catia_formula_output_dependencies",
                                )?;
                            }
                            let output_name = ctx.copy_retained_text(
                                &output_value.name.value,
                                "catia_formula_output_name",
                            )?;
                            let output_expression = ctx.copy_retained_text(
                                &expression.expression.value,
                                "catia_formula_output_expression",
                            )?;
                            let output_native_ref = ctx.copy_retained_text(
                                &output.id,
                                "catia_formula_output_native_ref",
                            )?;
                            ctx.push_vec(
                                &mut transferred,
                                FormulaParameterCandidate {
                                    parameter: DesignParameter {
                                        id: output_id,
                                        owner: None,
                                        ordinal: 0,
                                        name: output_name,
                                        expression: output_expression,
                                        display: None,
                                        value: match value {
                                            TypedParameterEvaluation::Unset => None,
                                            TypedParameterEvaluation::Value(value) => Some(value),
                                        },
                                        dependencies: output_dependencies,
                                        properties: parameter_properties(
                                            ctx,
                                            parameter_type.as_str(),
                                            Some(output_value.binding.value.as_str()),
                                        )?,
                                        pmi: None,
                                        native_ref: Some(output_native_ref),
                                    },
                                    parameter_type,
                                    role: FormulaParameterRole::FormulaOutput { fallback: None },
                                    source_order: output.byte_offset,
                                },
                                "catia_formula_transferred_inputs",
                            )?;
                        }
                    }
                }
            }
        }

        for candidate in transferred {
            merge_formula_parameter_candidate(
                ctx,
                &mut candidates,
                &mut conflicting_inputs,
                candidate,
            )?;
        }
    }

    for relation_entity in native
        .entity_records
        .iter()
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        let Some(instance) = relation_entity.relation_program_instance() else {
            continue;
        };
        let Some(output_entity) = instance
            .output_entity()
            .and_then(|output| output.entity())
            .and_then(|output| entities.get(output))
        else {
            continue;
        };
        let Some(expression_entity) = instance
            .relation_expression
            .as_deref()
            .and_then(|expression| entities.get(expression))
        else {
            continue;
        };
        let Some(expression) = expression_entity.relation_expression() else {
            continue;
        };
        let Some(inputs) = instance.inputs.as_ref() else {
            continue;
        };
        let Some((program, candidate)) = relation_program_output_candidate(
            ctx,
            relation_entity,
            expression_entity,
            output_entity,
            expression,
            inputs,
            &entities,
        )?
        else {
            continue;
        };
        ctx.push_vec(&mut programs, program, "catia_formula_programs")?;
        merge_formula_parameter_candidate(
            ctx,
            &mut candidates,
            &mut conflicting_inputs,
            candidate,
        )?;
    }

    for id in &conflicting_inputs {
        match candidates.get_mut(id) {
            Some(candidate) if candidate.role.is_formula_output() => {
                if let FormulaParameterRole::FormulaOutput { fallback } = &mut candidate.role {
                    *fallback = None;
                }
            }
            Some(_) => {
                candidates.remove(id);
            }
            None => {}
        }
    }
    candidates.retain(|id, candidate| {
        match (
            candidate.role.is_formula_output(),
            formula_definition_counts.get(id),
        ) {
            (true, Some(count)) if *count != 1 => demote_formula_output(candidate),
            (true, Some(_)) => true,
            (false, _) | (true, None) => true,
        }
    });
    let invalid_outputs = ctx.collect_hash_set(
        programs
            .iter()
            .filter(|program| {
                program.input_parameters.iter().any(|input| {
                    !candidates.get(&input.0.id).is_some_and(|candidate| {
                        formula_parameter_candidate_accepts_input(candidate, input)
                    })
                })
            })
            .map(|program| &program.output),
        "catia_formula_invalid_outputs",
    )?;
    for output in invalid_outputs {
        let Some(candidate) = candidates.get_mut(output) else {
            continue;
        };
        if !candidate.role.is_formula_output() {
            continue;
        }
        if !demote_formula_output(candidate) {
            candidates.remove(output);
        }
    }
    loop {
        let invalid = ctx.try_collect_vec(
            candidates
                .iter()
                .filter(|(_, parameter)| {
                    parameter
                        .parameter
                        .dependencies
                        .iter()
                        .any(|dependency| !candidates.contains_key(dependency))
                })
                .map(|(id, _)| id.try_clone_for_decode(ctx, "catia_formula_invalid_dependency_id")),
            "catia_formula_invalid_dependencies",
        )?;
        if invalid.is_empty() {
            break;
        }
        for id in invalid {
            candidates.remove(&id);
        }
    }
    let mut derivable = BTreeSet::new();
    loop {
        let previous_len = derivable.len();
        for (id, candidate) in &candidates {
            if candidate
                .parameter
                .dependencies
                .iter()
                .all(|dependency| derivable.contains(dependency))
                && !derivable.contains(id)
            {
                let id = id.try_clone_for_decode(ctx, "catia_formula_derivable_id")?;
                ctx.insert_btree_set(&mut derivable, id, "catia_formula_derivable_parameters")?;
            }
        }
        if derivable.len() == previous_len {
            break;
        }
    }
    candidates.retain(|id, _| derivable.contains(id));
    let relation_program_parameter_count = relation_program_parameters
        .iter()
        .filter(|(id, input)| {
            input.as_ref().is_some_and(|input| {
                candidates.get(*id).is_some_and(|candidate| {
                    formula_parameter_candidate_accepts_input(candidate, input)
                })
            })
        })
        .count();
    let mut consumed_entity_records = ctx.collect_string_set(
        candidates
            .values()
            .filter_map(|candidate| candidate.parameter.native_ref.as_deref()),
        "catia_formula_consumed_entities",
    )?;
    for program in programs {
        if candidates
            .get(&program.output)
            .is_some_and(|candidate| candidate.role.is_formula_output())
            && program
                .inputs
                .iter()
                .all(|input| candidates.contains_key(input))
        {
            ctx.insert_hash_set(
                &mut consumed_entity_records,
                program.relation_entity,
                "catia_formula_consumed_entities",
            )?;
            ctx.insert_hash_set(
                &mut consumed_entity_records,
                program.expression_entity,
                "catia_formula_consumed_entities",
            )?;
        }
    }
    let consumed_object_records = ctx.collect_string_set(
        consumed_entity_records.iter().filter_map(|entity| {
            let entity = entities.get(entity.as_str())?;
            let object = object_records.get(entity.object_record.as_str())?;
            (entity.formula_relation().is_some()
                || object.subtype() == crate::object_graph::PayloadSubtype::Empty
                    && object.references.is_empty())
            .then_some(object.id.as_str())
        }),
        "catia_formula_consumed_objects",
    )?;
    let transferred = candidates.len();
    let mut parameters =
        ctx.collect_vec(candidates.into_values(), "catia_formula_ordered_parameters")?;
    ctx.stable_sort_by(
        &mut parameters,
        |left, right| left.source_order.cmp(&right.source_order),
        |_| 0,
        "catia_formula_ordered_parameters_sort",
    )?;
    for (ordinal, candidate) in parameters.iter_mut().enumerate() {
        let Some(ordinal) = u32::try_from(ordinal).ok() else {
            return Ok(FormulaTransfer::default());
        };
        candidate.parameter.ordinal = ordinal;
    }
    let definition_chain_parameter_count = parameters
        .iter()
        .filter(|candidate| {
            candidate
                .parameter
                .native_ref
                .as_ref()
                .and_then(|native_ref| entities.get(native_ref.as_str()))
                .is_some_and(|entity| entity.definition_chain_value().is_some())
        })
        .count();
    let mut annotation_builder = AnnotationBuilder::resume(std::mem::take(annotations));
    for candidate in &parameters {
        resource::derived_annotation(
            ctx,
            &mut annotation_builder,
            candidate.parameter.id.as_str(),
            "properties",
            "catia_formula_annotations",
        )?;
        if !candidate.role.is_formula_output() && candidate.parameter.dependencies.is_empty() {
            resource::derived_annotation(
                ctx,
                &mut annotation_builder,
                candidate.parameter.id.as_str(),
                "expression",
                "catia_formula_annotations",
            )?;
        }
    }
    *annotations = annotation_builder.build();
    for candidate in parameters {
        ctx.push_vec(
            &mut ir.model.parameters,
            candidate.parameter,
            "catia_formula_neutral_parameters",
        )?;
    }
    let typed_parameter_count = transferred
        .checked_sub(legacy_transfer.parameters)
        .ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("CATIA formula transfer count is inconsistent")
        })?;
    Ok(FormulaTransfer {
        typed_parameter_count,
        definition_chain_parameter_count,
        relation_program_parameter_count,
        legacy_parameter_count: legacy_transfer.parameters,
        legacy_selector_parameter_count: legacy_transfer.selector_parameters,
        legacy_formula_count: legacy_transfer.formulas,
        consumed_object_records,
    })
}

/// Add only typed scalar values from the exact two-definition chain grammar.
///
/// The first definition names the parameter field and the second definition
/// names its source type. The suffix selector must already agree with the
/// first definition; the native decoder enforces that invariant. Other suffix
/// states remain native because they do not contain a neutral parameter value.
fn collect_definition_chain_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native: &CatiaNative,
    graph_scope: &crate::decode::ModelingGraphScope,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    conflicting_inputs: &mut BTreeSet<ParameterId>,
) -> Result<(), cadmpeg_core::CodecError> {
    for entity in native
        .entity_records
        .iter()
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        let Some(chain) = entity.definition_chain_value() else {
            continue;
        };
        let Some(candidate) = definition_chain_parameter_candidate(ctx, entity, chain)? else {
            continue;
        };
        let id = candidate
            .parameter
            .id
            .try_clone_for_decode(ctx, "catia_formula_chain_index_id")?;
        match candidates.get(&id) {
            None => {
                ctx.insert_btree_map(candidates, id, candidate, "catia_formula_chain_candidates")?;
            }
            Some(existing) if !formula_parameter_candidates_agree(existing, &candidate) => {
                ctx.insert_btree_set(conflicting_inputs, id, "catia_formula_chain_conflicts")?;
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn definition_chain_parameter_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &crate::native::entity_record::CatiaEntityRecord,
    chain: &crate::native::CatiaDefinitionChainValue,
) -> Result<Option<FormulaParameterCandidate>, cadmpeg_core::CodecError> {
    let (parameter_type, evaluation, evaluation_opcode_offset, atom_value) = match &chain.value {
        crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset,
            evaluation,
        } => {
            let Some((parameter_type, evaluation)) =
                typed_parameter_evaluation(&chain.role.value, evaluation)
            else {
                return Ok(None);
            };
            (parameter_type, evaluation, Some(*opcode_offset), None)
        }
        crate::native::CatiaEntitySuffixSchemaValue::Atom { value }
            if canonical_parameter_type(&chain.role.value)
                == Some(FormulaParameterType::Boolean) =>
        {
            (
                FormulaParameterType::Boolean,
                TypedParameterEvaluation::Value(ParameterValue::Boolean(match value {
                    0 => false,
                    1 => true,
                    _ => return Ok(None),
                })),
                None,
                Some(*value),
            )
        }
        _ => return Ok(None),
    };
    if chain.selector.value.is_empty() {
        return Ok(None);
    }
    let id = match neutral_parameter_id(ctx, &entity.id) {
        Ok(id) => id,
        Err(cadmpeg_core::CodecError::Malformed(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    ctx.charge_entities(1, "admit CATIA formula candidate")?;
    let name = ctx.copy_retained_text(&chain.selector.value, "catia_formula_chain_name")?;
    let (expression, value) = match evaluation {
        TypedParameterEvaluation::Unset => (String::new(), None),
        TypedParameterEvaluation::Value(value) => {
            let expression = parameter_expression(ctx, &value)?;
            (expression, Some(value))
        }
    };
    // A definition chain names the parameter in its first definition. It does
    // not carry the named-parameter value record's scope/expression binding,
    // so do not publish the definition name as `catia_binding`.
    let mut properties = parameter_properties(ctx, parameter_type.as_str(), None)?;
    insert_parameter_property(
        ctx,
        &mut properties,
        "catia_definition_selector_entry",
        format_args!("{}", chain.selector.entry),
    )?;
    insert_parameter_property(
        ctx,
        &mut properties,
        "catia_definition_selector_ordinal",
        format_args!("{}", chain.selector.ordinal),
    )?;
    insert_parameter_property(
        ctx,
        &mut properties,
        "catia_definition_selector_offset",
        format_args!("{}", chain.selector.offset),
    )?;
    insert_parameter_property(
        ctx,
        &mut properties,
        "catia_definition_role_entry",
        format_args!("{}", chain.role.entry),
    )?;
    insert_parameter_property(
        ctx,
        &mut properties,
        "catia_definition_role_ordinal",
        format_args!("{}", chain.role.ordinal),
    )?;
    insert_parameter_property(
        ctx,
        &mut properties,
        "catia_definition_role_offset",
        format_args!("{}", chain.role.offset),
    )?;
    if let Some(opcode_offset) = evaluation_opcode_offset {
        insert_parameter_property(
            ctx,
            &mut properties,
            "catia_definition_evaluation_opcode_offset",
            format_args!("{opcode_offset}"),
        )?;
    }
    if let Some(atom_value) = atom_value {
        insert_parameter_property(
            ctx,
            &mut properties,
            "catia_definition_value_kind",
            format_args!("atom"),
        )?;
        insert_parameter_property(
            ctx,
            &mut properties,
            "catia_definition_atom_value",
            format_args!("{atom_value}"),
        )?;
    }
    Ok(Some(FormulaParameterCandidate {
        parameter: DesignParameter {
            id,
            owner: None,
            ordinal: 0,
            name,
            expression,
            display: None,
            value,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties,
            pmi: None,
            native_ref: Some(ctx.copy_retained_text(&entity.id, "catia_formula_chain_native_ref")?),
        },
        parameter_type,
        role: FormulaParameterRole::Input,
        source_order: entity.byte_offset,
    }))
}

#[derive(Default)]
pub(crate) struct FormulaTransfer {
    pub(crate) typed_parameter_count: usize,
    pub(crate) definition_chain_parameter_count: usize,
    pub(crate) relation_program_parameter_count: usize,
    pub(crate) legacy_parameter_count: usize,
    pub(crate) legacy_selector_parameter_count: usize,
    pub(crate) legacy_formula_count: usize,
    pub(crate) consumed_object_records: HashSet<String>,
}

#[derive(Default)]
struct LegacyParameterTransfer {
    parameters: usize,
    selector_parameters: usize,
    formulas: usize,
}

#[derive(Clone, Copy)]
enum LegacyModelingScope<'a> {
    Unbounded,
    Unresolved,
    Container(&'a crate::native::CatiaOuterContainerBinding),
}

fn legacy_parameter_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    key: &str,
) -> Result<ParameterId, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(
        format_args!("catia:legacy:parameter#{key}"),
        "catia_legacy_parameter_id",
    )?;
    ParameterId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

fn insert_legacy_parameter(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    by_entity: &mut HashMap<u32, Vec<ParameterId>>,
    by_name: &mut HashMap<String, Vec<ParameterId>>,
    (entity_id, name): (u32, &str),
    id: ParameterId,
    candidate: FormulaParameterCandidate,
) -> Result<(), cadmpeg_core::CodecError> {
    let entity_member = id.try_clone_for_decode(ctx, "catia_legacy_entity_parameter_id")?;
    let name_member = id.try_clone_for_decode(ctx, "catia_legacy_name_parameter_id")?;
    ctx.insert_btree_map(
        candidates,
        id,
        candidate,
        "catia_legacy_parameter_candidates",
    )?;
    ctx.admit_hash_map_entry(by_entity, &entity_id, "catia_legacy_parameter_entity_index")?;
    let entity_members = by_entity.entry(entity_id).or_default();
    ctx.push_vec(
        entity_members,
        entity_member,
        "catia_legacy_parameter_entity_members",
    )?;
    let name_key = ctx.copy_retained_text(name, "catia_legacy_parameter_name_key")?;
    ctx.admit_hash_map_entry(by_name, &name_key, "catia_legacy_parameter_name_index")?;
    let name_members = by_name.entry(name_key).or_default();
    ctx.push_vec(
        name_members,
        name_member,
        "catia_legacy_parameter_name_members",
    )?;
    Ok(())
}

fn collect_legacy_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native: &CatiaNative,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    modeling_scope: LegacyModelingScope<'_>,
) -> Result<LegacyParameterTransfer, cadmpeg_core::CodecError> {
    let mut transfer = LegacyParameterTransfer::default();
    for run in native
        .legacy_entity_runs
        .iter()
        .filter(|run| outer_container_in_scope(run.outer_container.as_ref(), modeling_scope))
    {
        let mut parameters_by_entity = HashMap::<u32, Vec<ParameterId>>::new();
        let mut parameters_by_name = HashMap::<String, Vec<ParameterId>>::new();
        for scalar in &run.scalar_values {
            if scalar.encoding != crate::native::CatiaLegacyScalarEncoding::Named84 {
                continue;
            }
            let Some(name) = &scalar.name else {
                continue;
            };
            let Some((value_type, selected)) = resolved_legacy_type(ctx, run, scalar.entity_id)?
            else {
                continue;
            };
            let evaluation = match scalar.evaluation {
                crate::native::CatiaLegacyScalarEvaluation::Value { bits } => {
                    crate::native::CatiaEntityEvaluation::Scalar { bits }
                }
                crate::native::CatiaLegacyScalarEvaluation::Unset => {
                    crate::native::CatiaEntityEvaluation::Unset
                }
            };
            let Some((parameter_type, evaluation)) =
                typed_parameter_evaluation(value_type, &evaluation)
            else {
                continue;
            };
            let Some(key) = scalar.id.strip_prefix("catia:legacy:scalar#") else {
                continue;
            };
            let id = legacy_parameter_id(ctx, key)?;
            if candidates.contains_key(&id) {
                continue;
            }
            ctx.charge_entities(1, "admit CATIA formula candidate")?;
            let (expression, value) = match evaluation {
                TypedParameterEvaluation::Unset => (String::new(), None),
                TypedParameterEvaluation::Value(value) => {
                    let expression = parameter_expression(ctx, &value)?;
                    (expression, Some(value))
                }
            };
            let parameter_id = id.try_clone_for_decode(ctx, "catia_legacy_scalar_parameter_id")?;
            let parameter_name = ctx.copy_retained_text(name, "catia_legacy_scalar_name")?;
            let native_ref = ctx.copy_retained_text(&run.id, "catia_legacy_scalar_native_ref")?;
            let candidate = FormulaParameterCandidate {
                parameter: DesignParameter {
                    id: parameter_id,
                    owner: None,
                    ordinal: 0,
                    name: parameter_name,
                    expression,
                    display: None,
                    value,
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    properties: parameter_properties(ctx, parameter_type.as_str(), None)?,
                    pmi: None,
                    native_ref: Some(native_ref),
                },
                parameter_type,
                role: FormulaParameterRole::Input,
                source_order: scalar.byte_offset,
            };
            insert_legacy_parameter(
                ctx,
                candidates,
                &mut parameters_by_entity,
                &mut parameters_by_name,
                (scalar.entity_id, name),
                id,
                candidate,
            )?;
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        for string in &run.string_values {
            let Some(name) = &string.name else {
                continue;
            };
            let Some((value_type, selected)) = resolved_or_intrinsic_legacy_type(
                ctx,
                run,
                string.entity_id,
                string.byte_offset,
                string.name_field,
                name,
                "String",
            )?
            else {
                continue;
            };
            if value_type != "String" {
                continue;
            }
            let Some(key) = string.id.strip_prefix("catia:legacy:string#") else {
                continue;
            };
            let id = legacy_parameter_id(ctx, key)?;
            if candidates.contains_key(&id) {
                continue;
            }
            ctx.charge_entities(1, "admit CATIA formula candidate")?;
            let value = ParameterValue::String(
                ctx.copy_retained_text(&string.value, "catia_legacy_string_value")?,
            );
            let parameter_id = id.try_clone_for_decode(ctx, "catia_legacy_string_parameter_id")?;
            let parameter_name = ctx.copy_retained_text(name, "catia_legacy_string_name")?;
            let native_ref = ctx.copy_retained_text(&run.id, "catia_legacy_string_native_ref")?;
            let candidate = FormulaParameterCandidate {
                parameter: DesignParameter {
                    id: parameter_id,
                    owner: None,
                    ordinal: 0,
                    name: parameter_name,
                    expression: parameter_expression(ctx, &value)?,
                    display: None,
                    value: Some(value),
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    properties: parameter_properties(ctx, "String", None)?,
                    pmi: None,
                    native_ref: Some(native_ref),
                },
                parameter_type: FormulaParameterType::String,
                role: FormulaParameterRole::Input,
                source_order: string.byte_offset,
            };
            insert_legacy_parameter(
                ctx,
                candidates,
                &mut parameters_by_entity,
                &mut parameters_by_name,
                (string.entity_id, name),
                id,
                candidate,
            )?;
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        for integer in &run.integer_values {
            let Some(name) = &integer.name else {
                continue;
            };
            let Some((value_type, selected)) = resolved_or_intrinsic_legacy_type(
                ctx,
                run,
                integer.entity_id,
                integer.byte_offset,
                integer.name_field,
                name,
                "Integer",
            )?
            else {
                continue;
            };
            if !matches!(value_type, "Integer" | "I") {
                continue;
            }
            let Some(key) = integer.id.strip_prefix("catia:legacy:integer#") else {
                continue;
            };
            let id = legacy_parameter_id(ctx, key)?;
            if candidates.contains_key(&id) {
                continue;
            }
            ctx.charge_entities(1, "admit CATIA formula candidate")?;
            let value = ParameterValue::Integer(i64::from(integer.value));
            let parameter_id = id.try_clone_for_decode(ctx, "catia_legacy_integer_parameter_id")?;
            let parameter_name = ctx.copy_retained_text(name, "catia_legacy_integer_name")?;
            let native_ref = ctx.copy_retained_text(&run.id, "catia_legacy_integer_native_ref")?;
            let candidate = FormulaParameterCandidate {
                parameter: DesignParameter {
                    id: parameter_id,
                    owner: None,
                    ordinal: 0,
                    name: parameter_name,
                    expression: parameter_expression(ctx, &value)?,
                    display: None,
                    value: Some(value),
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    properties: parameter_properties(ctx, "Integer", None)?,
                    pmi: None,
                    native_ref: Some(native_ref),
                },
                parameter_type: FormulaParameterType::Integer,
                role: FormulaParameterRole::Input,
                source_order: integer.byte_offset,
            };
            insert_legacy_parameter(
                ctx,
                candidates,
                &mut parameters_by_entity,
                &mut parameters_by_name,
                (integer.entity_id, name),
                id,
                candidate,
            )?;
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        let mut relations_by_parameter =
            HashMap::<u32, Vec<&crate::native::CatiaLegacyRelation>>::new();
        for relation in &run.relations {
            if let Some(parameter) = relation.parameter_entity_id {
                ctx.admit_hash_map_entry(
                    &mut relations_by_parameter,
                    &parameter,
                    "catia_legacy_relation_index",
                )?;
                let relations = relations_by_parameter.entry(parameter).or_default();
                ctx.push_vec(relations, relation, "catia_legacy_relation_members")?;
            }
        }
        for (entity_id, relations) in relations_by_parameter {
            let ([parameter], [relation]) = (
                parameters_by_entity
                    .get(&entity_id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                relations.as_slice(),
            ) else {
                continue;
            };
            let Some(evaluation) =
                legacy_relation_evaluation(ctx, relation, &parameters_by_name, candidates)?
            else {
                continue;
            };
            let Some(candidate) = candidates.get_mut(parameter) else {
                continue;
            };
            if canonical_parameter_type(evaluation.source_type) != Some(candidate.parameter_type) {
                continue;
            }
            if let Some(stored) = candidate.parameter.value.as_ref() {
                let stored =
                    (stored).try_clone_for_decode(ctx, "catia_formula_parameter_value_copy")?;
                if !evaluation
                    .evaluated
                    .agrees_with(&TypedParameterEvaluation::Value(stored))
                {
                    continue;
                }
            }
            let expression =
                ctx.copy_retained_text(evaluation.expression, "catia_legacy_formula_expression")?;
            let mut dependencies = cadmpeg_ir::features::DistinctMembers::default();
            for id in evaluation.dependencies {
                dependencies.insert(ctx, id, "catia_legacy_formula_dependencies")?;
            }
            candidate.parameter.expression = expression;
            candidate.parameter.dependencies = dependencies;
            candidate.role = FormulaParameterRole::FormulaOutput { fallback: None };
            transfer.formulas += 1;
        }
    }
    Ok(transfer)
}

#[cfg(test)]
fn evaluate_legacy_output_assignment(
    source: &str,
    output_parameter: &str,
) -> Option<EvaluatedFormulaValue> {
    let expression = legacy_output_assignment_expression(source, output_parameter)?;
    evaluate_formula_expression(expression, &BTreeMap::new())
}

struct LegacyRelationEvaluation<'a> {
    source_type: &'a str,
    expression: &'a str,
    evaluated: EvaluatedFormulaValue,
    dependencies: Vec<ParameterId>,
}

// A legacy relation is evaluable only when its signature names a unique,
// typed, same-run packet for every input. This is the complete local binding
// rule; unresolved selector namespaces never participate in the join.
fn legacy_relation_evaluation<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    relation: &'a crate::native::CatiaLegacyRelation,
    parameters_by_name: &HashMap<String, Vec<ParameterId>>,
    candidates: &BTreeMap<ParameterId, FormulaParameterCandidate>,
) -> Result<Option<LegacyRelationEvaluation<'a>>, cadmpeg_core::CodecError> {
    let (source_type, expression) = match relation.output.as_ref() {
        Some(output) if relation.result_type == "VoidType" => {
            let Some(expression) =
                legacy_output_assignment_expression(&relation.expression, &output.parameter)
            else {
                return Ok(None);
            };
            (output.value_type.as_str(), expression)
        }
        None if relation.result_type != "VoidType" => {
            (relation.result_type.as_str(), relation.expression.as_str())
        }
        _ => return Ok(None),
    };
    let symbols = if relation.inputs.is_empty() {
        None
    } else {
        Some(crate::native::relation_symbols(ctx, &relation.expression)?)
    };
    let mut bindings = BTreeMap::new();
    let mut dependencies = Vec::new();
    for input in &relation.inputs {
        let Some(symbols) = symbols.as_ref() else {
            return Ok(None);
        };
        if !symbols
            .iter()
            .any(|(_, symbol)| legacy_symbol_matches_input(symbol, &input.parameter))
        {
            return Ok(None);
        }
        let [parameter_id] = parameters_by_name
            .get(&input.parameter)
            .map(Vec::as_slice)
            .unwrap_or_default()
        else {
            return Ok(None);
        };
        if dependencies.contains(parameter_id) {
            return Ok(None);
        }
        let Some(candidate) = candidates.get(parameter_id) else {
            return Ok(None);
        };
        if canonical_parameter_type(&input.value_type) != Some(candidate.parameter_type) {
            return Ok(None);
        }
        let Some(value) = candidate.parameter.value.as_ref() else {
            return Ok(None);
        };
        let evaluated = match value {
            ParameterValue::String(value) => {
                EvaluatedFormulaValue::String(EvaluatedFormulaString::known(
                    ctx.copy_retained_text(value, "catia_legacy_formula_binding_string")?,
                ))
            }
            _ => EvaluatedFormulaValue::from_parameter_value_charged(ctx, value)?,
        };
        ctx.insert_btree_map(
            &mut bindings,
            input.parameter.as_str(),
            evaluated,
            "catia_legacy_formula_bindings",
        )?;
        let dependency =
            parameter_id.try_clone_for_decode(ctx, "catia_legacy_formula_dependency_id")?;
        ctx.push_vec(
            &mut dependencies,
            dependency,
            "catia_legacy_formula_dependencies",
        )?;
    }
    let Some(evaluated) = evaluate_formula_expression_charged(ctx, expression, &bindings)? else {
        return Ok(None);
    };
    let Some(source_type_kind) = canonical_parameter_type(source_type) else {
        return Ok(None);
    };
    Ok(evaluated
        .satisfies_source_type(source_type_kind)
        .then_some(LegacyRelationEvaluation {
            source_type,
            expression,
            evaluated,
            dependencies,
        }))
}

fn legacy_symbol_matches_input(symbol: &str, input: &str) -> bool {
    let Some(suffix) = symbol.strip_prefix(input) else {
        return false;
    };
    let suffix = suffix.trim_start_matches(|character: char| character.is_ascii_whitespace());
    suffix.is_empty()
        || suffix.strip_prefix('/').is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn legacy_output_assignment_expression<'a>(
    source: &'a str,
    output_parameter: &str,
) -> Option<&'a str> {
    let source = source.trim_matches(|character: char| character.is_ascii_whitespace());
    let remainder = source.strip_prefix(output_parameter)?;
    let remainder = remainder.trim_start_matches(|character: char| character.is_ascii_whitespace());
    let remainder = remainder.strip_prefix('=')?;
    if remainder.starts_with('=') {
        return None;
    }
    let expression = remainder.trim_matches(|character: char| character.is_ascii_whitespace());
    (!expression.is_empty()).then_some(expression)
}

fn outer_container_in_scope(
    binding: Option<&crate::native::CatiaOuterContainerBinding>,
    modeling_scope: LegacyModelingScope<'_>,
) -> bool {
    match modeling_scope {
        LegacyModelingScope::Unbounded => true,
        LegacyModelingScope::Unresolved => false,
        LegacyModelingScope::Container(modeling_container) => binding == Some(modeling_container),
    }
}

fn resolved_legacy_type<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    run: &'a crate::native::CatiaLegacyEntityRun,
    mut entity_id: u32,
) -> Result<Option<(&'a str, bool)>, cadmpeg_core::CodecError> {
    let mut steps = 0usize;
    let mut selected = false;
    loop {
        if steps >= run.type_descriptors.len() {
            return Ok(None);
        }
        steps += 1;
        ctx.charge_work(1, "catia_legacy_type_selector_chain")?;
        let mut descriptors = run
            .type_descriptors
            .iter()
            .filter(|descriptor| descriptor.entity_id == entity_id);
        let Some(descriptor) = descriptors.next() else {
            return Ok(None);
        };
        if descriptors.next().is_some() {
            return Ok(None);
        }
        match &descriptor.value {
            crate::native::CatiaLegacyTypeValue::Name { value } => {
                return Ok(Some((value, selected)));
            }
            crate::native::CatiaLegacyTypeValue::Selector { value } => {
                entity_id = *value;
                selected = true;
            }
        }
    }
}

fn resolved_or_intrinsic_legacy_type<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    run: &'a crate::native::CatiaLegacyEntityRun,
    entity_id: u32,
    value_offset: u64,
    name_field: Option<u64>,
    name: &str,
    intrinsic_type: &'static str,
) -> Result<Option<(&'a str, bool)>, cadmpeg_core::CodecError> {
    if let Some(resolved) = resolved_legacy_type(ctx, run, entity_id)? {
        return Ok(Some(resolved));
    }
    if run
        .type_descriptors
        .iter()
        .any(|descriptor| descriptor.entity_id == entity_id)
    {
        return Ok(None);
    }
    let Some(name_field) = name_field else {
        return Ok(None);
    };
    Ok(crate::native::legacy_evaluated_value_name(
        &run.role_selectors,
        &run.text_fields,
        entity_id,
        value_offset,
    )
    .is_some_and(|field| field.byte_offset == name_field && field.value == name)
    .then_some((intrinsic_type, false)))
}

struct FormulaParameterCandidate {
    parameter: DesignParameter,
    parameter_type: FormulaParameterType,
    role: FormulaParameterRole,
    source_order: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FormulaParameterType {
    Length,
    Angle,
    Real,
    Integer,
    Boolean,
    String,
}

impl FormulaParameterType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Length => "LENGTH",
            Self::Angle => "ANGLE",
            Self::Real => "Real",
            Self::Integer => "Integer",
            Self::Boolean => "Boolean",
            Self::String => "String",
        }
    }
}

#[derive(Clone)]
enum FormulaParameterRole {
    Input,
    FormulaOutput {
        fallback: Option<Box<(DesignParameter, FormulaParameterType)>>,
    },
}

impl FormulaParameterRole {
    fn is_formula_output(&self) -> bool {
        matches!(self, Self::FormulaOutput { .. })
    }
}

fn typed_entity_parameter_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &crate::native::entity_record::CatiaEntityRecord,
    parameter: &crate::native::CatiaParameterValue,
    source_type: &str,
) -> Result<Option<FormulaParameterCandidate>, cadmpeg_core::CodecError> {
    let Some((parameter_type, evaluation)) =
        typed_parameter_evaluation(source_type, &parameter.evaluation)
    else {
        return Ok(None);
    };
    let id = match neutral_parameter_id(ctx, &entity.id) {
        Ok(id) => id,
        Err(cadmpeg_core::CodecError::Malformed(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    ctx.charge_entities(1, "admit CATIA formula candidate")?;
    let name =
        ctx.copy_retained_text(&parameter.name.value, "catia_formula_typed_parameter_name")?;
    let native_ref =
        ctx.copy_retained_text(&entity.id, "catia_formula_typed_parameter_native_ref")?;
    let (expression, value) = match evaluation {
        TypedParameterEvaluation::Unset => (String::new(), None),
        TypedParameterEvaluation::Value(value) => {
            let expression = parameter_expression(ctx, &value)?;
            (expression, Some(value))
        }
    };
    Ok(Some(FormulaParameterCandidate {
        parameter: DesignParameter {
            id,
            owner: None,
            ordinal: 0,
            name,
            expression,
            display: None,
            value,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: parameter_properties(
                ctx,
                parameter_type.as_str(),
                Some(parameter.binding.value.as_str()),
            )?,
            pmi: None,
            native_ref: Some(native_ref),
        },
        parameter_type,
        role: FormulaParameterRole::Input,
        source_order: entity.byte_offset,
    }))
}

fn typed_entity_parameter_candidate_for_source(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &crate::native::entity_record::CatiaEntityRecord,
    source_type: &str,
) -> Result<Option<FormulaParameterCandidate>, cadmpeg_core::CodecError> {
    if let Some(parameter) = entity.parameter_value() {
        return typed_entity_parameter_candidate(ctx, entity, parameter, source_type);
    }
    let Some(chain) = entity.definition_chain_value() else {
        return Ok(None);
    };
    if canonical_parameter_type(source_type) != canonical_parameter_type(&chain.role.value) {
        return Ok(None);
    }
    let Some(candidate) = definition_chain_parameter_candidate(ctx, entity, chain)? else {
        return Ok(None);
    };
    Ok(
        (canonical_parameter_type(source_type) == Some(candidate.parameter_type))
            .then_some(candidate),
    )
}

struct FormulaProgramCandidate {
    relation_entity: String,
    expression_entity: String,
    output: ParameterId,
    inputs: Vec<ParameterId>,
    input_parameters: Vec<(DesignParameter, FormulaParameterType)>,
}

fn merge_formula_parameter_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    conflicting_inputs: &mut BTreeSet<ParameterId>,
    mut candidate: FormulaParameterCandidate,
) -> Result<(), cadmpeg_core::CodecError> {
    match candidates.get_mut(&candidate.parameter.id) {
        Some(existing) if !formula_parameter_candidates_agree(existing, &candidate) => {
            match (
                existing.role.is_formula_output(),
                candidate.role.is_formula_output(),
            ) {
                (true, true) => {}
                (true, false) => {
                    ctx.insert_btree_set(
                        conflicting_inputs,
                        candidate.parameter.id,
                        "catia_formula_conflicting_inputs",
                    )?;
                }
                (false, true) => {
                    let conflict = candidate
                        .parameter
                        .id
                        .try_clone_for_decode(ctx, "catia_formula_conflict_id")?;
                    ctx.insert_btree_set(
                        conflicting_inputs,
                        conflict,
                        "catia_formula_conflicting_inputs",
                    )?;
                    let key = candidate
                        .parameter
                        .id
                        .try_clone_for_decode(ctx, "catia_formula_candidate_index_id")?;
                    ctx.insert_btree_map(candidates, key, candidate, "catia_formula_candidates")?;
                }
                (false, false) => {
                    ctx.insert_btree_set(
                        conflicting_inputs,
                        candidate.parameter.id,
                        "catia_formula_conflicting_inputs",
                    )?;
                }
            }
        }
        Some(existing)
            if !existing.role.is_formula_output() && candidate.role.is_formula_output() =>
        {
            candidate.role = FormulaParameterRole::FormulaOutput {
                fallback: Some(Box::new((
                    copy_design_parameter(ctx, &existing.parameter)?,
                    existing.parameter_type,
                ))),
            };
            let key = candidate
                .parameter
                .id
                .try_clone_for_decode(ctx, "catia_formula_candidate_index_id")?;
            ctx.insert_btree_map(candidates, key, candidate, "catia_formula_candidates")?;
        }
        Some(existing)
            if existing.role.is_formula_output() && !candidate.role.is_formula_output() =>
        {
            if let FormulaParameterRole::FormulaOutput { fallback } = &mut existing.role {
                fallback.get_or_insert_with(|| {
                    Box::new((candidate.parameter, candidate.parameter_type))
                });
            }
        }
        Some(_) => {}
        None => {
            let key = candidate
                .parameter
                .id
                .try_clone_for_decode(ctx, "catia_formula_candidate_index_id")?;
            ctx.insert_btree_map(candidates, key, candidate, "catia_formula_candidates")?;
        }
    }
    Ok(())
}

fn relation_program_output_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    relation_entity: &crate::native::entity_record::CatiaEntityRecord,
    expression_entity: &crate::native::entity_record::CatiaEntityRecord,
    output_entity: &crate::native::entity_record::CatiaEntityRecord,
    expression: &crate::native::CatiaRelationExpression,
    inputs: &[crate::native::CatiaRelationProgramInput],
    entities: &HashMap<&str, &crate::native::entity_record::CatiaEntityRecord>,
) -> Result<Option<(FormulaProgramCandidate, FormulaParameterCandidate)>, cadmpeg_core::CodecError>
{
    let Some(signature) = expression.signature_charged(ctx)? else {
        return Ok(None);
    };
    if inputs.len() != signature.inputs.len()
        || inputs
            .iter()
            .zip(&signature.inputs)
            .any(|(input, declared)| {
                input.parameter != declared.parameter || input.value_type != declared.input_type
            })
    {
        return Ok(None);
    }

    let mut dependencies = Vec::new();
    let mut input_parameters = Vec::new();
    let mut expression_bindings = BTreeMap::new();
    let mut type_bindings = BTreeMap::new();
    let mut all_inputs_complete = true;
    for input in inputs {
        let Some(input_entity) = input.entity.entity().and_then(|id| entities.get(id)) else {
            return Ok(None);
        };
        let Some(candidate) =
            typed_entity_parameter_candidate_for_source(ctx, input_entity, &input.value_type)?
        else {
            return Ok(None);
        };
        if dependencies.contains(&candidate.parameter.id) {
            return Ok(None);
        }
        let dependency = candidate
            .parameter
            .id
            .try_clone_for_decode(ctx, "catia_relation_program_dependency_id")?;
        ctx.push_vec(
            &mut dependencies,
            dependency,
            "catia_relation_program_dependencies",
        )?;
        ctx.insert_btree_map(
            &mut type_bindings,
            input.parameter.as_str(),
            static_formula_value(candidate.parameter_type),
            "catia_relation_program_type_bindings",
        )?;
        match candidate.parameter.value.as_ref() {
            Some(value) => {
                ctx.insert_btree_map(
                    &mut expression_bindings,
                    input.parameter.as_str(),
                    EvaluatedFormulaValue::from_parameter_value_charged(ctx, value)?,
                    "catia_relation_program_expression_bindings",
                )?;
            }
            None => all_inputs_complete = false,
        }
        ctx.push_vec(
            &mut input_parameters,
            (candidate.parameter, candidate.parameter_type),
            "catia_relation_program_input_parameters",
        )?;
    }

    let type_checked_expression = evaluate_formula_expression_with_mode_charged(
        ctx,
        &expression.expression.value,
        &type_bindings,
        false,
    )?
    .filter(|value| {
        canonical_parameter_type(&signature.result_type)
            .is_some_and(|source_type| value.satisfies_source_type(source_type))
    });
    let evaluated_expression = (if all_inputs_complete {
        evaluate_formula_expression_charged(
            ctx,
            &expression.expression.value,
            &expression_bindings,
        )?
    } else {
        None
    })
    .filter(|value| {
        canonical_parameter_type(&signature.result_type)
            .is_some_and(|source_type| value.satisfies_source_type(source_type))
    });
    let Some(_) = (if all_inputs_complete {
        evaluated_expression.as_ref()
    } else {
        type_checked_expression.as_ref()
    }) else {
        return Ok(None);
    };
    let Some(output_value) = output_entity.parameter_value() else {
        return Ok(None);
    };
    let output_id = match neutral_parameter_id(ctx, &output_entity.id) {
        Ok(id) => id,
        Err(cadmpeg_core::CodecError::Malformed(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if dependencies.contains(&output_id) {
        return Ok(None);
    }
    let Some((parameter_type, value)) =
        typed_parameter_evaluation(&signature.result_type, &output_value.evaluation)
    else {
        return Ok(None);
    };
    let accepted = match &value {
        TypedParameterEvaluation::Unset => true,
        TypedParameterEvaluation::Value(value) => {
            if let Some(evaluated) = evaluated_expression.as_ref() {
                evaluated.agrees_with(&TypedParameterEvaluation::Value(
                    (value).try_clone_for_decode(ctx, "catia_formula_parameter_value_copy")?,
                ))
            } else {
                false
            }
        }
    };
    if !accepted {
        return Ok(None);
    }
    ctx.charge_entities(1, "admit CATIA formula candidate")?;
    let candidate_id =
        output_id.try_clone_for_decode(ctx, "catia_relation_program_candidate_id")?;
    let output_name = ctx.copy_retained_text(
        &output_value.name.value,
        "catia_relation_program_output_name",
    )?;
    let output_expression = ctx.copy_retained_text(
        &expression.expression.value,
        "catia_relation_program_output_expression",
    )?;
    let output_native_ref = ctx.copy_retained_text(
        &output_entity.id,
        "catia_relation_program_output_native_ref",
    )?;
    let mut output_dependencies = cadmpeg_ir::features::DistinctMembers::default();
    for dependency in &dependencies {
        if !output_dependencies.contains(dependency) {
            output_dependencies.insert(
                ctx,
                dependency
                    .try_clone_for_decode(ctx, "catia_relation_program_output_dependency_id")?,
                "catia_relation_program_output_dependencies",
            )?;
        }
    }
    let candidate = FormulaParameterCandidate {
        parameter: DesignParameter {
            id: candidate_id,
            owner: None,
            ordinal: 0,
            name: output_name,
            expression: output_expression,
            display: None,
            value: match value {
                TypedParameterEvaluation::Unset => None,
                TypedParameterEvaluation::Value(value) => Some(value),
            },
            dependencies: output_dependencies,
            properties: parameter_properties(
                ctx,
                parameter_type.as_str(),
                Some(output_value.binding.value.as_str()),
            )?,
            pmi: None,
            native_ref: Some(output_native_ref),
        },
        parameter_type,
        role: FormulaParameterRole::FormulaOutput { fallback: None },
        source_order: output_entity.byte_offset,
    };
    Ok(Some((
        FormulaProgramCandidate {
            relation_entity: ctx
                .copy_retained_text(&relation_entity.id, "catia_relation_program_relation_id")?,
            expression_entity: ctx.copy_retained_text(
                &expression_entity.id,
                "catia_relation_program_expression_id",
            )?,
            output: output_id,
            inputs: dependencies,
            input_parameters,
        },
        candidate,
    )))
}

fn formula_parameter_candidates_agree(
    existing: &FormulaParameterCandidate,
    candidate: &FormulaParameterCandidate,
) -> bool {
    if existing.source_order != candidate.source_order
        || existing.parameter_type != candidate.parameter_type
    {
        return false;
    }
    match (
        existing.role.is_formula_output(),
        candidate.role.is_formula_output(),
    ) {
        (true, true) | (false, false) => existing.parameter == candidate.parameter,
        (true, false) => formula_parameter_matches_input(&existing.parameter, &candidate.parameter),
        (false, true) => formula_parameter_matches_input(&candidate.parameter, &existing.parameter),
    }
}

fn formula_parameter_matches_input(formula: &DesignParameter, input: &DesignParameter) -> bool {
    formula.id == input.id
        && formula.owner == input.owner
        && formula.ordinal == input.ordinal
        && formula.name == input.name
        && formula.display == input.display
        && formula.value == input.value
        && formula.properties == input.properties
        && formula.pmi == input.pmi
        && formula.native_ref == input.native_ref
}

fn formula_parameter_candidate_accepts_input(
    candidate: &FormulaParameterCandidate,
    input: &(DesignParameter, FormulaParameterType),
) -> bool {
    if candidate.parameter_type != input.1 {
        return false;
    }
    if candidate.role.is_formula_output() {
        formula_parameter_matches_input(&candidate.parameter, &input.0)
    } else {
        candidate.parameter == input.0
    }
}

fn demote_formula_output(candidate: &mut FormulaParameterCandidate) -> bool {
    let FormulaParameterRole::FormulaOutput {
        fallback: Some(input),
    } = std::mem::replace(&mut candidate.role, FormulaParameterRole::Input)
    else {
        candidate.role = FormulaParameterRole::Input;
        return false;
    };
    let (input, parameter_type) = *input;
    candidate.parameter = input;
    candidate.parameter_type = parameter_type;
    true
}

enum TypedParameterEvaluation {
    Unset,
    Value(ParameterValue),
}

fn parameter_expression(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &ParameterValue,
) -> Result<String, cadmpeg_core::CodecError> {
    let operation = "catia_formula_parameter_expression";
    match value {
        ParameterValue::Length(value) => {
            let value = value.get();
            ctx.format_retained(format_args!("{value} mm"), operation)
        }
        ParameterValue::Angle(value) => {
            let value = value.get();
            ctx.format_retained(format_args!("{value} rad"), operation)
        }
        ParameterValue::Real(value) => {
            ctx.format_retained(format_args!("{}", value.get()), operation)
        }
        ParameterValue::Integer(value) => ctx.format_retained(format_args!("{value}"), operation),
        ParameterValue::Boolean(value) => ctx.format_retained(format_args!("{value}"), operation),
        ParameterValue::String(value) => {
            Ok(string_literal_expression(ctx, value)?.unwrap_or_default())
        }
    }
}

fn copy_design_parameter(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &DesignParameter,
) -> Result<DesignParameter, cadmpeg_core::CodecError> {
    let operation = "catia_formula_parameter_copy";
    let id = source.id.try_clone_for_decode(ctx, operation)?;
    let owner = source
        .owner
        .as_ref()
        .map(|owner| owner.try_clone_for_decode(ctx, operation))
        .transpose()?;
    let name = ctx.copy_retained_text(&source.name, operation)?;
    let expression = ctx.copy_retained_text(&source.expression, operation)?;
    let value = source
        .value
        .as_ref()
        .map(|value| (value).try_clone_for_decode(ctx, "catia_formula_parameter_value_copy"))
        .transpose()?;
    let mut dependencies = cadmpeg_ir::features::DistinctMembers::default();
    if !source.dependencies.is_empty() {
        dependencies.reserve_for_decode(ctx, source.dependencies.len(), operation)?;
        for dependency in &source.dependencies {
            dependencies.insert(
                ctx,
                dependency.try_clone_for_decode(ctx, operation)?,
                operation,
            )?;
        }
    }
    let mut properties = BTreeMap::new();
    for (key, value) in &source.properties {
        let key = ctx.copy_retained_text(key.as_str(), operation)?;
        let key = cadmpeg_core::text::NonBlankString::new(key)
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("blank CATIA parameter property"))?;
        let value = ctx.copy_retained_text(value, operation)?;
        ctx.insert_btree_map(&mut properties, key, value, operation)?;
    }
    let pmi = match &source.pmi {
        Some(pmi) => {
            let subtype = match &pmi.subtype {
                cadmpeg_ir::features::PmiDimensionSubtype::Native(kind) => {
                    cadmpeg_ir::features::PmiDimensionSubtype::Native(
                        ctx.copy_retained_text(kind, operation)?,
                    )
                }
                other => other.clone(),
            };
            Some(cadmpeg_ir::features::ParameterPmi {
                subtype,
                precision: pmi.precision,
                display_text: pmi
                    .display_text
                    .as_ref()
                    .map(|value| ctx.copy_retained_text(value, operation))
                    .transpose()?,
                basic: pmi.basic,
                inspection: pmi.inspection,
                reference_only: pmi.reference_only,
                native_ref: ctx.copy_retained_text(&pmi.native_ref, operation)?,
            })
        }
        None => None,
    };
    let native_ref = source
        .native_ref
        .as_ref()
        .map(|value| ctx.copy_retained_text(value, operation))
        .transpose()?;
    Ok(DesignParameter {
        id,
        owner,
        ordinal: source.ordinal,
        name,
        expression,
        display: source.display,
        value,
        dependencies,
        properties,
        pmi,
        native_ref,
    })
}

fn string_literal_expression(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &str,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    if !value
        .chars()
        .all(|character| character != '"' && character != '\\' && !character.is_control())
    {
        return Ok(None);
    }
    ctx.format_retained(format_args!("\"{value}\""), "catia_formula_string_literal")
        .map(Some)
}

fn parameter_properties(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameter_type: &'static str,
    binding: Option<&str>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, cadmpeg_core::CodecError> {
    let mut properties = BTreeMap::new();
    insert_parameter_property(
        ctx,
        &mut properties,
        "value_type",
        format_args!("{parameter_type}"),
    )?;
    if let Some(binding) = binding {
        insert_parameter_property(
            ctx,
            &mut properties,
            "catia_binding",
            format_args!("{binding}"),
        )?;
    }
    Ok(properties)
}

fn insert_parameter_property(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: &str,
    value: std::fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let operation = "catia_formula_property";
    let key = ctx.copy_retained_text(key, operation)?;
    let key = cadmpeg_core::text::NonBlankString::new(key)
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("empty CATIA formula property key"))?;
    let value = ctx.format_retained(value, operation)?;
    ctx.insert_btree_map(properties, key, value, operation)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FormulaDimension {
    length: i32,
    angle: i32,
}

impl FormulaDimension {
    const SCALAR: Self = Self {
        length: 0,
        angle: 0,
    };
    const LENGTH: Self = Self {
        length: 1,
        angle: 0,
    };
    const ANGLE: Self = Self {
        length: 0,
        angle: 1,
    };

    fn product(self, right: Self) -> Option<Self> {
        Some(Self {
            length: self.length.checked_add(right.length)?,
            angle: self.angle.checked_add(right.angle)?,
        })
    }

    fn quotient(self, right: Self) -> Option<Self> {
        Some(Self {
            length: self.length.checked_sub(right.length)?,
            angle: self.angle.checked_sub(right.angle)?,
        })
    }

    fn square_root(self) -> Option<Self> {
        (self.length % 2 == 0 && self.angle % 2 == 0).then_some(Self {
            length: self.length / 2,
            angle: self.angle / 2,
        })
    }

    fn power(self, exponent: i32) -> Option<Self> {
        Some(Self {
            length: self.length.checked_mul(exponent)?,
            angle: self.angle.checked_mul(exponent)?,
        })
    }
}

fn formula_unit(unit: &str) -> Option<(FormulaDimension, f64)> {
    match unit {
        "micron" => Some((FormulaDimension::LENGTH, 0.001)),
        "mm" => Some((FormulaDimension::LENGTH, 1.0)),
        "cm" => Some((FormulaDimension::LENGTH, 10.0)),
        "m" => Some((FormulaDimension::LENGTH, 1_000.0)),
        "km" => Some((FormulaDimension::LENGTH, 1_000_000.0)),
        "in" => Some((FormulaDimension::LENGTH, 25.4)),
        "ft" => Some((FormulaDimension::LENGTH, 304.8)),
        "yard" => Some((FormulaDimension::LENGTH, 914.4)),
        "mile" => Some((FormulaDimension::LENGTH, 1_609_344.0)),
        "rad" => Some((FormulaDimension::ANGLE, 1.0)),
        "grad" => Some((FormulaDimension::ANGLE, std::f64::consts::PI / 200.0)),
        "deg" => Some((FormulaDimension::ANGLE, std::f64::consts::PI / 180.0)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum EvaluatedFormulaScalar {
    Static {
        value: f64,
        dimension: FormulaDimension,
        integral: Option<bool>,
    },
    Known {
        value: f64,
        dimension: FormulaDimension,
        integral: Option<bool>,
    },
}

impl EvaluatedFormulaScalar {
    fn from_parts(
        value: f64,
        dimension: FormulaDimension,
        integral: Option<bool>,
        known_value: Option<f64>,
    ) -> Self {
        match known_value {
            Some(value) => Self::Known {
                value,
                dimension,
                integral,
            },
            None => Self::Static {
                value,
                dimension,
                integral,
            },
        }
    }

    fn value(self) -> f64 {
        match self {
            Self::Static { value, .. } | Self::Known { value, .. } => value,
        }
    }

    fn dimension(self) -> FormulaDimension {
        match self {
            Self::Static { dimension, .. } | Self::Known { dimension, .. } => dimension,
        }
    }

    fn integral(self) -> Option<bool> {
        match self {
            Self::Static { integral, .. } | Self::Known { integral, .. } => integral,
        }
    }

    fn known_value(self) -> Option<f64> {
        match self {
            Self::Known { value, .. } => Some(value),
            Self::Static { .. } => None,
        }
    }

    fn satisfies_source_type(self, source_type: FormulaParameterType) -> bool {
        match source_type {
            FormulaParameterType::Length => self.dimension() == FormulaDimension::LENGTH,
            FormulaParameterType::Angle => self.dimension() == FormulaDimension::ANGLE,
            FormulaParameterType::Real => self.dimension() == FormulaDimension::SCALAR,
            FormulaParameterType::Integer => {
                self.dimension() == FormulaDimension::SCALAR
                    && self.integral() == Some(true)
                    && self
                        .known_value()
                        .is_none_or(|value| value >= FORMULA_INTEGER_LOWER)
                    && self
                        .known_value()
                        .is_none_or(|value| value < FORMULA_INTEGER_UPPER)
            }
            FormulaParameterType::Boolean | FormulaParameterType::String => false,
        }
    }
}

#[derive(Clone)]
enum EvaluatedFormulaValue {
    Scalar(EvaluatedFormulaScalar),
    Boolean(EvaluatedFormulaBoolean),
    String(EvaluatedFormulaString),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EvaluatedFormulaBoolean {
    Known(bool),
    Unknown,
}

impl EvaluatedFormulaBoolean {
    fn known(value: bool) -> Self {
        Self::Known(value)
    }

    fn unknown() -> Self {
        Self::Unknown
    }

    fn value(self) -> bool {
        match self {
            Self::Known(value) => value,
            Self::Unknown => false,
        }
    }

    fn known_value(self) -> Option<bool> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown => None,
        }
    }

    fn is_known(self) -> bool {
        matches!(self, Self::Known(_))
    }

    fn not(self) -> Self {
        self.known_value()
            .map_or(Self::Unknown, |value| Self::Known(!value))
    }

    fn and(self, right: Self) -> Self {
        match (self.known_value(), right.known_value()) {
            (Some(false), _) | (_, Some(false)) => Self::Known(false),
            (Some(true), Some(true)) => Self::Known(true),
            _ => Self::Unknown,
        }
    }

    fn or(self, right: Self) -> Self {
        match (self.known_value(), right.known_value()) {
            (Some(true), _) | (_, Some(true)) => Self::Known(true),
            (Some(false), Some(false)) => Self::Known(false),
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone)]
enum EvaluatedFormulaString {
    Known(String),
    Unknown,
}

impl EvaluatedFormulaString {
    fn known(value: impl Into<String>) -> Self {
        Self::Known(value.into())
    }

    fn unknown() -> Self {
        Self::Unknown
    }

    fn from_parts(value: String, known: bool) -> Self {
        if known {
            Self::Known(value)
        } else {
            Self::Unknown
        }
    }

    fn is_known(&self) -> bool {
        matches!(self, Self::Known(_))
    }

    fn value(&self) -> &str {
        match self {
            Self::Known(value) => value.as_str(),
            Self::Unknown => "",
        }
    }
}

impl EvaluatedFormulaValue {
    fn copy_charged(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(match self {
            Self::Scalar(value) => Self::Scalar(*value),
            Self::Boolean(value) => Self::Boolean(*value),
            Self::String(EvaluatedFormulaString::Known(value)) => {
                Self::String(EvaluatedFormulaString::Known(
                    ctx.copy_retained_text(value, "catia_formula_evaluated_value_copy")?,
                ))
            }
            Self::String(EvaluatedFormulaString::Unknown) => {
                Self::String(EvaluatedFormulaString::Unknown)
            }
        })
    }

    fn from_parameter_value_charged(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: &ParameterValue,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        match value {
            ParameterValue::String(text) => Ok(Self::String(EvaluatedFormulaString::known(
                ctx.copy_retained_text(text, "catia_formula_evaluated_string")?,
            ))),
            other => Ok(Self::from_parameter_value(other)),
        }
    }

    fn from_parameter_value(value: &ParameterValue) -> Self {
        match value {
            ParameterValue::Length(value) => {
                let value = value.get();

                Self::Scalar(EvaluatedFormulaScalar::from_parts(
                    value,
                    FormulaDimension::LENGTH,
                    Some(value.fract() == 0.0),
                    Some(value),
                ))
            }
            ParameterValue::Angle(value) => {
                let value = value.get();

                Self::Scalar(EvaluatedFormulaScalar::from_parts(
                    value,
                    FormulaDimension::ANGLE,
                    Some(value.fract() == 0.0),
                    Some(value),
                ))
            }
            ParameterValue::Real(value) => Self::Scalar(EvaluatedFormulaScalar::from_parts(
                value.get(),
                FormulaDimension::SCALAR,
                Some(value.get().fract() == 0.0),
                Some(value.get()),
            )),
            ParameterValue::Integer(value) => Self::Scalar(match f64_from_i64(*value) {
                Some(value) => EvaluatedFormulaScalar::from_parts(
                    value,
                    FormulaDimension::SCALAR,
                    Some(true),
                    Some(value),
                ),
                None => static_integral_result(0.0, FormulaDimension::SCALAR),
            }),
            ParameterValue::Boolean(value) => Self::Boolean(EvaluatedFormulaBoolean::known(*value)),
            ParameterValue::String(_) => Self::String(EvaluatedFormulaString::unknown()),
        }
    }

    fn scalar(self) -> Option<EvaluatedFormulaScalar> {
        match self {
            Self::Scalar(value) => Some(value),
            Self::Boolean(_) | Self::String(_) => None,
        }
    }

    fn boolean(self) -> Option<EvaluatedFormulaBoolean> {
        match self {
            Self::Boolean(value) => Some(value),
            Self::Scalar(_) | Self::String(_) => None,
        }
    }

    #[cfg(test)]
    fn string(self) -> Option<String> {
        match self {
            Self::String(value) => Some(value.value().to_owned()),
            Self::Scalar(_) | Self::Boolean(_) => None,
        }
    }

    fn satisfies_source_type(&self, source_type: FormulaParameterType) -> bool {
        match self {
            Self::Scalar(value) => value.satisfies_source_type(source_type),
            Self::Boolean(_) => source_type == FormulaParameterType::Boolean,
            Self::String(_) => source_type == FormulaParameterType::String,
        }
    }

    fn agrees_with(&self, evaluation: &TypedParameterEvaluation) -> bool {
        match evaluation {
            TypedParameterEvaluation::Unset => true,
            TypedParameterEvaluation::Value(value) => {
                if let (Self::String(left), ParameterValue::String(right)) = (self, value) {
                    return left.is_known() && left.value() == right;
                }
                match (self, Self::from_parameter_value(value)) {
                    (Self::Boolean(left), Self::Boolean(right)) => {
                        left.known_value() == right.known_value()
                    }
                    (Self::String(left), Self::String(right)) => {
                        left.is_known() && left.value() == right.value()
                    }
                    (Self::Scalar(left), Self::Scalar(right)) => {
                        left.dimension() == right.dimension()
                            && left.known_value() == right.known_value()
                    }
                    _ => false,
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ComparisonOperator {
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
}

impl ComparisonOperator {
    fn parse(input: &str) -> Option<(Self, usize)> {
        [
            ("==", Self::Eq),
            ("<>", Self::Ne),
            (">=", Self::Ge),
            ("<=", Self::Le),
            (">", Self::Gt),
            ("<", Self::Lt),
        ]
        .into_iter()
        .find_map(|(text, operator)| input.starts_with(text).then_some((operator, text.len())))
    }
}

struct ReplacedText<'a> {
    source: &'a str,
    from: &'a str,
    to: &'a str,
}

impl std::fmt::Display for ReplacedText<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.from.is_empty() {
            use std::fmt::Write;
            formatter.write_str(self.to)?;
            for character in self.source.chars() {
                formatter.write_char(character)?;
                formatter.write_str(self.to)?;
            }
            return Ok(());
        }
        let mut rest = self.source;
        while let Some(index) = rest.find(self.from) {
            formatter.write_str(&rest[..index])?;
            formatter.write_str(self.to)?;
            rest = &rest[index + self.from.len()..];
        }
        formatter.write_str(rest)
    }
}

struct CasedText<'a> {
    source: &'a str,
    upper: bool,
}

impl std::fmt::Display for CasedText<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::fmt::Write;
        for character in self.source.chars() {
            if self.upper {
                for mapped in character.to_uppercase() {
                    formatter.write_char(mapped)?;
                }
            } else {
                for mapped in character.to_lowercase() {
                    formatter.write_char(mapped)?;
                }
            }
        }
        Ok(())
    }
}

struct FormulaExpressionParser<'a, 'b, 'c, 'd> {
    source: &'a str,
    at: usize,
    bindings: &'b BTreeMap<&'a str, EvaluatedFormulaValue>,
    ctx: &'c cadmpeg_core::decode::DecodeContext<'d>,
    refusal: Option<cadmpeg_core::CodecError>,
    evaluate: bool,
    static_check: bool,
}

const MAX_FORMULA_EXPRESSION_DEPTH: usize = 128;
const MAX_FORMULA_FUNCTION_ARGUMENTS: usize = 128;

fn finite_scalar(value: f64) -> Option<EvaluatedFormulaScalar> {
    value
        .is_finite()
        .then_some(EvaluatedFormulaScalar::from_parts(
            value,
            FormulaDimension::SCALAR,
            finite_integrality(value),
            Some(value),
        ))
}

fn finite_angle(value: f64) -> Option<EvaluatedFormulaScalar> {
    value
        .is_finite()
        .then_some(EvaluatedFormulaScalar::from_parts(
            value,
            FormulaDimension::ANGLE,
            finite_integrality(value),
            Some(value),
        ))
}

fn finite_integrality(value: f64) -> Option<bool> {
    value.is_finite().then_some(value.fract() == 0.0)
}

fn static_integral_result(value: f64, dimension: FormulaDimension) -> EvaluatedFormulaScalar {
    EvaluatedFormulaScalar::from_parts(value, dimension, Some(true), None)
}

fn static_unknown_result(value: f64, dimension: FormulaDimension) -> EvaluatedFormulaScalar {
    EvaluatedFormulaScalar::from_parts(value, dimension, None, None)
}

fn static_all_integral(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    (left == Some(true) && right == Some(true)).then_some(true)
}

impl FormulaExpressionParser<'_, '_, '_, '_> {
    fn admit<T>(&mut self, result: Result<T, cadmpeg_core::CodecError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.refusal = Some(error);
                None
            }
        }
    }

    fn parse(mut self) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let value = self.conditional(0);
        if let Some(error) = self.refusal {
            return Err(error);
        }
        let Some(value) = value else { return Ok(None) };
        self.skip_whitespace();
        Ok((self.at == self.source.len()).then_some(value))
    }

    fn conditional(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let predicate = self.disjunction(depth)?;
        self.skip_whitespace();
        if self.peek() != Some(b'?') {
            return Some(predicate);
        }
        self.at += 1;
        let predicate = predicate.boolean()?;
        let evaluate = self.evaluate;
        let static_check = self.static_check;
        self.evaluate = evaluate && predicate.value();
        self.static_check = static_check && (!predicate.is_known() || predicate.value());
        let when_true = self.descend(depth, Self::conditional)?;
        self.skip_whitespace();
        (self.peek()? == b';').then_some(())?;
        self.at += 1;
        self.evaluate = evaluate && !predicate.value();
        self.static_check = static_check && (!predicate.is_known() || !predicate.value());
        let when_false = self.descend(depth, Self::conditional)?;
        self.evaluate = evaluate;
        self.static_check = static_check;
        Self::same_value_type(&when_true, &when_false)?;
        if evaluate {
            return Some(if predicate.value() {
                when_true
            } else {
                when_false
            });
        }
        Some(if let Some(predicate) = predicate.known_value() {
            if predicate {
                when_true
            } else {
                when_false
            }
        } else {
            self.merge_static_values(&when_true, &when_false)?
        })
    }

    fn merge_static_values(
        &mut self,
        left: &EvaluatedFormulaValue,
        right: &EvaluatedFormulaValue,
    ) -> Option<EvaluatedFormulaValue> {
        match (left, right) {
            (EvaluatedFormulaValue::Scalar(left), EvaluatedFormulaValue::Scalar(right))
                if left.dimension() == right.dimension() =>
            {
                Some(EvaluatedFormulaValue::Scalar(
                    EvaluatedFormulaScalar::from_parts(
                        0.0,
                        left.dimension(),
                        match (left.integral(), right.integral()) {
                            (Some(left), Some(right)) if left == right => Some(left),
                            _ => None,
                        },
                        match (left.known_value(), right.known_value()) {
                            (Some(left), Some(right)) if left == right => Some(left),
                            _ => None,
                        },
                    ),
                ))
            }
            (EvaluatedFormulaValue::Boolean(left), EvaluatedFormulaValue::Boolean(right)) => Some(
                EvaluatedFormulaValue::Boolean(match (left.known_value(), right.known_value()) {
                    (Some(left), Some(right)) if left == right => {
                        EvaluatedFormulaBoolean::known(left)
                    }
                    _ => EvaluatedFormulaBoolean::unknown(),
                }),
            ),
            (EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) => {
                Some(EvaluatedFormulaValue::String(
                    if left.is_known() && right.is_known() && left.value() == right.value() {
                        let result = self
                            .ctx
                            .copy_retained_text(left.value(), "catia_formula_static_string_merge");
                        EvaluatedFormulaString::known(self.admit(result)?)
                    } else {
                        EvaluatedFormulaString::unknown()
                    },
                ))
            }
            _ => None,
        }
    }

    fn same_value_type(left: &EvaluatedFormulaValue, right: &EvaluatedFormulaValue) -> Option<()> {
        match (left, right) {
            (EvaluatedFormulaValue::Scalar(left), EvaluatedFormulaValue::Scalar(right))
                if left.dimension() == right.dimension() =>
            {
                Some(())
            }
            (EvaluatedFormulaValue::Boolean(_), EvaluatedFormulaValue::Boolean(_))
            | (EvaluatedFormulaValue::String(_), EvaluatedFormulaValue::String(_)) => Some(()),
            _ => None,
        }
    }

    fn scalar_result(&self, value: f64) -> Option<EvaluatedFormulaScalar> {
        if self.evaluate {
            finite_scalar(value)
        } else {
            Some(static_unknown_result(0.0, FormulaDimension::SCALAR))
        }
    }

    fn angle_result(&self, value: f64) -> Option<EvaluatedFormulaScalar> {
        if self.evaluate {
            finite_angle(value)
        } else {
            Some(static_unknown_result(0.0, FormulaDimension::ANGLE))
        }
    }

    fn integral_result(&self, value: f64) -> Option<EvaluatedFormulaScalar> {
        if self.evaluate {
            finite_scalar(value)
        } else {
            Some(static_integral_result(0.0, FormulaDimension::SCALAR))
        }
    }

    fn disjunction(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.conjunction(depth)?;
        loop {
            self.skip_whitespace();
            if !self.consume_keyword("or") {
                return Some(value);
            }
            let evaluate = self.evaluate;
            let static_check = self.static_check;
            let left = value.boolean()?;
            self.evaluate = evaluate && !left.value();
            self.static_check = static_check && (!left.is_known() || !left.value());
            let right = self.conjunction(depth)?;
            let right = right.boolean()?;
            self.evaluate = evaluate;
            self.static_check = static_check;
            value = EvaluatedFormulaValue::Boolean(if evaluate {
                EvaluatedFormulaBoolean::known(left.value() || right.value())
            } else {
                left.or(right)
            });
        }
    }

    fn conjunction(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.comparison(depth)?;
        loop {
            self.skip_whitespace();
            if !self.consume_keyword("and") {
                return Some(value);
            }
            let evaluate = self.evaluate;
            let static_check = self.static_check;
            let left = value.boolean()?;
            self.evaluate = evaluate && left.value();
            self.static_check = static_check && (!left.is_known() || left.value());
            let right = self.comparison(depth)?;
            let right = right.boolean()?;
            self.evaluate = evaluate;
            self.static_check = static_check;
            value = EvaluatedFormulaValue::Boolean(if evaluate {
                EvaluatedFormulaBoolean::known(left.value() && right.value())
            } else {
                left.and(right)
            });
        }
    }

    fn comparison(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let left = self.sum(depth)?;
        self.skip_whitespace();
        let Some((operator, width)) = ComparisonOperator::parse(self.remaining()) else {
            return Some(left);
        };
        self.at += width;
        let right = self.sum(depth)?;
        let (value, known) = match (operator, left, right) {
            (
                ComparisonOperator::Eq,
                EvaluatedFormulaValue::Boolean(left),
                EvaluatedFormulaValue::Boolean(right),
            ) => (
                left.value() == right.value(),
                left.is_known() && right.is_known(),
            ),
            (
                ComparisonOperator::Ne,
                EvaluatedFormulaValue::Boolean(left),
                EvaluatedFormulaValue::Boolean(right),
            ) => (
                left.value() != right.value(),
                left.is_known() && right.is_known(),
            ),
            (
                ComparisonOperator::Eq,
                EvaluatedFormulaValue::String(left),
                EvaluatedFormulaValue::String(right),
            ) => (
                left.value() == right.value(),
                left.is_known() && right.is_known(),
            ),
            (
                ComparisonOperator::Ne,
                EvaluatedFormulaValue::String(left),
                EvaluatedFormulaValue::String(right),
            ) => (
                left.value() != right.value(),
                left.is_known() && right.is_known(),
            ),
            (
                operator,
                EvaluatedFormulaValue::Scalar(left),
                EvaluatedFormulaValue::Scalar(right),
            ) if left.dimension() == right.dimension() => (
                match operator {
                    ComparisonOperator::Eq => left.value() == right.value(),
                    ComparisonOperator::Ne => left.value() != right.value(),
                    ComparisonOperator::Ge => left.value() >= right.value(),
                    ComparisonOperator::Le => left.value() <= right.value(),
                    ComparisonOperator::Gt => left.value() > right.value(),
                    ComparisonOperator::Lt => left.value() < right.value(),
                },
                left.known_value().is_some() && right.known_value().is_some(),
            ),
            _ => return None,
        };
        Some(EvaluatedFormulaValue::Boolean(if known {
            EvaluatedFormulaBoolean::known(value)
        } else {
            EvaluatedFormulaBoolean::unknown()
        }))
    }

    fn sum(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.product(depth)?;
        loop {
            self.skip_whitespace();
            let Some(operator) = self.peek() else {
                return Some(value);
            };
            if !matches!(operator, b'+' | b'-') {
                return Some(value);
            }
            self.at += 1;
            let right = self.product(depth)?;
            if operator == b'+' {
                if let (EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) =
                    (&value, &right)
                {
                    let known = left.is_known() && right.is_known();
                    let joined = if known {
                        let formatted = self.ctx.format_retained(
                            format_args!("{}{}", left.value(), right.value()),
                            "catia_formula_string_concat",
                        );
                        self.admit(formatted)?
                    } else {
                        String::new()
                    };
                    value = EvaluatedFormulaValue::String(if known {
                        EvaluatedFormulaString::known(joined)
                    } else {
                        EvaluatedFormulaString::unknown()
                    });
                    continue;
                }
            }
            if operator == b'-' {
                if let (EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) =
                    (&value, &right)
                {
                    if (self.evaluate || self.static_check)
                        && right.is_known()
                        && right.value().is_empty()
                    {
                        return None;
                    }
                    let known = left.is_known() && right.is_known() && !right.value().is_empty();
                    let string_value = if known {
                        let formatted = self.ctx.format_retained(
                            format_args!(
                                "{}",
                                ReplacedText {
                                    source: left.value(),
                                    from: right.value(),
                                    to: "",
                                }
                            ),
                            "catia_formula_string_subtract",
                        );
                        self.admit(formatted)?
                    } else {
                        String::new()
                    };
                    value = EvaluatedFormulaValue::String(if known {
                        EvaluatedFormulaString::known(string_value)
                    } else {
                        EvaluatedFormulaString::unknown()
                    });
                    continue;
                }
            }
            let left = value.scalar()?;
            let right = right.scalar()?;
            if left.dimension() != right.dimension() {
                return None;
            }
            let left_known = left.known_value();
            let right_known = right.known_value();
            let integral = if self.evaluate {
                None
            } else {
                static_all_integral(left.integral(), right.integral())
            };
            let result_value = if operator == b'+' {
                left.value() + right.value()
            } else {
                left.value() - right.value()
            };
            if self.evaluate && !result_value.is_finite() {
                return None;
            }
            let known_value = if self.evaluate {
                Some(result_value)
            } else {
                left_known
                    .zip(right_known)
                    .map(|(left, right)| {
                        if operator == b'+' {
                            left + right
                        } else {
                            left - right
                        }
                    })
                    .filter(|value| value.is_finite())
            };
            if self.static_check
                && left_known.is_some()
                && right_known.is_some()
                && known_value.is_none()
            {
                return None;
            }
            value = EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar::from_parts(
                if result_value.is_finite() {
                    result_value
                } else {
                    0.0
                },
                left.dimension(),
                if self.evaluate {
                    finite_integrality(result_value)
                } else {
                    integral
                },
                known_value,
            ));
        }
    }

    fn product(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.unary(depth)?;
        loop {
            self.skip_whitespace();
            let Some(operator) = self.peek() else {
                return Some(value);
            };
            if !matches!(operator, b'*' | b'/') {
                return Some(value);
            }
            self.at += 1;
            let left = value.scalar()?;
            let right = self.unary(depth)?.scalar()?;
            let left_known = left.known_value();
            let right_known = right.known_value();
            if self.static_check
                && operator == b'/'
                && right_known.is_some_and(|value| value == 0.0)
            {
                return None;
            }
            let known_value = if operator == b'*' {
                left_known
                    .zip(right_known)
                    .map(|(left, right)| left * right)
                    .filter(|value| value.is_finite())
            } else if right.value() == 0.0 {
                None
            } else if self.evaluate {
                Some(left.value() / right.value())
            } else {
                left_known
                    .zip(right_known)
                    .map(|(left, right)| left / right)
                    .filter(|value| value.is_finite())
            };
            if self.static_check
                && left_known.is_some()
                && right_known.is_some()
                && known_value.is_none()
            {
                return None;
            }
            let result = if operator == b'*' {
                EvaluatedFormulaScalar::from_parts(
                    left.value() * right.value(),
                    left.dimension().product(right.dimension())?,
                    if self.evaluate {
                        None
                    } else {
                        static_all_integral(left.integral(), right.integral())
                    },
                    known_value,
                )
            } else {
                if self.evaluate && right.value() == 0.0 {
                    return None;
                }
                EvaluatedFormulaScalar::from_parts(
                    if right.value() == 0.0 {
                        0.0
                    } else {
                        left.value() / right.value()
                    },
                    left.dimension().quotient(right.dimension())?,
                    None,
                    known_value,
                )
            };
            if self.evaluate && !result.value().is_finite() {
                return None;
            }
            value = EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar::from_parts(
                if result.value().is_finite() {
                    result.value()
                } else {
                    0.0
                },
                result.dimension(),
                if self.evaluate {
                    finite_integrality(result.value())
                } else {
                    result.integral()
                },
                result.known_value(),
            ));
        }
    }

    fn unary(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        self.skip_whitespace();
        if self.consume_keyword("not") {
            let value = self.descend(depth, Self::unary)?.boolean()?;
            return Some(EvaluatedFormulaValue::Boolean(value.not()));
        }
        match self.peek()? {
            b'+' => {
                self.at += 1;
                self.descend(depth, Self::unary)
                    .and_then(EvaluatedFormulaValue::scalar)
                    .map(EvaluatedFormulaValue::Scalar)
            }
            b'-' => {
                self.at += 1;
                let value = self.descend(depth, Self::unary)?.scalar()?;
                Some(EvaluatedFormulaValue::Scalar(
                    EvaluatedFormulaScalar::from_parts(
                        -value.value(),
                        value.dimension(),
                        value.integral(),
                        value.known_value().map(|value| -value),
                    ),
                ))
            }
            _ => self.power(depth),
        }
    }

    fn power(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let base = self.postfix(depth)?;
        self.skip_whitespace();
        if !self.remaining().starts_with("**") {
            return Some(base);
        }
        self.at += 2;
        let base = base.scalar()?;
        let exponent = self.descend(depth, Self::unary)?.scalar()?;
        if exponent.dimension() != FormulaDimension::SCALAR {
            return None;
        }

        let dimension = if base.dimension() == FormulaDimension::SCALAR {
            FormulaDimension::SCALAR
        } else {
            let exponent_value = exponent.known_value()?;
            if exponent_value.fract() != 0.0
                || exponent_value < f64::from(i32::MIN)
                || exponent_value > f64::from(i32::MAX)
            {
                return None;
            }
            base.dimension()
                .power(truncate_f64_to_i32(exponent_value)?)?
        };
        let value = base.value().powf(exponent.value());
        let known_value = if self.evaluate {
            Some(value)
        } else {
            base.known_value()
                .zip(exponent.known_value())
                .map(|(base, exponent)| base.powf(exponent))
                .filter(|value| value.is_finite())
        };
        if self.static_check
            && base.known_value().is_some()
            && exponent.known_value().is_some()
            && known_value.is_none()
        {
            return None;
        }
        (value.is_finite() || !self.evaluate).then_some(EvaluatedFormulaValue::Scalar(
            EvaluatedFormulaScalar::from_parts(
                if value.is_finite() { value } else { 0.0 },
                dimension,
                if self.evaluate {
                    finite_integrality(value)
                } else {
                    None
                },
                known_value,
            ),
        ))
    }

    fn primary(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        self.skip_whitespace();
        if self.peek()? == b'"' {
            return self
                .string_literal()
                .map(EvaluatedFormulaString::known)
                .map(EvaluatedFormulaValue::String);
        }
        if self.consume_keyword("true") {
            return Some(EvaluatedFormulaValue::Boolean(
                EvaluatedFormulaBoolean::known(true),
            ));
        }
        if self.consume_keyword("false") {
            return Some(EvaluatedFormulaValue::Boolean(
                EvaluatedFormulaBoolean::known(false),
            ));
        }
        if self.peek()? == b'(' {
            self.at += 1;
            let value = self.descend(depth, Self::conditional)?;
            self.skip_whitespace();
            (self.peek()? == b')').then_some(())?;
            self.at += 1;
            return Some(value);
        }
        if self.peek()? == b'#' {
            return self.symbol();
        }
        if self.remaining().starts_with("PI")
            && self
                .source
                .as_bytes()
                .get(self.at + 2)
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        {
            self.at += 2;
            return Some(EvaluatedFormulaValue::Scalar(
                EvaluatedFormulaScalar::from_parts(
                    std::f64::consts::PI,
                    FormulaDimension::SCALAR,
                    Some(false),
                    Some(std::f64::consts::PI),
                ),
            ));
        }
        if self.remaining().starts_with('E')
            && self
                .source
                .as_bytes()
                .get(self.at + 1)
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        {
            self.at += 1;
            return finite_scalar(std::f64::consts::E).map(EvaluatedFormulaValue::Scalar);
        }
        if self.peek()?.is_ascii_alphabetic() {
            return self.function_call(depth);
        }
        self.literal().map(EvaluatedFormulaValue::Scalar)
    }

    fn postfix(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.primary(depth)?;
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'.') {
                return Some(value);
            }
            self.at += 1;
            let method_start = self.at;
            while self
                .peek()
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                self.at += 1;
            }
            (self.at > method_start).then_some(())?;
            let method = &self.source[method_start..self.at];
            let mut argument_storage =
                self.admit(self.ctx.reserve_scoped(0, "CATIA formula method arguments"))?;
            let parsed = argument_storage.with_storage(|| {
                Ok::<_, cadmpeg_core::CodecError>(self.descend(depth, Self::function_arguments))
            });
            let arguments = self.admit(parsed)??;
            value = match (method, value, arguments.as_slice()) {
                ("Length", EvaluatedFormulaValue::String(value), []) => {
                    self.admit(self.ctx.charge_work(
                        u64_from_index(value.value().len()),
                        "catia_formula_string_length",
                    ))?;
                    let length = u32::try_from(value.value().chars().count()).ok()?;
                    EvaluatedFormulaValue::Scalar(
                        if self.evaluate || (self.static_check && value.is_known()) {
                            finite_scalar(f64::from(length))?
                        } else {
                            static_integral_result(0.0, FormulaDimension::SCALAR)
                        },
                    )
                }
                (
                    "Search",
                    EvaluatedFormulaValue::String(value),
                    [EvaluatedFormulaValue::String(needle)],
                ) => EvaluatedFormulaValue::Scalar(
                    if self.evaluate || (self.static_check && value.is_known() && needle.is_known())
                    {
                        let index = self.search_string(value.value(), needle.value(), 0, true)?;
                        finite_scalar(f64_from_i64(index)?)?
                    } else {
                        static_integral_result(0.0, FormulaDimension::SCALAR)
                    },
                ),
                (
                    "Search",
                    EvaluatedFormulaValue::String(value),
                    [EvaluatedFormulaValue::String(needle), EvaluatedFormulaValue::Scalar(start)],
                ) => {
                    let start_value = *start;
                    let start = self.string_index(start_value)?;
                    let known = value.is_known()
                        && needle.is_known()
                        && start_value.known_value().is_some();
                    EvaluatedFormulaValue::Scalar(
                        if self.evaluate || (self.static_check && known) {
                            let index =
                                self.search_string(value.value(), needle.value(), start, true)?;
                            finite_scalar(f64_from_i64(index)?)?
                        } else {
                            static_integral_result(0.0, FormulaDimension::SCALAR)
                        },
                    )
                }
                (
                    "Search",
                    EvaluatedFormulaValue::String(value),
                    [EvaluatedFormulaValue::String(needle), EvaluatedFormulaValue::Scalar(start), EvaluatedFormulaValue::Boolean(forward)],
                ) => {
                    let start = self.string_index(*start)?;
                    EvaluatedFormulaValue::Scalar(if self.evaluate {
                        let index = self.search_string(
                            value.value(),
                            needle.value(),
                            start,
                            forward.value(),
                        )?;
                        finite_scalar(f64_from_i64(index)?)?
                    } else {
                        static_integral_result(0.0, FormulaDimension::SCALAR)
                    })
                }
                (
                    "Extract",
                    EvaluatedFormulaValue::String(value),
                    [EvaluatedFormulaValue::Scalar(start), EvaluatedFormulaValue::Scalar(length)],
                ) => {
                    let start_value = *start;
                    let length_value = *length;
                    let start = self.string_index(start_value)?;
                    let length = self.string_index(length_value)?;
                    let known = value.is_known()
                        && start_value.known_value().is_some()
                        && length_value.known_value().is_some();
                    let string_value = if self.evaluate || (self.static_check && known) {
                        let end = start.checked_add(length)?;
                        let start = self.string_boundary(value.value(), start)?;
                        let end = self.string_boundary(value.value(), end)?;
                        let copied = self.ctx.copy_retained_text(
                            &value.value()[start..end],
                            "catia_formula_string_extract",
                        );
                        self.admit(copied)?
                    } else {
                        String::new()
                    };
                    EvaluatedFormulaValue::String(EvaluatedFormulaString::from_parts(
                        string_value,
                        self.evaluate || (self.static_check && known),
                    ))
                }
                ("ToReal", EvaluatedFormulaValue::String(value), []) => {
                    EvaluatedFormulaValue::Scalar(
                        if self.evaluate || (self.static_check && value.is_known()) {
                            self.admit(self.ctx.charge_work(
                                u64_from_index(value.value().len()),
                                "catia_formula_string_real",
                            ))?;
                            finite_scalar(value.value().parse::<f64>().ok()?)?
                        } else {
                            static_unknown_result(0.0, FormulaDimension::SCALAR)
                        },
                    )
                }
                _ => return None,
            };
        }
    }

    fn string_index(&self, value: EvaluatedFormulaScalar) -> Option<usize> {
        (value.dimension() == FormulaDimension::SCALAR).then_some(())?;
        if (self.static_check || self.evaluate)
            && !value.satisfies_source_type(FormulaParameterType::Integer)
        {
            return None;
        }
        if self.static_check && value.known_value().is_some_and(|value| value < 0.0) {
            return None;
        }
        if !self.evaluate {
            return value
                .known_value()
                .and_then(truncate_f64_to_i64)
                .and_then(|value| usize::try_from(value).ok())
                .or(Some(0));
        }
        usize::try_from(truncate_f64_to_i64(value.value())?).ok()
    }

    fn string_boundary(&mut self, value: &str, index: usize) -> Option<usize> {
        let work = self.admit(u64_from_index(value.len()).checked_mul(2).ok_or_else(|| {
            self.ctx
                .refuse_codec_limit("catia_formula_string_boundary", u64::MAX, u64::MAX)
        }))?;
        self.admit(self.ctx.charge_work(work, "catia_formula_string_boundary"))?;
        if index == value.chars().count() {
            Some(value.len())
        } else {
            value.char_indices().nth(index).map(|(offset, _)| offset)
        }
    }

    fn search_string(
        &mut self,
        value: &str,
        needle: &str,
        start: usize,
        forward: bool,
    ) -> Option<i64> {
        let work = self.admit(
            u64_from_index(value.len())
                .checked_mul(3)
                .and_then(|work| work.checked_add(u64_from_index(needle.len())))
                .ok_or_else(|| {
                    self.ctx
                        .refuse_codec_limit("catia_formula_string_search", u64::MAX, u64::MAX)
                }),
        )?;
        self.admit(self.ctx.charge_work(work, "catia_formula_string_search"))?;
        let character_count = value.chars().count();
        if start > character_count {
            return Some(-1);
        }
        let byte_offset = if forward {
            let start_byte = self.string_boundary(value, start)?;
            value[start_byte..]
                .find(needle)
                .map(|offset| start_byte + offset)
        } else {
            let end_character = character_count.checked_sub(start)?;
            let end_byte = self.string_boundary(value, end_character)?;
            value[..end_byte].rfind(needle)
        };
        byte_offset.map_or(Some(-1), |offset| {
            i64::try_from(value[..offset].chars().count()).ok()
        })
    }

    fn string_literal(&mut self) -> Option<String> {
        (self.peek()? == b'"').then_some(())?;
        self.at += 1;
        let start = self.at;
        while let Some(character) = self.source.get(self.at..)?.chars().next() {
            if character == '"' {
                let source = self.source.get(start..self.at)?;
                let copied = self
                    .ctx
                    .copy_retained_text(source, "catia_formula_literal_text");
                let value = self.admit(copied)?;
                self.at += character.len_utf8();
                return Some(value);
            }
            if character.is_control() || character == '\\' {
                return None;
            }
            self.at += character.len_utf8();
        }
        None
    }

    fn function_call(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let function_start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
            self.at += 1;
        }
        let function = &self.source[function_start..self.at];
        let mut argument_storage =
            self.admit(self.ctx.reserve_scoped(0, "CATIA formula argument storage"))?;
        let parsed = argument_storage.with_storage(|| {
            Ok::<_, cadmpeg_core::CodecError>(self.descend(depth, Self::function_arguments))
        });
        let arguments = self.admit(parsed)??;

        if function == "ReplaceSubText" {
            let [EvaluatedFormulaValue::String(source), EvaluatedFormulaValue::String(from), EvaluatedFormulaValue::String(to)] =
                arguments.as_slice()
            else {
                return None;
            };
            if (self.evaluate || self.static_check) && from.is_known() && from.value().is_empty() {
                return None;
            }
            let known =
                source.is_known() && from.is_known() && to.is_known() && !from.value().is_empty();
            let value = if self.evaluate || (self.static_check && known) {
                // Two formatting passes scan the source and emit each replacement.
                let work = self.admit(
                    u64_from_index(from.value().len())
                        .checked_add(u64_from_index(to.value().len()))
                        .and_then(|width| width.checked_add(1))
                        .and_then(|width| u64_from_index(source.value().len()).checked_mul(width))
                        .and_then(|work| work.checked_mul(2))
                        .ok_or_else(|| {
                            self.ctx.refuse_codec_limit(
                                "catia_formula_replace_work",
                                u64::MAX,
                                u64::MAX,
                            )
                        }),
                )?;
                self.admit(self.ctx.charge_work(work, "catia_formula_replace_work"))?;
                let formatted = self.ctx.format_retained(
                    format_args!(
                        "{}",
                        ReplacedText {
                            source: source.value(),
                            from: from.value(),
                            to: to.value(),
                        }
                    ),
                    "catia_formula_replace_subtext",
                );
                self.admit(formatted)?
            } else {
                String::new()
            };
            return Some(EvaluatedFormulaValue::String(
                EvaluatedFormulaString::from_parts(
                    value,
                    self.evaluate || (self.static_check && known),
                ),
            ));
        }

        if function == "ToString" {
            let [EvaluatedFormulaValue::Scalar(value)] = arguments.as_slice() else {
                return None;
            };
            if (self.static_check || self.evaluate)
                && !value.satisfies_source_type(FormulaParameterType::Integer)
            {
                return None;
            }
            let known = value.known_value().is_some();
            let string_value = if self.evaluate || (self.static_check && known) {
                let formatted = self.ctx.format_retained(
                    format_args!("{:.0}", value.value()),
                    "catia_formula_to_string",
                );
                self.admit(formatted)?
            } else {
                String::new()
            };
            return Some(EvaluatedFormulaValue::String(
                EvaluatedFormulaString::from_parts(
                    string_value,
                    self.evaluate || (self.static_check && known),
                ),
            ));
        }

        if matches!(function, "ToUpper" | "ToLower") {
            let [EvaluatedFormulaValue::String(value)] = arguments.as_slice() else {
                return None;
            };
            let known = value.is_known();
            let string_value = if self.evaluate || (self.static_check && known) {
                // A Unicode character maps to at most three characters; formatting runs twice.
                let work = self.admit(
                    u64_from_index(value.value().len())
                        .checked_mul(12)
                        .ok_or_else(|| {
                            self.ctx.refuse_codec_limit(
                                "catia_formula_case_work",
                                u64::MAX,
                                u64::MAX,
                            )
                        }),
                )?;
                self.admit(self.ctx.charge_work(work, "catia_formula_case_work"))?;
                let formatted = self.ctx.format_retained(
                    format_args!(
                        "{}",
                        CasedText {
                            source: value.value(),
                            upper: function == "ToUpper",
                        }
                    ),
                    "catia_formula_string_case",
                );
                self.admit(formatted)?
            } else {
                String::new()
            };
            return Some(EvaluatedFormulaValue::String(
                EvaluatedFormulaString::from_parts(
                    string_value,
                    self.evaluate || (self.static_check && known),
                ),
            ));
        }

        if function == "round" && arguments.len() == 3 {
            let [EvaluatedFormulaValue::Scalar(value), EvaluatedFormulaValue::String(unit), EvaluatedFormulaValue::Scalar(digits)] =
                arguments.as_slice()
            else {
                return None;
            };
            if !matches!(
                value.dimension(),
                FormulaDimension::LENGTH | FormulaDimension::ANGLE
            ) || digits.dimension() != FormulaDimension::SCALAR
            {
                return None;
            }
            let unit_spec = formula_unit(unit.value());
            if let Some((unit_dimension, _)) = unit_spec {
                if value.dimension() != unit_dimension {
                    return None;
                }
            } else if self.evaluate || (self.static_check && unit.is_known()) {
                return None;
            }
            if (self.static_check || self.evaluate)
                && !digits.satisfies_source_type(FormulaParameterType::Integer)
            {
                return None;
            }
            if self.static_check
                && digits
                    .known_value()
                    .is_some_and(|value| value < 0.0 || value > f64::from(i32::MAX))
            {
                return None;
            }
            if !self.evaluate {
                return Some(EvaluatedFormulaValue::Scalar(
                    EvaluatedFormulaScalar::from_parts(0.0, value.dimension(), None, None),
                ));
            }
            let (_, unit_scale) = unit_spec?;
            if digits.value() < 0.0 || digits.value() > f64::from(i32::MAX) {
                return None;
            }
            let quantum = unit_scale * 10.0_f64.powi(-(truncate_f64_to_i32(digits.value())?));
            let rounded = if quantum == 0.0 {
                value.value()
            } else {
                let scaled = value.value() / quantum;
                if scaled.is_finite() {
                    scaled.round_ties_even() * quantum
                } else {
                    value.value()
                }
            };
            return rounded.is_finite().then_some(EvaluatedFormulaValue::Scalar(
                EvaluatedFormulaScalar::from_parts(
                    rounded,
                    value.dimension(),
                    finite_integrality(rounded),
                    Some(rounded),
                ),
            ));
        }

        let mut scalar_arguments = Vec::new();
        for argument in arguments {
            let scalar = argument.scalar()?;
            let pushed = argument_storage.with_storage(|| {
                self.ctx.push_vec(
                    &mut scalar_arguments,
                    scalar,
                    "catia_formula_scalar_arguments",
                )
            });
            self.admit(pushed)?;
        }
        let arguments = scalar_arguments;

        if matches!(function, "min" | "max") {
            let mut arguments = arguments.into_iter();
            let mut result = arguments.next()?;
            for argument in arguments {
                if result.dimension() != argument.dimension() {
                    return None;
                }
                let value = if function == "min" {
                    result.value().min(argument.value())
                } else {
                    result.value().max(argument.value())
                };
                let integral = if self.evaluate {
                    finite_integrality(value)
                } else {
                    static_all_integral(result.integral(), argument.integral())
                };
                let known_value = if self.evaluate {
                    Some(value)
                } else {
                    result
                        .known_value()
                        .zip(argument.known_value())
                        .map(|(result, argument)| {
                            if function == "min" {
                                result.min(argument)
                            } else {
                                result.max(argument)
                            }
                        })
                };
                result = EvaluatedFormulaScalar::from_parts(
                    value,
                    result.dimension(),
                    integral,
                    known_value,
                );
            }
            return Some(EvaluatedFormulaValue::Scalar(result));
        }

        if matches!(function, "LinearInterpolation" | "CubicInterpolation") {
            let [start, end, fraction] = arguments.as_slice() else {
                return None;
            };
            if start.dimension() != end.dimension()
                || fraction.dimension() != FormulaDimension::SCALAR
            {
                return None;
            }
            let fraction_value = if function == "CubicInterpolation" {
                fraction.value() * fraction.value() * (3.0 - 2.0 * fraction.value())
            } else {
                fraction.value()
            };
            let interpolate = |start: f64, end: f64, fraction: f64| {
                if (0.0..=1.0).contains(&fraction) {
                    cadmpeg_ir::math::interpolate(start, end, fraction)
                        .map(cadmpeg_ir::scalar::FiniteReal::get)
                } else {
                    Some(start + (end - start) * fraction)
                }
            };
            let value = interpolate(start.value(), end.value(), fraction_value)?;
            let known_value = if self.evaluate {
                Some(value)
            } else {
                start
                    .known_value()
                    .zip(end.known_value())
                    .zip(fraction.known_value())
                    .and_then(|((start, end), fraction)| {
                        if function == "CubicInterpolation" {
                            let fraction = fraction * fraction * (3.0 - 2.0 * fraction);
                            interpolate(start, end, fraction)
                        } else {
                            interpolate(start, end, fraction)
                        }
                    })
                    .filter(|value| value.is_finite())
            };
            if self.static_check
                && start.known_value().is_some()
                && end.known_value().is_some()
                && fraction.known_value().is_some()
                && known_value.is_none()
            {
                return None;
            }
            return (value.is_finite() || !self.evaluate).then_some(EvaluatedFormulaValue::Scalar(
                EvaluatedFormulaScalar::from_parts(
                    if value.is_finite() { value } else { 0.0 },
                    start.dimension(),
                    if self.evaluate {
                        finite_integrality(value)
                    } else {
                        None
                    },
                    known_value,
                ),
            ));
        }

        let (first, second) = match arguments.as_slice() {
            [first] => (*first, None),
            [first, second] => (*first, Some(*second)),
            _ => return None,
        };
        let value = match (function, first, second) {
            ("sin", argument, None)
                if matches!(
                    argument.dimension(),
                    FormulaDimension::ANGLE | FormulaDimension::SCALAR
                ) =>
            {
                self.scalar_result(argument.value().sin())
            }
            ("cos", argument, None)
                if matches!(
                    argument.dimension(),
                    FormulaDimension::ANGLE | FormulaDimension::SCALAR
                ) =>
            {
                self.scalar_result(argument.value().cos())
            }
            ("tan", argument, None)
                if matches!(
                    argument.dimension(),
                    FormulaDimension::ANGLE | FormulaDimension::SCALAR
                ) =>
            {
                self.scalar_result(argument.value().tan())
            }
            ("asin", argument, None)
                if argument.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value()
                            .is_none_or(|value| (-1.0..=1.0).contains(&value))) =>
            {
                self.angle_result(argument.value().asin())
            }
            ("acos", argument, None)
                if argument.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value()
                            .is_none_or(|value| (-1.0..=1.0).contains(&value))) =>
            {
                self.angle_result(argument.value().acos())
            }
            ("atan", argument, None) if argument.dimension() == FormulaDimension::SCALAR => {
                self.angle_result(argument.value().atan())
            }
            ("log", argument, None)
                if argument.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument.known_value().is_none_or(|value| value > 0.0))
                    && (!self.evaluate || argument.value() > 0.0) =>
            {
                self.scalar_result(if self.evaluate {
                    argument.value().log10()
                } else {
                    0.0
                })
            }
            ("ln", argument, None)
                if argument.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument.known_value().is_none_or(|value| value > 0.0))
                    && (!self.evaluate || argument.value() > 0.0) =>
            {
                self.scalar_result(if self.evaluate {
                    argument.value().ln()
                } else {
                    0.0
                })
            }
            ("exp", argument, None)
                if argument.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value()
                            .is_none_or(|value| value.exp().is_finite())) =>
            {
                self.scalar_result(argument.value().exp())
            }
            ("sinh", argument, None)
                if argument.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value()
                            .is_none_or(|value| value.sinh().is_finite())) =>
            {
                self.scalar_result(argument.value().sinh())
            }
            ("cosh", argument, None)
                if argument.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value()
                            .is_none_or(|value| value.cosh().is_finite())) =>
            {
                self.scalar_result(argument.value().cosh())
            }
            ("tanh", argument, None) if argument.dimension() == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value().tanh())
            }
            ("asinh", argument, None) if argument.dimension() == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value().asinh())
            }
            ("acosh", argument, None)
                if argument.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument.known_value().is_none_or(|value| value >= 1.0)) =>
            {
                self.scalar_result(argument.value().acosh())
            }
            ("atanh", argument, None)
                if argument.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value()
                            .is_none_or(|value| (-1.0..1.0).contains(&value))) =>
            {
                self.scalar_result(argument.value().atanh())
            }
            ("ceil", argument, None) if argument.dimension() == FormulaDimension::SCALAR => {
                self.integral_result(argument.value().ceil())
            }
            ("floor", argument, None) if argument.dimension() == FormulaDimension::SCALAR => {
                self.integral_result(argument.value().floor())
            }
            ("int", argument, None) if argument.dimension() == FormulaDimension::SCALAR => {
                self.integral_result(argument.value().trunc())
            }
            ("round", argument, None) if argument.dimension() == FormulaDimension::SCALAR => {
                self.integral_result(argument.value().round_ties_even())
            }
            ("mod", dividend, Some(divisor))
                if dividend.dimension() == FormulaDimension::SCALAR
                    && divisor.dimension() == FormulaDimension::SCALAR
                    && (!self.static_check && !self.evaluate
                        || divisor.satisfies_source_type(FormulaParameterType::Integer))
                    && (!self.static_check || divisor.known_value() != Some(0.0))
                    && (!self.evaluate || divisor.value() != 0.0) =>
            {
                let result = if self.evaluate {
                    dividend.value().trunc() % divisor.value()
                } else {
                    0.0
                };
                if self.evaluate {
                    self.scalar_result(result)
                } else {
                    Some(EvaluatedFormulaScalar::from_parts(
                        result,
                        FormulaDimension::SCALAR,
                        static_all_integral(dividend.integral(), divisor.integral()),
                        dividend.known_value().zip(divisor.known_value()).and_then(
                            |(dividend, divisor)| {
                                (divisor != 0.0).then_some(dividend.trunc() % divisor)
                            },
                        ),
                    ))
                }
            }
            ("abs", argument, None) => {
                if self.evaluate {
                    finite_integrality(argument.value().abs()).map(|integral| {
                        EvaluatedFormulaScalar::from_parts(
                            argument.value().abs(),
                            argument.dimension(),
                            Some(integral),
                            Some(argument.value().abs()),
                        )
                    })
                } else {
                    Some(EvaluatedFormulaScalar::from_parts(
                        0.0,
                        argument.dimension(),
                        argument.integral(),
                        argument.known_value().map(f64::abs),
                    ))
                }
            }
            ("sqrt", argument, None)
                if (!self.static_check
                    || argument.known_value().is_none_or(|value| value >= 0.0))
                    && (!self.evaluate || argument.value() >= 0.0) =>
            {
                Some(EvaluatedFormulaScalar::from_parts(
                    if self.evaluate {
                        argument.value().sqrt()
                    } else {
                        0.0
                    },
                    argument.dimension().square_root()?,
                    if self.evaluate {
                        finite_integrality(argument.value().sqrt())
                    } else {
                        None
                    },
                    if self.evaluate {
                        Some(argument.value().sqrt())
                    } else {
                        argument
                            .known_value()
                            .map(f64::sqrt)
                            .filter(|value| value.is_finite())
                    },
                ))
            }
            _ => None,
        }?;
        Some(EvaluatedFormulaValue::Scalar(value))
    }

    fn function_arguments(&mut self, depth: usize) -> Option<Vec<EvaluatedFormulaValue>> {
        self.skip_whitespace();
        (self.peek()? == b'(').then_some(())?;
        self.at += 1;
        let mut arguments = Vec::new();
        self.skip_whitespace();
        if self.peek()? == b')' {
            self.at += 1;
            return Some(arguments);
        }
        loop {
            let argument = self.conditional(depth)?;
            let pushed = self
                .ctx
                .push_vec(&mut arguments, argument, "catia_formula_arguments");
            self.admit(pushed)?;
            self.skip_whitespace();
            if self.peek()? == b')' {
                self.at += 1;
                break;
            }
            (self.peek()? == b',' && arguments.len() < MAX_FORMULA_FUNCTION_ARGUMENTS)
                .then_some(())?;
            self.at += 1;
        }
        Some(arguments)
    }

    fn symbol(&mut self) -> Option<EvaluatedFormulaValue> {
        let start = self.at;
        self.at += 1;
        let digits = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        (self.at > digits && self.peek()? == b'_').then_some(())?;
        self.at += 1;
        let name_end = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.at += 1;
        }
        if self.peek() == Some(b'/') {
            self.at += 1;
            let ordinal = self.at;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.at += 1;
            }
            (self.at > ordinal).then_some(())?;
        } else {
            self.at = name_end;
        }
        let value = self.bindings.get(&self.source[start..name_end])?;
        if self.evaluate
            && matches!(value, EvaluatedFormulaValue::Scalar(scalar) if scalar.known_value().is_none())
        {
            return None;
        }
        let copied = value.copy_charged(self.ctx);
        self.admit(copied)
    }

    fn literal(&mut self) -> Option<EvaluatedFormulaScalar> {
        let start = self.at;
        let mut saw_digit = false;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            saw_digit = true;
            self.at += 1;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                saw_digit = true;
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let exponent = self.at;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.at += 1;
            }
            (self.at > exponent).then_some(())?;
        }
        saw_digit.then_some(())?;
        let mut value = self.source[start..self.at].parse::<f64>().ok()?;
        let unit_boundary = self.at;
        self.skip_whitespace();
        let Some(unit) = [
            "micron", "mile", "yard", "grad", "rad", "deg", "mm", "cm", "km", "ft", "in", "m",
        ]
        .into_iter()
        .find(|unit| self.remaining().starts_with(unit)) else {
            self.at = unit_boundary;
            return value
                .is_finite()
                .then_some(EvaluatedFormulaScalar::from_parts(
                    value,
                    FormulaDimension::SCALAR,
                    finite_integrality(value),
                    Some(value),
                ));
        };
        let (dimension, scale) = formula_unit(unit)?;
        self.at += unit.len();
        value *= scale;
        value
            .is_finite()
            .then_some(EvaluatedFormulaScalar::from_parts(
                value,
                dimension,
                finite_integrality(value),
                Some(value),
            ))
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        if !self.remaining().starts_with(keyword) {
            return false;
        }
        let before_is_identifier = match self.at.checked_sub(1) {
            Some(before) => self
                .source
                .as_bytes()
                .get(before)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_'),
            None => false, // Position zero has no preceding identifier byte.
        };
        let after_is_identifier = self
            .source
            .as_bytes()
            .get(self.at + keyword.len())
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
        if before_is_identifier || after_is_identifier {
            return false;
        }
        self.at += keyword.len();
        true
    }

    fn peek(&self) -> Option<u8> {
        if self.refusal.is_some() {
            None
        } else {
            self.source.as_bytes().get(self.at).copied()
        }
    }

    fn remaining(&self) -> &str {
        &self.source[self.at..]
    }

    fn descend<T>(
        &mut self,
        depth: usize,
        parse: impl FnOnce(&mut Self, usize) -> Option<T>,
    ) -> Option<T> {
        if depth >= MAX_FORMULA_EXPRESSION_DEPTH {
            self.refusal = Some(self.ctx.refuse_codec_limit(
                "catia_formula_expression_local_depth",
                u64_from_index(MAX_FORMULA_EXPRESSION_DEPTH),
                u64_from_index(MAX_FORMULA_EXPRESSION_DEPTH) + 1,
            ));
            return None;
        }
        let ctx = self.ctx;
        let _depth = self.admit(ctx.enter_nested("catia_formula_expression_depth"))?;
        parse(self, depth + 1)
    }
}

fn evaluate_formula_expression_charged<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &'a str,
    bindings: &BTreeMap<&'a str, EvaluatedFormulaValue>,
) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
    evaluate_formula_expression_with_mode_charged(ctx, source, bindings, true)
}

fn evaluate_formula_expression_with_mode_charged<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &'a str,
    bindings: &BTreeMap<&'a str, EvaluatedFormulaValue>,
    evaluate: bool,
) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
    let source_bytes = u64_from_index(source.len());
    ctx.charge_work(source_bytes, "catia_formula_expression_scan")?;
    FormulaExpressionParser {
        source,
        at: 0,
        bindings,
        ctx,
        refusal: None,
        evaluate,
        static_check: !evaluate,
    }
    .parse()
}

#[cfg(test)]
fn evaluate_formula_expression<'a>(
    source: &'a str,
    bindings: &BTreeMap<&'a str, EvaluatedFormulaValue>,
) -> Option<EvaluatedFormulaValue> {
    crate::test_support::with_service_context(|ctx| {
        evaluate_formula_expression_charged(ctx, source, bindings)
    })
    .expect("service profile admits formula parser fixture")
}

#[cfg(test)]
fn evaluate_formula_expression_with_mode<'a>(
    source: &'a str,
    bindings: &BTreeMap<&'a str, EvaluatedFormulaValue>,
    evaluate: bool,
) -> Option<EvaluatedFormulaValue> {
    crate::test_support::with_service_context(|ctx| {
        evaluate_formula_expression_with_mode_charged(ctx, source, bindings, evaluate)
    })
    .expect("service profile admits formula parser fixture")
}

fn static_formula_value(parameter_type: FormulaParameterType) -> EvaluatedFormulaValue {
    match parameter_type {
        FormulaParameterType::Length => EvaluatedFormulaValue::Scalar(
            EvaluatedFormulaScalar::from_parts(0.0, FormulaDimension::LENGTH, None, None),
        ),
        FormulaParameterType::Angle => EvaluatedFormulaValue::Scalar(
            EvaluatedFormulaScalar::from_parts(0.0, FormulaDimension::ANGLE, None, None),
        ),
        FormulaParameterType::Real => EvaluatedFormulaValue::Scalar(
            EvaluatedFormulaScalar::from_parts(0.5, FormulaDimension::SCALAR, None, None),
        ),
        FormulaParameterType::Integer => EvaluatedFormulaValue::Scalar(
            EvaluatedFormulaScalar::from_parts(0.0, FormulaDimension::SCALAR, Some(true), None),
        ),
        FormulaParameterType::Boolean => {
            EvaluatedFormulaValue::Boolean(EvaluatedFormulaBoolean::unknown())
        }
        FormulaParameterType::String => {
            EvaluatedFormulaValue::String(EvaluatedFormulaString::unknown())
        }
    }
}

fn typed_parameter_evaluation(
    source_type: &str,
    evaluation: &crate::native::CatiaEntityEvaluation,
) -> Option<(FormulaParameterType, TypedParameterEvaluation)> {
    let parameter_type = canonical_parameter_type(source_type)?;
    let bits = match evaluation {
        crate::native::CatiaEntityEvaluation::Unset => {
            return Some((parameter_type, TypedParameterEvaluation::Unset));
        }
        crate::native::CatiaEntityEvaluation::Scalar { bits } => bits,
    };
    let value = f64::from_bits(*bits);
    if !value.is_finite() {
        return None;
    }
    let value = match parameter_type {
        FormulaParameterType::Length => ParameterValue::Length(Length::new(value)?),
        FormulaParameterType::Angle => ParameterValue::Angle(Angle::new(value)?),
        FormulaParameterType::Real => {
            ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::new(value)?)
        }
        FormulaParameterType::Integer => {
            if value.fract() != 0.0
                || !(FORMULA_INTEGER_LOWER..FORMULA_INTEGER_UPPER).contains(&value)
            {
                return None;
            }
            ParameterValue::Integer(truncate_f64_to_i64(value)?)
        }
        FormulaParameterType::Boolean | FormulaParameterType::String => return None,
    };
    Some((parameter_type, TypedParameterEvaluation::Value(value)))
}

fn canonical_parameter_type(source_type: &str) -> Option<FormulaParameterType> {
    match source_type {
        "LENGTH" => Some(FormulaParameterType::Length),
        "ANGLE" => Some(FormulaParameterType::Angle),
        "Real" | "R" => Some(FormulaParameterType::Real),
        "Integer" | "I" => Some(FormulaParameterType::Integer),
        "Boolean" => Some(FormulaParameterType::Boolean),
        "String" => Some(FormulaParameterType::String),
        _ => None,
    }
}

fn neutral_parameter_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native_id: &str,
) -> Result<ParameterId, cadmpeg_core::CodecError> {
    crate::ids::neutral_history_id(
        ctx,
        native_id,
        &cadmpeg_ir::identity_component!("parameter"),
    )
    .map(ParameterId::from)
}

#[cfg(test)]
mod parser_tests {
    use super::{
        evaluate_formula_expression, evaluate_formula_expression_with_mode,
        evaluate_legacy_output_assignment, finite_scalar, formula_parameter_candidates_agree,
        parameter_expression, static_formula_value, string_literal_expression,
        typed_parameter_evaluation, EvaluatedFormulaBoolean, EvaluatedFormulaString,
        EvaluatedFormulaValue, FormulaDimension, FormulaParameterCandidate, FormulaParameterRole,
        FormulaParameterType, TypedParameterEvaluation,
    };
    use cadmpeg_ir::features::DesignParameter;
    use cadmpeg_ir::features::ParameterId;
    use cadmpeg_ir::features::ParameterValue;
    use std::collections::BTreeMap;

    fn unset_candidate(parameter_type: FormulaParameterType) -> FormulaParameterCandidate {
        FormulaParameterCandidate {
            parameter: DesignParameter {
                id: ParameterId::mint("synthetic:test:id#parameter".to_string())
                    .expect("identity grammar"),
                owner: None,
                ordinal: 0,
                name: "Value".to_string(),
                expression: String::new(),
                display: None,
                value: None,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: Some("native-parameter".to_string()),
            },
            parameter_type,
            role: FormulaParameterRole::Input,
            source_order: 1,
        }
    }

    #[test]
    fn unset_parameter_candidates_require_one_canonical_type() {
        assert!(formula_parameter_candidates_agree(
            &unset_candidate(FormulaParameterType::Real),
            &unset_candidate(FormulaParameterType::Real)
        ));
        assert!(!formula_parameter_candidates_agree(
            &unset_candidate(FormulaParameterType::Length),
            &unset_candidate(FormulaParameterType::Real)
        ));
    }

    #[test]
    fn formula_design_parameter_copy_refuses_nested_admission() {
        let mut parameter = unset_candidate(FormulaParameterType::String).parameter;
        parameter
            .dependencies
            .insert(
                &cadmpeg_test_support::service_decode_context(),
                ParameterId::mint("synthetic:test:id#dependency".to_string())
                    .expect("identity grammar"),
                "insert fixture member",
            )
            .expect("member insertion admission");
        parameter.properties.insert(
            cadmpeg_core::nonblank_literal!("source"),
            "value".to_string(),
        );
        let retained = crate::test_support::with_retained_limit(0, |ctx| {
            super::copy_design_parameter(ctx, &parameter)
        });
        assert!(
            matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_parameter_copy")
        );
        let collection = crate::test_support::with_collection_limit(0, |ctx| {
            super::copy_design_parameter(ctx, &parameter)
        });
        assert!(
            matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_parameter_copy")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            super::copy_design_parameter(ctx, &parameter)
        })
        .expect("service profile admits parameter copy");
        assert_eq!(admitted, parameter);
    }

    #[test]
    fn formula_candidate_index_refuses_collection_limit() {
        let candidate = unset_candidate(FormulaParameterType::String);
        let refused = crate::test_support::with_collection_limit(0, |ctx| {
            super::merge_formula_parameter_candidate(
                ctx,
                &mut BTreeMap::new(),
                &mut std::collections::BTreeSet::new(),
                candidate,
            )
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_candidates")
        );
        let candidate = unset_candidate(FormulaParameterType::String);
        let mut candidates = BTreeMap::new();
        crate::test_support::with_service_context(|ctx| {
            super::merge_formula_parameter_candidate(
                ctx,
                &mut candidates,
                &mut std::collections::BTreeSet::new(),
                candidate,
            )
        })
        .expect("service profile admits candidate");
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn formula_input_value_and_dependency_copies_refuse_limits() {
        let value = ParameterValue::String("input text".to_string());
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            EvaluatedFormulaValue::from_parameter_value_charged(ctx, &value)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_evaluated_string")
        );
        let id =
            ParameterId::mint("synthetic:test:id#input".to_string()).expect("identity grammar");
        let refused = crate::test_support::with_collection_limit(0, |ctx| {
            ctx.try_collect_vec(
                std::slice::from_ref(&id)
                    .iter()
                    .map(|id| id.try_clone_for_decode(ctx, "catia_formula_program_inputs")),
                "catia_formula_program_inputs",
            )
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_program_inputs")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            ctx.try_collect_vec(
                std::slice::from_ref(&id)
                    .iter()
                    .map(|id| id.try_clone_for_decode(ctx, "catia_formula_program_inputs")),
                "catia_formula_program_inputs",
            )
        })
        .expect("service profile admits dependency copy");
        assert_eq!(admitted, vec![id]);
    }

    #[test]
    fn formula_parser_literal_refuses_retained_limit() {
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            super::evaluate_formula_expression_charged(ctx, "\"text\"", &BTreeMap::new())
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_literal_text")
        );
        assert_eq!(
            evaluate_formula_expression("\"text\"", &BTreeMap::new())
                .and_then(EvaluatedFormulaValue::string),
            Some("text".to_string())
        );
    }

    #[test]
    fn formula_parser_refuses_recursion_and_work_limits() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits input limit");
        let refused =
            super::evaluate_formula_expression_charged(&ctx, "true ? 1 ; 2", &BTreeMap::new());
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_expression_depth")
        );
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits input limit");
        let refused = super::evaluate_formula_expression_charged(&ctx, "1", &BTreeMap::new());
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_expression_scan")
        );
    }

    #[test]
    fn formula_parser_argument_vectors_refuse_collection_limits() {
        for (cap, operation) in [
            (0, "catia_formula_arguments"),
            (2, "catia_formula_scalar_arguments"),
        ] {
            let refused = crate::test_support::with_collection_limit(cap, |ctx| {
                super::evaluate_formula_expression_charged(ctx, "min(1,2)", &BTreeMap::new())
            });
            assert!(
                matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == operation),
                "missing refusal at {operation}"
            );
        }
        assert!(evaluate_formula_expression("min(1,2)", &BTreeMap::new()).is_some());
    }

    #[test]
    fn formula_parser_string_operations_refuse_retained_limits() {
        for (expression, _cap, operation) in [
            ("\"a\"+\"b\"", 2, "catia_formula_string_concat"),
            ("\"abc\"-\"b\"", 4, "catia_formula_string_subtract"),
            ("\"abc\".Extract(0,1)", 3, "catia_formula_string_extract"),
            (
                "ReplaceSubText(\"ab\",\"a\",\"xyz\")",
                6,
                "catia_formula_replace_subtext",
            ),
            ("ToString(2)", 0, "catia_formula_to_string"),
            ("ToUpper(\"é\")", 2, "catia_formula_string_case"),
        ] {
            let refused = crate::test_support::with_retained_refusal(&[], operation, |ctx| {
                super::evaluate_formula_expression_charged(ctx, expression, &BTreeMap::new())
            });
            assert!(
                matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == operation),
                "missing refusal at {operation}"
            );
            assert!(
                evaluate_formula_expression(expression, &BTreeMap::new()).is_some(),
                "service expression {expression}"
            );
        }
    }

    #[test]
    fn formula_parser_symbol_and_static_merge_refuse_retained_limits() {
        let bindings = BTreeMap::from([(
            "#1_",
            EvaluatedFormulaValue::String(EvaluatedFormulaString::known("text".to_string())),
        )]);
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            super::evaluate_formula_expression_charged(ctx, "#1_", &bindings)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_evaluated_value_copy")
        );
        let bindings = BTreeMap::from([(
            "#1_",
            EvaluatedFormulaValue::Boolean(EvaluatedFormulaBoolean::unknown()),
        )]);
        let refused = crate::test_support::with_retained_limit(2, |ctx| {
            super::evaluate_formula_expression_with_mode_charged(
                ctx,
                "#1_ ? \"a\" ; \"a\"",
                &bindings,
                false,
            )
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_static_string_merge")
        );
    }

    #[test]
    fn string_parameter_expressions_match_literal_grammar() {
        let literal = ParameterValue::String("Cilas Evans".to_string());
        crate::test_support::with_service_context(|ctx| {
            assert_eq!(
                parameter_expression(ctx, &literal).expect("service expression"),
                "\"Cilas Evans\""
            );
            assert!(string_literal_expression(ctx, "")
                .expect("service literal")
                .is_some_and(|expression| expression == "\"\""));
            for value in ["quote\"", "backslash\\", "line\n", "control\u{0085}"] {
                assert!(
                    string_literal_expression(ctx, value)
                        .expect("service literal")
                        .is_none(),
                    "{value:?}"
                );
                assert!(
                    parameter_expression(ctx, &ParameterValue::String(value.to_string()))
                        .expect("service expression")
                        .is_empty()
                );
            }
        });
        assert_eq!(
            evaluate_formula_expression("\"Cilas Evans\"", &BTreeMap::new())
                .and_then(EvaluatedFormulaValue::string),
            Some("Cilas Evans".to_string())
        );
    }

    #[test]
    fn formula_literal_and_property_projection_refuse_retained_limit() {
        let literal = ParameterValue::String("Cilas Evans".to_string());
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            super::parameter_expression(ctx, &literal)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_string_literal")
        );
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            super::parameter_properties(ctx, "String", Some("source-binding"))
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_property")
        );
        let properties = crate::test_support::with_service_context(|ctx| {
            super::parameter_properties(ctx, "String", Some("source-binding"))
        })
        .expect("service profile admits properties");
        assert_eq!(properties["value_type"], "String");
        assert_eq!(properties["catia_binding"], "source-binding");
    }

    #[test]
    fn formula_string_value_copy_refuses_retained_limit() {
        let value = ParameterValue::String("source string".to_string());
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            value.try_clone_for_decode(ctx, "catia_formula_parameter_value_copy")
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_formula_parameter_value_copy")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            value.try_clone_for_decode(ctx, "catia_formula_parameter_value_copy")
        })
        .expect("service profile admits string copy");
        assert_eq!(admitted, value);
    }

    #[test]
    fn legacy_output_assignment_requires_one_exact_assignment() {
        let value = evaluate_legacy_output_assignment("#1_ = 2 + 3", "#1_")
            .and_then(EvaluatedFormulaValue::scalar)
            .expect("numeric assignment result");
        assert_eq!(value.value(), 5.0);
        assert!(evaluate_legacy_output_assignment("#1_ == 5", "#1_").is_none());
        assert!(evaluate_legacy_output_assignment("#1_ = 2 = 3", "#1_").is_none());
        assert!(evaluate_legacy_output_assignment("#2_ = 5", "#1_").is_none());
        assert_eq!(
            evaluate_legacy_output_assignment("#1_ = \"a=b\"", "#1_")
                .and_then(EvaluatedFormulaValue::string),
            Some("a=b".to_string())
        );
    }

    #[test]
    fn static_formula_check_preserves_type_closure_without_values() {
        let bindings = BTreeMap::from([
            ("#1_", static_formula_value(FormulaParameterType::Length)),
            ("#2_", static_formula_value(FormulaParameterType::Integer)),
            ("#3_", static_formula_value(FormulaParameterType::Boolean)),
            ("#4_", static_formula_value(FormulaParameterType::String)),
            ("#5_", static_formula_value(FormulaParameterType::Real)),
        ]);

        assert!(
            evaluate_formula_expression_with_mode("#1_ /2+1mm", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type(FormulaParameterType::Length))
        );
        assert!(
            evaluate_formula_expression_with_mode("#3_ ? #2_ ; 1", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type(FormulaParameterType::Integer))
        );
        assert!(
            evaluate_formula_expression_with_mode("#4_", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type(FormulaParameterType::String))
        );
        assert!(
            evaluate_formula_expression_with_mode("ToString(#2_)", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type(FormulaParameterType::String))
        );
        assert!(
            evaluate_formula_expression_with_mode("(#1_) / #2_", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type(FormulaParameterType::Length))
        );
        assert!(
            evaluate_formula_expression_with_mode("#5_ + 0", &bindings, false)
                .is_none_or(|value| !value.satisfies_source_type(FormulaParameterType::Integer))
        );
        assert!(evaluate_formula_expression_with_mode("#1_ ** #2_", &bindings, false).is_none());
        assert!(
            evaluate_formula_expression_with_mode("#1_ ** 2", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .is_some_and(|value| {
                    value.dimension()
                        == FormulaDimension {
                            length: 2,
                            angle: 0,
                        }
                })
        );
    }

    #[test]
    fn static_formula_check_does_not_use_placeholder_values_as_facts() {
        let bindings = BTreeMap::from([
            ("#1_", static_formula_value(FormulaParameterType::Length)),
            ("#2_", static_formula_value(FormulaParameterType::Integer)),
            ("#3_", static_formula_value(FormulaParameterType::Real)),
            ("#4_", static_formula_value(FormulaParameterType::String)),
        ]);

        let unknown_predicate = evaluate_formula_expression_with_mode("#3_ > 1", &bindings, false)
            .and_then(EvaluatedFormulaValue::boolean)
            .expect("Boolean comparison type");
        assert_eq!(unknown_predicate.known_value(), None);

        let length = evaluate_formula_expression_with_mode("#4_.Length()", &bindings, false)
            .and_then(EvaluatedFormulaValue::scalar)
            .expect("string length type");
        assert_eq!(length.integral(), Some(true));
        assert_eq!(length.known_value(), None);

        let known_length =
            evaluate_formula_expression_with_mode("\"text\".Length()", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("known string length");
        assert_eq!(known_length.known_value(), Some(4.0));

        let known_real =
            evaluate_formula_expression_with_mode("\"12.5\".ToReal()", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("known string-to-real value");
        assert_eq!(known_real.known_value(), Some(12.5));

        let parsed = evaluate_formula_expression_with_mode("#4_.ToReal()", &bindings, false)
            .and_then(EvaluatedFormulaValue::scalar)
            .expect("string-to-real type");
        assert_eq!(parsed.known_value(), None);

        for expression in [
            "#4_ + \"suffix\"",
            "#4_.Search(\"x\")",
            "#4_.Search(\"x\", #2_)",
            "#4_.Extract(#2_, 1)",
            "ReplaceSubText(#4_, \"x\", \"y\")",
            "ToUpper(#4_)",
        ] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_some(),
                "{expression}"
            );
        }

        for expression in [
            "false and (1 / 0 > 2)",
            "true or (1 / 0 > 2)",
            "false ? 1 / 0 ; 5",
            "true ? 5 ; 1 / 0",
        ] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_some(),
                "{expression}"
            );
        }
        for expression in [
            "#3_ > 1 and (1 / 0 > 2)",
            "#3_ > 1 ? 5 ; 1 / 0",
            "false ? 1 / 0 ; 1mm",
        ] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_none(),
                "{expression}"
            );
        }

        for expression in [
            "ToString(#3_)",
            "#4_.Extract(#3_, 1)",
            "round(#1_, \"mm\", #3_)",
            "mod(2, #3_)",
            "1 / 0",
            "1e308 + 1e308",
            "1e308 * 1e308",
            "exp(10000)",
            "sinh(10000)",
            "cosh(10000)",
            "acosh(0)",
            "atanh(1)",
            "#4_.Extract(-1, 1)",
            "\"\".ToReal()",
            "\"not a number\".ToReal()",
            "\"text\".Extract(3, 2)",
            "ReplaceSubText(\"text\", \"\", \"x\")",
            "sqrt(-1)",
        ] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_none(),
                "{expression}"
            );
        }
        assert!(evaluate_formula_expression_with_mode("1 * 0", &bindings, false).is_some());
        for expression in ["exp(#3_)", "acosh(#3_)", "atanh(#3_)"] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_some(),
                "{expression}"
            );
        }

        assert!(
            evaluate_formula_expression_with_mode("round(#1_, \"mm\", #2_)", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .is_some_and(|value| value.dimension() == FormulaDimension::LENGTH)
        );
        assert!(
            evaluate_formula_expression_with_mode("round(#1_, #4_, #2_)", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .is_some_and(|value| value.dimension() == FormulaDimension::LENGTH)
        );
        assert!(evaluate_formula_expression_with_mode(
            "#3_ > 1 ? round(#1_, #4_, #2_) ; 5mm",
            &bindings,
            false,
        )
        .and_then(EvaluatedFormulaValue::scalar)
        .is_some_and(|value| value.dimension() == FormulaDimension::LENGTH));
        assert!(
            evaluate_formula_expression_with_mode("round(#3_, #4_, #2_)", &bindings, false)
                .is_none()
        );
    }

    #[test]
    fn static_formula_branches_use_the_known_scalar_value() {
        let bindings =
            BTreeMap::from([("#1_", static_formula_value(FormulaParameterType::Boolean))]);
        for predicate in [
            "sqrt(4) > 1",
            "abs(-2) == 2",
            "mod(5, 3) == 2",
            "(#1_ ? 2 ; 2) == 2",
            "ToString(abs(-2)) == \"2\"",
        ] {
            let invalid = format!("{predicate} ? 1 / 0 ; 1");
            assert!(
                evaluate_formula_expression_with_mode(&invalid, &bindings, false).is_none(),
                "{invalid}"
            );
            let valid = format!("{predicate} ? 1 ; 1 / 0");
            assert!(
                evaluate_formula_expression_with_mode(&valid, &bindings, false).is_some(),
                "{valid}"
            );
        }
    }

    #[test]
    fn typed_numeric_evaluations_require_finite_values() {
        for source_type in ["LENGTH", "ANGLE", "Real", "R", "Integer", "I"] {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let evaluation = crate::native::CatiaEntityEvaluation::Scalar {
                    bits: value.to_bits(),
                };
                assert!(
                    typed_parameter_evaluation(source_type, &evaluation).is_none(),
                    "{source_type} accepted non-finite value {value:?}"
                );
            }
        }

        assert!(matches!(
            typed_parameter_evaluation(
                "LENGTH",
                &crate::native::CatiaEntityEvaluation::Scalar {
                    bits: 12.5_f64.to_bits(),
                }
            ),
            Some((
                FormulaParameterType::Length,
                TypedParameterEvaluation::Value(ParameterValue::Length(actual_length))
            )) if actual_length.get() == 12.5
        ));
        assert!(matches!(
            typed_parameter_evaluation(
                "Integer",
                &crate::native::CatiaEntityEvaluation::Scalar {
                    bits: (-7.0_f64).to_bits(),
                }
            ),
            Some((
                FormulaParameterType::Integer,
                TypedParameterEvaluation::Value(ParameterValue::Integer(-7))
            ))
        ));
    }

    #[test]
    fn formula_function_argument_count_is_bounded() {
        let bindings = BTreeMap::new();
        for (argument_count, accepted) in [(128, true), (129, false)] {
            let expression = format!("max({})", vec!["1"; argument_count].join(","));
            assert_eq!(
                evaluate_formula_expression(&expression, &bindings).is_some(),
                accepted,
                "{argument_count}"
            );
        }
    }

    #[test]
    fn formula_dimensioned_rounding_uses_a_compatible_unit_and_decimal_precision() {
        let bindings = BTreeMap::new();
        for (expression, expected, dimension) in [
            ("round(12.333mm,\"mm\",1)", 12.3, FormulaDimension::LENGTH),
            ("round(1234mm,\"cm\",0)", 1_230.0, FormulaDimension::LENGTH),
            (
                "round(45.54deg,\"deg\",1)",
                45.5_f64.to_radians(),
                FormulaDimension::ANGLE,
            ),
        ] {
            let value = evaluate_formula_expression(expression, &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("dimensioned rounded value");
            assert!(value.dimension() == dimension, "{expression}");
            assert!(
                (value.value() - expected).abs() <= f64::EPSILON * expected.abs(),
                "{expression}: {} != {expected}",
                value.value()
            );
        }
        for expression in [
            "round(12.3,\"mm\",1)",
            "round(12.3mm,\"deg\",1)",
            "round(12.3mm,\"unknown\",1)",
            "round(12.3mm,\"mm\",-1)",
            "round(12.3mm,\"mm\",1.5)",
            "round(12.3mm,\"mm\",1mm)",
            "round(12.3mm,\"mm\")",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_interpolation_preserves_the_endpoint_dimension() {
        let bindings = BTreeMap::new();
        for (expression, expected, dimension) in [
            (
                "LinearInterpolation(10mm,30mm,0.25)",
                15.0,
                FormulaDimension::LENGTH,
            ),
            (
                "CubicInterpolation(10deg,30deg,0.5)",
                20.0_f64.to_radians(),
                FormulaDimension::ANGLE,
            ),
            (
                "LinearInterpolation(10,30,1.25)",
                35.0,
                FormulaDimension::SCALAR,
            ),
        ] {
            let value = evaluate_formula_expression(expression, &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("typed interpolation");
            assert!(value.dimension() == dimension, "{expression}");
            assert!(
                (value.value() - expected).abs() <= f64::EPSILON * expected.abs().max(1.0),
                "{expression}: {} != {expected}",
                value.value()
            );
        }
        for expression in [
            "LinearInterpolation(10mm,30deg,0.25)",
            "LinearInterpolation(10mm,30mm,0.25mm)",
            "CubicInterpolation(10deg,30,0.5)",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_interpolation_keeps_finite_values_across_an_overflowing_width() {
        let bindings = BTreeMap::new();
        for expression in [
            "LinearInterpolation(1e308,-1e308,0.5)",
            "CubicInterpolation(1e308,-1e308,0.5)",
        ] {
            for evaluate in [true, false] {
                let value = evaluate_formula_expression_with_mode(expression, &bindings, evaluate)
                    .and_then(EvaluatedFormulaValue::scalar)
                    .expect("finite interpolated value");
                assert_eq!(value.value(), 0.0);
                assert_eq!(value.known_value(), Some(0.0));
            }
        }
    }

    #[test]
    fn formula_ternary_depth_is_bounded() {
        let bindings = BTreeMap::new();
        for (depth, accepted) in [(128, true), (129, false)] {
            let expression = format!("{}1{}", "true ? ".repeat(depth), " ; 1".repeat(depth));
            crate::test_support::with_service_context(|ctx| {
                let result =
                    super::evaluate_formula_expression_charged(ctx, &expression, &bindings);
                if accepted {
                    assert!(
                        result.expect("128 descents fit the ceiling").is_some(),
                        "{depth}"
                    );
                } else {
                    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
                        panic!("resource refusal required")
                    };
                    assert_eq!(limit.operation, "catia_formula_expression_local_depth");
                    assert_eq!(limit.limit, 128);
                    assert_eq!(limit.additional, 1);
                    assert_eq!(ctx.resource_refusal(), Some(limit));
                }
            });
        }
    }

    #[test]
    fn formula_comparisons_and_logical_operators_are_typed_and_precedenced() {
        let bindings = BTreeMap::new();
        assert!(
            evaluate_formula_expression("(3 > 2) and (1mm <= 2mm)", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value)
                .is_some_and(|value| value)
        );
        assert_eq!(
            evaluate_formula_expression("false or true and false", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value),
            Some(false)
        );
        assert_eq!(
            evaluate_formula_expression("1mm == 1cm", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value),
            Some(false)
        );
        assert_eq!(
            evaluate_formula_expression("true <> false", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value),
            Some(true)
        );
        assert_eq!(
            evaluate_formula_expression("not false and false or not (1 > 2)", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value),
            Some(true)
        );
        for expression in ["not 1", "not \"false\"", "notable", "not"] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_boolean_operators_and_ternaries_evaluate_lazily() {
        let bindings = BTreeMap::new();
        for (expression, expected) in [
            ("false and (1 / 0 > 2)", false),
            ("true or (sqrt(-1) > 2)", true),
            ("false and (asin(2) > 0rad)", false),
            ("true or (exp(10000) > 0)", true),
            ("false and (1e308 * 1e308 > 0)", false),
            ("true ? 5 ; 1 / 0", true),
            ("false ? sqrt(-1) ; 5", true),
            ("true ? 5 ; (-1) ** 0.5", true),
            ("false ? false ? 1 / 0 ; 2 ; 3", true),
            ("true ? 5 ; mod(2, 1.5)", true),
        ] {
            let value = evaluate_formula_expression(expression, &bindings)
                .expect("lazy expression is complete");
            match value {
                EvaluatedFormulaValue::Boolean(value) => {
                    assert_eq!(value.value(), expected, "{expression}");
                }
                EvaluatedFormulaValue::Scalar(value) => {
                    assert_eq!(
                        value.value(),
                        if expression.ends_with("; 3") {
                            3.0
                        } else {
                            5.0
                        }
                    );
                }
                EvaluatedFormulaValue::String(_) => panic!("unexpected string for {expression}"),
            }
        }
        assert_eq!(
            evaluate_formula_expression(
                "true ? \"selected\" ; ReplaceSubText(\"text\",\"\",\"x\")",
                &bindings,
            )
            .and_then(EvaluatedFormulaValue::string)
            .as_deref(),
            Some("selected")
        );
        assert_eq!(
            evaluate_formula_expression("false ? ToString(1.5) ; \"selected\"", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("selected")
        );
        for expression in [
            "true ? \"selected\" ; \"text\".Extract(1.5, 2)",
            "true ? 5 ; \"text\".Search(\"e\", 1.5)",
            "true ? 5 ; \"not a number\".ToReal()",
            "true ? \"selected\" ; \"text\" - \"\"",
            "true ? 5mm ; round(12.3mm,\"mm\",-1)",
            "true ? 5mm ; round(12.3mm,\"unknown\",1)",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_some(),
                "{expression}"
            );
        }
        assert_eq!(
            evaluate_formula_expression("true ? \"selected\" ; \"text\".Extract(-1,1)", &bindings,)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("selected")
        );
        assert!(
            evaluate_formula_expression("false ? \"not a number\".ToReal() ; 5", &bindings,)
                .and_then(EvaluatedFormulaValue::scalar)
                .is_some_and(|value| value.value() == 5.0)
        );
    }

    #[test]
    fn formula_ternaries_require_boolean_predicates_and_common_branch_types() {
        let bindings = BTreeMap::new();
        for expression in [
            "1 ? 2 ; 3",
            "false ? false ; 1",
            "true ? 1 ;",
            "true ? 1 : 2",
            "true and (1 / 0 > 2)",
            "false or (sqrt(-1) > 2)",
            "true ? 1 / 0 ; 5",
            "false ? 5 ; exp(10000)",
            "true ? 5 ; \"text\".Search(\"t\", 1mm)",
            "true ? \"selected\" ; \"text\".Extract(1mm, 1)",
            "true ? 5mm ; round(12.3mm,\"deg\",1)",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
        for expression in ["true ? 1mm ; 1rad", "false ? 1mm ; 1rad", "true ? 1 ; 1mm"] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
        for (expression, expected_dimension) in [
            ("true ? 1mm ; 2mm", FormulaDimension::LENGTH),
            ("false ? 1rad ; 2rad", FormulaDimension::ANGLE),
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings)
                    .and_then(EvaluatedFormulaValue::scalar)
                    .is_some_and(|value| value.dimension() == expected_dimension),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_string_operations_preserve_typed_values() {
        let bindings = BTreeMap::from([
            (
                "#1_",
                EvaluatedFormulaValue::String(EvaluatedFormulaString::known("Cilas Evans")),
            ),
            (
                "#2_",
                EvaluatedFormulaValue::String(EvaluatedFormulaString::known("Evans")),
            ),
            (
                "#3_",
                EvaluatedFormulaValue::Scalar(finite_scalar(-1.0).expect("finite integer")),
            ),
        ]);
        assert_eq!(
            evaluate_formula_expression("#1_.Length()", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(super::EvaluatedFormulaScalar::value),
            Some(11.0)
        );
        assert_eq!(
            evaluate_formula_expression("#1_ .Search(#2_)", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(super::EvaluatedFormulaScalar::value),
            Some(6.0)
        );
        assert_eq!(
            evaluate_formula_expression("#1_.Search(\"missing\")", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(super::EvaluatedFormulaScalar::value),
            Some(-1.0)
        );
        assert_eq!(
            evaluate_formula_expression("ReplaceSubText(#1_,\"Cilas\",\"Easy\")", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("Easy Evans")
        );
        assert_eq!(
            evaluate_formula_expression("\"Revision\" + ToString(#3_)", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("Revision-1")
        );
        assert!(
            evaluate_formula_expression("\"Cilas Evans\" == #1_", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value)
                .is_some_and(|value| value)
        );
        assert_eq!(
            evaluate_formula_expression("\"Cilas Evans Evans\".Search(\"Evans\",7)", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(super::EvaluatedFormulaScalar::value),
            Some(12.0)
        );
        assert_eq!(
            evaluate_formula_expression(
                "\"Cilas Evans Evans\".Search(\"Evans\",0,false)",
                &bindings,
            )
            .and_then(EvaluatedFormulaValue::scalar)
            .map(super::EvaluatedFormulaScalar::value),
            Some(12.0)
        );
        assert_eq!(
            evaluate_formula_expression("\"é猫x猫\".Search(\"猫\",2)", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(super::EvaluatedFormulaScalar::value),
            Some(3.0)
        );
        assert_eq!(
            evaluate_formula_expression("\"text\".Search(\"t\",5)", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(super::EvaluatedFormulaScalar::value),
            Some(-1.0)
        );
        assert_eq!(
            evaluate_formula_expression("\"é猫x\".Extract(1,1)", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("猫")
        );
        assert_eq!(
            evaluate_formula_expression("\"é猫x\".Extract(3,0)", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("")
        );
        assert_eq!(
            evaluate_formula_expression("ToUpper(\"Mixed Straße\")", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("MIXED STRASSE")
        );
        assert_eq!(
            evaluate_formula_expression("ToLower(\"Mixed Case\")", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("mixed case")
        );
        assert_eq!(
            evaluate_formula_expression("\"12.5\".ToReal()", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(super::EvaluatedFormulaScalar::value),
            Some(12.5)
        );
        assert_eq!(
            evaluate_formula_expression("\"AAxxAA\" - \"AA\"", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("xx")
        );
    }

    #[test]
    fn formula_string_operations_reject_untyped_or_incomplete_forms() {
        let bindings = BTreeMap::new();
        for expression in [
            "\"unterminated",
            "\"unsupported\\\\escape\"",
            "\"control\u{0085}\"",
            "\"text\" + 1",
            "ToString(1.5)",
            "ReplaceSubText(\"text\",\"\",\"x\")",
            "\"text\".Search(1)",
            "\"text\".Search(\"t\",-1)",
            "\"text\".Length(1)",
            "\"text\".Extract(1)",
            "\"text\".Extract(-1,1)",
            "\"text\".Extract(3,2)",
            "\"not a number\".ToReal()",
            "ToUpper(1)",
            "\"text\" - \"\"",
            "\"text\".Unknown()",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_logical_operators_reject_mixed_types_and_chained_comparisons() {
        let bindings = BTreeMap::new();
        for expression in [
            "1mm > 1rad",
            "true + 1",
            "true and 1",
            "false and 1",
            "true or 1",
            "1 < 2 < 3",
            "true >= false",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_length_literals_normalize_every_admitted_unit_to_millimetres() {
        let bindings = BTreeMap::new();
        for (literal, expected) in [
            ("1micron", 0.001),
            ("1mile", 1_609_344.0),
            ("1yard", 914.4),
            ("1mm", 1.0),
            ("1cm", 10.0),
            ("1km", 1_000_000.0),
            ("1ft", 304.8),
            ("1in", 25.4),
            ("1m", 1_000.0),
        ] {
            let actual = evaluate_formula_expression(literal, &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("complete length literal");
            assert_eq!(actual.value(), expected, "{literal}");
            assert!(actual.dimension() == FormulaDimension::LENGTH, "{literal}");
        }
    }

    #[test]
    fn formula_angle_literals_normalize_every_admitted_unit_to_radians() {
        let bindings = BTreeMap::new();
        for (literal, expected) in [
            ("1rad", 1.0),
            ("1grad", std::f64::consts::PI / 200.0),
            ("1deg", std::f64::consts::PI / 180.0),
        ] {
            let actual = evaluate_formula_expression(literal, &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("complete angle literal");
            assert_eq!(actual.value(), expected, "{literal}");
            assert!(actual.dimension() == FormulaDimension::ANGLE, "{literal}");
        }
    }
}

#[cfg(test)]
mod tests;
