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

pub(crate) fn transfer_parameters<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    annotations: &mut Annotations,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<FormulaTransfer<'ctx>, cadmpeg_core::CodecError> {
    const ENTITY_LOOKUP: &str = "catia_formula_entity_lookup";
    // Indexes, candidate keys, conflicts and program snapshots are dropped
    // before this function returns.
    let mut scratch = ctx.reserve_scoped(0, "catia_formula_scratch")?;
    let entities = scratch.with_storage(|| {
        ctx.collect_hash_map(
            native
                .entity_records
                .iter()
                .map(|entity| (entity.id.as_str(), entity)),
            "catia_formula_entity_index",
        )
    })?;
    let entity_by_id = |id: &str| -> Result<_, cadmpeg_core::CodecError> {
        Ok(ctx.get_hash_map(&entities, id, ENTITY_LOOKUP)?.copied())
    };
    let mut candidates = BTreeMap::<ParameterId, FormulaParameterCandidate>::new();
    let mut conflicting_inputs = BTreeSet::<ParameterId>::new();
    collect_definition_chain_parameters(
        ctx,
        &mut scratch,
        native,
        graph_scope,
        &mut candidates,
        &mut conflicting_inputs,
    )?;
    let mut programs = Vec::<FormulaProgramCandidate<'_>>::new();
    let mut formula_definition_counts = HashMap::<ParameterId, usize>::new();
    for entity in ctx
        .admit_iter(&native.entity_records, "catia_formula_entity_visits")?
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
            scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                let output_id = neutral_parameter_id(ctx, output)?;
                *ctx.entry_hash_map(
                    &mut formula_definition_counts,
                    output_id,
                    "catia_formula_definition_counts",
                )?
                .or_default() += 1;
                Ok(())
            })?;
        }
    }
    let legacy_scope = if graph_scope.is_unscoped() {
        LegacyModelingScope::Unbounded
    } else {
        ctx.find_by(
            &native.object_graphs,
            |graph| Ok(graph_scope.contains(graph.id.as_str())),
            "catia_formula_scope_graph_visits",
        )?
        .and_then(|graph| graph.outer_container.as_ref())
        .map_or(
            LegacyModelingScope::Unresolved,
            LegacyModelingScope::Container,
        )
    };
    let legacy_transfer =
        collect_legacy_parameters(ctx, &mut scratch, native, &mut candidates, legacy_scope)?;
    let mut relation_program_parameters =
        BTreeMap::<ParameterId, Option<(DesignParameter, FormulaParameterType)>>::new();
    for program_entity in ctx
        .admit_iter(&native.entity_records, "catia_formula_entity_visits")?
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        const OPERATION: &str = "catia_relation_program_parameter_index";
        let Some(inputs) = program_entity
            .relation_program_instance()
            .and_then(|instance| instance.inputs.as_ref())
        else {
            continue;
        };
        for input in ctx.admit_iter(inputs, "catia_formula_input_visits")? {
            let Some(entity) = input
                .entity
                .entity()
                .map(entity_by_id)
                .transpose()?
                .flatten()
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
            match ctx.get_mut_btree_map(
                &mut relation_program_parameters,
                &candidate.parameter.id,
                OPERATION,
            )? {
                Some(existing) => {
                    if let Some(input) = existing.as_ref() {
                        if input.1 != candidate.parameter_type
                            || !ctx.equal(&input.0, &candidate.parameter, OPERATION)?
                        {
                            *existing = None;
                        }
                    }
                }
                None => scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                    let id = candidate
                        .parameter
                        .id
                        .try_clone_for_decode(ctx, "catia_relation_program_index_id")?;
                    let parameter = copy_design_parameter(ctx, &candidate.parameter)?;
                    ctx.insert_btree_map(
                        &mut relation_program_parameters,
                        id,
                        Some((parameter, candidate.parameter_type)),
                        OPERATION,
                    )?;
                    Ok(())
                })?,
            }
            match ctx.get_btree_map(
                &candidates,
                &candidate.parameter.id,
                "catia_formula_candidates",
            )? {
                Some(existing) => {
                    if !formula_parameter_candidates_agree(ctx, existing, &candidate)? {
                        insert_formula_conflict(
                            ctx,
                            &mut scratch,
                            &mut conflicting_inputs,
                            candidate.parameter.id,
                        )?;
                    }
                }
                None => insert_formula_candidate(ctx, &mut scratch, &mut candidates, candidate)?,
            }
        }
    }

    for formula_entity in ctx
        .admit_iter(&native.entity_records, "catia_formula_entity_visits")?
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        let Some(formula) = &formula_entity.formula_relation() else {
            continue;
        };
        let Some(expression_entity) = formula
            .expression_entity
            .reference
            .entity()
            .map(entity_by_id)
            .transpose()?
            .flatten()
        else {
            continue;
        };
        let Some(expression) = expression_entity.relation_expression() else {
            continue;
        };
        // Signature, bindings, evaluations and the transferred list are dropped
        // with this formula.
        let mut formula_scratch = ctx.reserve_scoped(0, "catia_formula_program_scratch")?;
        let Some(signature) = formula_scratch.with_storage(|| expression.signature_charged(ctx))?
        else {
            continue;
        };
        let mut transferred = Vec::new();
        let mut dependencies = Vec::new();
        let mut program_inputs = BTreeSet::new();
        let mut used_inputs = BTreeSet::new();
        let mut expression_bindings = BTreeMap::new();
        // E7 inputs have a declared type but no value. Keep a separate typed
        // binding set so their defining expression can still be retained as
        // an unset result after syntax and type validation.
        let mut type_bindings = BTreeMap::new();
        let mut all_inputs_complete = true;
        let mut all_inputs_typed = true;
        for dependency in ctx.admit_iter(
            &formula.parameter_dependencies,
            "catia_formula_dependency_visits",
        )? {
            let Some(input) = ctx.find_by(
                &signature.inputs,
                |input| crate::native::dependency_matches_input(ctx, dependency, input),
                "catia_formula_signature_input_visits",
            )?
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
            let Some(entity) = parameter.entity().map(entity_by_id).transpose()?.flatten() else {
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
            formula_scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut used_inputs,
                    input.parameter.as_str(),
                    "catia_formula_used_inputs",
                )
            })?;
            if ctx.contains_btree_set(
                &program_inputs,
                &candidate.parameter.id,
                "catia_formula_program_inputs",
            )? {
                continue;
            }
            scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                let id = candidate
                    .parameter
                    .id
                    .try_clone_for_decode(ctx, "catia_formula_program_inputs")?;
                ctx.insert_btree_set(&mut program_inputs, id, "catia_formula_program_inputs")?;
                Ok(())
            })?;
            let id = candidate
                .parameter
                .id
                .try_clone_for_decode(ctx, "catia_formula_dependency_id")?;
            ctx.push_vec(&mut dependencies, id, "catia_formula_dependencies")?;
            formula_scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                ctx.insert_btree_map(
                    &mut type_bindings,
                    input.parameter.as_str(),
                    static_formula_value(candidate.parameter_type),
                    "catia_formula_type_bindings",
                )?;
                match candidate.parameter.value.as_ref() {
                    None => all_inputs_complete = false,
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
                )
            })?;
        }
        let formula_complete = all_inputs_complete
            && used_inputs.len() == signature.inputs.len()
            && dependencies.len() == signature.inputs.len();
        let formula_type_complete = all_inputs_typed
            && used_inputs.len() == signature.inputs.len()
            && dependencies.len() == signature.inputs.len();
        let result_type = canonical_parameter_type(&signature.result_type);
        let (type_checked_expression, evaluated_expression) =
            formula_scratch.with_storage(|| -> Result<_, cadmpeg_core::CodecError> {
                let type_checked = if formula_type_complete {
                    evaluate_formula_expression_with_mode_charged(
                        ctx,
                        &expression.expression.value,
                        &type_bindings,
                        false,
                    )?
                } else {
                    None
                };
                let evaluated = if formula_complete {
                    evaluate_formula_expression_charged(
                        ctx,
                        &expression.expression.value,
                        &expression_bindings,
                    )?
                } else {
                    None
                };
                Ok((type_checked, evaluated))
            })?;
        let satisfies_result = |value: &EvaluatedFormulaValue| {
            result_type.is_some_and(|ty| value.satisfies_source_type(ty))
        };
        let type_checked_expression = type_checked_expression.filter(satisfies_result);
        let evaluated_expression = evaluated_expression.filter(satisfies_result);
        let transferable_expression = if formula_complete {
            evaluated_expression.as_ref()
        } else {
            type_checked_expression.as_ref()
        };
        if let Some(output) = formula
            .output_entity
            .reference
            .entity()
            .filter(|_| transferable_expression.is_some())
            .map(entity_by_id)
            .transpose()?
            .flatten()
        {
            if let Some(output_value) = output.parameter_value() {
                let output_id = neutral_parameter_id(ctx, &output.id)?;
                if !ctx.contains_btree_set(
                    &program_inputs,
                    &output_id,
                    "catia_formula_program_inputs",
                )? {
                    if let Some((parameter_type, value)) =
                        typed_parameter_evaluation(&signature.result_type, &output_value.evaluation)
                    {
                        let accepted = match &value {
                            TypedParameterEvaluation::Unset => transferable_expression.is_some(),
                            TypedParameterEvaluation::Value(value) => match &evaluated_expression {
                                Some(evaluated) => evaluated.agrees_with_value(ctx, value)?,
                                None => false,
                            },
                        };
                        if accepted {
                            let (program_output, input_parameters) = scratch.with_storage(
                                || -> Result<_, cadmpeg_core::CodecError> {
                                    let output = output_id.try_clone_for_decode(
                                        ctx,
                                        "catia_formula_program_output",
                                    )?;
                                    let inputs = ctx.try_collect_vec(
                                        transferred.iter().map(
                                            |candidate| -> Result<_, cadmpeg_core::CodecError> {
                                                Ok((
                                                    copy_design_parameter(
                                                        ctx,
                                                        &candidate.parameter,
                                                    )?,
                                                    candidate.parameter_type,
                                                ))
                                            },
                                        ),
                                        "catia_formula_input_parameters",
                                    )?;
                                    Ok((output, inputs))
                                },
                            )?;
                            scratch.with_storage(|| {
                                ctx.push_vec(
                                    &mut programs,
                                    FormulaProgramCandidate {
                                        relation_entity: &formula_entity.id,
                                        expression_entity: &expression_entity.id,
                                        output: program_output,
                                        inputs: std::mem::take(&mut program_inputs),
                                        input_parameters,
                                    },
                                    "catia_formula_programs",
                                )
                            })?;
                            ctx.charge_entities(1, "admit CATIA formula candidate")?;
                            let output_dependencies =
                                cadmpeg_ir::features::DistinctMembers::try_from(
                                    std::mem::take(&mut dependencies),
                                    ctx,
                                )?;
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
                            let properties = parameter_properties(
                                ctx,
                                parameter_type.as_str(),
                                Some(output_value.binding.value.as_str()),
                            )?;
                            formula_scratch.with_storage(|| {
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
                                                TypedParameterEvaluation::Value(value) => {
                                                    Some(value)
                                                }
                                            },
                                            dependencies: output_dependencies,
                                            properties,
                                            pmi: None,
                                            native_ref: Some(output_native_ref),
                                        },
                                        parameter_type,
                                        role: FormulaParameterRole::FormulaOutput {
                                            fallback: None,
                                        },
                                        source_order: output.byte_offset,
                                    },
                                    "catia_formula_transferred_inputs",
                                )
                            })?;
                        }
                    }
                }
            }
        }

        for candidate in ctx.admit_iter(transferred, "catia_formula_transferred_visits")? {
            merge_formula_parameter_candidate(
                ctx,
                &mut scratch,
                &mut candidates,
                &mut conflicting_inputs,
                candidate,
            )?;
        }
    }

    for relation_entity in ctx
        .admit_iter(&native.entity_records, "catia_formula_entity_visits")?
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        let Some(instance) = relation_entity.relation_program_instance() else {
            continue;
        };
        let Some(output_entity) = instance
            .output_entity()
            .and_then(|output| output.entity())
            .map(entity_by_id)
            .transpose()?
            .flatten()
        else {
            continue;
        };
        let Some(expression_entity) = instance
            .relation_expression
            .as_deref()
            .map(entity_by_id)
            .transpose()?
            .flatten()
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
            &mut scratch,
            RelationProgramEntities {
                relation: relation_entity,
                expression: expression_entity,
                output: output_entity,
            },
            expression,
            inputs,
            &entities,
        )?
        else {
            continue;
        };
        scratch.with_storage(|| ctx.push_vec(&mut programs, program, "catia_formula_programs"))?;
        merge_formula_parameter_candidate(
            ctx,
            &mut scratch,
            &mut candidates,
            &mut conflicting_inputs,
            candidate,
        )?;
    }

    for id in ctx.admit_iter(&conflicting_inputs, "catia_formula_conflict_visits")? {
        const OPERATION: &str = "catia_formula_conflicts";
        match ctx.get_mut_btree_map(&mut candidates, id, OPERATION)? {
            Some(candidate) if candidate.role.is_formula_output() => {
                if let FormulaParameterRole::FormulaOutput { fallback } = &mut candidate.role {
                    *fallback = None;
                }
            }
            Some(_) => {
                ctx.remove_btree_map(&mut candidates, id, OPERATION)?;
            }
            None => {}
        }
    }
    ctx.retain_btree_map(
        &mut candidates,
        |id, candidate| {
            if !candidate.role.is_formula_output() {
                return Ok(true);
            }
            Ok(
                match ctx.get_hash_map(
                    &formula_definition_counts,
                    id,
                    "catia_formula_definition_counts",
                )? {
                    Some(count) if *count != 1 => demote_formula_output(candidate),
                    Some(_) | None => true,
                },
            )
        },
        "catia_formula_definition_count_visits",
    )?;
    // Outputs are judged against the candidates as they stand before any
    // demotion; demotion is idempotent, so a repeated output is harmless.
    let mut invalid_outputs = Vec::new();
    for program in ctx.admit_iter(&programs, "catia_formula_invalid_output_visits")? {
        const OPERATION: &str = "catia_formula_invalid_outputs";
        if ctx.any_by(
            &program.input_parameters,
            |input| {
                Ok(
                    !match ctx.get_btree_map(&candidates, &input.0.id, OPERATION)? {
                        Some(candidate) => {
                            formula_parameter_candidate_accepts_input(ctx, candidate, input)?
                        }
                        None => false,
                    },
                )
            },
            "catia_formula_invalid_output_input_visits",
        )? {
            scratch
                .with_storage(|| ctx.push_vec(&mut invalid_outputs, &program.output, OPERATION))?;
        }
    }
    for output in ctx.admit_iter(
        invalid_outputs,
        "catia_formula_invalid_output_candidate_visits",
    )? {
        let Some(candidate) =
            ctx.get_mut_btree_map(&mut candidates, output, "catia_formula_invalid_outputs")?
        else {
            continue;
        };
        if !candidate.role.is_formula_output() {
            continue;
        }
        if !demote_formula_output(candidate) {
            ctx.remove_btree_map(&mut candidates, output, "catia_formula_invalid_outputs")?;
        }
    }
    let derivable = derivable_candidates(ctx, &mut scratch, &candidates)?;
    let mut position = 0;
    ctx.retain_btree_map(
        &mut candidates,
        |_, _| {
            let keep = derivable.get(position).copied().unwrap_or(false);
            position += 1;
            Ok(keep)
        },
        "catia_formula_derivable_visits",
    )?;
    let mut relation_program_parameter_count = 0;
    for (id, input) in ctx.admit_iter(
        &relation_program_parameters,
        "catia_formula_relation_parameter_count_visits",
    )? {
        let Some(input) = input else {
            continue;
        };
        if let Some(candidate) =
            ctx.get_btree_map(&candidates, id, "catia_formula_relation_parameter_count")?
        {
            if formula_parameter_candidate_accepts_input(ctx, candidate, input)? {
                relation_program_parameter_count += 1;
            }
        }
    }
    let mut consumed_entities = Vec::<&str>::new();
    for (_, candidate) in ctx.admit_iter(&candidates, "catia_formula_consumed_candidate_visits")? {
        if let Some(native_ref) = candidate.parameter.native_ref.as_deref() {
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut consumed_entities,
                    native_ref,
                    "catia_formula_consumed_entities",
                )
            })?;
        }
    }
    for program in ctx.admit_iter(&programs, "catia_formula_consumed_program_visits")? {
        const OPERATION: &str = "catia_formula_consumed_programs";
        let output = ctx
            .get_btree_map(&candidates, &program.output, OPERATION)?
            .is_some_and(|candidate| candidate.role.is_formula_output());
        if output
            && ctx.all_by(
                &program.inputs,
                |input| ctx.contains_key_btree_map(&candidates, input, OPERATION),
                "catia_formula_consumed_input_visits",
            )?
        {
            scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                ctx.push_vec(
                    &mut consumed_entities,
                    program.relation_entity,
                    "catia_formula_consumed_entities",
                )?;
                ctx.push_vec(
                    &mut consumed_entities,
                    program.expression_entity,
                    "catia_formula_consumed_entities",
                )
            })?;
        }
    }
    let mut consumed_storage = ctx.reserve_scoped(0, "catia_formula_consumed_objects")?;
    let mut consumed_object_records = HashSet::new();
    if !consumed_entities.is_empty() {
        let graphs = scratch.with_storage(|| {
            ctx.collect_hash_map(
                native
                    .object_graphs
                    .iter()
                    .map(|graph| (graph.id.as_str(), graph)),
                "catia_formula_object_graph_index",
            )
        })?;
        for entity_id in
            ctx.admit_iter(&consumed_entities, "catia_formula_consumed_entity_visits")?
        {
            const OPERATION: &str = "catia_formula_consumed_objects";
            let Some(entity) = entity_by_id(entity_id)? else {
                continue;
            };
            let Some(graph) = ctx.get_hash_map(&graphs, entity.object_graph.as_str(), OPERATION)?
            else {
                continue;
            };
            // An entity record's object record is the graph record at its ordinal.
            let Some(object) = usize::try_from(entity.ordinal)
                .ok()
                .and_then(|ordinal| graph.records.get(ordinal))
            else {
                continue;
            };
            if !ctx.equal_bytes(
                object.id.as_bytes(),
                entity.object_record.as_bytes(),
                OPERATION,
            )? {
                continue;
            }
            if entity.formula_relation().is_some()
                || object.subtype() == crate::object_graph::PayloadSubtype::Empty
                    && object.references.is_empty()
            {
                consumed_storage.with_storage(|| {
                    ctx.insert_string_set(
                        &mut consumed_object_records,
                        object.id.as_str(),
                        OPERATION,
                    )
                })?;
            }
        }
    }
    let transferred = candidates.len();
    let mut parameters = scratch.with_storage(|| {
        ctx.collection_vec(candidates.len(), "catia_formula_ordered_parameters")
    })?;
    for (_, candidate) in ctx.admit_iter(candidates, "catia_formula_ordered_parameter_visits")? {
        parameters.push(candidate);
    }
    ctx.stable_sort_by(
        &mut parameters,
        |value| &value.source_order,
        Ord::cmp,
        "catia_formula_ordered_parameters_sort",
    )?;
    for (ordinal, candidate) in ctx
        .admit_iter(&mut parameters, "catia_formula_parameter_ordinals")?
        .enumerate()
    {
        let Some(ordinal) = u32::try_from(ordinal).ok() else {
            return Ok(FormulaTransfer::default());
        };
        candidate.parameter.ordinal = ordinal;
    }
    let mut definition_chain_parameter_count = 0;
    for candidate in ctx.admit_iter(
        &parameters,
        "catia_formula_definition_parameter_count_visits",
    )? {
        let Some(native_ref) = candidate.parameter.native_ref.as_deref() else {
            continue;
        };
        if entity_by_id(native_ref)?.is_some_and(|entity| entity.definition_chain_value().is_some())
        {
            definition_chain_parameter_count += 1;
        }
    }
    let mut annotation_builder = AnnotationBuilder::resume(std::mem::take(annotations));
    for candidate in ctx.admit_iter(&parameters, "catia_formula_annotation_visits")? {
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
    ctx.reserve_vec(
        &mut ir.model.parameters,
        parameters.len(),
        "catia_formula_neutral_parameters",
    )?;
    for candidate in ctx.admit_iter(parameters, "catia_formula_neutral_parameter_visits")? {
        ir.model.parameters.push(candidate.parameter);
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
        _consumed_object_storage: Some(consumed_storage),
    })
}

