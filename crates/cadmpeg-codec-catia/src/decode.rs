// SPDX-License-Identifier: Apache-2.0
//! High-level CATPart-to-IR decoding.
//!
//! [`decode`] scans the container, selects a decoder from the identified storage
//! variant, and returns the transferred model with its [`DecodeBody`]; the
//! sealed wrapper stamps the identity authored in `ir.source` onto the report.
//!
//! Partial paths preserve the reconstructed B-rep stream or complete file as an
//! [`UnknownRecord`]. Their report identifies unresolved model layers.

use cadmpeg_core::decode::u64_from_index;

use std::collections::HashSet;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::codec::Decoded;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, SourceFidelity};

use crate::assemble::{build_container_report, build_metadata_fallback};
use crate::container::{self, ContainerScan};
use crate::design_feature;
use crate::entity_table;
use crate::families;
use crate::formula;
use crate::loss::CatiaLossCode;
use crate::native::{CatiaNative, CatiaObjectGraph};
use crate::pmi;
use crate::resource;
use crate::sketch;

/// Decodes a `.CATPart` reader into an IR document and decode report.
///
/// When [`DecodeOptions::container_only`] is set, the result contains source
/// metadata and container diagnostics without entity decoding.
///
/// Otherwise each route in [`crate::families::ROUTES`] whose applicability
/// predicate accepts the scanned variant is tried in table order; the first to
/// return a model wins, a `None` falls through to the next applicable route, and
/// exhausting the table yields the metadata-only fallback.
pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    // The sink outlives every route exit: a route that answers `None` after a
    // refusal has already stated the refusal here, and `finish_decode` drains
    // the sink into the report before its first fallible step and again after
    // the native decode, so no `?` sits between a push and its drain. The
    // fall-through to the next route and to the metadata fallback is stated in
    // the report by name.
    let mut refusal = crate::nurbs::LaneRefusals::new();
    decode_over_routes(ctx, root, families::ROUTES, &mut refusal)
}