/// Clones the candidate's identity as its index key and inserts it. The key
/// and the index nodes are dropped with the transfer.
fn insert_formula_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    candidate: FormulaParameterCandidate,
) -> Result<(), cadmpeg_core::CodecError> {
    scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
        let key = candidate
            .parameter
            .id
            .try_clone_for_decode(ctx, "catia_formula_candidate_index_id")?;
        ctx.insert_btree_map(candidates, key, candidate, "catia_formula_candidates")?;
        Ok(())
    })
}

fn insert_formula_conflict(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    conflicting_inputs: &mut BTreeSet<ParameterId>,
    id: ParameterId,
) -> Result<(), cadmpeg_core::CodecError> {
    scratch.with_storage(|| {
        ctx.insert_btree_set(conflicting_inputs, id, "catia_formula_conflicting_inputs")
    })?;
    Ok(())
}

/// Marks, in key order, the candidates whose every dependency is a derivable
/// candidate. A candidate on a dependency cycle, or depending on a missing
/// parameter, is not derivable. Each candidate is released once all of its
/// dependencies are, so every dependency edge is visited once.
fn derivable_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    candidates: &BTreeMap<ParameterId, FormulaParameterCandidate>,
) -> Result<Vec<bool>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "catia_formula_derivable_parameters";
    let count = candidates.len();
    let (mut keys, mut remaining, mut derivable) =
        scratch.with_storage(|| -> Result<_, cadmpeg_core::CodecError> {
            Ok((
                ctx.collection_vec(count, OPERATION)?,
                ctx.collection_vec(count, OPERATION)?,
                ctx.alloc_filled(count, false, OPERATION)?,
            ))
        })?;
    for (id, candidate) in ctx.admit_iter(candidates, "catia_formula_derivable_key_visits")? {
        keys.push(id);
        remaining.push(candidate.parameter.dependencies.len());
    }
    // (dependency position, dependent position) for each dependency that is a candidate.
    let mut edges = Vec::<(usize, usize)>::new();
    for (dependent, (_, candidate)) in ctx
        .admit_iter(candidates, "catia_formula_derivable_candidate_visits")?
        .enumerate()
    {
        for dependency in ctx.admit_iter(
            candidate.parameter.dependencies.as_slice(),
            "catia_formula_derivable_dependency_visits",
        )? {
            if let Ok(position) = ctx.binary_search_by(
                &keys,
                |key| ctx.compare(*key, dependency, OPERATION),
                OPERATION,
            )? {
                scratch
                    .with_storage(|| ctx.push_vec(&mut edges, (position, dependent), OPERATION))?;
            }
        }
    }
    ctx.sort_unstable_by_key(&mut edges, |edge| edge.0, Ord::cmp, OPERATION)?;
    let mut released = scratch.with_storage(|| ctx.collection_vec(count, OPERATION))?;
    for (position, dependencies) in ctx
        .admit_iter(&remaining, "catia_formula_derivable_root_visits")?
        .enumerate()
    {
        if *dependencies == 0 {
            released.push(position);
        }
    }
    while let Some(position) = released.pop() {
        derivable[position] = true;
        let start = ctx.partition_point(&edges, |edge| Ok(edge.0 < position), OPERATION)?;
        let end = ctx.partition_point(&edges, |edge| Ok(edge.0 <= position), OPERATION)?;
        for &(_, dependent) in
            ctx.admit_iter(&edges[start..end], "catia_formula_derivable_edge_visits")?
        {
            remaining[dependent] -= 1;
            if remaining[dependent] == 0 {
                released.push(dependent);
            }
        }
    }
    Ok(derivable)
}

/// Add only typed scalar values from the exact two-definition chain grammar.
///
/// The first definition names the parameter field and the second definition
/// names its source type. The suffix selector must already agree with the
/// first definition; the native decoder enforces that invariant. Other suffix
/// states remain native because they do not contain a neutral parameter value.
fn collect_definition_chain_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    native: &CatiaNative,
    graph_scope: &crate::decode::ModelingGraphScope,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    conflicting_inputs: &mut BTreeSet<ParameterId>,
) -> Result<(), cadmpeg_core::CodecError> {
    for entity in ctx
        .admit_iter(&native.entity_records, "catia_formula_entity_visits")?
        .filter(|entity| graph_scope.contains(entity.object_graph.as_str()))
    {
        let Some(chain) = entity.definition_chain_value() else {
            continue;
        };
        let Some(candidate) = definition_chain_parameter_candidate(ctx, entity, chain)? else {
            continue;
        };
        match ctx.get_btree_map(
            candidates,
            &candidate.parameter.id,
            "catia_formula_chain_candidates",
        )? {
            None => insert_formula_candidate(ctx, scratch, candidates, candidate)?,
            Some(existing) => {
                if !formula_parameter_candidates_agree(ctx, existing, &candidate)? {
                    insert_formula_conflict(
                        ctx,
                        scratch,
                        conflicting_inputs,
                        candidate.parameter.id,
                    )?;
                }
            }
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
pub(crate) struct FormulaTransfer<'ctx> {
    pub(crate) typed_parameter_count: usize,
    pub(crate) definition_chain_parameter_count: usize,
    pub(crate) relation_program_parameter_count: usize,
    pub(crate) legacy_parameter_count: usize,
    pub(crate) legacy_selector_parameter_count: usize,
    pub(crate) legacy_formula_count: usize,
    pub(crate) consumed_object_records: HashSet<String>,
    /// Holds the storage of `consumed_object_records`, which decode drops
    /// before it returns.
    _consumed_object_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

#[cfg(test)]
impl FormulaTransfer<'_> {
    /// Releases the consumed-record storage so a test can keep the counts
    /// after its decode context ends.
    pub(crate) fn detached(self) -> FormulaTransfer<'static> {
        FormulaTransfer {
            typed_parameter_count: self.typed_parameter_count,
            definition_chain_parameter_count: self.definition_chain_parameter_count,
            relation_program_parameter_count: self.relation_program_parameter_count,
            legacy_parameter_count: self.legacy_parameter_count,
            legacy_selector_parameter_count: self.legacy_selector_parameter_count,
            legacy_formula_count: self.legacy_formula_count,
            consumed_object_records: self.consumed_object_records,
            _consumed_object_storage: None,
        }
    }
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

/// Indexes one legacy parameter by its identity and by its name. A key that
/// names more than one parameter keeps a `None` tombstone.
fn insert_legacy_parameter<'run>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    run_scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    indexes: &mut LegacyParameterIndexes<'run>,
    (entity_id, name): (u32, &'run str),
    id: &ParameterId,
) -> Result<(), cadmpeg_core::CodecError> {
    run_scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
        insert_unique_legacy_member(
            ctx,
            &mut indexes.by_entity,
            entity_id,
            id,
            "catia_legacy_parameter_entity_index",
        )?;
        insert_unique_legacy_member(
            ctx,
            &mut indexes.by_name,
            name,
            id,
            "catia_legacy_parameter_name_index",
        )
    })
}

fn insert_unique_legacy_member<K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &mut HashMap<K, Option<ParameterId>>,
    key: K,
    id: &ParameterId,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    match ctx.entry_hash_map(index, key, operation)? {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            entry.insert(None);
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(Some(id.try_clone_for_decode(ctx, operation)?));
        }
    }
    Ok(())
}

/// The run's legacy parameters by identity and by name; a repeated key maps
/// to `None`.
#[derive(Default)]
struct LegacyParameterIndexes<'run> {
    by_entity: HashMap<u32, Option<ParameterId>>,
    by_name: HashMap<&'run str, Option<ParameterId>>,
}

/// Inserts a new legacy candidate. The returned identity is retained by the
/// candidate; its index key is a scoped copy.
fn insert_legacy_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    candidate: FormulaParameterCandidate,
) -> Result<(), cadmpeg_core::CodecError> {
    scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
        let key = candidate
            .parameter
            .id
            .try_clone_for_decode(ctx, "catia_legacy_parameter_candidates")?;
        ctx.insert_btree_map(
            candidates,
            key,
            candidate,
            "catia_legacy_parameter_candidates",
        )?;
        Ok(())
    })
}