/// Decodes `root` over `routes`, the ordered fall-back table.
///
/// `routes` is a parameter so a test can drive the router with a table whose
/// behaviour it states: the production call passes [`families::ROUTES`].
fn decode_over_routes(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    routes: &[families::Route],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Decoded, CodecError> {
    let scan = container::scan_bytes(ctx, root.window())?;
    let matched = crate::dialect::classify(ctx, &scan)?;

    if ctx.container_only() {
        let (ir, annotations, unknowns) = build_metadata_fallback(ctx, &scan)?;
        let report = build_container_report(ctx, &scan)?;
        return decode_result(ctx, &scan, &matched, ir, report, annotations, unknowns);
    }

    let applicable = ctx.collect_vec(
        routes
            .iter()
            .filter(|route| (route.applicable)(scan.variant)),
        "catia_applicable_routes",
    )?;
    let mut fell_through = Vec::new();
    for (index, route) in applicable.iter().enumerate() {
        let stated = refusal.note_count();
        let output = (route.decode)(ctx, &scan, refusal)?;
        if let Some(out) = output {
            return finish_decode(
                ctx,
                crate::decode::FinishDecodeInputs {
                    scan: &scan,
                    matched: &matched,
                    ir: out.ir,
                    report: out.report,
                    annotations: out.annotations,
                    unknowns: out.unknowns,
                    admitted_model_entities: out.admitted_model_entities,
                    standard_face_population: route.standard_face_population,
                    fell_through: &fell_through,
                    refusal,
                },
            );
        }
        let refused = refusal.note_count() - stated;
        if refused > 0 {
            let next = applicable
                .get(index + 1)
                .map_or("the metadata fallback", |next| next.name);
            let note = ctx.format_retained(
                format_args!(
                    "{} refused {refused} CATIA record(s) and then transferred no model; \
                     the decode continued to {next}",
                    route.name
                ),
                "catia_route_fallthrough_note",
            )?;
            ctx.push_vec(&mut fell_through, note, "catia_route_fallthroughs")?;
        }
    }

    let (ir, annotations, unknowns) = build_metadata_fallback(ctx, &scan)?;
    let report = build_container_report(ctx, &scan)?;
    finish_decode(
        ctx,
        crate::decode::FinishDecodeInputs {
            scan: &scan,
            matched: &matched,
            ir,
            report,
            annotations,
            unknowns,
            admitted_model_entities: 0,
            standard_face_population: false,
            fell_through: &fell_through,
            refusal,
        },
    )
}

#[derive(Default)]
struct IncomingEntityIncidenceCounts {
    payload: usize,
    storage: usize,
    classified: usize,
    zero: usize,
    one: usize,
    multiple: usize,
}

impl IncomingEntityIncidenceCounts {
    fn total(&self) -> usize {
        self.payload + self.storage
    }

    fn add(
        &mut self,
        ctx: &DecodeContext<'_>,
        payload_references: &[crate::native::CatiaEntityIncomingReference],
        storage_references: &[crate::native::CatiaEntityIncomingStorageReference],
    ) -> Result<(), CodecError> {
        let payload_count = payload_references.len();
        let storage_count = storage_references.len();
        let total = payload_count + storage_count;
        self.payload += payload_count;
        self.storage += storage_count;
        self.classified += ctx
            .admit_iter(payload_references, "catia_census_incoming_payload")?
            .filter_map(|reference| reference.source_entity.as_ref())
            .chain(
                ctx.admit_iter(storage_references, "catia_census_incoming_storage")?
                    .filter_map(|reference| reference.source_entity.as_ref()),
            )
            .filter(|entity| entity.class_name().is_some())
            .count();
        self.zero += usize::from(total == 0);
        self.one += usize::from(total == 1);
        self.multiple += usize::from(total > 1);
        Ok(())
    }
}

// Keep the single classified match explicit beside the independently built decode artifacts.
struct FinishDecodeInputs<'input0, 'input1, 'input2, 'input3> {
    scan: &'input0 ContainerScan<'input0>,
    matched: &'input1 DialectMatch,
    ir: CadIr,
    report: DecodeBody,
    annotations: Annotations,
    unknowns: Vec<UnknownRecord>,
    admitted_model_entities: u64,
    standard_face_population: bool,
    fell_through: &'input2 [String],
    refusal: &'input3 mut crate::nurbs::LaneRefusals,
}

fn finish_decode(
    ctx: &DecodeContext<'_>,
    inputs: FinishDecodeInputs<'_, '_, '_, '_>,
) -> Result<Decoded, CodecError> {
    let FinishDecodeInputs {
        scan,
        matched,
        mut ir,
        mut report,
        mut annotations,
        unknowns,
        mut admitted_model_entities,
        standard_face_population,
        fell_through,
        refusal,
    } = inputs;

    // Drain before the first fallible step: every refusal a route stated is in
    // the `report` value. The `Ok` route returns that value, so the notes reach
    // the caller. The `Err` route below drops the value and returns the bare
    // `CodecError`: a refusal note is subordinate to a hard failure by design.
    // The fall-through statements say which route refused and where the decode
    // went next.
    for statement in fell_through {
        let message = ctx.copy_retained_text(statement, "catia_route_fallthrough_loss")?;
        ctx.push_vec(
            &mut report.losses,
            CatiaLossCode::SourceRouteFellThrough.note_charged(
                ctx,
                message,
                "catia_route_fallthrough_loss",
            )?,
            "catia_route_fallthrough_loss",
        )?;
    }
    for note in ctx.admit_iter(refusal.take_notes(), "catia_lane_refusal_notes")? {
        ctx.push_vec(&mut report.losses, note, "catia_lane_refusal_loss")?;
    }
    ctx.admit_entities(
        u64_from_index(ir.model.entity_count()),
        &mut admitted_model_entities,
        "admit CATIA route entities",
    )?;
    let consolidated_record_sources = container::consolidated_record_sources(ctx, scan)?;
    let native = CatiaNative::decode_with_record_sources(
        ctx,
        &scan.data,
        &consolidated_record_sources,
        refusal,
    )?;
    // Drain lane refusals from a successful native decode before transfers run.
    for note in ctx.admit_iter(refusal.take_notes(), "catia_lane_refusal_notes")? {
        ctx.push_vec(&mut report.losses, note, "catia_lane_refusal_loss")?;
    }
    let modeling_graph_scope = modeling_graph_scope(
        ctx,
        !scan.outer_container_declarations.is_empty(),
        &native.object_graphs,
    )?;
    // Distinct modeling-scope record ids, in record order.
    let mut modeling_object_records = Vec::new();
    {
        let mut seen = HashSet::new();
        let mut seen_storage = ctx.reserve_scoped(0, "catia_modeling_object_records")?;
        for graph in ctx.admit_iter(&native.object_graphs, "catia_modeling_object_graphs")? {
            if !match &modeling_graph_scope {
                ModelingGraphScope::Unscoped => true,
                ModelingGraphScope::Unresolved => false,
                ModelingGraphScope::Scoped(part) => ctx.equal_bytes(
                    part.as_bytes(),
                    graph.id.as_bytes(),
                    "catia_census_graph_scope",
                )?,
            } {
                continue;
            }
            for record in ctx.admit_iter(&graph.records, "catia_modeling_object_records")? {
                if seen_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut seen,
                        record.id.as_str(),
                        "catia_modeling_object_records",
                    )
                })? {
                    ctx.push_vec(
                        &mut modeling_object_records,
                        record.id.as_str(),
                        "catia_modeling_object_records",
                    )?;
                }
            }
        }
    }
    let design_feature_transfer =
        design_feature::transfer_design_features(ctx, &mut ir, &native, &modeling_graph_scope)?;
    let transferred_native_sketch_entity_records = sketch::transfer_native_sketch_entities(
        ctx,
        &mut ir,
        &native,
        &design_feature_transfer,
        &modeling_graph_scope,
    )?;
    let transferred_native_sketch_constraint_records = sketch::transfer_native_sketch_constraints(
        ctx,
        &mut ir,
        &native,
        &design_feature_transfer,
        &modeling_graph_scope,
    )?;
    let transferred_constraint_range_records = sketch::transfer_constraint_ranges(
        ctx,
        &mut ir,
        &native,
        &design_feature_transfer,
        &modeling_graph_scope,
    )?;
    let transferred_pmi_dimension_count = pmi::transfer_dimensions(
        ctx,
        &mut ir,
        &native,
        &modeling_graph_scope,
        &transferred_constraint_range_records,
    )?;
    let formula_transfer = formula::transfer_parameters(
        ctx,
        &mut ir,
        &native,
        &mut annotations,
        &modeling_graph_scope,
    )?;
    design_feature_transfer.assign_parameter_owners(ctx, &mut ir, &native)?;
    let appearance_transfer = crate::appearance::transfer(
        ctx,
        &mut ir,
        &native,
        &modeling_graph_scope,
        standard_face_population
            .then_some(scan.main_data_stream.as_deref().or(scan.brep.as_deref()))
            .flatten(),
    )?;

    let mut owned_definition_value_ids = HashSet::new();
    let mut object_records_by_id = std::collections::HashMap::new();
    let mut structurally_owned_records = HashSet::new();
    let mut structurally_owned_definition_chain_value_ids = HashSet::new();
    let mut schema_configuration_entities = HashSet::new();
    let mut formula_referenced_relation_expressions = HashSet::new();
    let mut program_referenced_relation_expressions = HashSet::new();
    let mut typed_relation_expression_entities = HashSet::new();
    let mut distinct_relation_program_input_entities = HashSet::new();
    // Gather cross-entity identities before the census resolves references against them.

    let mut relation_expression_count = 0usize;
    let mut placeholder_state_relation_expression_count = 0usize;
    let mut parser_version_relation_expression_count = 0usize;
    let mut boolean_parser_version_relation_expression_count = 0usize;
    let mut opened_boolean_parser_version_relation_expression_count = 0usize;
    let mut typed_relation_expression_count = 0usize;
    for record in ctx.admit_iter(&native.entity_records, "catia_census_entity_ids")? {
        if record.schema_configuration_record().is_some() {
            ctx.insert_hash_set(
                &mut schema_configuration_entities,
                record.id.as_str(),
                "catia_schema_configuration_entities",
            )?;
        }
        if let Some(formula) = record.formula_relation() {
            if let Some(entity) = formula.expression_entity.reference.entity() {
                ctx.insert_hash_set(
                    &mut formula_referenced_relation_expressions,
                    entity,
                    "catia_formula_relation_expressions",
                )?;
            }
        }
        if let Some(instance) = record.relation_program_instance() {
            if let Some(entity) = instance.relation_expression.as_deref() {
                ctx.insert_hash_set(
                    &mut program_referenced_relation_expressions,
                    entity,
                    "catia_program_relation_expressions",
                )?;
            }
            if let Some(inputs) = &instance.inputs {
                for input in ctx.admit_iter(inputs, "catia_distinct_program_inputs")? {
                    if let Some(entity) = input.entity.entity() {
                        ctx.insert_hash_set(
                            &mut distinct_relation_program_input_entities,
                            entity,
                            "catia_distinct_program_inputs",
                        )?;
                    }
                }
            }
        }
        if let Some(expression) = record.relation_expression() {
            relation_expression_count += 1;
            match expression.framing {
                crate::native::CatiaRelationExpressionFraming::PlaceholderState { .. } => {
                    placeholder_state_relation_expression_count += 1;
                }
                crate::native::CatiaRelationExpressionFraming::ParserVersion { .. } => {
                    parser_version_relation_expression_count += 1;
                }
                crate::native::CatiaRelationExpressionFraming::BooleanParserVersion { .. } => {
                    boolean_parser_version_relation_expression_count += 1;
                }
                crate::native::CatiaRelationExpressionFraming::OpenedBooleanParserVersion {
                    ..
                } => opened_boolean_parser_version_relation_expression_count += 1,
            }
            if expression.signature_charged(ctx)?.is_some() {
                typed_relation_expression_count += 1;
                ctx.insert_hash_set(
                    &mut typed_relation_expression_entities,
                    record.id.as_str(),
                    "catia_typed_relation_expressions",
                )?;
            }
        }
    }
    let distinct_relation_program_input_entity_count =
        distinct_relation_program_input_entities.len();
    let instanced_relation_expression_count = program_referenced_relation_expressions.len();
    let mut native_operation_feature_ids = HashSet::new();
    for feature in
        ctx.admit_iter(&ir.model.features, "catia_census_features")?
            .filter(|feature| {
                feature.source_tag.as_deref().is_some_and(|name| {
                    design_feature::NativeOperationClass::try_from(name).is_ok()
                })
            })
    {
        if !ctx.contains_hash_set(
            &native_operation_feature_ids,
            &feature.id,
            "catia_census_lookup",
        )? {
            let id = feature
                .id
                .try_clone_for_decode(ctx, "catia_native_operation_feature_id")?;
            ctx.insert_hash_set(
                &mut native_operation_feature_ids,
                id,
                "catia_native_operation_feature_ids",
            )?;
        }
    }

    let complete_schema_configuration_row_chain_count =
        native.schema_configuration_row_chains.len();
    let mut unassigned_owner_slot_count = 0usize;
    let mut retained_unscoped_object_graph_count = 0usize;
    let mut retained_unscoped_object_record_count = 0usize;
    let mut object_record_count = 0usize;
    let mut resolved_storage_record_count = 0usize;
    let mut unresolved_storage_record_count = 0usize;
    let mut object_record_reference_count = 0usize;
    let mut resolved_object_record_reference_count = 0usize;
    let mut null_object_record_reference_count = 0usize;
    let mut repeated_reference_suffix_count = 0usize;
    let mut repeated_reference_schema_selection_count = 0usize;
    for graph in ctx.admit_iter(&native.object_graphs, "catia_census_object_graphs")? {
        let in_scope = match &modeling_graph_scope {
            ModelingGraphScope::Unscoped => true,
            ModelingGraphScope::Unresolved => false,
            ModelingGraphScope::Scoped(part) => ctx.equal_bytes(
                part.as_bytes(),
                graph.id.as_bytes(),
                "catia_census_graph_scope",
            )?,
        };
        if !in_scope {
            retained_unscoped_object_graph_count += 1;
            retained_unscoped_object_record_count += graph.records.len();
        }

        object_record_count += graph.records.len();
        for record in ctx.admit_iter(&graph.records, "catia_census_child")? {
            let previous = ctx.insert_hash_map(
                &mut object_records_by_id,
                record.id.as_str(),
                record,
                "catia_object_records_by_id",
            )?;
            // Only the final record for each id contributes to the owner count.
            if let Some(previous) = previous {
                unassigned_owner_slot_count -= usize::from(previous.has_unassigned_owner());
            }
            unassigned_owner_slot_count += usize::from(record.has_unassigned_owner());

            if record.storage_record().is_some() {
                resolved_storage_record_count += 1;
            }
            if record
                .storage_ref()
                .is_some_and(|storage_ref| storage_ref != 0)
                && record.storage_record().is_none()
            {
                unresolved_storage_record_count += 1;
            }
            object_record_reference_count += record.references.len();
            for reference in ctx.admit_iter(&record.references, "catia_census_child")? {
                if reference.target().is_some() {
                    resolved_object_record_reference_count += 1;
                }
                if reference.is_null() {
                    null_object_record_reference_count += 1;
                }
            }
            if crate::object_graph::has_repeated_reference_suffix(&record.payload) {
                repeated_reference_suffix_count += 1;
            }
            if record.repeated_reference_schema_selection.is_some() {
                repeated_reference_schema_selection_count += 1;
            }
        }
    }
    let mut transferred_feature_parent_count = 0usize;
    let mut counted_feature_objects = HashSet::new();
    let mut design_field_count = 0usize;
    let mut classified_design_object_count = 0usize;
    let mut design_object_relation_count = 0usize;
    let mut design_parallel_reference_table_count = 0usize;
    let mut design_parallel_reference_row_count = 0usize;
    let mut design_parallel_reference_column_count = 0usize;
    let mut design_parallel_reference_cell_count = 0usize;
    let mut design_parallel_reference_resolved_cell_count = 0usize;
    let mut design_parallel_reference_null_cell_count = 0usize;
    let mut design_parallel_reference_classified_cell_count = 0usize;
    let mut design_parallel_reference_classified_column_count = 0usize;
    let mut design_parallel_reference_matched_row_count = 0usize;
    let mut design_unowned_field_relation_count = 0usize;
    let mut design_same_object_relation_count = 0usize;
    let mut design_reflexive_field_relation_count = 0usize;
    let mut design_object_owner_link_count = 0usize;
    let mut owned_definition_value_count = 0usize;
    let mut unresolved_design_owner_count = 0usize;
    let mut structurally_owned_definition_chain_value_count = 0usize;
    for object in ctx.admit_iter(&native.design_objects, "catia_census_design_objects")? {
        if let Some(feature) = ctx.get_hash_map(
            &design_feature_transfer.feature_ids,
            object.id.as_str(),
            "catia_census_feature_parent",
        )? {
            // Transfer keys are design-object ids; each key contributes once.
            if ctx.insert_hash_set(
                &mut counted_feature_objects,
                object.id.as_str(),
                "catia_census_feature_parent",
            )? {
                transferred_feature_parent_count +=
                    usize::from(ir.model.feature_regeneration_parent(feature).is_some());
            }
        }

        for id in ctx.admit_iter(
            &object.definition_values,
            "catia_owned_definition_value_ids",
        )? {
            ctx.insert_hash_set(
                &mut owned_definition_value_ids,
                id.as_str(),
                "catia_owned_definition_value_ids",
            )?;
        }
        if object.owner_record.is_some() {
            for id in ctx.admit_iter(&object.fields, "catia_structurally_owned_records")? {
                ctx.insert_hash_set(
                    &mut structurally_owned_records,
                    id.as_str(),
                    "catia_structurally_owned_records",
                )?;
            }
            for id in ctx.admit_iter(
                &object.definition_chain_values,
                "catia_owned_definition_chain_values",
            )? {
                ctx.insert_hash_set(
                    &mut structurally_owned_definition_chain_value_ids,
                    id.as_str(),
                    "catia_owned_definition_chain_values",
                )?;
            }
        }

        design_field_count += object.fields.len();
        if object.owner_class.is_some() || !object.field_classes.is_empty() {
            classified_design_object_count += 1;
        }
        design_object_relation_count += object.relations.len();
        if object.parallel_reference_table.is_some() {
            design_parallel_reference_table_count += 1;
        }
        if let Some(table) = object.parallel_reference_table.as_ref() {
            design_parallel_reference_row_count += table.rows().len();
            design_parallel_reference_column_count += table.columns().len();
            for row in ctx.admit_iter(table.rows(), "catia_census_child")? {
                design_parallel_reference_cell_count += row.cells.len();
                for cell in ctx.admit_iter(&row.cells, "catia_census_child")? {
                    if cell.field().is_some() {
                        design_parallel_reference_resolved_cell_count += 1;
                    }
                    if cell.is_null() {
                        design_parallel_reference_null_cell_count += 1;
                    }
                    if cell.field_class().is_some() {
                        design_parallel_reference_classified_cell_count += 1;
                    }
                }
                if row.matching_design_object.is_some() {
                    design_parallel_reference_matched_row_count += 1;
                }
            }
            for column in ctx.admit_iter(table.columns(), "catia_census_child")? {
                if column.field_class.is_some() {
                    design_parallel_reference_classified_column_count += 1;
                }
            }
        }
        for relation in ctx.admit_iter(&object.relations, "catia_census_child")? {
            if let Some(target) = relation.target_design_object.as_deref() {
                design_same_object_relation_count += usize::from(ctx.equal_bytes(
                    target.as_bytes(),
                    object.id.as_bytes(),
                    "catia_census_relation_object",
                )?);
            }
            if relation.target_design_object.is_none() {
                design_unowned_field_relation_count += 1;
            }
            if ctx.equal_bytes(
                relation.source_field.as_bytes(),
                relation.target_field.as_bytes(),
                "catia_census_relation_field",
            )? {
                design_reflexive_field_relation_count += 1;
            }
        }
        if object.owner_design_object.is_some() {
            design_object_owner_link_count += 1;
        }
        owned_definition_value_count += object.definition_values.len();
        if object.owner_record.is_none() {
            unresolved_design_owner_count += 1;
        }
        if object.owner_record.is_some() {
            structurally_owned_definition_chain_value_count += object.definition_chain_values.len();
        }
    }
    let mut legacy_identity_lead_81_count = 0usize;
    let mut legacy_identity_lead_82_count = 0usize;
    let mut legacy_identity_lead_e5_count = 0usize;
    let mut legacy_identity_lead_fd_count = 0usize;
    let mut legacy_entity_identity_count = 0usize;
    let mut legacy_schema_program_count = 0usize;
    let mut legacy_vendor_footer_schema_program_count = 0usize;
    let mut legacy_directory_bound_schema_program_count = 0usize;
    let mut legacy_schema_identifier_count = 0usize;
    let mut legacy_evaluated_value_name_count = 0usize;
    let mut legacy_text_field_count = 0usize;
    let mut legacy_e3_role_tail_text_field_count = 0usize;
    let mut legacy_role_selector_count = 0usize;
    let mut legacy_selected_role_count = 0usize;
    let mut legacy_role_field_binding_count = 0usize;
    let mut legacy_role_text_field_count = 0usize;
    let mut legacy_schema_field_count = 0usize;
    let mut legacy_relation_count = 0usize;
    let mut legacy_parameter_relation_count = 0usize;
    let mut legacy_synchronous_state_count = 0usize;
    let mut legacy_synchronous_relation_count = 0usize;
    let mut legacy_type_descriptor_count = 0usize;
    let mut legacy_literal_type_descriptor_count = 0usize;
    let mut legacy_scalar_value_count = 0usize;
    let mut legacy_named_scalar_value_count = 0usize;
    let mut legacy_string_value_count = 0usize;
    let mut legacy_named_string_value_count = 0usize;
    let mut legacy_integer_value_count = 0usize;
    let mut legacy_named_integer_value_count = 0usize;
    for run in ctx.admit_iter(
        &native.legacy_entity_runs,
        "catia_census_legacy_entity_runs",
    )? {
        for identity in ctx.admit_iter(&run.identities, "catia_census_identities")? {
            match identity.lead {
                crate::legacy_entity::CatiaLegacyIdentityLead::Lead81 => {
                    legacy_identity_lead_81_count += 1;
                }
                crate::legacy_entity::CatiaLegacyIdentityLead::Lead82 => {
                    legacy_identity_lead_82_count += 1;
                }
                crate::legacy_entity::CatiaLegacyIdentityLead::LeadE5 => {
                    legacy_identity_lead_e5_count += 1;
                }
                crate::legacy_entity::CatiaLegacyIdentityLead::LeadFd => {
                    legacy_identity_lead_fd_count += 1;
                }
            }
        }
        legacy_entity_identity_count += run.identities.len();
        if run.schema_program.is_some() {
            legacy_schema_program_count += 1;
        }
        if let Some(program) = run.schema_program.as_ref() {
            if program.boundary == crate::native::CatiaLegacySchemaProgramBoundary::VendorFooter {
                legacy_vendor_footer_schema_program_count += 1;
            }
            if program.boundary == crate::native::CatiaLegacySchemaProgramBoundary::StreamDirectory
            {
                legacy_directory_bound_schema_program_count += 1;
            }
            legacy_schema_identifier_count += program.identifiers.len();
        }
        legacy_text_field_count += run.text_fields.len();
        for field in ctx.admit_iter(&run.text_fields, "catia_census_child")? {
            if field.encoding == crate::native::CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
            {
                legacy_e3_role_tail_text_field_count += 1;
            }
            if field.role.is_some() {
                legacy_role_text_field_count += 1;
            }
        }
        legacy_role_selector_count += run.role_selectors.len();
        for role in ctx.admit_iter(&run.role_selectors, "catia_census_child")? {
            if matches!(
                &role.name,
                crate::legacy_entity::LegacyRoleName::Selector(_)
            ) {
                legacy_selected_role_count += 1;
            }
            if role.field_code.is_some() {
                legacy_role_field_binding_count += 1;
            }
        }
        legacy_schema_field_count += run.schema_fields.len();
        legacy_relation_count += run.relations.len();
        for relation in ctx.admit_iter(&run.relations, "catia_census_child")? {
            if relation.parameter_entity_id.is_some() {
                legacy_parameter_relation_count += 1;
            }
        }
        legacy_synchronous_state_count += run.synchronous_states.len();
        for state in ctx.admit_iter(&run.synchronous_states, "catia_census_child")? {
            if state.synchronous {
                legacy_synchronous_relation_count += 1;
            }
        }
        legacy_type_descriptor_count += run.type_descriptors.len();
        for descriptor in ctx.admit_iter(&run.type_descriptors, "catia_census_child")? {
            if matches!(
                &descriptor.value,
                crate::native::CatiaLegacyTypeValue::Name { .. }
            ) {
                legacy_literal_type_descriptor_count += 1;
            }
        }
        legacy_scalar_value_count += run.scalar_values.len();
        for value in ctx.admit_iter(&run.scalar_values, "catia_census_child")? {
            if value.name.is_some() {
                legacy_named_scalar_value_count += 1;
                legacy_evaluated_value_name_count += usize::from(ctx.any_by(
                    &run.role_selectors,
                    |role| {
                        Ok(role.entity_id == value.entity_id
                            && role.field_code == Some(0x17c4)
                            && role.end_offset().and_then(|offset| offset.checked_add(6))
                                == Some(value.byte_offset))
                    },
                    "catia_census_evaluation_name",
                )?);
            }
        }
        legacy_string_value_count += run.string_values.len();
        for value in ctx.admit_iter(&run.string_values, "catia_census_child")? {
            if value.name.is_some() {
                legacy_named_string_value_count += 1;
                legacy_evaluated_value_name_count += usize::from(ctx.any_by(
                    &run.role_selectors,
                    |role| {
                        Ok(role.entity_id == value.entity_id
                            && role.field_code == Some(0x17c4)
                            && role.end_offset().and_then(|offset| offset.checked_add(6))
                                == Some(value.byte_offset))
                    },
                    "catia_census_evaluation_name",
                )?);
            }
        }
        legacy_integer_value_count += run.integer_values.len();
        for value in ctx.admit_iter(&run.integer_values, "catia_census_child")? {
            if value.name.is_some() {
                legacy_named_integer_value_count += 1;
                legacy_evaluated_value_name_count += usize::from(ctx.any_by(
                    &run.role_selectors,
                    |role| {
                        Ok(role.entity_id == value.entity_id
                            && role.field_code == Some(0x17c4)
                            && role.end_offset().and_then(|offset| offset.checked_add(6))
                                == Some(value.byte_offset))
                    },
                    "catia_census_evaluation_name",
                )?);
            }
        }
    }
    let mut entity_value_field_count = 0usize;

    let mut compact_entity_value_packet_count = 0;
    let mut numeric_entity_value_packet_count = 0;
    let mut layout_entity_value_packet_count = 0;
    let mut e9_scalar_entity_value_packet_count = 0;
    let mut referenced_relation_expression_count = 0usize;
    let mut reference_signature_instruction_count = 0usize;
    let mut reference_signature_token_count = 0usize;
    let mut resolved_reference_signature_entity_count = 0usize;
    let mut null_reference_signature_entity_count = 0usize;
    let mut unresolved_reference_signature_entity_count = 0usize;
    let mut classified_reference_signature_entity_count = 0usize;
    let mut range_interval_count = 0usize;
    let mut range_interval_no_slot_count = 0usize;
    let mut range_interval_nominal_count = 0usize;
    let mut range_interval_finite_slot_count = 0usize;
    let mut range_interval_unset_slot_count = 0usize;
    let mut constraint_range_count = 0usize;
    let mut dimension_constraint_range_count = 0usize;
    let mut complex_constraint_range_count = 0usize;
    let mut evaluated_constraint_range_count = 0usize;
    let mut unset_constraint_range_count = 0usize;
    let mut definition_chain_value_count = 0usize;
    let mut definition_chain_evaluation_count = 0usize;
    let mut evaluated_definition_chain_count = 0usize;
    let mut unset_definition_chain_count = 0usize;
    let mut definition_chain_atom_count = 0usize;
    let mut definition_chain_control_count = 0usize;
    let mut definition_chain_separator_count = 0usize;
    let mut definition_chain_schema_selector_count = 0usize;
    let mut lead12_relation_program_instance_count = 0usize;
    let mut lead54_relation_program_instance_count = 0usize;
    let mut escaped_word_entity_suffix_count = 0usize;
    let mut token_8149_entity_suffix_count = 0usize;
    let mut fixed_fe_f6_entity_suffix_count = 0usize;
    let mut paged_atom_state_01_entity_suffix_count = 0usize;
    let mut control_e8_entity_suffix_value_count = 0usize;
    let mut control_e9_entity_suffix_value_count = 0usize;
    let mut schema_selected_atom_entity_suffix_value_count = 0usize;
    let mut schema_selected_evaluation_entity_suffix_value_count = 0usize;
    let mut schema_selected_control_entity_suffix_value_count = 0usize;
    let mut schema_selected_separator_entity_suffix_value_count = 0usize;
    let mut schema_selected_schema_entity_suffix_value_count = 0usize;
    let mut definition_schema_selection_count = 0usize;
    let mut entity_value_schema_selection_count = 0usize;
    let mut numeric_entity_value_pair_count = 0usize;
    let mut reference_signature_count = 0usize;
    let mut reference_signature_prefix_atom_2_count = 0usize;
    let mut parameter_value_count = 0usize;
    let mut unresolved_dimension_quantity_count = 0usize;
    let mut definition_value_count = 0usize;
    let mut unowned_definition_value_count = 0usize;
    let mut formula_relation_count = 0usize;
    let mut relation_program_instance_count = 0usize;
    let mut relation_program_output_count = 0usize;
    let mut resolved_relation_program_output_count = 0usize;
    let mut null_relation_program_output_count = 0usize;
    let mut relation_program_reference_incidence_count = 0usize;
    let mut resolved_relation_program_reference_incidence_count = 0usize;
    let mut null_relation_program_reference_incidence_count = 0usize;
    let mut classified_relation_program_reference_incidence_count = 0usize;
    let mut resolved_lead54_relation_program_trailing_entity_count = 0usize;
    let mut null_lead54_relation_program_trailing_entity_count = 0usize;
    let mut resolved_lead12_relation_program_context_entity_count = 0usize;
    let mut null_lead12_relation_program_context_entity_count = 0usize;
    let mut classified_lead12_relation_program_context_entity_count = 0usize;
    let mut lead12_relation_program_paramout_context_entity_count = 0usize;
    let mut resolved_relation_program_instance_count = 0usize;
    let mut null_relation_program_instance_count = 0usize;
    let mut resolved_relation_program_repeated_reference_count = 0usize;
    let mut null_relation_program_repeated_reference_count = 0usize;
    let mut classified_relation_program_entity_count = 0usize;
    let mut classified_relation_program_repeated_entity_count = 0usize;
    let mut relation_expression_instance_count = 0usize;
    let mut typed_relation_program_instance_count = 0usize;
    let mut resolved_relation_program_input_instance_count = 0usize;
    let mut resolved_relation_program_input_count = 0usize;
    let mut relation_program_parameter_dependency_count = 0usize;
    let mut resolved_relation_program_parameter_dependency_count = 0usize;
    let mut ambiguous_relation_program_parameter_dependency_count = 0usize;
    let mut schema_configuration_record_count = 0usize;
    let mut resolved_schema_configuration_reference_count = 0usize;
    let mut null_schema_configuration_reference_count = 0usize;
    let mut classified_schema_configuration_entity_reference_count = 0usize;
    let mut schema_configuration_row_link_count = 0usize;
    let mut resolved_schema_configuration_row_class_count = 0usize;
    let mut null_schema_configuration_row_class_count = 0usize;
    let mut resolved_schema_configuration_row_successor_count = 0usize;
    let mut null_schema_configuration_row_successor_count = 0usize;
    let mut resolved_formula_output_count = 0usize;
    let mut null_formula_output_count = 0usize;
    let mut classified_formula_output_entity_count = 0usize;
    let mut classified_formula_expression_entity_count = 0usize;
    let mut formula_parameter_dependency_count = 0usize;
    let mut formula_parameter_dependency_candidate_count = 0usize;
    let mut classified_formula_parameter_dependency_candidate_count = 0usize;
    let mut resolved_formula_parameter_dependency_count = 0usize;
    let mut ambiguous_formula_parameter_dependency_count = 0usize;
    let mut scalar_entity_suffix_value_count = 0usize;
    let mut unset_entity_suffix_value_count = 0usize;
    let mut separator_entity_suffix_value_count = 0usize;
    let mut atom_entity_suffix_value_count = 0usize;
    let mut schema_selected_entity_suffix_value_count = 0usize;
    let mut wide_prefix_entity_suffix_value_count = 0usize;
    let mut unowned_definition_chain_value_count = 0usize;
    let mut unassigned_definition_chain_value_count = 0usize;
    let mut structurally_owned_definition_chain_evaluation_count = 0usize;
    let mut unowned_definition_chain_evaluation_count = 0usize;
    let mut unassigned_definition_chain_evaluation_count = 0usize;
    let mut constraint_range_incidences = IncomingEntityIncidenceCounts::default();
    let mut range_interval_incidences = IncomingEntityIncidenceCounts::default();
    for record in ctx.admit_iter(&native.entity_records, "catia_census_entity_records")? {
        const OPERATION: &str = "catia_referenced_relation_expressions";
        if record.relation_expression().is_some()
            && (ctx.contains_hash_set(
                &formula_referenced_relation_expressions,
                record.id.as_str(),
                OPERATION,
            )? || ctx.contains_hash_set(
                &program_referenced_relation_expressions,
                record.id.as_str(),
                OPERATION,
            )?)
        {
            referenced_relation_expression_count += 1;
        }

        let fields = record.value_fields_charged(ctx)?;
        entity_value_field_count = entity_value_field_count
            .checked_add(fields.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit("catia_entity_value_field_count", u64::MAX, u64::MAX)
            })?;
        let packets = record.value_packets(ctx, &fields)?;
        for packet in ctx.admit_iter(packets, "catia_census_value_packets")? {
            match packet {
                entity_table::EntityValuePacket::Compact { .. } => {
                    compact_entity_value_packet_count += 1;
                }
                entity_table::EntityValuePacket::Numeric { .. } => {
                    numeric_entity_value_packet_count += 1;
                }
                entity_table::EntityValuePacket::Layout { .. } => {
                    layout_entity_value_packet_count += 1;
                }
                entity_table::EntityValuePacket::E9Scalar { .. } => {
                    e9_scalar_entity_value_packet_count += 1;
                }
            }
        }

        if let Some(signature) = record.reference_signature.as_ref() {
            let (instructions, tokens) = signature.production.instruction_and_token_counts();
            reference_signature_instruction_count += instructions;
            reference_signature_token_count += tokens;
        }
        if let Some(signature) = record.reference_signature.as_ref() {
            for reference in [&signature.first_entity, &signature.second_entity] {
                classified_reference_signature_entity_count +=
                    usize::from(reference.class_name().is_some());
                if reference.is_null() {
                    null_reference_signature_entity_count += 1;
                } else if reference.entity().is_some() {
                    resolved_reference_signature_entity_count += 1;
                } else {
                    unresolved_reference_signature_entity_count += 1;
                }
            }
        }

        if let Some(range) = record.range_interval.as_ref() {
            range_interval_incidences.add(
                ctx,
                &range.incoming_references,
                &range.incoming_storage_references,
            )?;
            range_interval_count += 1;
            range_interval_nominal_count += usize::from(range.nominal.is_some());
            if let Some(slots) = &range.interval.slots {
                for slot in slots {
                    match slot {
                        crate::entity_table::RangeIntervalSlot::Binary64 { .. } => {
                            range_interval_finite_slot_count += 1;
                        }
                        crate::entity_table::RangeIntervalSlot::Unset { .. } => {
                            range_interval_unset_slot_count += 1;
                        }
                    }
                }
            } else {
                range_interval_no_slot_count += 1;
            }
        }
        if let Some(range) = record.constraint_range() {
            constraint_range_incidences.add(
                ctx,
                &range.incoming_references,
                &range.incoming_storage_references,
            )?;
            constraint_range_count += 1;
            match range.framing {
                crate::native::CatiaConstraintRangeFraming::DimensionB8
                | crate::native::CatiaConstraintRangeFraming::DimensionC1
                | crate::native::CatiaConstraintRangeFraming::DimensionDC
                | crate::native::CatiaConstraintRangeFraming::DimensionDF => {
                    dimension_constraint_range_count += 1;
                }
                crate::native::CatiaConstraintRangeFraming::ComplexC9 => {
                    complex_constraint_range_count += 1;
                }
            }
            match range.evaluation {
                crate::native::CatiaEntityEvaluation::Scalar { .. } => {
                    evaluated_constraint_range_count += 1;
                }
                crate::native::CatiaEntityEvaluation::Unset => unset_constraint_range_count += 1,
            }
        }
        if let Some(value) = record.definition_chain_value() {
            use crate::native::{CatiaEntityEvaluation, CatiaEntitySuffixSchemaValue as Selected};
            definition_chain_value_count += 1;
            match &value.value {
                Selected::Evaluation { evaluation, .. } => {
                    definition_chain_evaluation_count += 1;
                    match evaluation {
                        CatiaEntityEvaluation::Scalar { .. } => {
                            evaluated_definition_chain_count += 1;
                        }
                        CatiaEntityEvaluation::Unset => unset_definition_chain_count += 1,
                    }
                }
                Selected::Atom { .. } => definition_chain_atom_count += 1,
                Selected::ControlE8 => definition_chain_control_count += 1,
                Selected::Separator37 => definition_chain_separator_count += 1,
                Selected::SchemaSelector { .. } => definition_chain_schema_selector_count += 1,
            }
        }
        if let Some(instance) = record.relation_program_instance() {
            match instance.framing {
                crate::native::CatiaRelationProgramInstanceFraming::Lead12 { .. } => {
                    lead12_relation_program_instance_count += 1;
                }
                crate::native::CatiaRelationProgramInstanceFraming::Lead54 { .. } => {
                    lead54_relation_program_instance_count += 1;
                }
            }
        }
        match record.suffix_framing() {
            Some(crate::native::CatiaEntitySuffixFraming::EscapedWord(_)) => {
                escaped_word_entity_suffix_count += 1;
            }
            Some(crate::native::CatiaEntitySuffixFraming::Token8149) => {
                token_8149_entity_suffix_count += 1;
            }
            Some(crate::native::CatiaEntitySuffixFraming::FixedFeF6 { .. }) => {
                fixed_fe_f6_entity_suffix_count += 1;
            }
            Some(crate::native::CatiaEntitySuffixFraming::PagedAtomState01 { .. }) => {
                paged_atom_state_01_entity_suffix_count += 1;
            }
            None => {}
        }
        match record.suffix_value().map(|value| &value.payload) {
            Some(crate::native::CatiaEntitySuffixPayload::ControlE8) => {
                control_e8_entity_suffix_value_count += 1;
            }
            Some(crate::native::CatiaEntitySuffixPayload::ControlE9) => {
                control_e9_entity_suffix_value_count += 1;
            }
            _ => {}
        }
        if let Some(crate::native::CatiaEntitySuffixPayload::SchemaSelected { value, .. }) =
            record.suffix_value().map(|suffix| &suffix.payload)
        {
            match value {
                crate::native::CatiaEntitySuffixSelectedValue::Atom { .. } => {
                    schema_selected_atom_entity_suffix_value_count += 1;
                }
                crate::native::CatiaEntitySuffixSelectedValue::Evaluation { .. } => {
                    schema_selected_evaluation_entity_suffix_value_count += 1;
                }
                crate::native::CatiaEntitySuffixSelectedValue::ControlE8 => {
                    schema_selected_control_entity_suffix_value_count += 1;
                }
                crate::native::CatiaEntitySuffixSelectedValue::Separator37 => {
                    schema_selected_separator_entity_suffix_value_count += 1;
                }
                crate::native::CatiaEntitySuffixSelectedValue::SchemaSelector { .. } => {
                    schema_selected_schema_entity_suffix_value_count += 1;
                }
            }
        }
        definition_schema_selection_count += record.definition_schema_selections.len();
        entity_value_schema_selection_count += record.value_schema_selections.len();
        if record.numeric_pair().is_some() {
            numeric_entity_value_pair_count += 1;
        }
        if record.reference_signature.is_some() {
            reference_signature_count += 1;
        }
        if let Some(signature) = record.reference_signature.as_ref() {
            if signature.production.prefix() == entity_table::ReferenceSignaturePrefix::Atom2 {
                reference_signature_prefix_atom_2_count += 1;
            }
        }
        if record.parameter_value().is_some() {
            parameter_value_count += 1;
        }
        if let Some(range) = record.constraint_range() {
            if matches!(
                range.framing,
                crate::native::CatiaConstraintRangeFraming::DimensionB8
                    | crate::native::CatiaConstraintRangeFraming::DimensionC1
                    | crate::native::CatiaConstraintRangeFraming::DimensionDC
                    | crate::native::CatiaConstraintRangeFraming::DimensionDF
            ) && match range.evaluation {
                crate::native::CatiaEntityEvaluation::Scalar { bits } => {
                    f64::from_bits(bits).is_finite()
                }
                crate::native::CatiaEntityEvaluation::Unset => false,
            } {
                unresolved_dimension_quantity_count += 1;
            }
        }
        if record.definition_value().is_some() {
            definition_value_count += 1;
        }
        if record.definition_value().is_some()
            && !ctx.contains_hash_set(
                &owned_definition_value_ids,
                record.id.as_str(),
                "catia_census_lookup",
            )?
        {
            unowned_definition_value_count += 1;
        }
        if record.formula_relation().is_some() {
            formula_relation_count += 1;
        }
        if record.relation_program_instance().is_some() {
            relation_program_instance_count += 1;
        }
        if let Some(instance) = record.relation_program_instance() {
            if instance.output_entity().is_some() {
                relation_program_output_count += 1;
            }
            if let Some(output) = instance.output_entity() {
                if output.entity().is_some() {
                    resolved_relation_program_output_count += 1;
                }
                if output.is_null() {
                    null_relation_program_output_count += 1;
                }
            }
            relation_program_reference_incidence_count += instance.reference_incidences.len();
            for incidence in ctx.admit_iter(&instance.reference_incidences, "catia_census_child")? {
                if incidence.reference.entity().is_some() {
                    resolved_relation_program_reference_incidence_count += 1;
                }
                if incidence.reference.is_null() {
                    null_relation_program_reference_incidence_count += 1;
                }
                if incidence.reference.class_name().is_some() {
                    classified_relation_program_reference_incidence_count += 1;
                }
            }
            if let Some(trailing) = instance.lead54_trailing_entity() {
                if trailing.entity().is_some() {
                    resolved_lead54_relation_program_trailing_entity_count += 1;
                }
                if trailing.is_null() {
                    null_lead54_relation_program_trailing_entity_count += 1;
                }
            }
            if let Some(context) = instance.lead12_context_entity() {
                if context.entity().is_some() {
                    resolved_lead12_relation_program_context_entity_count += 1;
                }
                if context.is_null() {
                    null_lead12_relation_program_context_entity_count += 1;
                }
                if context.class_name().is_some() {
                    classified_lead12_relation_program_context_entity_count += 1;
                }
                if match context.class_name() {
                    Some(name) => {
                        ctx.equal_bytes(name.as_bytes(), b"paramout", "catia_census_context_class")?
                    }
                    None => false,
                } {
                    lead12_relation_program_paramout_context_entity_count += 1;
                }
            }
            if instance.program_entity.entity().is_some() {
                resolved_relation_program_instance_count += 1;
            }
            if instance.program_entity.is_null() {
                null_relation_program_instance_count += 1;
            }
            if instance.repeated_entity.entity().is_some() {
                resolved_relation_program_repeated_reference_count += 1;
            }
            if instance.repeated_entity.is_null() {
                null_relation_program_repeated_reference_count += 1;
            }
            if instance.program_entity.class_name().is_some() {
                classified_relation_program_entity_count += 1;
            }
            if instance.repeated_entity.class_name().is_some() {
                classified_relation_program_repeated_entity_count += 1;
            }
            if instance.relation_expression.is_some() {
                relation_expression_instance_count += 1;
            }
            if let Some(entity) = instance.relation_expression.as_deref() {
                if ctx.contains_hash_set(
                    &typed_relation_expression_entities,
                    entity,
                    "catia_census_lookup",
                )? {
                    typed_relation_program_instance_count += 1;
                }
            }
            if instance.inputs.is_some() {
                resolved_relation_program_input_instance_count += 1;
            }
            if let Some(link) = instance.inputs.as_ref() {
                resolved_relation_program_input_count += link.len();
            }
            relation_program_parameter_dependency_count += instance.parameter_dependencies.len();
            for dependency in
                ctx.admit_iter(&instance.parameter_dependencies, "catia_census_child")?
            {
                if dependency.candidates.len() == 1 {
                    resolved_relation_program_parameter_dependency_count += 1;
                }
                if dependency.candidates.len() > 1 {
                    ambiguous_relation_program_parameter_dependency_count += 1;
                }
            }
        }
        if record.schema_configuration_record().is_some() {
            schema_configuration_record_count += 1;
        }
        if let Some(configuration) = record.schema_configuration_record() {
            if configuration.entity_reference.reference.entity().is_some() {
                resolved_schema_configuration_reference_count += 1;
            }
            if configuration.entity_reference.reference.is_null() {
                null_schema_configuration_reference_count += 1;
            }
            if configuration
                .entity_reference
                .reference
                .class_name()
                .is_some()
            {
                classified_schema_configuration_entity_reference_count += 1;
            }
        }
        if record.schema_configuration_row_link().is_some() {
            schema_configuration_row_link_count += 1;
        }
        if let Some(link) = record.schema_configuration_row_link() {
            if link.class_reference.entity().is_some() {
                resolved_schema_configuration_row_class_count += 1;
            }
            if link.class_reference.is_null() {
                null_schema_configuration_row_class_count += 1;
            }
            if link.successor.entity().is_some() {
                resolved_schema_configuration_row_successor_count += 1;
            }
            if link.successor.is_null() {
                null_schema_configuration_row_successor_count += 1;
            }
        }
        if let Some(formula) = record.formula_relation() {
            if formula.output_entity.reference.entity().is_some() {
                resolved_formula_output_count += 1;
            }
            if formula.output_entity.reference.is_null() {
                null_formula_output_count += 1;
            }
            if formula.output_entity.reference.class_name().is_some() {
                classified_formula_output_entity_count += 1;
            }
            if formula.expression_entity.reference.class_name().is_some() {
                classified_formula_expression_entity_count += 1;
            }
            formula_parameter_dependency_count += formula.parameter_dependencies.len();
            for dependency in
                ctx.admit_iter(&formula.parameter_dependencies, "catia_census_child")?
            {
                formula_parameter_dependency_candidate_count += dependency.candidates.len();
                for candidate in ctx.admit_iter(&dependency.candidates, "catia_census_child")? {
                    if candidate.class_name().is_some() {
                        classified_formula_parameter_dependency_candidate_count += 1;
                    }
                }
                if dependency.candidates.len() == 1 {
                    resolved_formula_parameter_dependency_count += 1;
                }
                if dependency.candidates.len() > 1 {
                    ambiguous_formula_parameter_dependency_count += 1;
                }
            }
        }
        if record.suffix_value().is_some_and(|value| {
            matches!(
                value.payload,
                crate::native::CatiaEntitySuffixPayload::Evaluation {
                    evaluation: crate::native::CatiaEntityEvaluation::Scalar { .. },
                    ..
                }
            )
        }) {
            scalar_entity_suffix_value_count += 1;
        }
        if record.suffix_value().is_some_and(|value| {
            matches!(
                value.payload,
                crate::native::CatiaEntitySuffixPayload::Evaluation {
                    evaluation: crate::native::CatiaEntityEvaluation::Unset,
                    ..
                }
            )
        }) {
            unset_entity_suffix_value_count += 1;
        }
        if record.suffix_value().is_some_and(|value| {
            matches!(
                value.payload,
                crate::native::CatiaEntitySuffixPayload::Separator37
            )
        }) {
            separator_entity_suffix_value_count += 1;
        }
        if record.suffix_value().is_some_and(|value| {
            matches!(
                value.payload,
                crate::native::CatiaEntitySuffixPayload::Atom { .. }
            )
        }) {
            atom_entity_suffix_value_count += 1;
        }
        if record.suffix_schema_selection.is_some() {
            schema_selected_entity_suffix_value_count += 1;
        }
        if record.suffix_value().as_ref().is_some_and(|value| {
            value.prefix_atom_widths[0] > 1
                || value.prefix_atom_widths[1] > 1
                || value.prefix_atom_widths[2] > 1
        }) {
            wide_prefix_entity_suffix_value_count += 1;
        }
        if let Some(value) = record.definition_chain_value() {
            unowned_definition_chain_value_count += usize::from(!ctx.contains_hash_set(
                &structurally_owned_definition_chain_value_ids,
                record.id.as_str(),
                "catia_census_lookup",
            )?);
            let unassigned = ctx
                .get_hash_map(
                    &object_records_by_id,
                    record.object_record.as_str(),
                    "catia_census_lookup",
                )?
                .is_some_and(|record| record.has_unassigned_owner());
            unassigned_definition_chain_value_count += usize::from(unassigned);
            if matches!(
                &value.value,
                crate::native::CatiaEntitySuffixSchemaValue::Evaluation { .. }
            ) {
                let owned = ctx.contains_hash_set(
                    &structurally_owned_records,
                    record.object_record.as_str(),
                    "catia_census_lookup",
                )?;
                structurally_owned_definition_chain_evaluation_count += usize::from(owned);
                unowned_definition_chain_evaluation_count += usize::from(!owned);
                unassigned_definition_chain_evaluation_count += usize::from(unassigned);
            }
        }
    }
    let mut multi_member_reference_signature_cohort_count = 0usize;
    let mut reference_signature_cohort_member_count = 0usize;
    let mut schema_selected_reference_signature_cohort_count = 0usize;
    for cohort in ctx.admit_iter(
        &native.reference_signature_cohorts,
        "catia_census_reference_signature_cohorts",
    )? {
        if cohort.members.len() > 1 {
            multi_member_reference_signature_cohort_count += 1;
        }
        reference_signature_cohort_member_count += cohort.members.len();
        if cohort.schema_selection.is_some() {
            schema_selected_reference_signature_cohort_count += 1;
        }
    }
    let mut unresolved_consolidated_edge_run_count = 0usize;
    let mut partially_resolved_consolidated_edge_run_count = 0usize;
    let mut fully_resolved_consolidated_edge_run_count = 0usize;
    let mut consolidated_edge_run_support_binding_count = 0usize;
    let mut consolidated_edge_run_shared_locus_count = 0usize;
    let mut consolidated_edge_run_endpoint_locus_count = 0usize;
    for run in ctx.admit_iter(
        &native.consolidated_edge_runs,
        "catia_census_consolidated_edge_runs",
    )? {
        match &run.support_bindings {
            [None, None] => unresolved_consolidated_edge_run_count += 1,
            [Some(_), None] | [None, Some(_)] => {
                partially_resolved_consolidated_edge_run_count += 1;
            }
            [Some(_), Some(_)] => fully_resolved_consolidated_edge_run_count += 1,
        }
        for binding in &run.support_bindings {
            if binding.is_some() {
                consolidated_edge_run_support_binding_count += 1;
            }
        }
        if run.shared_loci.is_some() {
            consolidated_edge_run_shared_locus_count += 1;
        }
        if run.endpoint_loci.is_some() {
            consolidated_edge_run_endpoint_locus_count += 1;
        }
    }
    let mut ordered_schema_configuration_row_link_count = 0usize;
    let mut resolved_schema_configuration_row_chain_terminal_count = 0usize;
    let mut null_schema_configuration_row_chain_terminal_count = 0usize;
    let mut classified_schema_configuration_row_chain_terminal_count = 0usize;
    let mut schema_configuration_row_intervening_entity_count = 0usize;
    let mut schema_configuration_row_source_interval_chain_count = 0usize;
    let mut schema_configuration_row_intervening_schema_configuration_count = 0usize;
    for chain in ctx.admit_iter(
        &native.schema_configuration_row_chains,
        "catia_census_schema_configuration_row_chains",
    )? {
        ordered_schema_configuration_row_link_count += chain.links().len();
        if chain.terminal.entity().is_some() {
            resolved_schema_configuration_row_chain_terminal_count += 1;
        }
        if chain.terminal.is_null() {
            null_schema_configuration_row_chain_terminal_count += 1;
        }
        if chain.terminal.class_name().is_some() {
            classified_schema_configuration_row_chain_terminal_count += 1;
        }
        let mut complete_source_intervals = true;
        for link in ctx.admit_iter(chain.links(), "catia_census_row_links")? {
            let Some(references) = &link.intervening_entities else {
                complete_source_intervals = false;
                continue;
            };
            for reference in ctx.admit_iter(references, "catia_census_intervening_entities")? {
                schema_configuration_row_intervening_entity_count += 1;
                if let Some(entity) = reference.entity() {
                    schema_configuration_row_intervening_schema_configuration_count +=
                        usize::from(ctx.contains_hash_set(
                            &schema_configuration_entities,
                            entity,
                            "catia_census_lookup",
                        )?);
                }
            }
        }
        schema_configuration_row_source_interval_chain_count +=
            usize::from(complete_source_intervals);
    }
    let mut value_field_count = 0usize;
    let mut value_selection_count = 0usize;
    for block in ctx.admit_iter(&native.value_blocks, "catia_census_value_blocks")? {
        value_field_count = value_field_count
            .checked_add(crate::value_block::tokenize_charged(ctx, &block.payload)?.len())
            .ok_or_else(|| ctx.refuse_codec_limit("catia_value_field_count", u64::MAX, u64::MAX))?;

        value_selection_count += block.schema_selections.len();
    }
    let mut transferred_line_profile_count = 0usize;
    for curve in ctx.admit_iter(&ir.model.curves, "catia_census_curves")? {
        if ctx.starts_with(
            curve.id.as_str(),
            "catia:consolidated:line-profile-curve#",
            "catia_census_id_prefix",
        )? {
            transferred_line_profile_count += 1;
        }
    }
    let mut transferred_native_sketch_entity_count = 0usize;
    for entity in ctx.admit_iter(&ir.model.sketch_entities, "catia_census_sketch_entities")? {
        if matches!(
            entity.geometry.definition(),
            cadmpeg_ir::sketches::SketchGeometryDefinition::Native { .. }
        ) {
            transferred_native_sketch_entity_count += 1;
        }
    }
    let mut transferred_native_operation_parameter_count = 0usize;
    for parameter in ctx.admit_iter(&ir.model.parameters, "catia_census_parameters")? {
        if match parameter.owner.as_ref() {
            Some(owner) => {
                ctx.contains_hash_set(&native_operation_feature_ids, owner, "catia_census_lookup")?
            }
            None => false,
        } {
            transferred_native_operation_parameter_count += 1;
        }
    }
    let mut decoded_consolidated_cone_face_parameter_point_count = 0usize;
    for face in ctx.admit_iter(
        &native.consolidated_cone_faces,
        "catia_census_consolidated_cone_faces",
    )? {
        decoded_consolidated_cone_face_parameter_point_count += face.parameter_points.len();
    }
    let mut transferred_consolidated_revolution_count = 0usize;
    for surface in ctx.admit_iter(
        &ir.model.procedural_surfaces,
        "catia_census_procedural_surfaces",
    )? {
        if ctx.starts_with(
            surface.id.as_str(),
            "catia:consolidated:surface-revolution#",
            "catia_census_id_prefix",
        )? {
            transferred_consolidated_revolution_count += 1;
        }
    }
    let mut decoded_zero_entity_edge_stride_allocation_count = 0usize;
    let mut decoded_zero_entity_edge_stride_topology_ref_count = 0usize;
    let mut decoded_zero_entity_edge_stride_surface_support_ref_count = 0usize;
    for stride in ctx.admit_iter(
        &native.zero_entity_edge_strides,
        "catia_census_zero_entity_edge_strides",
    )? {
        decoded_zero_entity_edge_stride_allocation_count += stride.allocations.len();
        decoded_zero_entity_edge_stride_topology_ref_count += stride.topology_refs.len();
        decoded_zero_entity_edge_stride_surface_support_ref_count +=
            stride.surface_support_refs.len();
    }
    let mut decoded_zero_entity_face_bound_support_run_count = 0usize;
    let mut decoded_zero_entity_face_terminal_control_03_count = 0usize;
    let mut decoded_zero_entity_face_terminal_control_05_count = 0usize;
    let mut decoded_zero_entity_loop_terminal_count = 0usize;
    let mut decoded_zero_entity_loop_record_count = 0usize;
    let mut decoded_zero_entity_loop_class_41_count = 0usize;
    let mut decoded_zero_entity_loop_class_50_count = 0usize;
    let mut decoded_zero_entity_loop_class_c1_count = 0usize;
    let mut decoded_zero_entity_forward_loop_member_count = 0usize;
    let mut decoded_zero_entity_reversed_loop_member_count = 0usize;
    let mut decoded_zero_entity_oriented_loop_member_count = 0usize;
    let mut decoded_zero_entity_oriented_model_endpoint_pair_count = 0usize;
    let mut decoded_zero_entity_bound_support_member_count = 0usize;
    let mut decoded_zero_entity_bound_typed_loop_reference_count = 0usize;
    let mut decoded_zero_entity_support_occurrence_count = 0usize;
    let mut decoded_zero_entity_support_pcurve_count = 0usize;
    let mut decoded_zero_entity_support_model_curve_count = 0usize;
    let mut decoded_zero_entity_support_model_construction_count = 0usize;
    let mut decoded_zero_entity_uv_endpoint_pair_count = 0usize;
    let mut decoded_zero_entity_model_endpoint_pair_count = 0usize;
    let mut decoded_zero_entity_model_midpoint_count = 0usize;
    for run in ctx.admit_iter(
        &native.zero_entity_support_runs,
        "catia_census_zero_entity_support_runs",
    )? {
        if run.face.is_some() {
            decoded_zero_entity_face_bound_support_run_count += 1;
        }
        if let Some(face) = run.face.as_ref() {
            if face.terminal_control == 0x03 {
                decoded_zero_entity_face_terminal_control_03_count += 1;
            }
            if face.terminal_control == 0x05 {
                decoded_zero_entity_face_terminal_control_05_count += 1;
            }
            decoded_zero_entity_loop_terminal_count += face.loop_terminals.len();
            decoded_zero_entity_loop_record_count += face.loops.len();
            for loop_record in ctx.admit_iter(&face.loops, "catia_census_child")? {
                if loop_record.loop_class == 0x41 {
                    decoded_zero_entity_loop_class_41_count += 1;
                }
                if loop_record.loop_class == 0x50 {
                    decoded_zero_entity_loop_class_50_count += 1;
                }
                if loop_record.loop_class == 0xc1 {
                    decoded_zero_entity_loop_class_c1_count += 1;
                }
                for sense in ctx.admit_iter(&loop_record.forward_senses, "catia_census_child")? {
                    if *sense {
                        decoded_zero_entity_forward_loop_member_count += 1;
                    }
                    if !*sense {
                        decoded_zero_entity_reversed_loop_member_count += 1;
                    }
                }
                decoded_zero_entity_oriented_loop_member_count += loop_record.forward_senses.len();
                decoded_zero_entity_oriented_model_endpoint_pair_count +=
                    loop_record.oriented_model_endpoints.len();
                decoded_zero_entity_bound_support_member_count +=
                    loop_record.support_record_ordinals.len();
                decoded_zero_entity_bound_typed_loop_reference_count +=
                    loop_record.typed_records.len();
            }
        }
        decoded_zero_entity_support_occurrence_count += run.supports.len();
        for support in ctx.admit_iter(&run.supports, "catia_census_child")? {
            if support.pcurve.is_some() {
                decoded_zero_entity_support_pcurve_count += 1;
            }
            if support.model_curve.is_some() {
                decoded_zero_entity_support_model_curve_count += 1;
            }
            if support.model_curve_construction.is_some() {
                decoded_zero_entity_support_model_construction_count += 1;
            }
            if support.uv_endpoints.is_some() {
                decoded_zero_entity_uv_endpoint_pair_count += 1;
            }
            if support.model_endpoints.is_some() {
                decoded_zero_entity_model_endpoint_pair_count += 1;
            }
            if support.model_midpoint.is_some() {
                decoded_zero_entity_model_midpoint_count += 1;
            }
        }
    }
    let mut decoded_zero_entity_oriented_use_count = 0usize;
    let mut decoded_zero_entity_oriented_use_allocation_count = 0usize;
    for pair in ctx.admit_iter(
        &native.zero_entity_oriented_use_pairs,
        "catia_census_zero_entity_oriented_use_pairs",
    )? {
        decoded_zero_entity_oriented_use_count += pair.uses.len();
        for use_record in &pair.uses {
            decoded_zero_entity_oriented_use_allocation_count += use_record.allocations.len();
        }
    }
    let mut decoded_zero_entity_vertex_incidence_allocation_count = 0usize;
    let mut decoded_zero_entity_vertex_owner_binding_count = 0usize;
    for incidence in ctx.admit_iter(
        &native.zero_entity_vertex_incidences,
        "catia_census_zero_entity_vertex_incidences",
    )? {
        decoded_zero_entity_vertex_incidence_allocation_count += incidence.allocations.len();
        if incidence.vertex_record.is_some() {
            decoded_zero_entity_vertex_owner_binding_count += 1;
        }
    }
    let modeling_scope_is_unresolved =
        matches!(modeling_graph_scope, ModelingGraphScope::Unresolved);
    let unresolved_object_record_reference_count = object_record_reference_count
        - resolved_object_record_reference_count
        - null_object_record_reference_count;

    let design_parallel_reference_unclassified_column_count =
        design_parallel_reference_column_count - design_parallel_reference_classified_column_count;
    let design_parallel_reference_unresolved_cell_count = design_parallel_reference_cell_count
        - design_parallel_reference_resolved_cell_count
        - design_parallel_reference_null_cell_count;
    let design_parallel_reference_unclassified_cell_count =
        design_parallel_reference_cell_count - design_parallel_reference_classified_cell_count;

    let design_parallel_reference_unmatched_row_count =
        design_parallel_reference_row_count - design_parallel_reference_matched_row_count;

    let legacy_asynchronous_relation_count =
        legacy_synchronous_state_count - legacy_synchronous_relation_count;

    let reference_signature_prefix_atom_35_count =
        reference_signature_count - reference_signature_prefix_atom_2_count;
    let reference_signature_cohort_count = native.reference_signature_cohorts.len();

    let consolidated_edge_run_count = native.consolidated_edge_runs.len();

    let constraint_range_incoming_reference_count = constraint_range_incidences.total();
    let IncomingEntityIncidenceCounts {
        payload: constraint_range_incoming_payload_reference_count,
        storage: constraint_range_incoming_storage_reference_count,
        classified: classified_constraint_range_source_entity_count,
        zero: unreferenced_constraint_range_count,
        one: uniquely_referenced_constraint_range_count,
        multiple: multiply_referenced_constraint_range_count,
    } = constraint_range_incidences;
    let range_interval_incoming_reference_count = range_interval_incidences.total();
    let IncomingEntityIncidenceCounts {
        payload: range_interval_incoming_payload_reference_count,
        storage: range_interval_incoming_storage_reference_count,
        classified: classified_range_interval_source_entity_count,
        zero: unreferenced_range_interval_count,
        one: uniquely_referenced_range_interval_count,
        multiple: multiply_referenced_range_interval_count,
    } = range_interval_incidences;

    let unresolved_relation_program_output_count = relation_program_output_count
        - resolved_relation_program_output_count
        - null_relation_program_output_count;

    let unresolved_relation_program_reference_incidence_count =
        relation_program_reference_incidence_count
            - resolved_relation_program_reference_incidence_count
            - null_relation_program_reference_incidence_count;

    let unresolved_lead54_relation_program_trailing_entity_count =
        lead54_relation_program_instance_count
            - resolved_lead54_relation_program_trailing_entity_count
            - null_lead54_relation_program_trailing_entity_count;

    let unresolved_lead12_relation_program_context_entity_count =
        lead12_relation_program_instance_count
            - resolved_lead12_relation_program_context_entity_count
            - null_lead12_relation_program_context_entity_count;

    let other_lead12_relation_program_context_class_count =
        classified_lead12_relation_program_context_entity_count
            - lead12_relation_program_paramout_context_entity_count;
    let unclassified_lead12_relation_program_context_entity_count =
        lead12_relation_program_instance_count
            - classified_lead12_relation_program_context_entity_count;

    let unresolved_relation_program_instance_count = relation_program_instance_count
        - resolved_relation_program_instance_count
        - null_relation_program_instance_count;

    let unresolved_relation_program_repeated_reference_count = relation_program_instance_count
        - resolved_relation_program_repeated_reference_count
        - null_relation_program_repeated_reference_count;

    let unresolved_relation_program_input_instance_count =
        typed_relation_program_instance_count - resolved_relation_program_input_instance_count;

    let unresolved_relation_program_parameter_dependency_count =
        relation_program_parameter_dependency_count
            - resolved_relation_program_parameter_dependency_count;
    let other_relation_program_instance_count =
        resolved_relation_program_instance_count - relation_expression_instance_count;

    let unresolved_schema_configuration_reference_count = schema_configuration_record_count
        - resolved_schema_configuration_reference_count
        - null_schema_configuration_reference_count;

    let unordered_schema_configuration_row_link_count =
        schema_configuration_row_link_count - ordered_schema_configuration_row_link_count;

    let unresolved_schema_configuration_row_chain_terminal_count =
        complete_schema_configuration_row_chain_count
            - resolved_schema_configuration_row_chain_terminal_count
            - null_schema_configuration_row_chain_terminal_count;

    let formula_referenced_relation_expression_count =
        formula_referenced_relation_expressions.len();
    let program_referenced_relation_expression_count =
        program_referenced_relation_expressions.len();

    let unreferenced_relation_expression_count =
        relation_expression_count - referenced_relation_expression_count;

    let unresolved_formula_output_count =
        formula_relation_count - resolved_formula_output_count - null_formula_output_count;

    let unresolved_formula_parameter_dependency_count =
        formula_parameter_dependency_count - resolved_formula_parameter_dependency_count;

    let control_entity_suffix_value_count =
        control_e8_entity_suffix_value_count + control_e9_entity_suffix_value_count;

    // One pass over the owned records counts the formula and principal-plane
    // records among them and collects those some transfer consumed.
    let mut transferred_storage = ctx.reserve_scoped(0, "catia_transferred_design_records")?;
    let mut transferred_design_records = HashSet::new();
    let mut transferred_formula_design_record_count = 0usize;
    let mut transferred_principal_plane_record_count = 0usize;
    {
        const OPERATION: &str = "catia_transferred_design_records";
        let mut counted = HashSet::new();
        for object in ctx.admit_iter(&native.design_objects, "catia_owned_design_objects")? {
            if object.owner_record.is_none() {
                continue;
            }
            for field in ctx.admit_iter(&object.fields, "catia_owned_design_fields")? {
                if !transferred_storage
                    .with_storage(|| ctx.insert_hash_set(&mut counted, field.as_str(), OPERATION))?
                {
                    continue;
                }
                let formula = ctx.contains_hash_set(
                    &formula_transfer.consumed_object_records,
                    field.as_str(),
                    OPERATION,
                )?;
                let principal = ctx.contains_hash_set(
                    &design_feature_transfer.principal_plane_records,
                    field.as_str(),
                    OPERATION,
                )?;
                transferred_formula_design_record_count += usize::from(formula);
                transferred_principal_plane_record_count += usize::from(principal);
                let transferred = formula
                    || design_feature_transfer.consumes(ctx, field)?
                    || ctx.contains_hash_set(
                        &transferred_native_sketch_entity_records,
                        field.as_str(),
                        OPERATION,
                    )?
                    || ctx.contains_hash_set(
                        &transferred_native_sketch_constraint_records,
                        field.as_str(),
                        OPERATION,
                    )?
                    || ctx.contains_hash_set(
                        &transferred_constraint_range_records,
                        field.as_str(),
                        OPERATION,
                    )?;
                if transferred {
                    transferred_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut transferred_design_records,
                            field.as_str(),
                            OPERATION,
                        )
                    })?;
                }
            }
        }
    }
    let mut unresolved_object_record_count = 0usize;
    for record in ctx.admit_iter(&modeling_object_records, "catia_unresolved_object_records")? {
        if !ctx.contains_hash_set(
            &transferred_design_records,
            record,
            "catia_unresolved_object_records",
        )? {
            unresolved_object_record_count += 1;
        }
    }
    let mut unresolved_design_object_count = 0usize;
    for object in ctx.admit_iter(&native.design_objects, "catia_unresolved_design_objects")? {
        if match &modeling_graph_scope {
            ModelingGraphScope::Unscoped => true,
            ModelingGraphScope::Unresolved => false,
            ModelingGraphScope::Scoped(part) => ctx.equal_bytes(
                part.as_bytes(),
                object.parent.as_bytes(),
                "catia_census_graph_scope",
            )?,
        } && ctx.any_by(
            &object.fields,
            |field| {
                Ok(!ctx.contains_hash_set(
                    &transferred_design_records,
                    field.as_str(),
                    "catia_unresolved_design_objects",
                )?)
            },
            "catia_unresolved_design_objects",
        )? {
            unresolved_design_object_count += 1;
        }
    }

    for (key, count) in [
        (
            crate::coverage::DECODED_APPEARANCE_PACKET_COUNT,
            appearance_transfer.decoded_packets(),
        ),
        (
            crate::coverage::UNRESOLVED_APPEARANCE_PACKET_COUNT,
            appearance_transfer.unresolved_packets(),
        ),
        (
            crate::coverage::TRANSFERRED_APPEARANCE_ASSET_COUNT,
            appearance_transfer.emitted_assets,
        ),
        (
            crate::coverage::TRANSFERRED_APPEARANCE_BINDING_COUNT,
            appearance_transfer.emitted_bindings,
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_CIRCLE_COUNT,
            native.consolidated_circles.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_CLASS61_RECORD_COUNT,
            native.consolidated_class61_records.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_CONE_FACE_COUNT,
            native.consolidated_cone_faces.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_CONE_FACE_PARAMETER_POINT_COUNT,
            decoded_consolidated_cone_face_parameter_point_count,
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_CONE_COUNT,
            native.consolidated_cones.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_CYLINDER_COUNT,
            native.consolidated_cylinders.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_GROUP_COUNT,
            native.consolidated_groups.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_LINE_PROFILE_COUNT,
            native.consolidated_line_profiles.len(),
        ),
        (
            crate::coverage::TRANSFERRED_CONSOLIDATED_LINE_PROFILE_COUNT,
            transferred_line_profile_count,
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_PARAMETER_POINT_COUNT,
            native.consolidated_parameter_points.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_PLANE_CARRIER_COUNT,
            native.consolidated_plane_carriers.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_PCURVE_COUNT,
            native.consolidated_pcurves.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_COUNT,
            consolidated_edge_run_count,
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_SUPPORT_BINDING_COUNT,
            consolidated_edge_run_support_binding_count,
        ),
        (
            crate::coverage::UNRESOLVED_CONSOLIDATED_EDGE_RUN_COUNT,
            unresolved_consolidated_edge_run_count,
        ),
        (
            crate::coverage::PARTIALLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT,
            partially_resolved_consolidated_edge_run_count,
        ),
        (
            crate::coverage::FULLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT,
            fully_resolved_consolidated_edge_run_count,
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_SHARED_LOCUS_COUNT,
            consolidated_edge_run_shared_locus_count,
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_ENDPOINT_LOCUS_COUNT,
            consolidated_edge_run_endpoint_locus_count,
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_REFERENCE_LIST_COUNT,
            native.consolidated_reference_lists.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_REVOLUTION_COUNT,
            native.consolidated_revolutions.len(),
        ),
        (
            crate::coverage::TRANSFERRED_CONSOLIDATED_REVOLUTION_COUNT,
            transferred_consolidated_revolution_count,
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_SPHERE_COUNT,
            native.consolidated_spheres.len(),
        ),
        (
            crate::coverage::DECODED_CONSOLIDATED_TORUS_COUNT,
            native.consolidated_tori.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_COUNT,
            native.zero_entity_edge_strides.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_ALLOCATION_COUNT,
            decoded_zero_entity_edge_stride_allocation_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_TOPOLOGY_REF_COUNT,
            decoded_zero_entity_edge_stride_topology_ref_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_SURFACE_SUPPORT_REF_COUNT,
            decoded_zero_entity_edge_stride_surface_support_ref_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_FACE_BOUND_SUPPORT_RUN_COUNT,
            decoded_zero_entity_face_bound_support_run_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_03_COUNT,
            decoded_zero_entity_face_terminal_control_03_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_05_COUNT,
            decoded_zero_entity_face_terminal_control_05_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_TERMINAL_COUNT,
            decoded_zero_entity_loop_terminal_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_RECORD_COUNT,
            decoded_zero_entity_loop_record_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_41_COUNT,
            decoded_zero_entity_loop_class_41_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_50_COUNT,
            decoded_zero_entity_loop_class_50_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_C1_COUNT,
            decoded_zero_entity_loop_class_c1_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_FORWARD_LOOP_MEMBER_COUNT,
            decoded_zero_entity_forward_loop_member_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_REVERSED_LOOP_MEMBER_COUNT,
            decoded_zero_entity_reversed_loop_member_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_LOOP_MEMBER_COUNT,
            decoded_zero_entity_oriented_loop_member_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_MODEL_ENDPOINT_PAIR_COUNT,
            decoded_zero_entity_oriented_model_endpoint_pair_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_BOUND_SUPPORT_MEMBER_COUNT,
            decoded_zero_entity_bound_support_member_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_BOUND_TYPED_LOOP_REFERENCE_COUNT,
            decoded_zero_entity_bound_typed_loop_reference_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_PAIR_COUNT,
            native.zero_entity_oriented_use_pairs.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_COUNT,
            decoded_zero_entity_oriented_use_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_ALLOCATION_COUNT,
            decoded_zero_entity_oriented_use_allocation_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ENDPOINT_PAIR_CANDIDATE_COUNT,
            native.zero_entity_endpoint_pair_candidates.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ENDPOINT_LOCUS_CANDIDATE_COUNT,
            native.zero_entity_endpoint_locus_candidates.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_RECORD_COUNT,
            native.zero_entity_records.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_RUN_COUNT,
            native.zero_entity_support_runs.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_OCCURRENCE_COUNT,
            decoded_zero_entity_support_occurrence_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_PCURVE_COUNT,
            decoded_zero_entity_support_pcurve_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_MODEL_CURVE_COUNT,
            decoded_zero_entity_support_model_curve_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_MODEL_CONSTRUCTION_COUNT,
            decoded_zero_entity_support_model_construction_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_UV_ENDPOINT_PAIR_COUNT,
            decoded_zero_entity_uv_endpoint_pair_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_MODEL_ENDPOINT_PAIR_COUNT,
            decoded_zero_entity_model_endpoint_pair_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_MODEL_MIDPOINT_COUNT,
            decoded_zero_entity_model_midpoint_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_COUNT,
            native.zero_entity_vertex_incidences.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_ALLOCATION_COUNT,
            decoded_zero_entity_vertex_incidence_allocation_count,
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_VERTEX_OWNER_BINDING_COUNT,
            decoded_zero_entity_vertex_owner_binding_count,
        ),
        (
            crate::coverage::DECODED_OBJECT_GRAPH_COUNT,
            native.object_graphs.len(),
        ),
        (crate::coverage::DECODED_OBJECT_RECORD_COUNT, object_record_count),
        (
            crate::coverage::MODELING_OBJECT_GRAPH_COUNT,
            modeling_graph_scope.graph_count(native.object_graphs.len()),
        ),
        (
            crate::coverage::MODELING_OBJECT_RECORD_COUNT,
            modeling_object_records.len(),
        ),
        (
            crate::coverage::RETAINED_UNSCOPED_OBJECT_GRAPH_COUNT,
            retained_unscoped_object_graph_count,
        ),
        (
            crate::coverage::RETAINED_UNSCOPED_OBJECT_RECORD_COUNT,
            retained_unscoped_object_record_count,
        ),
        (
            crate::coverage::DECODED_STORAGE_RECORD_LINK_COUNT,
            resolved_storage_record_count,
        ),
        (
            crate::coverage::UNRESOLVED_STORAGE_RECORD_COUNT,
            unresolved_storage_record_count,
        ),
        (
            crate::coverage::DECODED_OBJECT_RECORD_REFERENCE_COUNT,
            object_record_reference_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_OBJECT_RECORD_REFERENCE_COUNT,
            resolved_object_record_reference_count,
        ),
        (
            crate::coverage::DECODED_NULL_OBJECT_RECORD_REFERENCE_COUNT,
            null_object_record_reference_count,
        ),
        (
            crate::coverage::UNRESOLVED_OBJECT_RECORD_REFERENCE_COUNT,
            unresolved_object_record_reference_count,
        ),
        (
            crate::coverage::DECODED_REPEATED_REFERENCE_SUFFIX_COUNT,
            repeated_reference_suffix_count,
        ),
        (
            crate::coverage::DECODED_REPEATED_REFERENCE_SCHEMA_SELECTION_COUNT,
            repeated_reference_schema_selection_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_OBJECT_COUNT,
            native.design_objects.len(),
        ),
        (crate::coverage::DECODED_DESIGN_FIELD_COUNT, design_field_count),
        (
            crate::coverage::CLASSIFIED_DESIGN_OBJECT_COUNT,
            classified_design_object_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_OBJECT_RELATION_COUNT,
            design_object_relation_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_PARALLEL_REFERENCE_TABLE_COUNT,
            design_parallel_reference_table_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_PARALLEL_REFERENCE_ROW_COUNT,
            design_parallel_reference_row_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_PARALLEL_REFERENCE_COLUMN_COUNT,
            design_parallel_reference_column_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_DESIGN_PARALLEL_REFERENCE_COLUMN_COUNT,
            design_parallel_reference_unclassified_column_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_PARALLEL_REFERENCE_CELL_COUNT,
            design_parallel_reference_cell_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_PARALLEL_REFERENCE_RESOLVED_CELL_COUNT,
            design_parallel_reference_resolved_cell_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_PARALLEL_REFERENCE_NULL_CELL_COUNT,
            design_parallel_reference_null_cell_count,
        ),
        (
            crate::coverage::UNRESOLVED_DESIGN_PARALLEL_REFERENCE_CELL_COUNT,
            design_parallel_reference_unresolved_cell_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_PARALLEL_REFERENCE_CLASSIFIED_CELL_COUNT,
            design_parallel_reference_classified_cell_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_DESIGN_PARALLEL_REFERENCE_CELL_COUNT,
            design_parallel_reference_unclassified_cell_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_PARALLEL_REFERENCE_CLASSIFIED_COLUMN_COUNT,
            design_parallel_reference_classified_column_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_PARALLEL_REFERENCE_MATCHED_ROW_COUNT,
            design_parallel_reference_matched_row_count,
        ),
        (
            crate::coverage::UNMATCHED_DESIGN_PARALLEL_REFERENCE_ROW_COUNT,
            design_parallel_reference_unmatched_row_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_UNOWNED_FIELD_RELATION_COUNT,
            design_unowned_field_relation_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_SAME_OBJECT_RELATION_COUNT,
            design_same_object_relation_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_REFLEXIVE_FIELD_RELATION_COUNT,
            design_reflexive_field_relation_count,
        ),
        (
            crate::coverage::DECODED_DESIGN_OBJECT_OWNER_LINK_COUNT,
            design_object_owner_link_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_ENTITY_RUN_COUNT,
            native.legacy_entity_runs.len(),
        ),
        (
            crate::coverage::DECODED_LEGACY_ENTITY_IDENTITY_COUNT,
            legacy_entity_identity_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_SCHEMA_PROGRAM_COUNT,
            legacy_schema_program_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_VENDOR_FOOTER_SCHEMA_PROGRAM_COUNT,
            legacy_vendor_footer_schema_program_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_DIRECTORY_BOUND_SCHEMA_PROGRAM_COUNT,
            legacy_directory_bound_schema_program_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_SCHEMA_IDENTIFIER_COUNT,
            legacy_schema_identifier_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_EVALUATED_VALUE_NAME_COUNT,
            legacy_evaluated_value_name_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_81_COUNT,
            legacy_identity_lead_81_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_82_COUNT,
            legacy_identity_lead_82_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_E5_COUNT,
            legacy_identity_lead_e5_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_IDENTITY_LEAD_FD_COUNT,
            legacy_identity_lead_fd_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_TEXT_FIELD_COUNT,
            legacy_text_field_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_E3_ROLE_TAIL_TEXT_FIELD_COUNT,
            legacy_e3_role_tail_text_field_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_ROLE_SELECTOR_COUNT,
            legacy_role_selector_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_SELECTED_ROLE_COUNT,
            legacy_selected_role_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_ROLE_FIELD_BINDING_COUNT,
            legacy_role_field_binding_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_ROLE_TEXT_FIELD_COUNT,
            legacy_role_text_field_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_SCHEMA_FIELD_COUNT,
            legacy_schema_field_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_RELATION_COUNT,
            legacy_relation_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_PARAMETER_RELATION_COUNT,
            legacy_parameter_relation_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_SYNCHRONOUS_STATE_COUNT,
            legacy_synchronous_state_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_SYNCHRONOUS_RELATION_COUNT,
            legacy_synchronous_relation_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_ASYNCHRONOUS_RELATION_COUNT,
            legacy_asynchronous_relation_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_TYPE_DESCRIPTOR_COUNT,
            legacy_type_descriptor_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_LITERAL_TYPE_DESCRIPTOR_COUNT,
            legacy_literal_type_descriptor_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_SCALAR_VALUE_COUNT,
            legacy_scalar_value_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_NAMED_SCALAR_VALUE_COUNT,
            legacy_named_scalar_value_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_STRING_VALUE_COUNT,
            legacy_string_value_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_NAMED_STRING_VALUE_COUNT,
            legacy_named_string_value_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_INTEGER_VALUE_COUNT,
            legacy_integer_value_count,
        ),
        (
            crate::coverage::DECODED_LEGACY_NAMED_INTEGER_VALUE_COUNT,
            legacy_named_integer_value_count,
        ),
        (
            crate::coverage::DECODED_DEFINITION_SCHEMA_SELECTION_COUNT,
            definition_schema_selection_count,
        ),
        (
            crate::coverage::DECODED_ENTITY_VALUE_FIELD_COUNT,
            entity_value_field_count,
        ),
        (
            crate::coverage::DECODED_ENTITY_VALUE_SCHEMA_SELECTION_COUNT,
            entity_value_schema_selection_count,
        ),
        (
            crate::coverage::DECODED_NUMERIC_ENTITY_VALUE_PACKET_COUNT,
            numeric_entity_value_packet_count,
        ),
        (
            crate::coverage::DECODED_NUMERIC_ENTITY_VALUE_PAIR_COUNT,
            numeric_entity_value_pair_count,
        ),
        (
            crate::coverage::DECODED_REFERENCE_SIGNATURE_COUNT,
            reference_signature_count,
        ),
        (
            crate::coverage::DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_2_COUNT,
            reference_signature_prefix_atom_2_count,
        ),
        (
            crate::coverage::DECODED_REFERENCE_SIGNATURE_PREFIX_ATOM_35_COUNT,
            reference_signature_prefix_atom_35_count,
        ),
        (
            crate::coverage::DECODED_REFERENCE_SIGNATURE_COHORT_COUNT,
            reference_signature_cohort_count,
        ),
        (
            crate::coverage::DECODED_MULTI_MEMBER_REFERENCE_SIGNATURE_COHORT_COUNT,
            multi_member_reference_signature_cohort_count,
        ),
        (
            crate::coverage::DECODED_REFERENCE_SIGNATURE_COHORT_MEMBER_COUNT,
            reference_signature_cohort_member_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_SELECTED_REFERENCE_SIGNATURE_COHORT_COUNT,
            schema_selected_reference_signature_cohort_count,
        ),
        (
            crate::coverage::DECODED_REFERENCE_SIGNATURE_INSTRUCTION_COUNT,
            reference_signature_instruction_count,
        ),
        (
            crate::coverage::DECODED_REFERENCE_SIGNATURE_TOKEN_COUNT,
            reference_signature_token_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT,
            resolved_reference_signature_entity_count,
        ),
        (
            crate::coverage::DECODED_NULL_REFERENCE_SIGNATURE_ENTITY_COUNT,
            null_reference_signature_entity_count,
        ),
        (
            crate::coverage::DECODED_UNRESOLVED_REFERENCE_SIGNATURE_ENTITY_COUNT,
            unresolved_reference_signature_entity_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_REFERENCE_SIGNATURE_ENTITY_COUNT,
            classified_reference_signature_entity_count,
        ),
        (
            crate::coverage::DECODED_COMPACT_ENTITY_VALUE_PACKET_COUNT,
            compact_entity_value_packet_count,
        ),
        (
            crate::coverage::DECODED_LAYOUT_ENTITY_VALUE_PACKET_COUNT,
            layout_entity_value_packet_count,
        ),
        (
            crate::coverage::DECODED_RELATION_EXPRESSION_COUNT,
            relation_expression_count,
        ),
        (
            crate::coverage::DECODED_PLACEHOLDER_STATE_RELATION_EXPRESSION_COUNT,
            placeholder_state_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_PARSER_VERSION_RELATION_EXPRESSION_COUNT,
            parser_version_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_BOOLEAN_PARSER_VERSION_RELATION_EXPRESSION_COUNT,
            boolean_parser_version_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_OPENED_BOOLEAN_PARSER_VERSION_RELATION_EXPRESSION_COUNT,
            opened_boolean_parser_version_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_TYPED_RELATION_EXPRESSION_COUNT,
            typed_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_UNTYPED_RELATION_EXPRESSION_COUNT,
            relation_expression_count - typed_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_REFERENCED_RELATION_EXPRESSION_COUNT,
            referenced_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_FORMULA_REFERENCED_RELATION_EXPRESSION_COUNT,
            formula_referenced_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_PROGRAM_REFERENCED_RELATION_EXPRESSION_COUNT,
            program_referenced_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_RELATION_PROGRAM_INSTANCE_COUNT,
            relation_program_instance_count,
        ),
        (
            crate::coverage::DECODED_RELATION_PROGRAM_OUTPUT_COUNT,
            relation_program_output_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_OUTPUT_COUNT,
            resolved_relation_program_output_count,
        ),
        (
            crate::coverage::DECODED_NULL_RELATION_PROGRAM_OUTPUT_COUNT,
            null_relation_program_output_count,
        ),
        (
            crate::coverage::UNRESOLVED_RELATION_PROGRAM_OUTPUT_COUNT,
            unresolved_relation_program_output_count,
        ),
        (
            crate::coverage::DECODED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
            relation_program_reference_incidence_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
            resolved_relation_program_reference_incidence_count,
        ),
        (
            crate::coverage::DECODED_NULL_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
            null_relation_program_reference_incidence_count,
        ),
        (
            crate::coverage::UNRESOLVED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
            unresolved_relation_program_reference_incidence_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
            classified_relation_program_reference_incidence_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_RELATION_PROGRAM_REFERENCE_INCIDENCE_COUNT,
            relation_program_reference_incidence_count
                - classified_relation_program_reference_incidence_count,
        ),
        (
            crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_INSTANCE_COUNT,
            lead12_relation_program_instance_count,
        ),
        (
            crate::coverage::DECODED_LEAD54_RELATION_PROGRAM_INSTANCE_COUNT,
            lead54_relation_program_instance_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT,
            resolved_lead12_relation_program_context_entity_count,
        ),
        (
            crate::coverage::DECODED_NULL_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT,
            null_lead12_relation_program_context_entity_count,
        ),
        (
            crate::coverage::UNRESOLVED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT,
            unresolved_lead12_relation_program_context_entity_count,
        ),
        (
            crate::coverage::DECODED_LEAD12_RELATION_PROGRAM_PARAMOUT_CONTEXT_ENTITY_COUNT,
            lead12_relation_program_paramout_context_entity_count,
        ),
        (
            crate::coverage::DECODED_OTHER_LEAD12_RELATION_PROGRAM_CONTEXT_CLASS_COUNT,
            other_lead12_relation_program_context_class_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_LEAD12_RELATION_PROGRAM_CONTEXT_ENTITY_COUNT,
            unclassified_lead12_relation_program_context_entity_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT,
            resolved_lead54_relation_program_trailing_entity_count,
        ),
        (
            crate::coverage::DECODED_NULL_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT,
            null_lead54_relation_program_trailing_entity_count,
        ),
        (
            crate::coverage::UNRESOLVED_LEAD54_RELATION_PROGRAM_TRAILING_ENTITY_COUNT,
            unresolved_lead54_relation_program_trailing_entity_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_INSTANCE_COUNT,
            resolved_relation_program_instance_count,
        ),
        (
            crate::coverage::DECODED_NULL_RELATION_PROGRAM_INSTANCE_COUNT,
            null_relation_program_instance_count,
        ),
        (
            crate::coverage::UNRESOLVED_RELATION_PROGRAM_INSTANCE_COUNT,
            unresolved_relation_program_instance_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT,
            resolved_relation_program_repeated_reference_count,
        ),
        (
            crate::coverage::DECODED_NULL_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT,
            null_relation_program_repeated_reference_count,
        ),
        (
            crate::coverage::UNRESOLVED_RELATION_PROGRAM_REPEATED_REFERENCE_COUNT,
            unresolved_relation_program_repeated_reference_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT,
            classified_relation_program_entity_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_RELATION_PROGRAM_ENTITY_COUNT,
            relation_program_instance_count - classified_relation_program_entity_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT,
            classified_relation_program_repeated_entity_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_RELATION_PROGRAM_REPEATED_ENTITY_COUNT,
            relation_program_instance_count - classified_relation_program_repeated_entity_count,
        ),
        (
            crate::coverage::DECODED_RELATION_EXPRESSION_PROGRAM_INSTANCE_COUNT,
            relation_expression_instance_count,
        ),
        (
            crate::coverage::DECODED_OTHER_RELATION_PROGRAM_INSTANCE_COUNT,
            other_relation_program_instance_count,
        ),
        (
            crate::coverage::DECODED_TYPED_RELATION_PROGRAM_INSTANCE_COUNT,
            typed_relation_program_instance_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT,
            resolved_relation_program_input_instance_count,
        ),
        (
            crate::coverage::UNRESOLVED_RELATION_PROGRAM_INPUT_INSTANCE_COUNT,
            unresolved_relation_program_input_instance_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_INPUT_COUNT,
            resolved_relation_program_input_count,
        ),
        (
            crate::coverage::DECODED_DISTINCT_RELATION_PROGRAM_INPUT_ENTITY_COUNT,
            distinct_relation_program_input_entity_count,
        ),
        (
            crate::coverage::DECODED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT,
            relation_program_parameter_dependency_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT,
            resolved_relation_program_parameter_dependency_count,
        ),
        (
            crate::coverage::UNRESOLVED_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT,
            unresolved_relation_program_parameter_dependency_count,
        ),
        (
            crate::coverage::AMBIGUOUS_RELATION_PROGRAM_PARAMETER_DEPENDENCY_COUNT,
            ambiguous_relation_program_parameter_dependency_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_RECORD_COUNT,
            schema_configuration_record_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_SELECTOR_COUNT,
            schema_configuration_record_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
            resolved_schema_configuration_reference_count,
        ),
        (
            crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
            null_schema_configuration_reference_count,
        ),
        (
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
            unresolved_schema_configuration_reference_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
            classified_schema_configuration_entity_reference_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_SCHEMA_CONFIGURATION_ENTITY_REFERENCE_COUNT,
            schema_configuration_record_count
                - classified_schema_configuration_entity_reference_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT,
            schema_configuration_row_link_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT,
            resolved_schema_configuration_row_class_count,
        ),
        (
            crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT,
            null_schema_configuration_row_class_count,
        ),
        (
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CLASS_COUNT,
            schema_configuration_row_link_count
                - resolved_schema_configuration_row_class_count
                - null_schema_configuration_row_class_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT,
            resolved_schema_configuration_row_successor_count,
        ),
        (
            crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT,
            null_schema_configuration_row_successor_count,
        ),
        (
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_SUCCESSOR_COUNT,
            schema_configuration_row_link_count
                - resolved_schema_configuration_row_successor_count
                - null_schema_configuration_row_successor_count,
        ),
        (
            crate::coverage::DECODED_COMPLETE_SCHEMA_CONFIGURATION_ROW_CHAIN_COUNT,
            complete_schema_configuration_row_chain_count,
        ),
        (
            crate::coverage::DECODED_ORDERED_SCHEMA_CONFIGURATION_ROW_LINK_COUNT,
            ordered_schema_configuration_row_link_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT,
            resolved_schema_configuration_row_chain_terminal_count,
        ),
        (
            crate::coverage::DECODED_NULL_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT,
            null_schema_configuration_row_chain_terminal_count,
        ),
        (
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT,
            unresolved_schema_configuration_row_chain_terminal_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT,
            classified_schema_configuration_row_chain_terminal_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_SCHEMA_CONFIGURATION_ROW_CHAIN_TERMINAL_COUNT,
            complete_schema_configuration_row_chain_count
                - classified_schema_configuration_row_chain_terminal_count,
        ),
        (
            crate::coverage::UNRESOLVED_SCHEMA_CONFIGURATION_ROW_ORDER_COUNT,
            unordered_schema_configuration_row_link_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_ENTITY_COUNT,
            schema_configuration_row_intervening_entity_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_SOURCE_INTERVAL_CHAIN_COUNT,
            schema_configuration_row_source_interval_chain_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_CONFIGURATION_ROW_INTERVENING_SCHEMA_CONFIGURATION_COUNT,
            schema_configuration_row_intervening_schema_configuration_count,
        ),
        (
            crate::coverage::DECODED_INSTANCED_RELATION_EXPRESSION_COUNT,
            instanced_relation_expression_count,
        ),
        (
            crate::coverage::UNRESOLVED_UNREFERENCED_RELATION_EXPRESSION_COUNT,
            unreferenced_relation_expression_count,
        ),
        (
            crate::coverage::DECODED_PARAMETER_VALUE_COUNT,
            parameter_value_count,
        ),
        (crate::coverage::DECODED_RANGE_INTERVAL_COUNT, range_interval_count),
        (
            crate::coverage::DECODED_RANGE_INTERVAL_NO_SLOT_COUNT,
            range_interval_no_slot_count,
        ),
        (
            crate::coverage::DECODED_RANGE_INTERVAL_NOMINAL_COUNT,
            range_interval_nominal_count,
        ),
        (
            crate::coverage::DECODED_RANGE_INTERVAL_FINITE_SLOT_COUNT,
            range_interval_finite_slot_count,
        ),
        (
            crate::coverage::DECODED_RANGE_INTERVAL_UNSET_SLOT_COUNT,
            range_interval_unset_slot_count,
        ),
        (
            crate::coverage::DECODED_RANGE_INTERVAL_INCOMING_REFERENCE_COUNT,
            range_interval_incoming_reference_count,
        ),
        (
            crate::coverage::DECODED_RANGE_INTERVAL_INCOMING_PAYLOAD_REFERENCE_COUNT,
            range_interval_incoming_payload_reference_count,
        ),
        (
            crate::coverage::DECODED_RANGE_INTERVAL_INCOMING_STORAGE_REFERENCE_COUNT,
            range_interval_incoming_storage_reference_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_RANGE_INTERVAL_SOURCE_ENTITY_COUNT,
            classified_range_interval_source_entity_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_RANGE_INTERVAL_SOURCE_ENTITY_COUNT,
            range_interval_incoming_reference_count - classified_range_interval_source_entity_count,
        ),
        (
            crate::coverage::UNREFERENCED_RANGE_INTERVAL_COUNT,
            unreferenced_range_interval_count,
        ),
        (
            crate::coverage::UNIQUELY_REFERENCED_RANGE_INTERVAL_COUNT,
            uniquely_referenced_range_interval_count,
        ),
        (
            crate::coverage::MULTIPLY_REFERENCED_RANGE_INTERVAL_COUNT,
            multiply_referenced_range_interval_count,
        ),
        (
            crate::coverage::DECODED_CONSTRAINT_RANGE_COUNT,
            constraint_range_count,
        ),
        (
            crate::coverage::DECODED_DIMENSION_CONSTRAINT_RANGE_COUNT,
            dimension_constraint_range_count,
        ),
        (
            crate::coverage::DECODED_COMPLEX_CONSTRAINT_RANGE_COUNT,
            complex_constraint_range_count,
        ),
        (
            crate::coverage::DECODED_EVALUATED_CONSTRAINT_RANGE_COUNT,
            evaluated_constraint_range_count,
        ),
        (
            crate::coverage::DECODED_UNSET_CONSTRAINT_RANGE_COUNT,
            unset_constraint_range_count,
        ),
        (
            crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_REFERENCE_COUNT,
            constraint_range_incoming_reference_count,
        ),
        (
            crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_PAYLOAD_REFERENCE_COUNT,
            constraint_range_incoming_payload_reference_count,
        ),
        (
            crate::coverage::DECODED_CONSTRAINT_RANGE_INCOMING_STORAGE_REFERENCE_COUNT,
            constraint_range_incoming_storage_reference_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT,
            classified_constraint_range_source_entity_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_CONSTRAINT_RANGE_SOURCE_ENTITY_COUNT,
            constraint_range_incoming_reference_count
                - classified_constraint_range_source_entity_count,
        ),
        (
            crate::coverage::UNREFERENCED_CONSTRAINT_RANGE_COUNT,
            unreferenced_constraint_range_count,
        ),
        (
            crate::coverage::UNIQUELY_REFERENCED_CONSTRAINT_RANGE_COUNT,
            uniquely_referenced_constraint_range_count,
        ),
        (
            crate::coverage::MULTIPLY_REFERENCED_CONSTRAINT_RANGE_COUNT,
            multiply_referenced_constraint_range_count,
        ),
        (
            crate::coverage::DECODED_DEFINITION_VALUE_COUNT,
            definition_value_count,
        ),
        (
            crate::coverage::DECODED_OWNED_DEFINITION_VALUE_COUNT,
            owned_definition_value_count,
        ),
        (
            crate::coverage::UNRESOLVED_DEFINITION_VALUE_OWNER_COUNT,
            unowned_definition_value_count,
        ),
        (
            crate::coverage::DECODED_DEFINITION_CHAIN_VALUE_COUNT,
            definition_chain_value_count,
        ),
        (
            crate::coverage::DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_VALUE_COUNT,
            structurally_owned_definition_chain_value_count,
        ),
        (
            crate::coverage::UNRESOLVED_DEFINITION_CHAIN_VALUE_OWNER_COUNT,
            unowned_definition_chain_value_count,
        ),
        (
            crate::coverage::DECODED_UNASSIGNED_DEFINITION_CHAIN_VALUE_COUNT,
            unassigned_definition_chain_value_count,
        ),
        (
            crate::coverage::DECODED_DEFINITION_CHAIN_EVALUATION_COUNT,
            definition_chain_evaluation_count,
        ),
        (
            crate::coverage::DECODED_EVALUATED_DEFINITION_CHAIN_COUNT,
            evaluated_definition_chain_count,
        ),
        (
            crate::coverage::DECODED_UNSET_DEFINITION_CHAIN_COUNT,
            unset_definition_chain_count,
        ),
        (
            crate::coverage::DECODED_DEFINITION_CHAIN_ATOM_COUNT,
            definition_chain_atom_count,
        ),
        (
            crate::coverage::DECODED_DEFINITION_CHAIN_CONTROL_COUNT,
            definition_chain_control_count,
        ),
        (
            crate::coverage::DECODED_DEFINITION_CHAIN_SEPARATOR_COUNT,
            definition_chain_separator_count,
        ),
        (
            crate::coverage::DECODED_DEFINITION_CHAIN_SCHEMA_SELECTOR_COUNT,
            definition_chain_schema_selector_count,
        ),
        (
            crate::coverage::DECODED_STRUCTURALLY_OWNED_DEFINITION_CHAIN_EVALUATION_COUNT,
            structurally_owned_definition_chain_evaluation_count,
        ),
        (
            crate::coverage::UNRESOLVED_DEFINITION_CHAIN_EVALUATION_OWNER_COUNT,
            unowned_definition_chain_evaluation_count,
        ),
        (
            crate::coverage::DECODED_UNASSIGNED_DEFINITION_CHAIN_EVALUATION_COUNT,
            unassigned_definition_chain_evaluation_count,
        ),
        (
            crate::coverage::DECODED_UNASSIGNED_OBJECT_OWNER_SLOT_COUNT,
            unassigned_owner_slot_count,
        ),
        (
            crate::coverage::DECODED_FORMULA_RELATION_COUNT,
            formula_relation_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_FORMULA_OUTPUT_COUNT,
            resolved_formula_output_count,
        ),
        (
            crate::coverage::DECODED_NULL_FORMULA_OUTPUT_COUNT,
            null_formula_output_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT,
            classified_formula_output_entity_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT,
            formula_relation_count - classified_formula_output_entity_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT,
            classified_formula_expression_entity_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_FORMULA_EXPRESSION_ENTITY_COUNT,
            formula_relation_count - classified_formula_expression_entity_count,
        ),
        (
            crate::coverage::UNRESOLVED_FORMULA_OUTPUT_COUNT,
            unresolved_formula_output_count,
        ),
        (
            crate::coverage::DECODED_FORMULA_PARAMETER_DEPENDENCY_COUNT,
            formula_parameter_dependency_count,
        ),
        (
            crate::coverage::DECODED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT,
            formula_parameter_dependency_candidate_count,
        ),
        (
            crate::coverage::DECODED_CLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT,
            classified_formula_parameter_dependency_candidate_count,
        ),
        (
            crate::coverage::UNCLASSIFIED_FORMULA_PARAMETER_DEPENDENCY_CANDIDATE_COUNT,
            formula_parameter_dependency_candidate_count
                - classified_formula_parameter_dependency_candidate_count,
        ),
        (
            crate::coverage::DECODED_RESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT,
            resolved_formula_parameter_dependency_count,
        ),
        (
            crate::coverage::UNRESOLVED_FORMULA_PARAMETER_DEPENDENCY_COUNT,
            unresolved_formula_parameter_dependency_count,
        ),
        (
            crate::coverage::AMBIGUOUS_FORMULA_PARAMETER_DEPENDENCY_COUNT,
            ambiguous_formula_parameter_dependency_count,
        ),
        (
            crate::coverage::DECODED_ESCAPED_WORD_ENTITY_SUFFIX_COUNT,
            escaped_word_entity_suffix_count,
        ),
        (
            crate::coverage::DECODED_TOKEN_8149_ENTITY_SUFFIX_COUNT,
            token_8149_entity_suffix_count,
        ),
        (
            crate::coverage::DECODED_FIXED_FE_F6_ENTITY_SUFFIX_COUNT,
            fixed_fe_f6_entity_suffix_count,
        ),
        (
            crate::coverage::DECODED_PAGED_ATOM_STATE_01_ENTITY_SUFFIX_COUNT,
            paged_atom_state_01_entity_suffix_count,
        ),
        (
            crate::coverage::DECODED_SCALAR_ENTITY_SUFFIX_VALUE_COUNT,
            scalar_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_UNSET_ENTITY_SUFFIX_VALUE_COUNT,
            unset_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT,
            control_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_CONTROL_E8_ENTITY_SUFFIX_VALUE_COUNT,
            control_e8_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_CONTROL_E9_ENTITY_SUFFIX_VALUE_COUNT,
            control_e9_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT,
            separator_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_ATOM_ENTITY_SUFFIX_VALUE_COUNT,
            atom_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_SELECTED_ATOM_ENTITY_SUFFIX_VALUE_COUNT,
            schema_selected_atom_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_SELECTED_EVALUATION_ENTITY_SUFFIX_VALUE_COUNT,
            schema_selected_evaluation_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_SELECTED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT,
            schema_selected_control_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_SELECTED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT,
            schema_selected_separator_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_SELECTED_SCHEMA_ENTITY_SUFFIX_VALUE_COUNT,
            schema_selected_schema_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_SCHEMA_SELECTED_ENTITY_SUFFIX_VALUE_COUNT,
            schema_selected_entity_suffix_value_count,
        ),
        (
            crate::coverage::DECODED_WIDE_PREFIX_ENTITY_SUFFIX_VALUE_COUNT,
            wide_prefix_entity_suffix_value_count,
        ),
        (
            crate::coverage::UNRESOLVED_DESIGN_OWNER_COUNT,
            unresolved_design_owner_count,
        ),
        (
            crate::coverage::DECODED_VALUE_BLOCK_COUNT,
            native.value_blocks.len(),
        ),
        (crate::coverage::DECODED_VALUE_FIELD_COUNT, value_field_count),
        (
            crate::coverage::DECODED_VALUE_SCHEMA_SELECTION_COUNT,
            value_selection_count,
        ),
        (crate::coverage::TRANSFERRED_FEATURE_COUNT, ir.model.features.len()),
        (
            crate::coverage::TRANSFERRED_FEATURE_PARENT_COUNT,
            transferred_feature_parent_count,
        ),
        (
            crate::coverage::TRANSFERRED_PARAMETER_COUNT,
            ir.model.parameters.len(),
        ),
        (
            crate::coverage::TRANSFERRED_RELATION_PROGRAM_INPUT_PARAMETER_COUNT,
            formula_transfer.relation_program_parameter_count,
        ),
        (
            crate::coverage::TRANSFERRED_LEGACY_PARAMETER_COUNT,
            formula_transfer.legacy_parameter_count,
        ),
        (
            crate::coverage::TRANSFERRED_LEGACY_SELECTOR_PARAMETER_COUNT,
            formula_transfer.legacy_selector_parameter_count,
        ),
        (
            crate::coverage::TRANSFERRED_LEGACY_FORMULA_COUNT,
            formula_transfer.legacy_formula_count,
        ),
        (
            crate::coverage::TRANSFERRED_FORMULA_DESIGN_RECORD_COUNT,
            transferred_formula_design_record_count,
        ),
        (
            crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT,
            formula_transfer.definition_chain_parameter_count,
        ),
        (
            crate::coverage::TRANSFERRED_PRINCIPAL_PLANE_RECORD_COUNT,
            transferred_principal_plane_record_count,
        ),
        (
            crate::coverage::TRANSFERRED_NATIVE_OPERATION_COUNT,
            design_feature_transfer.native_operation_records.len(),
        ),
        (
            crate::coverage::TRANSFERRED_NATIVE_OPERATION_DEFINITION_VALUE_COUNT,
            design_feature_transfer.native_operation_definition_value_count,
        ),
        (
            crate::coverage::TRANSFERRED_NATIVE_OPERATION_DEFINITION_CHAIN_VALUE_COUNT,
            design_feature_transfer.native_operation_definition_chain_value_count,
        ),
        (
            crate::coverage::TRANSFERRED_NATIVE_OPERATION_RANGE_COUNT,
            design_feature_transfer.native_operation_range_count,
        ),
        (
            crate::coverage::TRANSFERRED_NATIVE_OPERATION_PARAMETER_COUNT,
            transferred_native_operation_parameter_count,
        ),
        (
            crate::coverage::UNRESOLVED_DESIGN_RECORD_COUNT,
            unresolved_object_record_count,
        ),
        (crate::coverage::TRANSFERRED_SKETCH_COUNT, ir.model.sketches.len()),
        (
            crate::coverage::TRANSFERRED_SKETCH_ENTITY_COUNT,
            ir.model.sketch_entities.len(),
        ),
        (
            crate::coverage::TRANSFERRED_NATIVE_SKETCH_ENTITY_COUNT,
            transferred_native_sketch_entity_count,
        ),
        (
            crate::coverage::TRANSFERRED_SKETCH_CONSTRAINT_COUNT,
            ir.model.sketch_constraints.len(),
        ),
        (
            crate::coverage::TRANSFERRED_CONFIGURATION_COUNT,
            ir.model.configurations.len(),
        ),
    ] {
        report.coverage.record(ctx, key, count)?;
    }
    if transferred_pmi_dimension_count != 0 {
        report.coverage.record(
            ctx,
            crate::coverage::TRANSFERRED_PMI_DIMENSION_COUNT,
            transferred_pmi_dimension_count,
        )?;
    }
    let untransferred_line_profile_count = native
        .consolidated_line_profiles
        .len()
        .checked_sub(transferred_line_profile_count)
        .map_or(0, |count| count);
    if untransferred_line_profile_count > 0 {
        resource::push_loss(
            ctx,
            &mut report.losses,
            CatiaLossCode::GeometryLineProfileNotTransferred,
            format_args!(
                "{untransferred_line_profile_count} consolidated line-profile record(s) retain \
             exact line geometry but were not transferred by the active geometry route."
            ),
            "catia_report_line_profile_loss",
        )?;
    }
    if !native.zero_entity_support_runs.is_empty()
        || !native.zero_entity_edge_strides.is_empty()
        || !native.zero_entity_oriented_use_pairs.is_empty()
        || !native.zero_entity_vertex_incidences.is_empty()
    {
        let ownership_face_count = native
            .zero_entity_ownership_roots
            .first()
            .map_or(0, |root| root.face_slots.len());
        resource::push_loss(
            ctx,
            &mut report.losses,
            CatiaLossCode::TopologyZeroEntitySupportsRetained,
            format_args!(
                "{} zero-entity surface-support run(s) retain {decoded_zero_entity_support_occurrence_count} face-local \
                 occurrence(s), including {decoded_zero_entity_support_pcurve_count} complete parameter-space \
                 curve(s), {decoded_zero_entity_support_model_curve_count} with exact model-space carriers, \
                 {decoded_zero_entity_support_model_construction_count} with exact procedural model-space carriers, \
                 {decoded_zero_entity_uv_endpoint_pair_count} with exact UV endpoint pairs, and \
                 {decoded_zero_entity_model_endpoint_pair_count} lifted model-space endpoint pairs with \
                 {decoded_zero_entity_model_midpoint_count} bounded-curve midpoint witnesses; \
                 {decoded_zero_entity_face_bound_support_run_count} run(s) bind the complete face roster with {decoded_zero_entity_loop_terminal_count} \
                 ordered loop terminal(s), {decoded_zero_entity_loop_record_count} loop record(s), and \
                 {decoded_zero_entity_oriented_loop_member_count} stored member sense(s), including \
                 {decoded_zero_entity_oriented_model_endpoint_pair_count} sense-oriented model-space endpoint pair(s), \
                 {decoded_zero_entity_bound_support_member_count} member(s) bound to face-local support records and \
                 {decoded_zero_entity_bound_typed_loop_reference_count} typed reference(s) bound to global records; {} \
                 edge-stride allocation tuple(s) remain separate, and \
                 {decoded_zero_entity_vertex_owner_binding_count} of {} vertex-incidence record(s) bind their \
                 adjacent vertex owner; {} ownership root(s) bind {ownership_face_count} face \
                 allocation(s) through a shell and body; {} radial occurrence endpoint-pair \
                 candidate(s) and {} \
                 complete endpoint-locus candidate(s) are established from matching bounded \
                 model-space endpoint and midpoint witnesses; curve coincidence, loop-to-use, \
                 use-to-incidence, physical \
                 endpoint identity remain unresolved; {} oriented-use \
                 pair(s) remain separate.",
                native.zero_entity_support_runs.len(),
                native.zero_entity_edge_strides.len(),
                native.zero_entity_vertex_incidences.len(),
                native.zero_entity_ownership_roots.len(),
                native.zero_entity_endpoint_pair_candidates.len(),
                native.zero_entity_endpoint_locus_candidates.len(),
                native.zero_entity_oriented_use_pairs.len(),
            ),
            "catia_report_zero_entity_loss",
        )?;
    }
    if modeling_scope_is_unresolved {
        resource::push_loss(
            ctx,
            &mut report.losses,
            CatiaLossCode::HistoryModelingScopeUnresolved,
            format_args!(
                "CATIA outer declarations do not unambiguously select one object graph physically \
             contained by the declared CATPrtCont stream; \
             {retained_unscoped_object_graph_count} retained object graph(s) with \
             {retained_unscoped_object_record_count} field record(s) remain outside the \
             modeling scope, and feature, formula, sketch, constraint, configuration, and \
             history authorship remains unresolved."
            ),
            "catia_report_modeling_scope_loss",
        )?;
    }
    if unresolved_object_record_count != 0 {
        resource::push_loss(
            ctx,
            &mut report.losses,
            CatiaLossCode::HistoryObjectRecordsUnresolved,
            format_args!(
            "CATIA native data retains {} design object(s), {design_field_count} grouped field(s), {object_record_count} object-graph field record(s), including {unassigned_owner_slot_count} with an explicit literal unassigned owner slot, {object_record_reference_count} payload reference(s), comprising {resolved_object_record_reference_count} resolved, {null_object_record_reference_count} terminal-null, and {unresolved_object_record_reference_count} unresolved identities, {entity_value_field_count} entity-value field(s), {entity_value_schema_selection_count} entity-value schema selection(s), {numeric_entity_value_pair_count} complete numeric entity-value pair(s), {reference_signature_count} complete reference-signature packet(s) containing {reference_signature_token_count} descriptor token(s) and selecting {resolved_reference_signature_entity_count} resolved, {null_reference_signature_entity_count} terminal-null, and {unresolved_reference_signature_entity_count} unresolved entity incidences, including {classified_reference_signature_entity_count} with a resolved class, {numeric_entity_value_packet_count} embedded numeric entity-value packet(s), {compact_entity_value_packet_count} compact value packet(s), {layout_entity_value_packet_count} layout-bearing value packet(s), {e9_scalar_entity_value_packet_count} E9 scalar packet(s), {escaped_word_entity_suffix_count} escaped-word entity suffix(es), {token_8149_entity_suffix_count} standalone 8149 suffix token(s), {fixed_fe_f6_entity_suffix_count} fixed FE-F6 suffix frame(s), {paged_atom_state_01_entity_suffix_count} paged-atom state-01 suffix(es), {scalar_entity_suffix_value_count} scalar entity-suffix value(s), {unset_entity_suffix_value_count} unset entity-suffix value(s), {atom_entity_suffix_value_count} atom entity-suffix value(s), {separator_entity_suffix_value_count} separator entity-suffix value(s), {schema_selected_atom_entity_suffix_value_count} schema-selected atom value(s), {schema_selected_evaluation_entity_suffix_value_count} schema-selected evaluation(s), {schema_selected_control_entity_suffix_value_count} schema-selected control value(s), {schema_selected_separator_entity_suffix_value_count} schema-selected separator(s), {schema_selected_schema_entity_suffix_value_count} schema-selected schema value(s), {schema_selected_entity_suffix_value_count} suffix value(s) with resolved schema selectors, {wide_prefix_entity_suffix_value_count} suffix value(s) with multi-byte prefix atoms, {control_entity_suffix_value_count} direct control entity-suffix value(s), comprising {control_e8_entity_suffix_value_count} E8 and {control_e9_entity_suffix_value_count} E9 state(s), {relation_expression_count} complete relation expression(s), {relation_program_instance_count} complete compound relation-program instance(s), comprising {lead12_relation_program_instance_count} lead-12 and {lead54_relation_program_instance_count} lead-54 frames, {resolved_relation_program_instance_count} resolved and {unresolved_relation_program_instance_count} unresolved program identities, with {resolved_relation_program_repeated_reference_count} resolved and {unresolved_relation_program_repeated_reference_count} unresolved repeated-reference identities, {resolved_lead12_relation_program_context_entity_count} resolved and {unresolved_lead12_relation_program_context_entity_count} unresolved lead-12 context identities, and {resolved_lead54_relation_program_trailing_entity_count} resolved and {unresolved_lead54_relation_program_trailing_entity_count} unresolved lead-54 trailing identities; {relation_expression_instance_count} select relation-expression programs, {other_relation_program_instance_count} select other resolved entities, and those relation-expression instances select {instanced_relation_expression_count} distinct expression entity or entities and retain {relation_program_parameter_dependency_count} parameter symbol occurrence(s), comprising {resolved_relation_program_parameter_dependency_count} uniquely resolved and {unresolved_relation_program_parameter_dependency_count} unresolved, including {ambiguous_relation_program_parameter_dependency_count} with multiple candidates; {typed_relation_program_instance_count} typed program instance(s) comprise {resolved_relation_program_input_instance_count} with complete ordered inputs and {unresolved_relation_program_input_instance_count} with incomplete input binding, retaining {resolved_relation_program_input_count} resolved input occurrence(s) selecting {distinct_relation_program_input_entity_count} distinct entity identity or identities; {schema_configuration_record_count} complete schema-configuration Configuration record(s) retain {resolved_schema_configuration_reference_count} resolved, {null_schema_configuration_reference_count} terminal-null, and {unresolved_schema_configuration_reference_count} unresolved reference identities; {schema_configuration_row_link_count} complete configrow link(s) retain {resolved_schema_configuration_row_class_count} resolved and {null_schema_configuration_row_class_count} terminal-null class identities plus {resolved_schema_configuration_row_successor_count} resolved and {null_schema_configuration_row_successor_count} terminal-null successor identities, with {ordered_schema_configuration_row_link_count} row link(s) in {complete_schema_configuration_row_chain_count} complete chain(s), comprising {resolved_schema_configuration_row_chain_terminal_count} resolved, {null_schema_configuration_row_chain_terminal_count} terminal-null, and {unresolved_schema_configuration_row_chain_terminal_count} unresolved terminals; {schema_configuration_row_source_interval_chain_count} source-ordered chain(s) retain {schema_configuration_row_intervening_entity_count} entity or entities from the open intervals between rows and successors, including {schema_configuration_row_intervening_schema_configuration_count} complete schema-configuration Configuration record(s), while {unordered_schema_configuration_row_link_count} row link(s) have unresolved order; {parameter_value_count} complete named parameter value(s), {range_interval_count} complete source-schema Range interval(s), comprising {range_interval_no_slot_count} no-slot production(s), {range_interval_nominal_count} finite nominal(s), {range_interval_finite_slot_count} finite deviation slot(s), and {range_interval_unset_slot_count} unset deviation slot(s), {constraint_range_count} complete constraint-range value(s), comprising {dimension_constraint_range_count} dimension and {complex_constraint_range_count} complex-constraint range(s), with {evaluated_constraint_range_count} finite evaluation(s) and {unset_constraint_range_count} unset evaluation(s), {definition_value_count} definition-bound suffix value(s), including {owned_definition_value_count} assigned to design objects and {unowned_definition_value_count} without a resolved owner, {definition_chain_evaluation_count} two-definition chain evaluation(s), comprising {evaluated_definition_chain_count} finite and {unset_definition_chain_count} unset value(s), with {structurally_owned_definition_chain_evaluation_count} structurally owned and {unowned_definition_chain_evaluation_count} without a resolved structural owner; {unassigned_definition_chain_value_count} chain value(s), including {unassigned_definition_chain_evaluation_count} evaluation(s), occupy explicit literal unassigned owner slots; {formula_relation_count} complete formula relation(s), comprising {resolved_formula_output_count} resolved, {null_formula_output_count} terminal-null, and {unresolved_formula_output_count} unresolved output identities, {formula_parameter_dependency_count} formula parameter symbol occurrence(s), comprising {resolved_formula_parameter_dependency_count} uniquely resolved and {unresolved_formula_parameter_dependency_count} unresolved, including {ambiguous_formula_parameter_dependency_count} with multiple candidates, {repeated_reference_suffix_count} repeated-reference suffix(es), {repeated_reference_schema_selection_count} repeated-reference schema selection(s), {definition_schema_selection_count} definition-schema selection(s), {design_object_owner_link_count} structural owner link(s), and {design_object_relation_count} exact outbound design-field relation occurrence(s), including {design_same_object_relation_count} within one design object, {design_reflexive_field_relation_count} reflexive field occurrence(s), and {design_unowned_field_relation_count} to fields without owner groups; {classified_design_object_count} design object(s) have class evidence and {unresolved_design_owner_count} owner identity or identities remain unresolved; {} typed parameter(s), including {} selected through complete relation-program inputs, {} exact formula, expression, or parameter field record(s), and {} exact principal-plane field record(s) transferred, while {unresolved_object_record_count} modeling-scope field record(s) across {unresolved_design_object_count} design object(s), neutral features with unresolved semantics, other parameters, sketch placement, geometry, profiles, constraints, configurations, and re-derivable history remain unresolved; {} sketch identity record(s) transfer.",
            native.design_objects.len(),
            formula_transfer.typed_parameter_count,
            formula_transfer.relation_program_parameter_count,
            transferred_formula_design_record_count,
            transferred_principal_plane_record_count,
            ir.model.sketches.len(),
        ),
            "catia_report_history_objects_loss",
        )?;
    }
    if !native.legacy_entity_runs.is_empty() {
        resource::push_loss(
            ctx,
            &mut report.losses,
            CatiaLossCode::HistoryLegacyRunsUnresolved,
            format_args!(
            "CATIA native data retains {} legacy design run(s) with {legacy_schema_program_count} complete compact schema program(s), containing {legacy_schema_identifier_count} complete identifier packet(s), and {legacy_entity_identity_count} source-ordered entity identity marker(s), comprising {legacy_identity_lead_81_count} lead-81, {legacy_identity_lead_82_count} lead-82, {legacy_identity_lead_e5_count} lead-E5, and {legacy_identity_lead_fd_count} lead-FD record(s), {legacy_role_selector_count} complete schema role selector(s), including {legacy_selected_role_count} unresolved schema-selected role name(s) and {legacy_role_field_binding_count} immediate schema-field binding(s), {legacy_schema_field_count} complete role-bounded schema field(s), {legacy_text_field_count} complete schema text field(s), including {legacy_e3_role_tail_text_field_count} with E3 paged-role tails and {legacy_role_text_field_count} role-bound text field(s), {legacy_relation_count} typed expression/signature pair(s), including {legacy_parameter_relation_count} with exact parameter identities, {legacy_synchronous_state_count} relation update-state field(s), comprising {legacy_synchronous_relation_count} synchronous and {legacy_asynchronous_relation_count} asynchronous state(s), {legacy_type_descriptor_count} type descriptor(s), including {legacy_literal_type_descriptor_count} literal name(s), {legacy_scalar_value_count} typed scalar evaluation(s), including {legacy_named_scalar_value_count} named scalar(s), {legacy_string_value_count} string value(s), including {legacy_named_string_value_count} named string(s), and {legacy_integer_value_count} signed integer value(s), including {legacy_named_integer_value_count} named integer(s); {} uniquely named, literal-typed parameter(s), including {} resolved through descriptor selectors, and {} local-input legacy formula(s) transferred, while remaining selector semantics, unbound relation ownership and parameters, unresolved selector types, feature semantics, and feature history remain unresolved.",
            native.legacy_entity_runs.len(),
            formula_transfer.legacy_parameter_count,
            formula_transfer.legacy_selector_parameter_count,
            formula_transfer.legacy_formula_count,
        ),
            "catia_report_legacy_loss",
        )?;
    }
    if unresolved_dimension_quantity_count != 0 {
        resource::push_loss(
            ctx,
            &mut report.losses,
            CatiaLossCode::AttributesDimensionQuantityUnresolved,
            format_args!(
                "{unresolved_dimension_quantity_count} finite `Range`/`CstAttr_Dimension` \
                 scalar production(s) remain native because the admitted selectors, suffix \
                 framing, interval, and owner incidences do not assign a physical quantity."
            ),
            "catia_report_dimension_loss",
        )?;
    }
    if !native.value_blocks.is_empty() {
        resource::push_loss(
            ctx,
            &mut report.losses,
            CatiaLossCode::AttributesVisualizationUnbound,
            format_args!(
                "CATIA native data retains {} visualization value block(s), {value_field_count} encoded field(s), and {value_selection_count} schema-selected presentation value(s); {} display-color packet(s) remain without a proven typed face or body target ({} packet(s) transferred), while other visualization fields remain native.",
                native.value_blocks.len(),
                appearance_transfer.unresolved_packets(),
                appearance_transfer.transferred_packets(),
            ),
            "catia_report_visualization_loss",
        )?;
    }
    native.store_owned(ctx, ir.native.namespace_mut("catia"))?;
    decode_result(ctx, scan, matched, ir, report, annotations, unknowns)
}