fn collect_legacy_parameters(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    native: &CatiaNative,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    modeling_scope: LegacyModelingScope<'_>,
) -> Result<LegacyParameterTransfer, cadmpeg_core::CodecError> {
    let mut transfer = LegacyParameterTransfer::default();
    for run in ctx
        .admit_iter(
            &native.legacy_entity_runs,
            "catia_formula_legacy_run_visits",
        )?
        .filter(|run| outer_container_in_scope(run.outer_container.as_ref(), modeling_scope))
    {
        let mut run_scratch = ctx.reserve_scoped(0, "catia_formula_legacy_run_scratch")?;
        let mut types = LegacyTypeResolver::new(ctx, run)?;
        let mut value_names = None;
        let mut indexes = LegacyParameterIndexes::default();
        for scalar in ctx.admit_iter(&run.scalar_values, "catia_formula_legacy_scalar_visits")? {
            if scalar.encoding != crate::native::CatiaLegacyScalarEncoding::Named84 {
                continue;
            }
            let Some(name) = &scalar.name else {
                continue;
            };
            let Some((value_type, selected)) = types.resolve(ctx, scalar.entity_id)? else {
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
            if ctx.contains_key_btree_map(candidates, &id, "catia_legacy_parameter_candidates")? {
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
            let parameter_name = ctx.copy_retained_text(name, "catia_legacy_scalar_name")?;
            let native_ref = ctx.copy_retained_text(&run.id, "catia_legacy_scalar_native_ref")?;
            insert_legacy_parameter(
                ctx,
                &mut run_scratch,
                &mut indexes,
                (scalar.entity_id, name),
                &id,
            )?;
            insert_legacy_candidate(
                ctx,
                scratch,
                candidates,
                FormulaParameterCandidate {
                    parameter: DesignParameter {
                        id,
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
                },
            )?;
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        for string in ctx.admit_iter(&run.string_values, "catia_formula_legacy_string_visits")? {
            let Some(name) = &string.name else {
                continue;
            };
            let Some((value_type, selected)) = resolved_or_intrinsic_legacy_type(
                ctx,
                run,
                &mut types,
                &mut value_names,
                LegacyIntrinsicValue {
                    entity_id: string.entity_id,
                    value_offset: string.byte_offset,
                    name_field: string.name_field,
                    name,
                    intrinsic_type: "String",
                },
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
            if ctx.contains_key_btree_map(candidates, &id, "catia_legacy_parameter_candidates")? {
                continue;
            }
            ctx.charge_entities(1, "admit CATIA formula candidate")?;
            let value = ParameterValue::String(
                ctx.copy_retained_text(&string.value, "catia_legacy_string_value")?,
            );
            let parameter_name = ctx.copy_retained_text(name, "catia_legacy_string_name")?;
            let native_ref = ctx.copy_retained_text(&run.id, "catia_legacy_string_native_ref")?;
            insert_legacy_parameter(
                ctx,
                &mut run_scratch,
                &mut indexes,
                (string.entity_id, name),
                &id,
            )?;
            insert_legacy_candidate(
                ctx,
                scratch,
                candidates,
                FormulaParameterCandidate {
                    parameter: DesignParameter {
                        id,
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
                },
            )?;
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        for integer in ctx.admit_iter(&run.integer_values, "catia_formula_legacy_integer_visits")? {
            let Some(name) = &integer.name else {
                continue;
            };
            let Some((value_type, selected)) = resolved_or_intrinsic_legacy_type(
                ctx,
                run,
                &mut types,
                &mut value_names,
                LegacyIntrinsicValue {
                    entity_id: integer.entity_id,
                    value_offset: integer.byte_offset,
                    name_field: integer.name_field,
                    name,
                    intrinsic_type: "Integer",
                },
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
            if ctx.contains_key_btree_map(candidates, &id, "catia_legacy_parameter_candidates")? {
                continue;
            }
            ctx.charge_entities(1, "admit CATIA formula candidate")?;
            let value = ParameterValue::Integer(i64::from(integer.value));
            let parameter_name = ctx.copy_retained_text(name, "catia_legacy_integer_name")?;
            let native_ref = ctx.copy_retained_text(&run.id, "catia_legacy_integer_native_ref")?;
            insert_legacy_parameter(
                ctx,
                &mut run_scratch,
                &mut indexes,
                (integer.entity_id, name),
                &id,
            )?;
            insert_legacy_candidate(
                ctx,
                scratch,
                candidates,
                FormulaParameterCandidate {
                    parameter: DesignParameter {
                        id,
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
                },
            )?;
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        // The relation of each parameter identity, or `None` when several name it.
        let mut relations_by_parameter =
            BTreeMap::<u32, Option<&crate::native::CatiaLegacyRelation>>::new();
        for relation in ctx.admit_iter(&run.relations, "catia_formula_legacy_relation_visits")? {
            if let Some(parameter) = relation.parameter_entity_id {
                run_scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                    match ctx.entry_btree_map(
                        &mut relations_by_parameter,
                        parameter,
                        "catia_legacy_relation_index",
                    )? {
                        std::collections::btree_map::Entry::Occupied(mut entry) => {
                            entry.insert(None);
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(Some(relation));
                        }
                    }
                    Ok(())
                })?;
            }
        }
        for (entity_id, relation) in ctx.admit_iter(
            &relations_by_parameter,
            "catia_legacy_relation_group_visits",
        )? {
            let Some(relation) = relation else {
                continue;
            };
            let Some(Some(parameter)) = ctx.get_hash_map(
                &indexes.by_entity,
                entity_id,
                "catia_legacy_parameter_entity_index",
            )?
            else {
                continue;
            };
            let Some(evaluation) = legacy_relation_evaluation(
                ctx,
                &mut run_scratch,
                relation,
                &indexes.by_name,
                candidates,
            )?
            else {
                continue;
            };
            let Some(candidate) =
                ctx.get_mut_btree_map(candidates, parameter, "catia_legacy_parameter_candidates")?
            else {
                continue;
            };
            if canonical_parameter_type(evaluation.source_type) != Some(candidate.parameter_type) {
                continue;
            }
            if let Some(stored) = candidate.parameter.value.as_ref() {
                if !evaluation.evaluated.agrees_with_value(ctx, stored)? {
                    continue;
                }
            }
            let expression =
                ctx.copy_retained_text(evaluation.expression, "catia_legacy_formula_expression")?;
            candidate.parameter.expression = expression;
            candidate.parameter.dependencies =
                cadmpeg_ir::features::DistinctMembers::try_from(evaluation.dependencies, ctx)?;
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
    crate::test_support::with_service_context(|ctx| {
        let Some(expression) = legacy_output_assignment_expression(ctx, source, output_parameter)?
        else {
            return Ok(None);
        };
        evaluate_formula_expression_charged(ctx, expression, &BTreeMap::new())
    })
    .expect("service profile admits legacy assignment fixture")
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
    run_scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    relation: &'a crate::native::CatiaLegacyRelation,
    parameters_by_name: &HashMap<&str, Option<ParameterId>>,
    candidates: &BTreeMap<ParameterId, FormulaParameterCandidate>,
) -> Result<Option<LegacyRelationEvaluation<'a>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "catia_legacy_formula_bindings";
    let (source_type, expression) = match relation.output.as_ref() {
        Some(output) if relation.result_type == "VoidType" => {
            let Some(expression) =
                legacy_output_assignment_expression(ctx, &relation.expression, &output.parameter)?
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
    // Symbols, bindings and the evaluated value are dropped with the run.
    let symbols = if relation.inputs.is_empty() {
        None
    } else {
        Some(
            run_scratch
                .with_storage(|| crate::native::relation_symbols(ctx, &relation.expression))?,
        )
    };
    let mut bindings = BTreeMap::new();
    let mut bound = HashSet::new();
    let mut dependencies = Vec::new();
    for input in ctx.admit_iter(&relation.inputs, "catia_formula_legacy_input_visits")? {
        let Some(symbols) = symbols.as_ref() else {
            return Ok(None);
        };
        if !ctx.any_by(
            symbols,
            |(_, symbol)| Ok(legacy_symbol_matches_input(ctx, symbol, &input.parameter)?),
            "catia_legacy_formula_symbol_visits",
        )? {
            return Ok(None);
        }
        let Some(Some(parameter_id)) = ctx.get_hash_map(
            parameters_by_name,
            input.parameter.as_str(),
            "catia_legacy_parameter_name_index",
        )?
        else {
            return Ok(None);
        };
        if !run_scratch.with_storage(|| ctx.insert_hash_set(&mut bound, parameter_id, OPERATION))? {
            return Ok(None);
        }
        let Some(candidate) = ctx.get_btree_map(
            candidates,
            parameter_id,
            "catia_legacy_parameter_candidates",
        )?
        else {
            return Ok(None);
        };
        if canonical_parameter_type(&input.value_type) != Some(candidate.parameter_type) {
            return Ok(None);
        }
        let Some(value) = candidate.parameter.value.as_ref() else {
            return Ok(None);
        };
        run_scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
            ctx.insert_btree_map(
                &mut bindings,
                input.parameter.as_str(),
                EvaluatedFormulaValue::from_parameter_value_charged(ctx, value)?,
                OPERATION,
            )?;
            Ok(())
        })?;
        let dependency =
            parameter_id.try_clone_for_decode(ctx, "catia_legacy_formula_dependency_id")?;
        ctx.push_vec(
            &mut dependencies,
            dependency,
            "catia_legacy_formula_dependencies",
        )?;
    }
    let Some(evaluated) = run_scratch
        .with_storage(|| evaluate_formula_expression_charged(ctx, expression, &bindings))?
    else {
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

fn legacy_symbol_matches_input(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    symbol: &str,
    input: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    const OPERATION: &str = "catia_legacy_symbol_ordinal_visits";
    let Some(suffix) = ctx.strip_prefix(symbol, input, OPERATION)? else {
        return Ok(false);
    };
    let suffix = ctx.trim_start_matches(
        suffix,
        |character| Ok(character.is_ascii_whitespace()),
        OPERATION,
    )?;
    if suffix.is_empty() {
        return Ok(true);
    }
    let Some(ordinal) = suffix.strip_prefix('/') else {
        return Ok(false);
    };
    Ok(!ordinal.is_empty()
        && ctx.all_by(
            ordinal.as_bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            OPERATION,
        )?)
}

fn legacy_output_assignment_expression<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &'a str,
    output_parameter: &str,
) -> Result<Option<&'a str>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "catia_legacy_output_assignment";
    let source = ctx.trim_matches(
        source,
        |character| Ok(character.is_ascii_whitespace()),
        OPERATION,
    )?;
    let Some(remainder) = ctx.strip_prefix(source, output_parameter, OPERATION)? else {
        return Ok(None);
    };
    let remainder = ctx.trim_start_matches(
        remainder,
        |character| Ok(character.is_ascii_whitespace()),
        OPERATION,
    )?;
    let Some(remainder) = remainder.strip_prefix('=') else {
        return Ok(None);
    };
    if remainder.starts_with('=') {
        return Ok(None);
    }
    let expression = ctx.trim_matches(
        remainder,
        |character| Ok(character.is_ascii_whitespace()),
        OPERATION,
    )?;
    Ok((!expression.is_empty()).then_some(expression))
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

/// Resolves legacy value types through the run's type descriptors. A
/// resolution is remembered for every identity on its selector chain, so each
/// chain is walked once per run.
struct LegacyTypeResolver<'run, 'ctx> {
    /// The unique descriptor of each identity; a repeated identity maps to `None`.
    descriptors: HashMap<u32, Option<&'run crate::native::CatiaLegacyTypeDescriptor>>,
    resolved: HashMap<u32, Option<(&'run str, bool)>>,
    descriptor_count: usize,
    descriptor_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'run, 'ctx> LegacyTypeResolver<'run, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        run: &'run crate::native::CatiaLegacyEntityRun,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let (descriptors, descriptor_storage) = ctx.unique_index(
            run.type_descriptors
                .iter()
                .map(|descriptor| (descriptor.entity_id, descriptor)),
            "catia_legacy_type_descriptor_index",
        )?;
        Ok(Self {
            descriptors,
            resolved: HashMap::new(),
            descriptor_count: run.type_descriptors.len(),
            descriptor_storage,
        })
    }

    /// The type name selected for an identity, and whether a selector chain
    /// led to it. A missing or repeated descriptor, or a chain longer than the
    /// run's descriptors, has no type.
    fn resolve(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        entity_id: u32,
    ) -> Result<Option<(&'run str, bool)>, cadmpeg_core::CodecError> {
        const OPERATION: &str = "catia_legacy_type_selector_chain";
        let mut chain = Vec::new();
        let mut current = entity_id;
        let (terminal, remembered) = loop {
            if let Some(resolved) = ctx.get_hash_map(&self.resolved, &current, OPERATION)? {
                break (*resolved, true);
            }
            if chain.len() >= self.descriptor_count {
                break (None, false);
            }
            let Some(Some(descriptor)) = ctx
                .get_hash_map(&self.descriptors, &current, OPERATION)?
                .copied()
            else {
                break (None, false);
            };
            match &descriptor.value {
                crate::native::CatiaLegacyTypeValue::Name { value } => {
                    break (Some((value.as_str(), false)), false);
                }
                crate::native::CatiaLegacyTypeValue::Selector { value } => {
                    self.descriptor_storage
                        .with_storage(|| ctx.push_vec(&mut chain, current, OPERATION))?;
                    current = *value;
                }
            }
        };
        let selected = terminal.map(|(name, _)| (name, true));
        let resolved = &mut self.resolved;
        self.descriptor_storage
            .with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                if !remembered {
                    ctx.insert_hash_map(resolved, current, terminal, OPERATION)?;
                }
                for identity in ctx.admit_iter(&chain, OPERATION)? {
                    ctx.insert_hash_map(resolved, *identity, selected, OPERATION)?;
                }
                Ok(())
            })?;
        Ok(if chain.is_empty() { terminal } else { selected })
    }

    /// Whether the run has any descriptor, unique or repeated, for an identity.
    fn has_descriptor(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        entity_id: u32,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        // A repeated descriptor keeps its key, so presence counts every descriptor.
        ctx.contains_key_hash_map(
            &self.descriptors,
            &entity_id,
            "catia_legacy_type_descriptor_visits",
        )
    }
}

/// A legacy value whose type may come from its bound evaluation name.
struct LegacyIntrinsicValue<'a> {
    entity_id: u32,
    value_offset: u64,
    name_field: Option<u64>,
    name: &'a str,
    intrinsic_type: &'static str,
}

fn resolved_or_intrinsic_legacy_type<'run, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    run: &'run crate::native::CatiaLegacyEntityRun,
    types: &mut LegacyTypeResolver<'run, '_>,
    value_names: &mut Option<crate::native::LegacyEvaluatedValueNames<'run, 'ctx>>,
    value: LegacyIntrinsicValue<'_>,
) -> Result<Option<(&'run str, bool)>, cadmpeg_core::CodecError> {
    if let Some(resolved) = types.resolve(ctx, value.entity_id)? {
        return Ok(Some(resolved));
    }
    if types.has_descriptor(ctx, value.entity_id)? {
        return Ok(None);
    }
    let Some(name_field) = value.name_field else {
        return Ok(None);
    };
    if value_names.is_none() {
        *value_names = Some(crate::native::LegacyEvaluatedValueNames::new(
            ctx,
            &run.role_selectors,
            &run.text_fields,
        )?);
    }
    let Some(value_names) = value_names.as_ref() else {
        return Ok(None);
    };
    let Some(field) = value_names.name(ctx, value.entity_id, value.value_offset)? else {
        return Ok(None);
    };
    Ok((field.byte_offset == name_field
        && ctx.equal_bytes(
            field.value.as_bytes(),
            value.name.as_bytes(),
            "catia_legacy_intrinsic_name",
        )?)
    .then_some((value.intrinsic_type, false)))
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

struct FormulaProgramCandidate<'a> {
    relation_entity: &'a str,
    expression_entity: &'a str,
    output: ParameterId,
    inputs: BTreeSet<ParameterId>,
    input_parameters: Vec<(DesignParameter, FormulaParameterType)>,
}

fn merge_formula_parameter_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    conflicting_inputs: &mut BTreeSet<ParameterId>,
    mut candidate: FormulaParameterCandidate,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(existing) = ctx.get_mut_btree_map(
        candidates,
        &candidate.parameter.id,
        "catia_formula_candidates",
    )?
    else {
        return insert_formula_candidate(ctx, scratch, candidates, candidate);
    };
    let existing_output = existing.role.is_formula_output();
    let candidate_output = candidate.role.is_formula_output();
    if !formula_parameter_candidates_agree(ctx, existing, &candidate)? {
        match (existing_output, candidate_output) {
            (true, true) => {}
            (true, false) | (false, false) => {
                insert_formula_conflict(ctx, scratch, conflicting_inputs, candidate.parameter.id)?;
            }
            (false, true) => {
                let conflict = scratch.with_storage(|| {
                    candidate
                        .parameter
                        .id
                        .try_clone_for_decode(ctx, "catia_formula_conflict_id")
                })?;
                insert_formula_conflict(ctx, scratch, conflicting_inputs, conflict)?;
                insert_formula_candidate(ctx, scratch, candidates, candidate)?;
            }
        }
        return Ok(());
    }
    match (existing_output, candidate_output) {
        (false, true) => {
            candidate.role = FormulaParameterRole::FormulaOutput {
                fallback: Some(Box::new((
                    copy_design_parameter(ctx, &existing.parameter)?,
                    existing.parameter_type,
                ))),
            };
            insert_formula_candidate(ctx, scratch, candidates, candidate)?;
        }
        (true, false) => {
            if let FormulaParameterRole::FormulaOutput { fallback } = &mut existing.role {
                fallback.get_or_insert_with(|| {
                    Box::new((candidate.parameter, candidate.parameter_type))
                });
            }
        }
        (true, true) | (false, false) => {}
    }
    Ok(())
}

/// The relation, expression and output entities of one relation program.
struct RelationProgramEntities<'a> {
    relation: &'a crate::native::entity_record::CatiaEntityRecord,
    expression: &'a crate::native::entity_record::CatiaEntityRecord,
    output: &'a crate::native::entity_record::CatiaEntityRecord,
}

fn relation_program_output_candidate<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    program_entities: RelationProgramEntities<'a>,
    expression: &crate::native::CatiaRelationExpression,
    inputs: &[crate::native::CatiaRelationProgramInput],
    entities: &HashMap<&str, &crate::native::entity_record::CatiaEntityRecord>,
) -> Result<
    Option<(FormulaProgramCandidate<'a>, FormulaParameterCandidate)>,
    cadmpeg_core::CodecError,
> {
    const OPERATION: &str = "catia_relation_program_inputs";
    let RelationProgramEntities {
        relation: relation_entity,
        expression: expression_entity,
        output: output_entity,
    } = program_entities;
    // The signature, input snapshots, bindings and evaluations are dropped
    // unless the program is kept; kept parts move to the transfer scratch.
    let mut program_scratch = ctx.reserve_scoped(0, "catia_relation_program_scratch")?;
    let Some(signature) = program_scratch.with_storage(|| expression.signature_charged(ctx))?
    else {
        return Ok(None);
    };
    if inputs.len() != signature.inputs.len()
        || ctx.any_by(
            inputs.iter().zip(&signature.inputs),
            |(input, declared)| {
                Ok(!ctx.equal_bytes(
                    input.parameter.as_bytes(),
                    declared.parameter.as_bytes(),
                    OPERATION,
                )? || !ctx.equal_bytes(
                    input.value_type.as_bytes(),
                    declared.input_type.as_bytes(),
                    OPERATION,
                )?)
            },
            "catia_relation_program_signature_input_visits",
        )?
    {
        return Ok(None);
    }

    let mut dependencies = BTreeSet::new();
    let mut input_parameters = Vec::new();
    let mut expression_bindings = BTreeMap::new();
    let mut type_bindings = BTreeMap::new();
    let mut all_inputs_complete = true;
    for input in ctx.admit_iter(inputs, "catia_formula_input_visits")? {
        let Some(input_entity) = input
            .entity
            .entity()
            .map(|id| ctx.get_hash_map(entities, id, "catia_formula_entity_lookup"))
            .transpose()?
            .flatten()
        else {
            return Ok(None);
        };
        let Some(candidate) = program_scratch.with_storage(|| {
            typed_entity_parameter_candidate_for_source(ctx, input_entity, &input.value_type)
        })?
        else {
            return Ok(None);
        };
        if ctx.contains_btree_set(&dependencies, &candidate.parameter.id, OPERATION)? {
            return Ok(None);
        }
        program_scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
            let dependency = candidate
                .parameter
                .id
                .try_clone_for_decode(ctx, "catia_relation_program_dependency_id")?;
            ctx.insert_btree_set(
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
            )
        })?;
    }

    let result_type = canonical_parameter_type(&signature.result_type);
    let satisfies_result = |value: &EvaluatedFormulaValue| {
        result_type.is_some_and(|ty| value.satisfies_source_type(ty))
    };
    let (type_checked_expression, evaluated_expression) =
        program_scratch.with_storage(|| -> Result<_, cadmpeg_core::CodecError> {
            let type_checked = evaluate_formula_expression_with_mode_charged(
                ctx,
                &expression.expression.value,
                &type_bindings,
                false,
            )?;
            let evaluated = if all_inputs_complete {
                evaluate_formula_expression_charged(
                    ctx,
                    &expression.expression.value,
                    &expression_bindings,
                )?
            } else {
                None
            };
            Ok((type_checked, evaluated))
        })?;
    let type_checked_expression = type_checked_expression.filter(satisfies_result);
    let evaluated_expression = evaluated_expression.filter(satisfies_result);
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
    if ctx.contains_btree_set(&dependencies, &output_id, OPERATION)? {
        return Ok(None);
    }
    let Some((parameter_type, value)) =
        typed_parameter_evaluation(&signature.result_type, &output_value.evaluation)
    else {
        return Ok(None);
    };
    let accepted = match &value {
        TypedParameterEvaluation::Unset => true,
        TypedParameterEvaluation::Value(value) => match &evaluated_expression {
            Some(evaluated) => evaluated.agrees_with_value(ctx, value)?,
            None => false,
        },
    };
    if !accepted {
        return Ok(None);
    }
    ctx.charge_entities(1, "admit CATIA formula candidate")?;
    let program_output = scratch.with_storage(|| {
        output_id.try_clone_for_decode(ctx, "catia_relation_program_candidate_id")
    })?;
    let (program_inputs, program_parameters) =
        scratch.with_storage(|| -> Result<_, cadmpeg_core::CodecError> {
            let mut inputs = BTreeSet::new();
            for dependency in
                ctx.admit_iter(&dependencies, "catia_formula_program_dependency_visits")?
            {
                ctx.insert_btree_set(
                    &mut inputs,
                    dependency.try_clone_for_decode(ctx, "catia_relation_program_dependency_id")?,
                    "catia_relation_program_dependencies",
                )?;
            }
            let parameters = ctx.try_collect_vec(
                input_parameters.iter().map(
                    |(parameter, parameter_type)| -> Result<_, cadmpeg_core::CodecError> {
                        Ok((copy_design_parameter(ctx, parameter)?, *parameter_type))
                    },
                ),
                "catia_relation_program_input_parameters",
            )?;
            Ok((inputs, parameters))
        })?;
    let output_dependencies = ctx.try_collect_vec(
        input_parameters.iter().map(|(parameter, _)| {
            parameter
                .id
                .try_clone_for_decode(ctx, "catia_relation_program_output_dependency_id")
        }),
        "catia_relation_program_output_dependencies",
    )?;
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
    let candidate = FormulaParameterCandidate {
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
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(
                output_dependencies,
                ctx,
            )?,
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
            relation_entity: &relation_entity.id,
            expression_entity: &expression_entity.id,
            output: program_output,
            inputs: program_inputs,
            input_parameters: program_parameters,
        },
        candidate,
    )))
}