/// Modeling scope of a part's decoded object graphs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelingGraphScope {
    /// No outer container declarations: every object graph is in modeling scope.
    Unscoped,
    /// Outer declarations exist, but no single part graph is resolvable.
    Unresolved,
    /// The uniquely resolved part object graph.
    Scoped(String),
}

impl ModelingGraphScope {
    /// Reports whether an object graph identity is inside the modeling scope.
    pub(crate) fn contains(&self, graph: &str) -> bool {
        match self {
            Self::Unscoped => true,
            Self::Unresolved => false,
            Self::Scoped(part) => part == graph,
        }
    }

    /// Reports whether no outer container declaration bounds the modeling scope.
    pub(crate) fn is_unscoped(&self) -> bool {
        matches!(self, Self::Unscoped)
    }

    /// Returns the number of scoped graphs among `decoded` decoded graphs.
    fn graph_count(&self, decoded: usize) -> usize {
        match self {
            Self::Unscoped => decoded,
            Self::Unresolved => 0,
            Self::Scoped(_) => 1,
        }
    }
}

fn modeling_graph_scope(
    ctx: &DecodeContext<'_>,
    has_outer_declarations: bool,
    graphs: &[CatiaObjectGraph],
) -> Result<ModelingGraphScope, CodecError> {
    if !has_outer_declarations {
        return Ok(ModelingGraphScope::Unscoped);
    }
    let mut remaining = graphs.iter();
    let is_part = |graph: &&CatiaObjectGraph| match &graph.outer_container {
        Some(container) => ctx.equal_bytes(
            container.class_name.as_bytes(),
            b"CATPrtCont",
            "catia_modeling_scope_class",
        ),
        None => Ok(false),
    };
    let first = ctx.find_by(&mut remaining, is_part, "catia_modeling_scope_search")?;
    let Some(graph) = first else {
        return Ok(ModelingGraphScope::Unresolved);
    };
    if ctx
        .find_by(&mut remaining, is_part, "catia_modeling_scope_search")?
        .is_some()
    {
        return Ok(ModelingGraphScope::Unresolved);
    }
    Ok(ModelingGraphScope::Scoped(ctx.copy_retained_text(
        &graph.id,
        "catia_modeling_scope_graph",
    )?))
}

/// The single site that finishes a decode and charges dialect admission loss.
///
/// Identity is authored once from the match classified at the decode entry;
/// the sealed wrapper stamps it onto the report. This function merges the
/// container-level notes and charges dialect loss from that same match.
fn decode_result(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    matched: &DialectMatch,
    mut ir: CadIr,
    mut body: DecodeBody,
    annotations: Annotations,
    unknowns: Vec<UnknownRecord>,
) -> Result<Decoded, CodecError> {
    ir.source = Some(crate::assemble::source_meta(ctx, scan, matched)?);
    body.notes = crate::container::notes(ctx, scan)?;
    if let Some(loss) = crate::dialect::dialect_loss(ctx, matched)? {
        ctx.push_vec(&mut body.losses, loss, "catia_decode_dialect_loss")?;
    }
    let mut source_fidelity = SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "catia", unknowns, ctx)?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

#[cfg(test)]
mod tests;