fn formula_parameter_candidates_agree(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    existing: &FormulaParameterCandidate,
    candidate: &FormulaParameterCandidate,
) -> Result<bool, cadmpeg_core::CodecError> {
    if existing.source_order != candidate.source_order
        || existing.parameter_type != candidate.parameter_type
    {
        return Ok(false);
    }
    match (
        existing.role.is_formula_output(),
        candidate.role.is_formula_output(),
    ) {
        (true, true) | (false, false) => ctx.equal(
            &existing.parameter,
            &candidate.parameter,
            "catia_formula_parameter_comparison",
        ),
        (true, false) => {
            formula_parameter_matches_input(ctx, &existing.parameter, &candidate.parameter)
        }
        (false, true) => {
            formula_parameter_matches_input(ctx, &candidate.parameter, &existing.parameter)
        }
    }
}

/// Compares a formula output with an input parameter, ignoring the output's
/// expression and dependencies.
fn formula_parameter_matches_input(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    formula: &DesignParameter,
    input: &DesignParameter,
) -> Result<bool, cadmpeg_core::CodecError> {
    const OPERATION: &str = "catia_formula_parameter_comparison";
    Ok(formula.ordinal == input.ordinal
        && formula.display == input.display
        && ctx.equal(&formula.id, &input.id, OPERATION)?
        && ctx.equal(&formula.owner, &input.owner, OPERATION)?
        && ctx.equal_bytes(formula.name.as_bytes(), input.name.as_bytes(), OPERATION)?
        && ctx.equal(&formula.value, &input.value, OPERATION)?
        && formula.properties.len() == input.properties.len()
        && ctx.all_by(
            formula.properties.iter().zip(&input.properties),
            |((formula_key, formula_value), (input_key, input_value))| {
                Ok(ctx.equal(formula_key, input_key, OPERATION)?
                    && ctx.equal_bytes(
                        formula_value.as_bytes(),
                        input_value.as_bytes(),
                        OPERATION,
                    )?)
            },
            OPERATION,
        )?
        && ctx.equal(&formula.pmi, &input.pmi, OPERATION)?
        && ctx.equal(&formula.native_ref, &input.native_ref, OPERATION)?)
}

fn formula_parameter_candidate_accepts_input(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidate: &FormulaParameterCandidate,
    input: &(DesignParameter, FormulaParameterType),
) -> Result<bool, cadmpeg_core::CodecError> {
    if candidate.parameter_type != input.1 {
        return Ok(false);
    }
    if candidate.role.is_formula_output() {
        formula_parameter_matches_input(ctx, &candidate.parameter, &input.0)
    } else {
        ctx.equal(
            &candidate.parameter,
            &input.0,
            "catia_formula_parameter_comparison",
        )
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
        for dependency in ctx.admit_iter(source.dependencies.as_slice(), operation)? {
            dependencies.insert(
                ctx,
                dependency.try_clone_for_decode(ctx, operation)?,
                operation,
            )?;
        }
    }
    let mut properties = BTreeMap::new();
    for (key, value) in ctx.admit_iter(&source.properties, operation)? {
        let key = ctx.copy_retained_text(key.as_str(), operation)?;
        let key =
            cadmpeg_core::text::NonBlankString::for_decode(ctx, key, "validate nonblank text")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("blank CATIA parameter property")
                })?;
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
    if !ctx.all_by(
        value.chars(),
        |character| Ok(character != '"' && character != '\\' && !character.is_control()),
        "catia_formula_string_literal_scan",
    )? {
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
    let key = cadmpeg_core::text::NonBlankString::for_decode(ctx, key, "validate nonblank text")?
        .ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("empty CATIA formula property key")
    })?;
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

    /// Whether this evaluated value equals a stored parameter value.
    fn agrees_with_value(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: &ParameterValue,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        if let ParameterValue::String(right) = value {
            return Ok(match self {
                Self::String(left) => {
                    left.is_known()
                        && ctx.equal_bytes(
                            left.value().as_bytes(),
                            right.as_bytes(),
                            "catia_formula_value_agreement",
                        )?
                }
                Self::Scalar(_) | Self::Boolean(_) => false,
            });
        }
        Ok(match (self, Self::from_parameter_value(value)) {
            (Self::Boolean(left), Self::Boolean(right)) => {
                left.known_value() == right.known_value()
            }
            (Self::Scalar(left), Self::Scalar(right)) => {
                left.dimension() == right.dimension() && left.known_value() == right.known_value()
            }
            _ => false,
        })
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

/// Replaces every non-overlapping occurrence of a pattern; an empty pattern
/// inserts the replacement around every character.
fn replaced_text(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    (source, from, to): (&str, &str, &str),
    work_operation: &'static str,
    text_operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    // Formatting measures the text and then writes it. Each pass visits every
    // source character, or runs forward linear-time searches from one match to
    // the next that visit every source byte once and every match's pattern bytes
    // once; matches do not overlap. The written text is charged when formatted.
    let pass = u64_from_index(source.len())
        .checked_mul(2)
        .and_then(|work| work.checked_add(u64_from_index(from.len())))
        .and_then(|work| work.checked_mul(2))
        .ok_or_else(|| ctx.refuse_codec_limit(work_operation, u64::MAX, u64::MAX))?;
    ctx.charge_work(pass, work_operation)?;
    ctx.format_retained(
        format_args!("{}", ReplacedText { source, from, to }),
        text_operation,
    )
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

macro_rules! formula_value {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

impl FormulaExpressionParser<'_, '_, '_, '_> {
    fn parse(mut self) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let Some(value) = self.conditional(0)? else {
            return Ok(None);
        };
        self.skip_whitespace();
        Ok((self.at == self.source.len()).then_some(value))
    }

    fn conditional(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let predicate = formula_value!(self.disjunction(depth)?);
        self.skip_whitespace();
        if self.peek() != Some(b'?') {
            return Ok(Some(predicate));
        }
        self.at += 1;
        let predicate = formula_value!(predicate.boolean());
        let evaluate = self.evaluate;
        let static_check = self.static_check;
        self.evaluate = evaluate && predicate.value();
        self.static_check = static_check && (!predicate.is_known() || predicate.value());
        let when_true = formula_value!(self.descend(depth, Self::conditional)?);
        self.skip_whitespace();
        formula_value!((formula_value!(self.peek()) == b';').then_some(()));
        self.at += 1;
        self.evaluate = evaluate && !predicate.value();
        self.static_check = static_check && (!predicate.is_known() || !predicate.value());
        let when_false = formula_value!(self.descend(depth, Self::conditional)?);
        self.evaluate = evaluate;
        self.static_check = static_check;
        formula_value!(Self::same_value_type(&when_true, &when_false));
        if evaluate {
            return Ok(Some(if predicate.value() {
                when_true
            } else {
                when_false
            }));
        }
        Ok(Some(if let Some(predicate) = predicate.known_value() {
            if predicate {
                when_true
            } else {
                when_false
            }
        } else {
            formula_value!(self.merge_static_values(&when_true, &when_false)?)
        }))
    }

    fn merge_static_values(
        &mut self,
        left: &EvaluatedFormulaValue,
        right: &EvaluatedFormulaValue,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        Ok(match (left, right) {
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
                    if left.is_known()
                        && right.is_known()
                        && self.ctx.equal_bytes(
                            left.value().as_bytes(),
                            right.value().as_bytes(),
                            "catia_formula_static_string_merge",
                        )?
                    {
                        let result = self
                            .ctx
                            .copy_retained_text(left.value(), "catia_formula_static_string_merge");
                        EvaluatedFormulaString::known(result?)
                    } else {
                        EvaluatedFormulaString::unknown()
                    },
                ))
            }
            _ => None,
        })
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

    fn disjunction(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let mut value = formula_value!(self.conjunction(depth)?);
        loop {
            self.skip_whitespace();
            if !self.consume_keyword("or") {
                return Ok(Some(value));
            }
            let evaluate = self.evaluate;
            let static_check = self.static_check;
            let left = formula_value!(value.boolean());
            self.evaluate = evaluate && !left.value();
            self.static_check = static_check && (!left.is_known() || !left.value());
            let right = formula_value!(self.conjunction(depth)?);
            let right = formula_value!(right.boolean());
            self.evaluate = evaluate;
            self.static_check = static_check;
            value = EvaluatedFormulaValue::Boolean(if evaluate {
                EvaluatedFormulaBoolean::known(left.value() || right.value())
            } else {
                left.or(right)
            });
        }
    }

    fn conjunction(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let mut value = formula_value!(self.comparison(depth)?);
        loop {
            self.skip_whitespace();
            if !self.consume_keyword("and") {
                return Ok(Some(value));
            }
            let evaluate = self.evaluate;
            let static_check = self.static_check;
            let left = formula_value!(value.boolean());
            self.evaluate = evaluate && left.value();
            self.static_check = static_check && (!left.is_known() || left.value());
            let right = formula_value!(self.comparison(depth)?);
            let right = formula_value!(right.boolean());
            self.evaluate = evaluate;
            self.static_check = static_check;
            value = EvaluatedFormulaValue::Boolean(if evaluate {
                EvaluatedFormulaBoolean::known(left.value() && right.value())
            } else {
                left.and(right)
            });
        }
    }

    fn comparison(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let left = formula_value!(self.sum(depth)?);
        self.skip_whitespace();
        let Some((operator, width)) = ComparisonOperator::parse(self.remaining()) else {
            return Ok(Some(left));
        };
        self.at += width;
        let right = formula_value!(self.sum(depth)?);
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
                operator @ (ComparisonOperator::Eq | ComparisonOperator::Ne),
                EvaluatedFormulaValue::String(left),
                EvaluatedFormulaValue::String(right),
            ) => {
                let equal = self.ctx.equal_bytes(
                    left.value().as_bytes(),
                    right.value().as_bytes(),
                    "catia_formula_string_comparison",
                )?;
                (
                    equal == matches!(operator, ComparisonOperator::Eq),
                    left.is_known() && right.is_known(),
                )
            }
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
            _ => return Ok(None),
        };
        Ok(Some(EvaluatedFormulaValue::Boolean(if known {
            EvaluatedFormulaBoolean::known(value)
        } else {
            EvaluatedFormulaBoolean::unknown()
        })))
    }

    fn sum(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let mut value = formula_value!(self.product(depth)?);
        loop {
            self.skip_whitespace();
            let Some(operator) = self.peek() else {
                return Ok(Some(value));
            };
            if !matches!(operator, b'+' | b'-') {
                return Ok(Some(value));
            }
            self.at += 1;
            let right = formula_value!(self.product(depth)?);
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
                        formatted?
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
                        return Ok(None);
                    }
                    let known = left.is_known() && right.is_known() && !right.value().is_empty();
                    let string_value = if known {
                        replaced_text(
                            self.ctx,
                            (left.value(), right.value(), ""),
                            "catia_formula_subtract_work",
                            "catia_formula_string_subtract",
                        )?
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
            let left = formula_value!(value.scalar());
            let right = formula_value!(right.scalar());
            if left.dimension() != right.dimension() {
                return Ok(None);
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
                return Ok(None);
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
                return Ok(None);
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

    fn product(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let mut value = formula_value!(self.unary(depth)?);
        loop {
            self.skip_whitespace();
            let Some(operator) = self.peek() else {
                return Ok(Some(value));
            };
            if !matches!(operator, b'*' | b'/') {
                return Ok(Some(value));
            }
            self.at += 1;
            let left = formula_value!(value.scalar());
            let right = formula_value!(formula_value!(self.unary(depth)?).scalar());
            let left_known = left.known_value();
            let right_known = right.known_value();
            if self.static_check
                && operator == b'/'
                && right_known.is_some_and(|value| value == 0.0)
            {
                return Ok(None);
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
                return Ok(None);
            }
            let result = if operator == b'*' {
                EvaluatedFormulaScalar::from_parts(
                    left.value() * right.value(),
                    formula_value!(left.dimension().product(right.dimension())),
                    if self.evaluate {
                        None
                    } else {
                        static_all_integral(left.integral(), right.integral())
                    },
                    known_value,
                )
            } else {
                if self.evaluate && right.value() == 0.0 {
                    return Ok(None);
                }
                EvaluatedFormulaScalar::from_parts(
                    if right.value() == 0.0 {
                        0.0
                    } else {
                        left.value() / right.value()
                    },
                    formula_value!(left.dimension().quotient(right.dimension())),
                    None,
                    known_value,
                )
            };
            if self.evaluate && !result.value().is_finite() {
                return Ok(None);
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

    fn unary(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        self.skip_whitespace();
        if self.consume_keyword("not") {
            let value = formula_value!(formula_value!(self.descend(depth, Self::unary)?).boolean());
            return Ok(Some(EvaluatedFormulaValue::Boolean(value.not())));
        }
        Ok(match formula_value!(self.peek()) {
            b'+' => {
                self.at += 1;
                self.descend(depth, Self::unary)?
                    .and_then(EvaluatedFormulaValue::scalar)
                    .map(EvaluatedFormulaValue::Scalar)
            }
            b'-' => {
                self.at += 1;
                let value =
                    formula_value!(formula_value!(self.descend(depth, Self::unary)?).scalar());
                Some(EvaluatedFormulaValue::Scalar(
                    EvaluatedFormulaScalar::from_parts(
                        -value.value(),
                        value.dimension(),
                        value.integral(),
                        value.known_value().map(|value| -value),
                    ),
                ))
            }
            _ => self.power(depth)?,
        })
    }

    fn power(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let base = formula_value!(self.postfix(depth)?);
        self.skip_whitespace();
        if !self.remaining().starts_with("**") {
            return Ok(Some(base));
        }
        self.at += 2;
        let base = formula_value!(base.scalar());
        let exponent = formula_value!(formula_value!(self.descend(depth, Self::unary)?).scalar());
        if exponent.dimension() != FormulaDimension::SCALAR {
            return Ok(None);
        }

        let dimension = if base.dimension() == FormulaDimension::SCALAR {
            FormulaDimension::SCALAR
        } else {
            let exponent_value = formula_value!(exponent.known_value());
            if exponent_value.fract() != 0.0
                || exponent_value < f64::from(i32::MIN)
                || exponent_value > f64::from(i32::MAX)
            {
                return Ok(None);
            }
            formula_value!(base
                .dimension()
                .power(formula_value!(truncate_f64_to_i32(exponent_value))))
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
            return Ok(None);
        }
        Ok(
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
            )),
        )
    }

    fn primary(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        self.skip_whitespace();
        if formula_value!(self.peek()) == b'"' {
            return Ok(self
                .string_literal()?
                .map(EvaluatedFormulaString::known)
                .map(EvaluatedFormulaValue::String));
        }
        if self.consume_keyword("true") {
            return Ok(Some(EvaluatedFormulaValue::Boolean(
                EvaluatedFormulaBoolean::known(true),
            )));
        }
        if self.consume_keyword("false") {
            return Ok(Some(EvaluatedFormulaValue::Boolean(
                EvaluatedFormulaBoolean::known(false),
            )));
        }
        if formula_value!(self.peek()) == b'(' {
            self.at += 1;
            let value = formula_value!(self.descend(depth, Self::conditional)?);
            self.skip_whitespace();
            formula_value!((formula_value!(self.peek()) == b')').then_some(()));
            self.at += 1;
            return Ok(Some(value));
        }
        if formula_value!(self.peek()) == b'#' {
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
            return Ok(Some(EvaluatedFormulaValue::Scalar(
                EvaluatedFormulaScalar::from_parts(
                    std::f64::consts::PI,
                    FormulaDimension::SCALAR,
                    Some(false),
                    Some(std::f64::consts::PI),
                ),
            )));
        }
        if self.remaining().starts_with('E')
            && self
                .source
                .as_bytes()
                .get(self.at + 1)
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        {
            self.at += 1;
            return Ok(finite_scalar(std::f64::consts::E).map(EvaluatedFormulaValue::Scalar));
        }
        if formula_value!(self.peek()).is_ascii_alphabetic() {
            return self.function_call(depth);
        }
        Ok(self.literal()?.map(EvaluatedFormulaValue::Scalar))
    }

    fn postfix(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let mut value = formula_value!(self.primary(depth)?);
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'.') {
                return Ok(Some(value));
            }
            self.at += 1;
            let method_start = self.at;
            while self
                .peek()
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                self.at += 1;
            }
            formula_value!((self.at > method_start).then_some(()));
            let method = &self.source[method_start..self.at];
            let mut argument_storage = self
                .ctx
                .reserve_scoped(0, "CATIA formula method arguments")?;
            let parsed =
                argument_storage.with_storage(|| self.descend(depth, Self::function_arguments));
            let arguments = formula_value!(parsed?);
            value = match (method, value, arguments.as_slice()) {
                ("Length", EvaluatedFormulaValue::String(value), []) => {
                    let characters = self
                        .ctx
                        .admit_iter(value.value(), "catia_formula_string_length")?
                        .count();
                    let length = formula_value!(u32::try_from(characters).ok());
                    EvaluatedFormulaValue::Scalar(
                        if self.evaluate || (self.static_check && value.is_known()) {
                            formula_value!(finite_scalar(f64::from(length)))
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
                        let index = formula_value!(self.search_string(
                            value.value(),
                            needle.value(),
                            0,
                            true
                        )?);
                        formula_value!(finite_scalar(formula_value!(f64_from_i64(index))))
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
                    let start = formula_value!(self.string_index(start_value));
                    let known = value.is_known()
                        && needle.is_known()
                        && start_value.known_value().is_some();
                    EvaluatedFormulaValue::Scalar(
                        if self.evaluate || (self.static_check && known) {
                            let index = formula_value!(self.search_string(
                                value.value(),
                                needle.value(),
                                start,
                                true
                            )?);
                            formula_value!(finite_scalar(formula_value!(f64_from_i64(index))))
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
                    let start = formula_value!(self.string_index(*start));
                    EvaluatedFormulaValue::Scalar(if self.evaluate {
                        let index = formula_value!(self.search_string(
                            value.value(),
                            needle.value(),
                            start,
                            forward.value(),
                        )?);
                        formula_value!(finite_scalar(formula_value!(f64_from_i64(index))))
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
                    let start = formula_value!(self.string_index(start_value));
                    let length = formula_value!(self.string_index(length_value));
                    let known = value.is_known()
                        && start_value.known_value().is_some()
                        && length_value.known_value().is_some();
                    let string_value = if self.evaluate || (self.static_check && known) {
                        let end = formula_value!(start.checked_add(length));
                        let start = formula_value!(self.string_boundary(value.value(), start)?);
                        let end = formula_value!(self.string_boundary(value.value(), end)?);
                        let copied = self.ctx.copy_retained_text(
                            &value.value()[start..end],
                            "catia_formula_string_extract",
                        );
                        copied?
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
                            self.ctx.charge_work(
                                u64_from_index(value.value().len()),
                                "catia_formula_string_real",
                            )?;
                            formula_value!(finite_scalar(formula_value!(value
                                .value()
                                .parse::<f64>()
                                .ok())))
                        } else {
                            static_unknown_result(0.0, FormulaDimension::SCALAR)
                        },
                    )
                }
                _ => return Ok(None),
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

    /// The byte offset of a character index, which may equal the character
    /// count. The search visits the characters before the index.
    fn string_boundary(
        &mut self,
        value: &str,
        index: usize,
    ) -> Result<Option<usize>, cadmpeg_core::CodecError> {
        self.ctx.find_map(
            value
                .char_indices()
                .map(|(offset, _)| offset)
                .chain(std::iter::once(value.len()))
                .enumerate(),
            |(position, offset)| Ok((position == index).then_some(offset)),
            "catia_formula_string_boundary",
        )
    }

    fn search_string(
        &mut self,
        value: &str,
        needle: &str,
        start: usize,
        forward: bool,
    ) -> Result<Option<i64>, cadmpeg_core::CodecError> {
        let work = u64_from_index(value.len())
            .checked_add(u64_from_index(needle.len()))
            .ok_or_else(|| {
                self.ctx
                    .refuse_codec_limit("catia_formula_string_search", u64::MAX, u64::MAX)
            })?;
        self.ctx.charge_work(work, "catia_formula_string_search")?;
        let byte_offset = if forward {
            let Some(start_byte) = self.string_boundary(value, start)? else {
                return Ok(Some(-1));
            };
            value[start_byte..]
                .find(needle)
                .map(|offset| start_byte + offset)
        } else {
            let character_count = self
                .ctx
                .admit_iter(value, "catia_formula_string_search_count")?
                .count();
            let Some(end_character) = character_count.checked_sub(start) else {
                return Ok(Some(-1));
            };
            let end_byte = formula_value!(self.string_boundary(value, end_character)?);
            value[..end_byte].rfind(needle)
        };
        let Some(offset) = byte_offset else {
            return Ok(Some(-1));
        };
        let index = self
            .ctx
            .admit_iter(&value[..offset], "catia_formula_string_search_index")?
            .count();
        Ok(i64::try_from(index).ok())
    }

    fn string_literal(&mut self) -> Result<Option<String>, cadmpeg_core::CodecError> {
        formula_value!((formula_value!(self.peek()) == b'"').then_some(()));
        self.at += 1;
        let start = self.at;
        while let Some(character) = formula_value!(self.source.get(self.at..)).chars().next() {
            if character == '"' {
                let source = formula_value!(self.source.get(start..self.at));
                let copied = self
                    .ctx
                    .copy_retained_text(source, "catia_formula_literal_text");
                let value = copied?;
                self.at += character.len_utf8();
                return Ok(Some(value));
            }
            if character.is_control() || character == '\\' {
                return Ok(None);
            }
            self.at += character.len_utf8();
        }
        Ok(None)
    }

    fn function_call(
        &mut self,
        depth: usize,
    ) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let function_start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
            self.at += 1;
        }
        let function = &self.source[function_start..self.at];
        let mut argument_storage = self
            .ctx
            .reserve_scoped(0, "CATIA formula argument storage")?;
        let parsed =
            argument_storage.with_storage(|| self.descend(depth, Self::function_arguments));
        let arguments = formula_value!(parsed?);

        if function == "ReplaceSubText" {
            let [EvaluatedFormulaValue::String(source), EvaluatedFormulaValue::String(from), EvaluatedFormulaValue::String(to)] =
                arguments.as_slice()
            else {
                return Ok(None);
            };
            if (self.evaluate || self.static_check) && from.is_known() && from.value().is_empty() {
                return Ok(None);
            }
            let known =
                source.is_known() && from.is_known() && to.is_known() && !from.value().is_empty();
            let value = if self.evaluate || (self.static_check && known) {
                replaced_text(
                    self.ctx,
                    (source.value(), from.value(), to.value()),
                    "catia_formula_replace_work",
                    "catia_formula_replace_subtext",
                )?
            } else {
                String::new()
            };
            return Ok(Some(EvaluatedFormulaValue::String(
                EvaluatedFormulaString::from_parts(
                    value,
                    self.evaluate || (self.static_check && known),
                ),
            )));
        }

        if function == "ToString" {
            let [EvaluatedFormulaValue::Scalar(value)] = arguments.as_slice() else {
                return Ok(None);
            };
            if (self.static_check || self.evaluate)
                && !value.satisfies_source_type(FormulaParameterType::Integer)
            {
                return Ok(None);
            }
            let known = value.known_value().is_some();
            let string_value = if self.evaluate || (self.static_check && known) {
                let formatted = self.ctx.format_retained(
                    format_args!("{:.0}", value.value()),
                    "catia_formula_to_string",
                );
                formatted?
            } else {
                String::new()
            };
            return Ok(Some(EvaluatedFormulaValue::String(
                EvaluatedFormulaString::from_parts(
                    string_value,
                    self.evaluate || (self.static_check && known),
                ),
            )));
        }

        if matches!(function, "ToUpper" | "ToLower") {
            let [EvaluatedFormulaValue::String(value)] = arguments.as_slice() else {
                return Ok(None);
            };
            let known = value.is_known();
            let string_value = if self.evaluate || (self.static_check && known) {
                // A Unicode character maps to at most three characters; formatting runs twice.
                let work = u64_from_index(value.value().len())
                    .checked_mul(12)
                    .ok_or_else(|| {
                        self.ctx
                            .refuse_codec_limit("catia_formula_case_work", u64::MAX, u64::MAX)
                    })?;
                self.ctx.charge_work(work, "catia_formula_case_work")?;
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
                formatted?
            } else {
                String::new()
            };
            return Ok(Some(EvaluatedFormulaValue::String(
                EvaluatedFormulaString::from_parts(
                    string_value,
                    self.evaluate || (self.static_check && known),
                ),
            )));
        }

        if function == "round" && arguments.len() == 3 {
            let [EvaluatedFormulaValue::Scalar(value), EvaluatedFormulaValue::String(unit), EvaluatedFormulaValue::Scalar(digits)] =
                arguments.as_slice()
            else {
                return Ok(None);
            };
            if !matches!(
                value.dimension(),
                FormulaDimension::LENGTH | FormulaDimension::ANGLE
            ) || digits.dimension() != FormulaDimension::SCALAR
            {
                return Ok(None);
            }
            let unit_spec = formula_unit(unit.value());
            if let Some((unit_dimension, _)) = unit_spec {
                if value.dimension() != unit_dimension {
                    return Ok(None);
                }
            } else if self.evaluate || (self.static_check && unit.is_known()) {
                return Ok(None);
            }
            if (self.static_check || self.evaluate)
                && !digits.satisfies_source_type(FormulaParameterType::Integer)
            {
                return Ok(None);
            }
            if self.static_check
                && digits
                    .known_value()
                    .is_some_and(|value| value < 0.0 || value > f64::from(i32::MAX))
            {
                return Ok(None);
            }
            if !self.evaluate {
                return Ok(Some(EvaluatedFormulaValue::Scalar(
                    EvaluatedFormulaScalar::from_parts(0.0, value.dimension(), None, None),
                )));
            }
            let (_, unit_scale) = formula_value!(unit_spec);
            if digits.value() < 0.0 || digits.value() > f64::from(i32::MAX) {
                return Ok(None);
            }
            let quantum =
                unit_scale * 10.0_f64.powi(-(formula_value!(truncate_f64_to_i32(digits.value()))));
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
            return Ok(rounded.is_finite().then_some(EvaluatedFormulaValue::Scalar(
                EvaluatedFormulaScalar::from_parts(
                    rounded,
                    value.dimension(),
                    finite_integrality(rounded),
                    Some(rounded),
                ),
            )));
        }

        let mut scalar_arguments = Vec::new();
        for argument in self
            .ctx
            .admit_iter(&arguments, "catia_formula_scalar_argument_visits")?
        {
            let EvaluatedFormulaValue::Scalar(scalar) = argument else {
                return Ok(None);
            };
            let scalar = *scalar;
            let pushed = argument_storage.with_storage(|| {
                self.ctx.push_vec(
                    &mut scalar_arguments,
                    scalar,
                    "catia_formula_scalar_arguments",
                )
            });
            pushed?;
        }
        let arguments = scalar_arguments;

        if matches!(function, "min" | "max") {
            let mut arguments = self
                .ctx
                .admit_iter(&arguments, "catia_formula_extremum_argument_visits")?;
            let mut result = *formula_value!(arguments.next());
            for argument in arguments {
                if result.dimension() != argument.dimension() {
                    return Ok(None);
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
            return Ok(Some(EvaluatedFormulaValue::Scalar(result)));
        }

        if matches!(function, "LinearInterpolation" | "CubicInterpolation") {
            let [start, end, fraction] = arguments.as_slice() else {
                return Ok(None);
            };
            if start.dimension() != end.dimension()
                || fraction.dimension() != FormulaDimension::SCALAR
            {
                return Ok(None);
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
            let value = formula_value!(interpolate(start.value(), end.value(), fraction_value));
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
                return Ok(None);
            }
            return Ok((value.is_finite() || !self.evaluate).then_some(
                EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar::from_parts(
                    if value.is_finite() { value } else { 0.0 },
                    start.dimension(),
                    if self.evaluate {
                        finite_integrality(value)
                    } else {
                        None
                    },
                    known_value,
                )),
            ));
        }

        let (first, second) = match arguments.as_slice() {
            [first] => (*first, None),
            [first, second] => (*first, Some(*second)),
            _ => return Ok(None),
        };
        let value = formula_value!(match (function, first, second) {
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
                    formula_value!(argument.dimension().square_root()),
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
        });
        Ok(Some(EvaluatedFormulaValue::Scalar(value)))
    }

    fn function_arguments(
        &mut self,
        depth: usize,
    ) -> Result<Option<Vec<EvaluatedFormulaValue>>, cadmpeg_core::CodecError> {
        self.skip_whitespace();
        formula_value!((formula_value!(self.peek()) == b'(').then_some(()));
        self.at += 1;
        let mut arguments = Vec::new();
        self.skip_whitespace();
        if formula_value!(self.peek()) == b')' {
            self.at += 1;
            return Ok(Some(arguments));
        }
        loop {
            let argument = formula_value!(self.conditional(depth)?);
            let pushed = self
                .ctx
                .push_vec(&mut arguments, argument, "catia_formula_arguments");
            pushed?;
            self.skip_whitespace();
            if formula_value!(self.peek()) == b')' {
                self.at += 1;
                break;
            }
            formula_value!((formula_value!(self.peek()) == b','
                && arguments.len() < MAX_FORMULA_FUNCTION_ARGUMENTS)
                .then_some(()));
            self.at += 1;
        }
        Ok(Some(arguments))
    }

    fn symbol(&mut self) -> Result<Option<EvaluatedFormulaValue>, cadmpeg_core::CodecError> {
        let start = self.at;
        self.at += 1;
        let digits = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        formula_value!((self.at > digits && formula_value!(self.peek()) == b'_').then_some(()));
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
            formula_value!((self.at > ordinal).then_some(()));
        } else {
            self.at = name_end;
        }
        let value = formula_value!(self.bindings.get(&self.source[start..name_end]));
        if self.evaluate
            && matches!(value, EvaluatedFormulaValue::Scalar(scalar) if scalar.known_value().is_none())
        {
            return Ok(None);
        }
        let copied = value.copy_charged(self.ctx);
        Ok(Some(copied?))
    }

    fn literal(&mut self) -> Result<Option<EvaluatedFormulaScalar>, cadmpeg_core::CodecError> {
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
            formula_value!((self.at > exponent).then_some(()));
        }
        formula_value!(saw_digit.then_some(()));
        let mut value = formula_value!(self.source[start..self.at].parse::<f64>().ok());
        let unit_boundary = self.at;
        self.skip_whitespace();
        let Some(unit) = [
            "micron", "mile", "yard", "grad", "rad", "deg", "mm", "cm", "km", "ft", "in", "m",
        ]
        .into_iter()
        .find(|unit| self.remaining().starts_with(unit)) else {
            self.at = unit_boundary;
            return Ok(value
                .is_finite()
                .then_some(EvaluatedFormulaScalar::from_parts(
                    value,
                    FormulaDimension::SCALAR,
                    finite_integrality(value),
                    Some(value),
                )));
        };
        let (dimension, scale) = formula_value!(formula_unit(unit));
        self.at += unit.len();
        value *= scale;
        Ok(value
            .is_finite()
            .then_some(EvaluatedFormulaScalar::from_parts(
                value,
                dimension,
                finite_integrality(value),
                Some(value),
            )))
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
        self.source.as_bytes().get(self.at).copied()
    }

    fn remaining(&self) -> &str {
        &self.source[self.at..]
    }

    fn descend<T>(
        &mut self,
        depth: usize,
        parse: impl FnOnce(&mut Self, usize) -> Result<Option<T>, cadmpeg_core::CodecError>,
    ) -> Result<Option<T>, cadmpeg_core::CodecError> {
        if depth >= MAX_FORMULA_EXPRESSION_DEPTH {
            return Err(self.ctx.refuse_codec_limit(
                "catia_formula_expression_local_depth",
                u64_from_index(MAX_FORMULA_EXPRESSION_DEPTH),
                u64_from_index(MAX_FORMULA_EXPRESSION_DEPTH) + 1,
            ));
        }
        let ctx = self.ctx;
        let _depth = ctx.enter_nested("catia_formula_expression_depth")?;
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
        crate::test_support::with_service_context(|ctx| {
            assert!(formula_parameter_candidates_agree(
                ctx,
                &unset_candidate(FormulaParameterType::Real),
                &unset_candidate(FormulaParameterType::Real)
            )
            .expect("service comparison budget"));
            assert!(!formula_parameter_candidates_agree(
                ctx,
                &unset_candidate(FormulaParameterType::Length),
                &unset_candidate(FormulaParameterType::Real)
            )
            .expect("service comparison budget"));
        });
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
                &mut ctx.reserve_scoped(0, "test scratch")?,
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
                &mut ctx.reserve_scoped(0, "test scratch")?,
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
