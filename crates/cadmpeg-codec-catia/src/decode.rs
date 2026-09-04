// SPDX-License-Identifier: Apache-2.0
//! High-level CATPart-to-IR decoding.
//!
//! [`decode`] scans the container, selects a decoder from the identified storage
//! variant, and returns the transferred model with its [`DecodeBody`]; the
//! sealed wrapper stamps the identity authored in `ir.source` onto the report.
//!
//! Partial paths preserve the reconstructed B-rep stream or complete file as an
//! [`UnknownRecord`]. Their report identifies unresolved model layers.

use std::collections::{HashMap, HashSet};

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
use crate::sketch;

fn schema_configuration_row_chain_coverage(native: &CatiaNative) -> (usize, usize) {
    (
        native.schema_configuration_row_chains.len(),
        native
            .schema_configuration_row_chains
            .iter()
            .map(|chain| chain.links.len())
            .sum(),
    )
}

/// Decodes a `.CATPart` reader into an IR document and decode report.
///
/// When [`DecodeOptions::container_only`] is set, the result contains source
/// metadata and container diagnostics without entity decoding.
///
/// Otherwise each route in [`crate::families::ROUTES`] whose applicability
/// predicate accepts the scanned variant is tried in table order; the first to
/// return a model wins, a `None` falls through to the next applicable route, and
/// exhausting the table yields the metadata-only fallback.
pub fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let scan = container::scan_bytes(root.window());
    let matched = crate::dialect::classify(&scan);

    if ctx.container_only() {
        let (ir, annotations, unknowns) = build_metadata_fallback(&scan);
        let report = build_container_report(&scan);
        return decode_result(&scan, &matched, ir, report, annotations, unknowns);
    }

    for route in families::ROUTES {
        if (route.applicable)(scan.variant) {
            if let Some(out) = (route.decode)(ctx, &scan) {
                return finish_decode(
                    ctx,
                    &scan,
                    &matched,
                    out.ir,
                    out.report,
                    out.annotations,
                    out.unknowns,
                    out.standard_face_population,
                );
            }
        }
    }

    let (ir, annotations, unknowns) = build_metadata_fallback(&scan);
    let report = build_container_report(&scan);
    finish_decode(
        ctx,
        &scan,
        &matched,
        ir,
        report,
        annotations,
        unknowns,
        false,
    )
}

#[derive(Default)]
struct IncomingEntityIncidenceCounts {
    total: usize,
    payload: usize,
    storage: usize,
    classified: usize,
    zero: usize,
    one: usize,
    multiple: usize,
}

fn incoming_entity_incidence_counts<'a>(
    incidences: impl Iterator<
        Item = (
            &'a [crate::native::CatiaEntityIncomingReference],
            &'a [crate::native::CatiaEntityIncomingStorageReference],
        ),
    >,
) -> IncomingEntityIncidenceCounts {
    let mut counts = IncomingEntityIncidenceCounts::default();
    for (payload_references, storage_references) in incidences {
        let payload_count = payload_references.len();
        let storage_count = storage_references.len();
        let total = payload_count + storage_count;
        counts.total += total;
        counts.payload += payload_count;
        counts.storage += storage_count;
        counts.classified += payload_references
            .iter()
            .filter_map(|reference| reference.source_entity.as_ref())
            .chain(
                storage_references
                    .iter()
                    .filter_map(|reference| reference.source_entity.as_ref()),
            )
            .filter(|entity| entity.class_name.is_some())
            .count();
        counts.zero += usize::from(total == 0);
        counts.one += usize::from(total == 1);
        counts.multiple += usize::from(total > 1);
    }
    counts
}

// Keep the single classified match explicit beside the independently built decode artifacts.
#[allow(clippy::too_many_arguments)]
fn finish_decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    matched: &DialectMatch,
    mut ir: CadIr,
    mut report: DecodeBody,
    mut annotations: Annotations,
    unknowns: Vec<UnknownRecord>,
    standard_face_population: bool,
) -> Result<Decoded, CodecError> {
    // Retained unknown records are source entities even when a route transfers
    // no neutral model entity (for example, an unrecognized storage variant).
    ctx.charge_entities(unknowns.len() as u64, "admit CATIA retained source records")?;
    // Charge route-built entities before native decode and transfer work so
    // max_entities refuses that work rather than only reporting afterward.
    let mut admitted_entities = 0_u64;
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        &mut admitted_entities,
        "admit CATIA route entities",
    )?;
    let consolidated_record_sources = container::consolidated_record_sources(scan);
    let native = CatiaNative::decode_with_record_sources(&scan.data, &consolidated_record_sources);
    let modeling_graph_scope = modeling_graph_scope(
        !scan.outer_container_declarations.is_empty(),
        &native.object_graphs,
    );
    let modeling_object_records = native
        .object_graphs
        .iter()
        .filter(|graph| {
            modeling_graph_scope
                .as_ref()
                .is_none_or(|scope| scope.contains(graph.id.as_str()))
        })
        .flat_map(|graph| graph.records.iter().map(|record| record.id.clone()))
        .collect::<HashSet<_>>();
    let design_feature_transfer =
        design_feature::transfer_design_features(&mut ir, &native, modeling_graph_scope.as_ref());
    let transferred_native_sketch_entity_records = sketch::transfer_native_sketch_entities(
        &mut ir,
        &native,
        &design_feature_transfer,
        modeling_graph_scope.as_ref(),
    );
    let transferred_native_sketch_constraint_records = sketch::transfer_native_sketch_constraints(
        &mut ir,
        &native,
        &design_feature_transfer,
        modeling_graph_scope.as_ref(),
    );
    let transferred_constraint_range_records = sketch::transfer_constraint_ranges(
        &mut ir,
        &native,
        &design_feature_transfer,
        modeling_graph_scope.as_ref(),
    );
    let transferred_pmi_dimension_count = pmi::transfer_dimensions(
        &mut ir,
        &native,
        modeling_graph_scope.as_ref(),
        &transferred_constraint_range_records,
    );
    let formula_transfer = formula::transfer_parameters(
        &mut ir,
        &native,
        &mut annotations,
        modeling_graph_scope.as_ref(),
    );
    design_feature_transfer.assign_parameter_owners(&mut ir, &native);
    let appearance_transfer = crate::appearance::transfer(
        &mut ir,
        &native,
        modeling_graph_scope.as_ref(),
        standard_face_population
            .then_some(scan.main_data_stream.as_deref().or(scan.brep.as_deref()))
            .flatten(),
    );
    let object_record_count: usize = native
        .object_graphs
        .iter()
        .map(|graph| graph.records.len())
        .sum();
    let modeling_scope_is_unresolved = modeling_graph_scope.as_ref().is_some_and(HashSet::is_empty);
    let retained_unscoped_object_graph_count = modeling_graph_scope.as_ref().map_or(0, |scope| {
        native
            .object_graphs
            .iter()
            .filter(|graph| !scope.contains(graph.id.as_str()))
            .count()
    });
    let retained_unscoped_object_record_count = modeling_graph_scope.as_ref().map_or(0, |scope| {
        native
            .object_graphs
            .iter()
            .filter(|graph| !scope.contains(graph.id.as_str()))
            .map(|graph| graph.records.len())
            .sum()
    });
    let resolved_storage_record_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .filter(|record| record.storage_record.is_some())
        .count();
    let unresolved_storage_record_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .filter(|record| {
            record
                .storage_ref
                .is_some_and(|storage_ref| storage_ref != 0)
                && record.storage_record.is_none()
        })
        .count();
    let object_record_reference_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| record.references.len())
        .sum::<usize>();
    let resolved_object_record_reference_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .flat_map(|record| &record.references)
        .filter(|reference| reference.target.is_some())
        .count();
    let null_object_record_reference_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .flat_map(|record| &record.references)
        .filter(|reference| reference.is_null)
        .count();
    let unresolved_object_record_reference_count = object_record_reference_count
        - resolved_object_record_reference_count
        - null_object_record_reference_count;
    let repeated_reference_suffix_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .filter(|record| record.repeated_reference_suffix.is_some())
        .count();
    let repeated_reference_schema_selection_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .filter(|record| record.repeated_reference_schema_selection.is_some())
        .count();
    let design_field_count = native
        .design_objects
        .iter()
        .map(|object| object.fields.len())
        .sum();
    let classified_design_object_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_class.is_some() || !object.field_classes.is_empty())
        .count();
    let design_object_relation_count = native
        .design_objects
        .iter()
        .map(|object| object.relations.len())
        .sum();
    let design_parallel_reference_table_count = native
        .design_objects
        .iter()
        .filter(|object| object.parallel_reference_table.is_some())
        .count();
    let design_parallel_reference_row_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .map(|table| table.rows.len())
        .sum();
    let design_parallel_reference_column_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .map(|table| table.columns.len())
        .sum();
    let design_parallel_reference_cell_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.rows)
        .map(|row| row.cells.len())
        .sum();
    let design_parallel_reference_resolved_cell_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.rows)
        .flat_map(|row| &row.cells)
        .filter(|cell| cell.field.is_some())
        .count();
    let design_parallel_reference_null_cell_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.rows)
        .flat_map(|row| &row.cells)
        .filter(|cell| cell.is_null)
        .count();
    let design_parallel_reference_classified_cell_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.rows)
        .flat_map(|row| &row.cells)
        .filter(|cell| cell.field_class.is_some())
        .count();
    let design_parallel_reference_classified_column_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.columns)
        .filter(|column| column.field_class.is_some())
        .count();
    let design_parallel_reference_unclassified_column_count =
        design_parallel_reference_column_count - design_parallel_reference_classified_column_count;
    let design_parallel_reference_unresolved_cell_count = design_parallel_reference_cell_count
        - design_parallel_reference_resolved_cell_count
        - design_parallel_reference_null_cell_count;
    let design_parallel_reference_unclassified_cell_count =
        design_parallel_reference_cell_count - design_parallel_reference_classified_cell_count;
    let design_parallel_reference_matched_row_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.rows)
        .filter(|row| row.matching_design_object.is_some())
        .count();
    let design_parallel_reference_unmatched_row_count =
        design_parallel_reference_row_count - design_parallel_reference_matched_row_count;
    let design_unowned_field_relation_count = native
        .design_objects
        .iter()
        .flat_map(|object| &object.relations)
        .filter(|relation| relation.target_design_object.is_none())
        .count();
    let design_same_object_relation_count = native
        .design_objects
        .iter()
        .map(|object| {
            object
                .relations
                .iter()
                .filter(|relation| {
                    relation.target_design_object.as_deref() == Some(object.id.as_str())
                })
                .count()
        })
        .sum();
    let design_reflexive_field_relation_count = native
        .design_objects
        .iter()
        .flat_map(|object| &object.relations)
        .filter(|relation| relation.source_field == relation.target_field)
        .count();
    let design_object_owner_link_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_design_object.is_some())
        .count();
    let legacy_entity_identity_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.identities.len())
        .sum();
    let legacy_schema_program_count = native
        .legacy_entity_runs
        .iter()
        .filter(|run| run.schema_program.is_some())
        .count();
    let legacy_vendor_footer_schema_program_count = native
        .legacy_entity_runs
        .iter()
        .filter_map(|run| run.schema_program.as_ref())
        .filter(|program| {
            program.boundary == crate::native::CatiaLegacySchemaProgramBoundary::VendorFooter
        })
        .count();
    let legacy_directory_bound_schema_program_count = native
        .legacy_entity_runs
        .iter()
        .filter_map(|run| run.schema_program.as_ref())
        .filter(|program| {
            program.boundary == crate::native::CatiaLegacySchemaProgramBoundary::StreamDirectory
        })
        .count();
    let legacy_schema_identifier_count = native
        .legacy_entity_runs
        .iter()
        .filter_map(|run| run.schema_program.as_ref())
        .map(|program| program.identifiers.len())
        .sum();
    let legacy_evaluated_value_name_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| {
            let evaluation_name_bound = |entity_id, value_offset| {
                run.role_selectors.iter().any(|role| {
                    role.entity_id == entity_id
                        && role.field_code == Some(0x17c4)
                        && role.end_offset().and_then(|offset| offset.checked_add(6))
                            == Some(value_offset)
                })
            };
            run.scalar_values
                .iter()
                .filter(|value| {
                    value.name.is_some()
                        && evaluation_name_bound(value.entity_id, value.byte_offset)
                })
                .count()
                + run
                    .string_values
                    .iter()
                    .filter(|value| {
                        value.name.is_some()
                            && evaluation_name_bound(value.entity_id, value.byte_offset)
                    })
                    .count()
                + run
                    .integer_values
                    .iter()
                    .filter(|value| {
                        value.name.is_some()
                            && evaluation_name_bound(value.entity_id, value.byte_offset)
                    })
                    .count()
        })
        .sum();
    let (
        legacy_identity_lead_81_count,
        legacy_identity_lead_82_count,
        legacy_identity_lead_e5_count,
        legacy_identity_lead_fd_count,
    ) = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.identities)
        .fold(
            (0, 0, 0, 0),
            |(lead_81, lead_82, lead_e5, lead_fd), identity| match identity.lead {
                0x81 => (lead_81 + 1, lead_82, lead_e5, lead_fd),
                0x82 => (lead_81, lead_82 + 1, lead_e5, lead_fd),
                0xe5 => (lead_81, lead_82, lead_e5 + 1, lead_fd),
                0xfd => (lead_81, lead_82, lead_e5, lead_fd + 1),
                _ => unreachable!("validated legacy identity lead"),
            },
        );
    let legacy_text_field_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.text_fields.len())
        .sum();
    let legacy_e3_role_tail_text_field_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.text_fields)
        .filter(|field| {
            field.encoding == crate::native::CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
        })
        .count();
    let legacy_role_selector_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.role_selectors.len())
        .sum();
    let legacy_selected_role_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.role_selectors)
        .filter(|role| matches!(&role.name, crate::native::CatiaLegacyRoleName::Selector(_)))
        .count();
    let legacy_role_field_binding_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.role_selectors)
        .filter(|role| role.field_code.is_some())
        .count();
    let legacy_role_text_field_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.text_fields)
        .filter(|field| field.role.is_some())
        .count();
    let legacy_schema_field_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.schema_fields.len())
        .sum();
    let legacy_relation_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.relations.len())
        .sum();
    let legacy_parameter_relation_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.relations)
        .filter(|relation| relation.parameter_entity_id.is_some())
        .count();
    let legacy_synchronous_state_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.synchronous_states.len())
        .sum();
    let legacy_synchronous_relation_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.synchronous_states)
        .filter(|state| state.synchronous)
        .count();
    let legacy_asynchronous_relation_count =
        legacy_synchronous_state_count - legacy_synchronous_relation_count;
    let legacy_type_descriptor_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.type_descriptors.len())
        .sum();
    let legacy_literal_type_descriptor_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.type_descriptors)
        .filter(|descriptor| {
            matches!(
                &descriptor.value,
                crate::native::CatiaLegacyTypeValue::Name { .. }
            )
        })
        .count();
    let legacy_scalar_value_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.scalar_values.len())
        .sum();
    let legacy_named_scalar_value_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.scalar_values)
        .filter(|value| value.name.is_some())
        .count();
    let legacy_string_value_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.string_values.len())
        .sum();
    let legacy_named_string_value_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.string_values)
        .filter(|value| value.name.is_some())
        .count();
    let legacy_integer_value_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.integer_values.len())
        .sum();
    let legacy_named_integer_value_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.integer_values)
        .filter(|value| value.name.is_some())
        .count();
    let definition_schema_selection_count = native
        .entity_records
        .iter()
        .map(|record| record.definition_schema_selections.len())
        .sum();
    let entity_value_field_count = native
        .entity_records
        .iter()
        .map(|record| record.value_fields.len())
        .sum();
    let entity_value_schema_selection_count = native
        .entity_records
        .iter()
        .map(|record| record.value_schema_selections.len())
        .sum();
    let compact_entity_value_packet_count = native
        .entity_records
        .iter()
        .flat_map(|record| &record.value_packets)
        .filter(|packet| matches!(packet, entity_table::EntityValuePacket::Compact { .. }))
        .count();
    let numeric_entity_value_packet_count = native
        .entity_records
        .iter()
        .flat_map(|record| &record.value_packets)
        .filter(|packet| matches!(packet, entity_table::EntityValuePacket::Numeric { .. }))
        .count();
    let numeric_entity_value_pair_count = native
        .entity_records
        .iter()
        .filter(|record| record.numeric_pair.is_some())
        .count();
    let reference_signature_count = native
        .entity_records
        .iter()
        .filter(|record| record.reference_signature.is_some())
        .count();
    let reference_signature_prefix_atom_2_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.reference_signature.as_ref())
        .filter(|signature| {
            signature.production.prefix == entity_table::ReferenceSignaturePrefix::Atom2
        })
        .count();
    let reference_signature_prefix_atom_35_count =
        reference_signature_count - reference_signature_prefix_atom_2_count;
    let reference_signature_cohort_count = native.reference_signature_cohorts.len();
    let multi_member_reference_signature_cohort_count = native
        .reference_signature_cohorts
        .iter()
        .filter(|cohort| cohort.members.len() > 1)
        .count();
    let reference_signature_cohort_member_count = native
        .reference_signature_cohorts
        .iter()
        .map(|cohort| cohort.members.len())
        .sum();
    let schema_selected_reference_signature_cohort_count = native
        .reference_signature_cohorts
        .iter()
        .filter(|cohort| cohort.schema_selection.is_some())
        .count();
    let (reference_signature_instruction_count, reference_signature_token_count) = native
        .entity_records
        .iter()
        .filter_map(|record| record.reference_signature.as_ref())
        .fold((0_usize, 0_usize), |(instructions, tokens), signature| {
            let program = &signature.production.signature_program;
            let qualifier_count = program
                .iter()
                .filter(|instruction| {
                    matches!(
                        instruction,
                        crate::entity_table::ReferenceSignatureInstruction::Qualifier { .. }
                    )
                })
                .count();
            (
                instructions + program.len(),
                tokens + program.len() + qualifier_count,
            )
        });
    let (
        resolved_reference_signature_entity_count,
        null_reference_signature_entity_count,
        unresolved_reference_signature_entity_count,
        classified_reference_signature_entity_count,
    ) = native
        .entity_records
        .iter()
        .filter_map(|record| record.reference_signature.as_ref())
        .flat_map(|signature| [&signature.first_entity, &signature.second_entity])
        .fold(
            (0_usize, 0_usize, 0_usize, 0_usize),
            |(resolved, null, unresolved, classified), reference| {
                let classified = classified + usize::from(reference.class_name.is_some());
                if reference.is_null {
                    (resolved, null + 1, unresolved, classified)
                } else if reference.entity.is_some() {
                    (resolved + 1, null, unresolved, classified)
                } else {
                    (resolved, null, unresolved + 1, classified)
                }
            },
        );
    let consolidated_edge_run_count = native.consolidated_edge_runs.len();
    let consolidated_edge_run_support_binding_count = native
        .consolidated_edge_runs
        .iter()
        .flat_map(|run| &run.support_bindings)
        .filter(|binding| binding.is_some())
        .count();
    let (
        unresolved_consolidated_edge_run_count,
        partially_resolved_consolidated_edge_run_count,
        fully_resolved_consolidated_edge_run_count,
    ) = native.consolidated_edge_runs.iter().fold(
        (0_usize, 0_usize, 0_usize),
        |(unresolved, partial, full), run| match run
            .support_bindings
            .iter()
            .filter(|binding| binding.is_some())
            .count()
        {
            0 => (unresolved + 1, partial, full),
            1 => (unresolved, partial + 1, full),
            2 => (unresolved, partial, full + 1),
            _ => unreachable!("a consolidated edge run has exactly two support sides"),
        },
    );
    let consolidated_edge_run_shared_locus_count = native
        .consolidated_edge_runs
        .iter()
        .filter(|run| run.shared_loci.is_some())
        .count();
    let consolidated_edge_run_endpoint_locus_count = native
        .consolidated_edge_runs
        .iter()
        .filter(|run| run.endpoint_loci.is_some())
        .count();
    let layout_entity_value_packet_count = native
        .entity_records
        .iter()
        .flat_map(|record| &record.value_packets)
        .filter(|packet| matches!(packet, entity_table::EntityValuePacket::Layout { .. }))
        .count();
    let e9_scalar_entity_value_packet_count = native
        .entity_records
        .iter()
        .flat_map(|record| &record.value_packets)
        .filter(|packet| matches!(packet, entity_table::EntityValuePacket::E9Scalar { .. }))
        .count();
    let (
        relation_expression_count,
        placeholder_state_relation_expression_count,
        parser_version_relation_expression_count,
        boolean_parser_version_relation_expression_count,
        opened_boolean_parser_version_relation_expression_count,
        typed_relation_expression_count,
    ) = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_expression.as_ref())
        .fold(
            (0_usize, 0_usize, 0_usize, 0_usize, 0_usize, 0_usize),
            |(total, placeholder, parser, boolean, opened, typed), expression| {
                let (placeholder, parser, boolean, opened) = match expression.framing {
                    crate::native::CatiaRelationExpressionFraming::PlaceholderState { .. } => {
                        (placeholder + 1, parser, boolean, opened)
                    }
                    crate::native::CatiaRelationExpressionFraming::ParserVersion { .. } => {
                        (placeholder, parser + 1, boolean, opened)
                    }
                    crate::native::CatiaRelationExpressionFraming::BooleanParserVersion {
                        ..
                    } => (placeholder, parser, boolean + 1, opened),
                    crate::native::CatiaRelationExpressionFraming::OpenedBooleanParserVersion {
                        ..
                    } => (placeholder, parser, boolean, opened + 1),
                };
                (
                    total + 1,
                    placeholder,
                    parser,
                    boolean,
                    opened,
                    typed + usize::from(expression.signature.is_some()),
                )
            },
        );
    let parameter_value_count = native
        .entity_records
        .iter()
        .filter(|record| record.parameter_value.is_some())
        .count();
    let (
        range_interval_count,
        range_interval_no_slot_count,
        range_interval_nominal_count,
        range_interval_finite_slot_count,
        range_interval_unset_slot_count,
    ) = native
        .entity_records
        .iter()
        .filter_map(|record| record.range_interval.as_ref())
        .fold(
            (0_usize, 0_usize, 0_usize, 0_usize, 0_usize),
            |(total, no_slot, nominal, finite, unset), range| {
                let nominal = nominal + usize::from(range.nominal.is_some());
                let Some(slots) = &range.interval.slots else {
                    return (total + 1, no_slot + 1, nominal, finite, unset);
                };
                let (finite_slots, unset_slots) =
                    slots
                        .iter()
                        .fold((0_usize, 0_usize), |(finite, unset), slot| match slot {
                            crate::entity_table::RangeIntervalSlot::Binary64 { .. } => {
                                (finite + 1, unset)
                            }
                            crate::entity_table::RangeIntervalSlot::Unset { .. } => {
                                (finite, unset + 1)
                            }
                        });
                (
                    total + 1,
                    no_slot,
                    nominal,
                    finite + finite_slots,
                    unset + unset_slots,
                )
            },
        );
    let (
        constraint_range_count,
        dimension_constraint_range_count,
        complex_constraint_range_count,
        evaluated_constraint_range_count,
        unset_constraint_range_count,
    ) = native
        .entity_records
        .iter()
        .filter_map(|record| record.constraint_range.as_ref())
        .fold(
            (0_usize, 0_usize, 0_usize, 0_usize, 0_usize),
            |(total, dimensions, complex, evaluated, unset), range| {
                let (dimensions, complex) = match range.framing {
                    crate::native::CatiaConstraintRangeFraming::DimensionB8
                    | crate::native::CatiaConstraintRangeFraming::DimensionC1
                    | crate::native::CatiaConstraintRangeFraming::DimensionDC
                    | crate::native::CatiaConstraintRangeFraming::DimensionDF => {
                        (dimensions + 1, complex)
                    }
                    crate::native::CatiaConstraintRangeFraming::ComplexC9 => {
                        (dimensions, complex + 1)
                    }
                };
                let (evaluated, unset) = match range.evaluation {
                    crate::native::CatiaEntityEvaluation::Scalar { .. } => (evaluated + 1, unset),
                    crate::native::CatiaEntityEvaluation::Unset => (evaluated, unset + 1),
                };
                (total + 1, dimensions, complex, evaluated, unset)
            },
        );
    let unresolved_dimension_quantity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.constraint_range.as_ref())
        .filter(|range| {
            matches!(
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
            }
        })
        .count();
    let IncomingEntityIncidenceCounts {
        total: constraint_range_incoming_reference_count,
        payload: constraint_range_incoming_payload_reference_count,
        storage: constraint_range_incoming_storage_reference_count,
        classified: classified_constraint_range_source_entity_count,
        zero: unreferenced_constraint_range_count,
        one: uniquely_referenced_constraint_range_count,
        multiple: multiply_referenced_constraint_range_count,
    } = incoming_entity_incidence_counts(
        native
            .entity_records
            .iter()
            .filter_map(|record| record.constraint_range.as_ref())
            .map(|range| {
                (
                    range.incoming_references.as_slice(),
                    range.incoming_storage_references.as_slice(),
                )
            }),
    );
    let IncomingEntityIncidenceCounts {
        total: range_interval_incoming_reference_count,
        payload: range_interval_incoming_payload_reference_count,
        storage: range_interval_incoming_storage_reference_count,
        classified: classified_range_interval_source_entity_count,
        zero: unreferenced_range_interval_count,
        one: uniquely_referenced_range_interval_count,
        multiple: multiply_referenced_range_interval_count,
    } = incoming_entity_incidence_counts(
        native
            .entity_records
            .iter()
            .filter_map(|record| record.range_interval.as_ref())
            .map(|range| {
                (
                    range.incoming_references.as_slice(),
                    range.incoming_storage_references.as_slice(),
                )
            }),
    );
    let definition_value_count = native
        .entity_records
        .iter()
        .filter(|record| record.definition_value.is_some())
        .count();
    let owned_definition_value_count = native
        .design_objects
        .iter()
        .map(|object| object.definition_values.len())
        .sum::<usize>();
    let unowned_definition_value_count = definition_value_count
        .checked_sub(owned_definition_value_count)
        .expect("owned CATIA definition values are a subset of decoded values");
    let (
        definition_chain_value_count,
        definition_chain_evaluation_count,
        evaluated_definition_chain_count,
        unset_definition_chain_count,
        definition_chain_atom_count,
        definition_chain_control_count,
        definition_chain_separator_count,
        definition_chain_schema_selector_count,
    ) = native
        .entity_records
        .iter()
        .filter_map(|record| record.definition_chain_value.as_ref())
        .fold(
            (
                0_usize, 0_usize, 0_usize, 0_usize, 0_usize, 0_usize, 0_usize, 0_usize,
            ),
            |(total, evaluations, evaluated, unset, atoms, controls, separators, schemas),
             value| {
                use crate::native::{
                    CatiaEntityEvaluation, CatiaEntitySuffixSchemaValue as Selected,
                };
                match &value.value {
                    Selected::Evaluation {
                        evaluation: CatiaEntityEvaluation::Scalar { .. },
                        ..
                    } => (
                        total + 1,
                        evaluations + 1,
                        evaluated + 1,
                        unset,
                        atoms,
                        controls,
                        separators,
                        schemas,
                    ),
                    Selected::Evaluation {
                        evaluation: CatiaEntityEvaluation::Unset,
                        ..
                    } => (
                        total + 1,
                        evaluations + 1,
                        evaluated,
                        unset + 1,
                        atoms,
                        controls,
                        separators,
                        schemas,
                    ),
                    Selected::Atom { .. } => (
                        total + 1,
                        evaluations,
                        evaluated,
                        unset,
                        atoms + 1,
                        controls,
                        separators,
                        schemas,
                    ),
                    Selected::ControlE8 => (
                        total + 1,
                        evaluations,
                        evaluated,
                        unset,
                        atoms,
                        controls + 1,
                        separators,
                        schemas,
                    ),
                    Selected::Separator37 => (
                        total + 1,
                        evaluations,
                        evaluated,
                        unset,
                        atoms,
                        controls,
                        separators + 1,
                        schemas,
                    ),
                    Selected::SchemaSelector { .. } => (
                        total + 1,
                        evaluations,
                        evaluated,
                        unset,
                        atoms,
                        controls,
                        separators,
                        schemas + 1,
                    ),
                }
            },
        );
    let formula_relation_count = native
        .entity_records
        .iter()
        .filter(|record| record.formula_relation.is_some())
        .count();
    let relation_program_instance_count = native
        .entity_records
        .iter()
        .filter(|record| record.relation_program_instance.is_some())
        .count();
    let relation_program_output_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter(|instance| instance.output_entity.is_some())
        .count();
    let resolved_relation_program_output_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.output_entity.as_ref())
        .filter(|output| output.entity.is_some())
        .count();
    let null_relation_program_output_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.output_entity.as_ref())
        .filter(|output| output.is_null)
        .count();
    let unresolved_relation_program_output_count = relation_program_output_count
        - resolved_relation_program_output_count
        - null_relation_program_output_count;
    let relation_program_reference_incidence_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .map(|instance| instance.reference_incidences.len())
        .sum::<usize>();
    let resolved_relation_program_reference_incidence_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .flat_map(|instance| &instance.reference_incidences)
        .filter(|incidence| incidence.reference.entity.is_some())
        .count();
    let null_relation_program_reference_incidence_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .flat_map(|instance| &instance.reference_incidences)
        .filter(|incidence| incidence.reference.is_null)
        .count();
    let unresolved_relation_program_reference_incidence_count =
        relation_program_reference_incidence_count
            - resolved_relation_program_reference_incidence_count
            - null_relation_program_reference_incidence_count;
    let classified_relation_program_reference_incidence_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .flat_map(|instance| &instance.reference_incidences)
        .filter(|incidence| incidence.reference.class_name.is_some())
        .count();
    let (lead12_relation_program_instance_count, lead54_relation_program_instance_count) = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .fold((0, 0), |(lead12, lead54), instance| {
            match instance.framing {
                crate::native::CatiaRelationProgramInstanceFraming::Lead12 => (lead12 + 1, lead54),
                crate::native::CatiaRelationProgramInstanceFraming::Lead54 => (lead12, lead54 + 1),
            }
        });
    let resolved_lead54_relation_program_trailing_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.lead54_trailing_entity.as_ref())
        .filter(|trailing| trailing.entity.is_some())
        .count();
    let null_lead54_relation_program_trailing_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.lead54_trailing_entity.as_ref())
        .filter(|trailing| trailing.is_null)
        .count();
    let unresolved_lead54_relation_program_trailing_entity_count =
        lead54_relation_program_instance_count
            - resolved_lead54_relation_program_trailing_entity_count
            - null_lead54_relation_program_trailing_entity_count;
    let resolved_lead12_relation_program_context_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.lead12_context_entity.as_ref())
        .filter(|context| context.entity.is_some())
        .count();
    let null_lead12_relation_program_context_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.lead12_context_entity.as_ref())
        .filter(|context| context.is_null)
        .count();
    let unresolved_lead12_relation_program_context_entity_count =
        lead12_relation_program_instance_count
            - resolved_lead12_relation_program_context_entity_count
            - null_lead12_relation_program_context_entity_count;
    let classified_lead12_relation_program_context_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.lead12_context_entity.as_ref())
        .filter(|context| context.class_name.is_some())
        .count();
    let lead12_relation_program_paramout_context_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.lead12_context_entity.as_ref())
        .filter(|context| context.class_name.as_deref() == Some("paramout"))
        .count();
    let other_lead12_relation_program_context_class_count =
        classified_lead12_relation_program_context_entity_count
            - lead12_relation_program_paramout_context_entity_count;
    let unclassified_lead12_relation_program_context_entity_count =
        lead12_relation_program_instance_count
            - classified_lead12_relation_program_context_entity_count;
    let resolved_relation_program_instance_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter(|instance| instance.program_entity.entity.is_some())
        .count();
    let null_relation_program_instance_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter(|instance| instance.program_entity.is_null)
        .count();
    let unresolved_relation_program_instance_count = relation_program_instance_count
        - resolved_relation_program_instance_count
        - null_relation_program_instance_count;
    let resolved_relation_program_repeated_reference_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter(|instance| instance.repeated_entity.entity.is_some())
        .count();
    let null_relation_program_repeated_reference_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter(|instance| instance.repeated_entity.is_null)
        .count();
    let unresolved_relation_program_repeated_reference_count = relation_program_instance_count
        - resolved_relation_program_repeated_reference_count
        - null_relation_program_repeated_reference_count;
    let classified_relation_program_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter(|instance| instance.program_entity.class_name.is_some())
        .count();
    let classified_relation_program_repeated_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter(|instance| instance.repeated_entity.class_name.is_some())
        .count();
    let relation_expression_instance_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter(|instance| instance.relation_expression.is_some())
        .count();
    let typed_relation_expression_entities = native
        .entity_records
        .iter()
        .filter(|entity| {
            entity
                .relation_expression
                .as_ref()
                .is_some_and(|expression| expression.signature.is_some())
        })
        .map(|entity| entity.id.as_str())
        .collect::<HashSet<_>>();
    let typed_relation_program_instance_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.relation_expression.as_deref())
        .filter(|entity| typed_relation_expression_entities.contains(entity))
        .count();
    let resolved_relation_program_input_instance_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter(|instance| instance.inputs.is_some())
        .count();
    let unresolved_relation_program_input_instance_count =
        typed_relation_program_instance_count - resolved_relation_program_input_instance_count;
    let resolved_relation_program_input_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.inputs.as_ref())
        .map(Vec::len)
        .sum::<usize>();
    let distinct_relation_program_input_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.inputs.as_ref())
        .flatten()
        .filter_map(|input| input.entity.entity.as_deref())
        .collect::<HashSet<_>>()
        .len();
    let instanced_relation_expression_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.relation_expression.as_deref())
        .collect::<HashSet<_>>()
        .len();
    let relation_program_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .map(|instance| instance.parameter_dependencies.len())
        .sum::<usize>();
    let resolved_relation_program_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .flat_map(|instance| &instance.parameter_dependencies)
        .filter(|dependency| dependency.candidates.len() == 1)
        .count();
    let ambiguous_relation_program_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .flat_map(|instance| &instance.parameter_dependencies)
        .filter(|dependency| dependency.candidates.len() > 1)
        .count();
    let unresolved_relation_program_parameter_dependency_count =
        relation_program_parameter_dependency_count
            - resolved_relation_program_parameter_dependency_count;
    let other_relation_program_instance_count =
        resolved_relation_program_instance_count - relation_expression_instance_count;
    let schema_configuration_record_count = native
        .entity_records
        .iter()
        .filter(|record| record.schema_configuration_record.is_some())
        .count();
    let resolved_schema_configuration_reference_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.schema_configuration_record.as_ref())
        .filter(|record| record.entity_reference.reference.entity.is_some())
        .count();
    let null_schema_configuration_reference_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.schema_configuration_record.as_ref())
        .filter(|record| record.entity_reference.reference.is_null)
        .count();
    let unresolved_schema_configuration_reference_count = schema_configuration_record_count
        - resolved_schema_configuration_reference_count
        - null_schema_configuration_reference_count;
    let classified_schema_configuration_entity_reference_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.schema_configuration_record.as_ref())
        .filter(|record| record.entity_reference.reference.class_name.is_some())
        .count();
    let schema_configuration_row_link_count = native
        .entity_records
        .iter()
        .filter(|record| record.schema_configuration_row_link.is_some())
        .count();
    let resolved_schema_configuration_row_class_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.schema_configuration_row_link.as_ref())
        .filter(|link| link.class_reference.entity.is_some())
        .count();
    let null_schema_configuration_row_class_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.schema_configuration_row_link.as_ref())
        .filter(|link| link.class_reference.is_null)
        .count();
    let resolved_schema_configuration_row_successor_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.schema_configuration_row_link.as_ref())
        .filter(|link| link.successor.entity.is_some())
        .count();
    let null_schema_configuration_row_successor_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.schema_configuration_row_link.as_ref())
        .filter(|link| link.successor.is_null)
        .count();
    let (
        complete_schema_configuration_row_chain_count,
        ordered_schema_configuration_row_link_count,
    ) = schema_configuration_row_chain_coverage(&native);
    let unordered_schema_configuration_row_link_count =
        schema_configuration_row_link_count - ordered_schema_configuration_row_link_count;
    let resolved_schema_configuration_row_chain_terminal_count = native
        .schema_configuration_row_chains
        .iter()
        .filter_map(|chain| chain.links.last())
        .filter(|link| link.successor.entity.is_some())
        .count();
    let null_schema_configuration_row_chain_terminal_count = native
        .schema_configuration_row_chains
        .iter()
        .filter_map(|chain| chain.links.last())
        .filter(|link| link.successor.is_null)
        .count();
    let unresolved_schema_configuration_row_chain_terminal_count =
        complete_schema_configuration_row_chain_count
            - resolved_schema_configuration_row_chain_terminal_count
            - null_schema_configuration_row_chain_terminal_count;
    let classified_schema_configuration_row_chain_terminal_count = native
        .schema_configuration_row_chains
        .iter()
        .filter_map(|chain| chain.links.last())
        .filter(|link| link.successor.class_name.is_some())
        .count();
    let schema_configuration_row_intervening_entity_count = native
        .schema_configuration_row_chains
        .iter()
        .flat_map(|chain| &chain.links)
        .filter_map(|link| link.intervening_entities.as_ref())
        .flatten()
        .count();
    let schema_configuration_row_source_interval_chain_count = native
        .schema_configuration_row_chains
        .iter()
        .filter(|chain| {
            chain
                .links
                .iter()
                .all(|link| link.intervening_entities.is_some())
        })
        .count();
    let schema_configuration_entities = native
        .entity_records
        .iter()
        .filter(|entity| entity.schema_configuration_record.is_some())
        .map(|entity| entity.id.as_str())
        .collect::<HashSet<_>>();
    let schema_configuration_row_intervening_schema_configuration_count = native
        .schema_configuration_row_chains
        .iter()
        .flat_map(|chain| &chain.links)
        .filter_map(|link| link.intervening_entities.as_ref())
        .flatten()
        .filter_map(|reference| reference.entity.as_deref())
        .filter(|entity| schema_configuration_entities.contains(entity))
        .count();
    let formula_referenced_relation_expressions = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .filter_map(|formula| formula.expression_entity.reference.entity.as_deref())
        .collect::<HashSet<_>>();
    let program_referenced_relation_expressions = native
        .entity_records
        .iter()
        .filter_map(|record| record.relation_program_instance.as_ref())
        .filter_map(|instance| instance.relation_expression.as_deref())
        .collect::<HashSet<_>>();
    let referenced_relation_expressions = formula_referenced_relation_expressions
        .union(&program_referenced_relation_expressions)
        .copied()
        .collect::<HashSet<_>>();
    let formula_referenced_relation_expression_count =
        formula_referenced_relation_expressions.len();
    let program_referenced_relation_expression_count =
        program_referenced_relation_expressions.len();
    let referenced_relation_expression_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.relation_expression.is_some()
                && referenced_relation_expressions.contains(record.id.as_str())
        })
        .count();
    let unreferenced_relation_expression_count =
        relation_expression_count - referenced_relation_expression_count;
    let resolved_formula_output_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .filter(|formula| formula.output_entity.reference.entity.is_some())
        .count();
    let null_formula_output_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .filter(|formula| formula.output_entity.reference.is_null)
        .count();
    let classified_formula_output_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .filter(|formula| formula.output_entity.reference.class_name.is_some())
        .count();
    let classified_formula_expression_entity_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .filter(|formula| formula.expression_entity.reference.class_name.is_some())
        .count();
    let unresolved_formula_output_count =
        formula_relation_count - resolved_formula_output_count - null_formula_output_count;
    let formula_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .map(|formula| formula.parameter_dependencies.len())
        .sum();
    let formula_parameter_dependency_candidate_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .flat_map(|formula| &formula.parameter_dependencies)
        .map(|dependency| dependency.candidates.len())
        .sum();
    let classified_formula_parameter_dependency_candidate_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .flat_map(|formula| &formula.parameter_dependencies)
        .flat_map(|dependency| &dependency.candidates)
        .filter(|candidate| candidate.class_name.is_some())
        .count();
    let resolved_formula_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .flat_map(|formula| &formula.parameter_dependencies)
        .filter(|dependency| dependency.candidates.len() == 1)
        .count();
    let ambiguous_formula_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .flat_map(|formula| &formula.parameter_dependencies)
        .filter(|dependency| dependency.candidates.len() > 1)
        .count();
    let unresolved_formula_parameter_dependency_count =
        formula_parameter_dependency_count - resolved_formula_parameter_dependency_count;
    let (
        escaped_word_entity_suffix_count,
        token_8149_entity_suffix_count,
        fixed_fe_f6_entity_suffix_count,
        paged_atom_state_01_entity_suffix_count,
    ) = native.entity_records.iter().fold(
        (0, 0, 0, 0),
        |(escaped, token, fixed, paged), record| match record.suffix_framing.as_ref() {
            Some(crate::native::CatiaEntitySuffixFraming::EscapedWord(_)) => {
                (escaped + 1, token, fixed, paged)
            }
            Some(crate::native::CatiaEntitySuffixFraming::Token8149) => {
                (escaped, token + 1, fixed, paged)
            }
            Some(crate::native::CatiaEntitySuffixFraming::FixedFeF6 { .. }) => {
                (escaped, token, fixed + 1, paged)
            }
            Some(crate::native::CatiaEntitySuffixFraming::PagedAtomState01 { .. }) => {
                (escaped, token, fixed, paged + 1)
            }
            None => (escaped, token, fixed, paged),
        },
    );
    let scalar_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.suffix_value.as_ref().is_some_and(|value| {
                matches!(
                    value.payload,
                    crate::native::CatiaEntitySuffixPayload::Evaluation {
                        evaluation: crate::native::CatiaEntityEvaluation::Scalar { .. },
                        ..
                    }
                )
            })
        })
        .count();
    let unset_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.suffix_value.as_ref().is_some_and(|value| {
                matches!(
                    value.payload,
                    crate::native::CatiaEntitySuffixPayload::Evaluation {
                        evaluation: crate::native::CatiaEntityEvaluation::Unset,
                        ..
                    }
                )
            })
        })
        .count();
    let (control_e8_entity_suffix_value_count, control_e9_entity_suffix_value_count) = native
        .entity_records
        .iter()
        .fold((0, 0), |(e8, e9), record| {
            match record.suffix_value.as_ref().map(|value| &value.payload) {
                Some(crate::native::CatiaEntitySuffixPayload::ControlE8) => (e8 + 1, e9),
                Some(crate::native::CatiaEntitySuffixPayload::ControlE9) => (e8, e9 + 1),
                _ => (e8, e9),
            }
        });
    let control_entity_suffix_value_count =
        control_e8_entity_suffix_value_count + control_e9_entity_suffix_value_count;
    let separator_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.suffix_value.as_ref().is_some_and(|value| {
                matches!(
                    value.payload,
                    crate::native::CatiaEntitySuffixPayload::Separator37
                )
            })
        })
        .count();
    let atom_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.suffix_value.as_ref().is_some_and(|value| {
                matches!(
                    value.payload,
                    crate::native::CatiaEntitySuffixPayload::Atom { .. }
                )
            })
        })
        .count();
    let (
        schema_selected_atom_entity_suffix_value_count,
        schema_selected_evaluation_entity_suffix_value_count,
        schema_selected_control_entity_suffix_value_count,
        schema_selected_separator_entity_suffix_value_count,
        schema_selected_schema_entity_suffix_value_count,
    ) = native.entity_records.iter().fold(
        (0, 0, 0, 0, 0),
        |(atoms, evaluations, controls, separators, schemas), record| {
            let Some(crate::native::CatiaEntitySuffixPayload::SchemaSelected { value, .. }) =
                record.suffix_value.as_ref().map(|suffix| &suffix.payload)
            else {
                return (atoms, evaluations, controls, separators, schemas);
            };
            match value {
                crate::native::CatiaEntitySuffixSelectedValue::Atom { .. } => {
                    (atoms + 1, evaluations, controls, separators, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::Evaluation { .. } => {
                    (atoms, evaluations + 1, controls, separators, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::ControlE8 => {
                    (atoms, evaluations, controls + 1, separators, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::Separator37 => {
                    (atoms, evaluations, controls, separators + 1, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::SchemaSelector { .. } => {
                    (atoms, evaluations, controls, separators, schemas + 1)
                }
            }
        },
    );
    let schema_selected_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| record.suffix_schema_selection.is_some())
        .count();
    let wide_prefix_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record
                .suffix_value
                .as_ref()
                .is_some_and(|value| value.prefix_atom_widths.iter().any(|width| *width > 1))
        })
        .count();
    let unresolved_design_owner_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_record.is_none())
        .count();
    let object_records_by_id = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    let unassigned_owner_slot_count = object_records_by_id
        .values()
        .filter(|record| record.has_unassigned_owner())
        .count();
    let structurally_owned_records = native
        .design_objects
        .iter()
        .filter(|object| object.owner_record.is_some())
        .flat_map(|object| object.fields.iter().cloned())
        .collect::<HashSet<_>>();
    let structurally_owned_definition_chain_value_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_record.is_some())
        .map(|object| object.definition_chain_values.len())
        .sum::<usize>();
    let unowned_definition_chain_value_count = definition_chain_value_count
        .checked_sub(structurally_owned_definition_chain_value_count)
        .expect("owned CATIA definition-chain values are a subset of decoded values");
    let unassigned_definition_chain_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.definition_chain_value.is_some()
                && object_records_by_id
                    .get(record.object_record.as_str())
                    .is_some_and(|record| record.has_unassigned_owner())
        })
        .count();
    let structurally_owned_definition_chain_evaluation_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.definition_chain_value.as_ref().is_some_and(|value| {
                matches!(
                    &value.value,
                    crate::native::CatiaEntitySuffixSchemaValue::Evaluation { .. }
                )
            }) && structurally_owned_records.contains(&record.object_record)
        })
        .count();
    let unowned_definition_chain_evaluation_count = definition_chain_evaluation_count
        .checked_sub(structurally_owned_definition_chain_evaluation_count)
        .expect("owned CATIA definition-chain evaluations are a subset of decoded values");
    let unassigned_definition_chain_evaluation_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.definition_chain_value.as_ref().is_some_and(|value| {
                matches!(
                    &value.value,
                    crate::native::CatiaEntitySuffixSchemaValue::Evaluation { .. }
                )
            }) && object_records_by_id
                .get(record.object_record.as_str())
                .is_some_and(|record| record.has_unassigned_owner())
        })
        .count();
    let transferred_formula_design_records = formula_transfer
        .consumed_object_records
        .intersection(&structurally_owned_records)
        .cloned()
        .collect::<HashSet<_>>();
    let transferred_principal_plane_records = design_feature_transfer
        .principal_plane_records
        .intersection(&structurally_owned_records)
        .cloned()
        .collect::<HashSet<_>>();
    let transferred_design_feature_records = design_feature_transfer
        .consumed_records()
        .intersection(&structurally_owned_records)
        .cloned()
        .collect::<HashSet<_>>();
    let transferred_design_records = transferred_formula_design_records
        .union(&transferred_design_feature_records)
        .chain(transferred_native_sketch_entity_records.intersection(&structurally_owned_records))
        .chain(
            transferred_native_sketch_constraint_records.intersection(&structurally_owned_records),
        )
        .chain(transferred_constraint_range_records.intersection(&structurally_owned_records))
        .cloned()
        .collect::<HashSet<_>>();
    let unresolved_object_record_count = modeling_object_records
        .difference(&transferred_design_records)
        .count();
    let unresolved_design_object_count = native
        .design_objects
        .iter()
        .filter(|object| {
            modeling_graph_scope
                .as_ref()
                .is_none_or(|scope| scope.contains(object.parent.as_str()))
        })
        .filter(|object| {
            object
                .fields
                .iter()
                .any(|field| !transferred_design_records.contains(field))
        })
        .count();
    let value_field_count = native
        .value_blocks
        .iter()
        .map(|block| block.fields.len())
        .sum();
    let value_selection_count = native
        .value_blocks
        .iter()
        .map(|block| block.schema_selections.len())
        .sum();
    let transferred_line_profile_count = ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            curve
                .id
                .0
                .starts_with("catia:consolidated:line-profile-curve#")
        })
        .count();
    let transferred_native_sketch_entity_count = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| {
            matches!(
                &entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::Native { .. }
            )
        })
        .count();
    let native_operation_feature_ids = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            feature
                .source_tag
                .as_deref()
                .is_some_and(design_feature::is_admitted_native_operation_class)
        })
        .map(|feature| feature.id.clone())
        .collect::<HashSet<_>>();
    let transferred_native_operation_parameter_count = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter
                .owner
                .as_ref()
                .is_some_and(|owner| native_operation_feature_ids.contains(owner))
        })
        .count();
    report.coverage.extend([
        (
            crate::coverage::DECODED_APPEARANCE_PACKET_COUNT,
            appearance_transfer.decoded_packets,
        ),
        (
            crate::coverage::UNRESOLVED_APPEARANCE_PACKET_COUNT,
            appearance_transfer.unresolved_packets,
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
            native
                .consolidated_cone_faces
                .iter()
                .map(|face| face.parameter_points.len())
                .sum(),
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
            ir.model
                .procedural_surfaces
                .iter()
                .filter(|surface| {
                    surface
                        .id
                        .0
                        .starts_with("catia:consolidated:surface-revolution#")
                })
                .count(),
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
            native
                .zero_entity_edge_strides
                .iter()
                .map(|stride| stride.allocations.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_TOPOLOGY_REF_COUNT,
            native
                .zero_entity_edge_strides
                .iter()
                .map(|stride| stride.topology_refs.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_SURFACE_SUPPORT_REF_COUNT,
            native
                .zero_entity_edge_strides
                .iter()
                .map(|stride| stride.surface_support_refs.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_FACE_BOUND_SUPPORT_RUN_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter(|run| run.face.is_some())
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_03_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .filter(|face| face.terminal_control == 0x03)
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_05_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .filter(|face| face.terminal_control == 0x05)
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_TERMINAL_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .map(|face| face.loop_terminals.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_RECORD_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .map(|face| face.loops.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_41_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .filter(|loop_| loop_.loop_class == 0x41)
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_50_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .filter(|loop_| loop_.loop_class == 0x50)
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_C1_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .filter(|loop_| loop_.loop_class == 0xc1)
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_FORWARD_LOOP_MEMBER_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .flat_map(|loop_| &loop_.forward_senses)
                .filter(|sense| **sense)
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_REVERSED_LOOP_MEMBER_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .flat_map(|loop_| &loop_.forward_senses)
                .filter(|sense| !**sense)
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_LOOP_MEMBER_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .map(|loop_record| loop_record.forward_senses.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_MODEL_ENDPOINT_PAIR_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .map(|loop_record| loop_record.oriented_model_endpoints.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_BOUND_SUPPORT_MEMBER_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .map(|loop_record| loop_record.support_record_ordinals.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_BOUND_TYPED_LOOP_REFERENCE_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .map(|loop_record| loop_record.typed_records.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_PAIR_COUNT,
            native.zero_entity_oriented_use_pairs.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_COUNT,
            native
                .zero_entity_oriented_use_pairs
                .iter()
                .map(|pair| pair.uses.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_ALLOCATION_COUNT,
            native
                .zero_entity_oriented_use_pairs
                .iter()
                .flat_map(|pair| &pair.uses)
                .map(|use_| use_.allocations.len())
                .sum(),
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
            native
                .zero_entity_support_runs
                .iter()
                .map(|run| run.supports.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_PCURVE_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.pcurve.is_some())
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_MODEL_CURVE_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.model_curve.is_some())
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_MODEL_CONSTRUCTION_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.model_curve_construction.is_some())
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_UV_ENDPOINT_PAIR_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.uv_endpoints.is_some())
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_MODEL_ENDPOINT_PAIR_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.model_endpoints.is_some())
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_MODEL_MIDPOINT_COUNT,
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.model_midpoint.is_some())
                .count(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_COUNT,
            native.zero_entity_vertex_incidences.len(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_ALLOCATION_COUNT,
            native
                .zero_entity_vertex_incidences
                .iter()
                .map(|incidence| incidence.allocations.len())
                .sum(),
        ),
        (
            crate::coverage::DECODED_ZERO_ENTITY_VERTEX_OWNER_BINDING_COUNT,
            native
                .zero_entity_vertex_incidences
                .iter()
                .filter(|incidence| incidence.vertex_record.is_some())
                .count(),
        ),
        (
            crate::coverage::DECODED_OBJECT_GRAPH_COUNT,
            native.object_graphs.len(),
        ),
        (crate::coverage::DECODED_OBJECT_RECORD_COUNT, object_record_count),
        (
            crate::coverage::MODELING_OBJECT_GRAPH_COUNT,
            modeling_graph_scope
                .as_ref()
                .map_or(native.object_graphs.len(), HashSet::len),
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
            design_feature_transfer.feature_parent_count(&ir, &native),
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
            transferred_formula_design_records.len(),
        ),
        (
            crate::coverage::TRANSFERRED_DEFINITION_CHAIN_PARAMETER_COUNT,
            formula_transfer.definition_chain_parameter_count,
        ),
        (
            crate::coverage::TRANSFERRED_PRINCIPAL_PLANE_RECORD_COUNT,
            transferred_principal_plane_records.len(),
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
    ]);
    if transferred_pmi_dimension_count != 0 {
        report.coverage.record(
            crate::coverage::TRANSFERRED_PMI_DIMENSION_COUNT,
            transferred_pmi_dimension_count,
        );
    }
    let untransferred_line_profile_count = native
        .consolidated_line_profiles
        .len()
        .saturating_sub(transferred_line_profile_count);
    if untransferred_line_profile_count > 0 {
        report.losses.push(
            CatiaLossCode::GeometryLineProfileNotTransferred.note(format!(
                "{untransferred_line_profile_count} consolidated line-profile record(s) retain \
             exact line geometry but were not transferred by the active geometry route."
            )),
        );
    }
    if !native.zero_entity_support_runs.is_empty()
        || !native.zero_entity_edge_strides.is_empty()
        || !native.zero_entity_oriented_use_pairs.is_empty()
        || !native.zero_entity_vertex_incidences.is_empty()
    {
        let support_count = native
            .zero_entity_support_runs
            .iter()
            .map(|run| run.supports.len())
            .sum::<usize>();
        let endpoint_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.uv_endpoints.is_some())
            .count();
        let support_pcurve_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.pcurve.is_some())
            .count();
        let support_model_curve_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.model_curve.is_some())
            .count();
        let support_model_construction_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.model_curve_construction.is_some())
            .count();
        let model_endpoint_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.model_endpoints.is_some())
            .count();
        let model_midpoint_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.model_midpoint.is_some())
            .count();
        let face_count = native
            .zero_entity_support_runs
            .iter()
            .filter(|run| run.face.is_some())
            .count();
        let loop_terminal_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .map(|face| face.loop_terminals.len())
            .sum::<usize>();
        let loop_record_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .map(|face| face.loops.len())
            .sum::<usize>();
        let oriented_loop_member_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .flat_map(|face| &face.loops)
            .map(|loop_record| loop_record.forward_senses.len())
            .sum::<usize>();
        let oriented_model_endpoint_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .flat_map(|face| &face.loops)
            .map(|loop_record| loop_record.oriented_model_endpoints.len())
            .sum::<usize>();
        let bound_support_member_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .flat_map(|face| &face.loops)
            .map(|loop_record| loop_record.support_record_ordinals.len())
            .sum::<usize>();
        let bound_typed_loop_reference_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .flat_map(|face| &face.loops)
            .map(|loop_record| loop_record.typed_records.len())
            .sum::<usize>();
        let vertex_owner_binding_count = native
            .zero_entity_vertex_incidences
            .iter()
            .filter(|incidence| incidence.vertex_record.is_some())
            .count();
        let ownership_face_count = native
            .zero_entity_ownership_roots
            .first()
            .map_or(0, |root| root.face_slots.len());
        report.losses.push(
            CatiaLossCode::TopologyZeroEntitySupportsRetained.note(format!(
                "{} zero-entity surface-support run(s) retain {support_count} face-local \
                 occurrence(s), including {support_pcurve_count} complete parameter-space \
                 curve(s), {support_model_curve_count} with exact model-space carriers, \
                 {support_model_construction_count} with exact procedural model-space carriers, \
                 {endpoint_count} with exact UV endpoint pairs, and \
                 {model_endpoint_count} lifted model-space endpoint pairs with \
                 {model_midpoint_count} bounded-curve midpoint witnesses; \
                 {face_count} run(s) bind the complete face roster with {loop_terminal_count} \
                 ordered loop terminal(s), {loop_record_count} loop record(s), and \
                 {oriented_loop_member_count} stored member sense(s), including \
                 {oriented_model_endpoint_count} sense-oriented model-space endpoint pair(s), \
                 {bound_support_member_count} member(s) bound to face-local support records and \
                 {bound_typed_loop_reference_count} typed reference(s) bound to global records; {} \
                 edge-stride allocation tuple(s) remain separate, and \
                 {vertex_owner_binding_count} of {} vertex-incidence record(s) bind their \
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
            )),
        );
    }
    if modeling_scope_is_unresolved {
        report
            .losses
            .push(CatiaLossCode::HistoryModelingScopeUnresolved.note(format!(
                "CATIA outer declarations do not unambiguously select one object graph physically \
             contained by the declared CATPrtCont stream; \
             {retained_unscoped_object_graph_count} retained object graph(s) with \
             {retained_unscoped_object_record_count} field record(s) remain outside the \
             modeling scope, and feature, formula, sketch, constraint, configuration, and \
             history authorship remains unresolved."
            )));
    }
    if unresolved_object_record_count != 0 {
        report.losses.push(CatiaLossCode::HistoryObjectRecordsUnresolved.note(format!(
            "CATIA native data retains {} design object(s), {design_field_count} grouped field(s), {object_record_count} object-graph field record(s), including {unassigned_owner_slot_count} with an explicit literal unassigned owner slot, {object_record_reference_count} payload reference(s), comprising {resolved_object_record_reference_count} resolved, {null_object_record_reference_count} terminal-null, and {unresolved_object_record_reference_count} unresolved identities, {entity_value_field_count} entity-value field(s), {entity_value_schema_selection_count} entity-value schema selection(s), {numeric_entity_value_pair_count} complete numeric entity-value pair(s), {reference_signature_count} complete reference-signature packet(s) containing {reference_signature_token_count} descriptor token(s) and selecting {resolved_reference_signature_entity_count} resolved, {null_reference_signature_entity_count} terminal-null, and {unresolved_reference_signature_entity_count} unresolved entity incidences, including {classified_reference_signature_entity_count} with a resolved class, {numeric_entity_value_packet_count} embedded numeric entity-value packet(s), {compact_entity_value_packet_count} compact value packet(s), {layout_entity_value_packet_count} layout-bearing value packet(s), {e9_scalar_entity_value_packet_count} E9 scalar packet(s), {escaped_word_entity_suffix_count} escaped-word entity suffix(es), {token_8149_entity_suffix_count} standalone 8149 suffix token(s), {fixed_fe_f6_entity_suffix_count} fixed FE-F6 suffix frame(s), {paged_atom_state_01_entity_suffix_count} paged-atom state-01 suffix(es), {scalar_entity_suffix_value_count} scalar entity-suffix value(s), {unset_entity_suffix_value_count} unset entity-suffix value(s), {atom_entity_suffix_value_count} atom entity-suffix value(s), {separator_entity_suffix_value_count} separator entity-suffix value(s), {schema_selected_atom_entity_suffix_value_count} schema-selected atom value(s), {schema_selected_evaluation_entity_suffix_value_count} schema-selected evaluation(s), {schema_selected_control_entity_suffix_value_count} schema-selected control value(s), {schema_selected_separator_entity_suffix_value_count} schema-selected separator(s), {schema_selected_schema_entity_suffix_value_count} schema-selected schema value(s), {schema_selected_entity_suffix_value_count} suffix value(s) with resolved schema selectors, {wide_prefix_entity_suffix_value_count} suffix value(s) with multi-byte prefix atoms, {control_entity_suffix_value_count} direct control entity-suffix value(s), comprising {control_e8_entity_suffix_value_count} E8 and {control_e9_entity_suffix_value_count} E9 state(s), {relation_expression_count} complete relation expression(s), {relation_program_instance_count} complete compound relation-program instance(s), comprising {lead12_relation_program_instance_count} lead-12 and {lead54_relation_program_instance_count} lead-54 frames, {resolved_relation_program_instance_count} resolved and {unresolved_relation_program_instance_count} unresolved program identities, with {resolved_relation_program_repeated_reference_count} resolved and {unresolved_relation_program_repeated_reference_count} unresolved repeated-reference identities, {resolved_lead12_relation_program_context_entity_count} resolved and {unresolved_lead12_relation_program_context_entity_count} unresolved lead-12 context identities, and {resolved_lead54_relation_program_trailing_entity_count} resolved and {unresolved_lead54_relation_program_trailing_entity_count} unresolved lead-54 trailing identities; {relation_expression_instance_count} select relation-expression programs, {other_relation_program_instance_count} select other resolved entities, and those relation-expression instances select {instanced_relation_expression_count} distinct expression entity or entities and retain {relation_program_parameter_dependency_count} parameter symbol occurrence(s), comprising {resolved_relation_program_parameter_dependency_count} uniquely resolved and {unresolved_relation_program_parameter_dependency_count} unresolved, including {ambiguous_relation_program_parameter_dependency_count} with multiple candidates; {typed_relation_program_instance_count} typed program instance(s) comprise {resolved_relation_program_input_instance_count} with complete ordered inputs and {unresolved_relation_program_input_instance_count} with incomplete input binding, retaining {resolved_relation_program_input_count} resolved input occurrence(s) selecting {distinct_relation_program_input_entity_count} distinct entity identity or identities; {schema_configuration_record_count} complete schema-configuration Configuration record(s) retain {resolved_schema_configuration_reference_count} resolved, {null_schema_configuration_reference_count} terminal-null, and {unresolved_schema_configuration_reference_count} unresolved reference identities; {schema_configuration_row_link_count} complete configrow link(s) retain {resolved_schema_configuration_row_class_count} resolved and {null_schema_configuration_row_class_count} terminal-null class identities plus {resolved_schema_configuration_row_successor_count} resolved and {null_schema_configuration_row_successor_count} terminal-null successor identities, with {ordered_schema_configuration_row_link_count} row link(s) in {complete_schema_configuration_row_chain_count} complete chain(s), comprising {resolved_schema_configuration_row_chain_terminal_count} resolved, {null_schema_configuration_row_chain_terminal_count} terminal-null, and {unresolved_schema_configuration_row_chain_terminal_count} unresolved terminals; {schema_configuration_row_source_interval_chain_count} source-ordered chain(s) retain {schema_configuration_row_intervening_entity_count} entity or entities from the open intervals between rows and successors, including {schema_configuration_row_intervening_schema_configuration_count} complete schema-configuration Configuration record(s), while {unordered_schema_configuration_row_link_count} row link(s) have unresolved order; {parameter_value_count} complete named parameter value(s), {range_interval_count} complete source-schema Range interval(s), comprising {range_interval_no_slot_count} no-slot production(s), {range_interval_nominal_count} finite nominal(s), {range_interval_finite_slot_count} finite deviation slot(s), and {range_interval_unset_slot_count} unset deviation slot(s), {constraint_range_count} complete constraint-range value(s), comprising {dimension_constraint_range_count} dimension and {complex_constraint_range_count} complex-constraint range(s), with {evaluated_constraint_range_count} finite evaluation(s) and {unset_constraint_range_count} unset evaluation(s), {definition_value_count} definition-bound suffix value(s), including {owned_definition_value_count} assigned to design objects and {unowned_definition_value_count} without a resolved owner, {definition_chain_evaluation_count} two-definition chain evaluation(s), comprising {evaluated_definition_chain_count} finite and {unset_definition_chain_count} unset value(s), with {structurally_owned_definition_chain_evaluation_count} structurally owned and {unowned_definition_chain_evaluation_count} without a resolved structural owner; {unassigned_definition_chain_value_count} chain value(s), including {unassigned_definition_chain_evaluation_count} evaluation(s), occupy explicit literal unassigned owner slots; {formula_relation_count} complete formula relation(s), comprising {resolved_formula_output_count} resolved, {null_formula_output_count} terminal-null, and {unresolved_formula_output_count} unresolved output identities, {formula_parameter_dependency_count} formula parameter symbol occurrence(s), comprising {resolved_formula_parameter_dependency_count} uniquely resolved and {unresolved_formula_parameter_dependency_count} unresolved, including {ambiguous_formula_parameter_dependency_count} with multiple candidates, {repeated_reference_suffix_count} repeated-reference suffix(es), {repeated_reference_schema_selection_count} repeated-reference schema selection(s), {definition_schema_selection_count} definition-schema selection(s), {design_object_owner_link_count} structural owner link(s), and {design_object_relation_count} exact outbound design-field relation occurrence(s), including {design_same_object_relation_count} within one design object, {design_reflexive_field_relation_count} reflexive field occurrence(s), and {design_unowned_field_relation_count} to fields without owner groups; {classified_design_object_count} design object(s) have class evidence and {unresolved_design_owner_count} owner identity or identities remain unresolved; {} typed parameter(s), including {} selected through complete relation-program inputs, {} exact formula, expression, or parameter field record(s), and {} exact principal-plane field record(s) transferred, while {unresolved_object_record_count} modeling-scope field record(s) across {unresolved_design_object_count} design object(s), neutral features with unresolved semantics, other parameters, sketch placement, geometry, profiles, constraints, configurations, and re-derivable history remain unresolved; {} sketch identity record(s) transfer.",
            native.design_objects.len(),
            formula_transfer.typed_parameter_count,
            formula_transfer.relation_program_parameter_count,
            transferred_formula_design_records.len(),
            transferred_principal_plane_records.len(),
            ir.model.sketches.len(),
        )));
    }
    if !native.legacy_entity_runs.is_empty() {
        report.losses.push(CatiaLossCode::HistoryLegacyRunsUnresolved.note(format!(
            "CATIA native data retains {} legacy design run(s) with {legacy_schema_program_count} complete compact schema program(s), containing {legacy_schema_identifier_count} complete identifier packet(s), and {legacy_entity_identity_count} source-ordered entity identity marker(s), comprising {legacy_identity_lead_81_count} lead-81, {legacy_identity_lead_82_count} lead-82, {legacy_identity_lead_e5_count} lead-E5, and {legacy_identity_lead_fd_count} lead-FD record(s), {legacy_role_selector_count} complete schema role selector(s), including {legacy_selected_role_count} unresolved schema-selected role name(s) and {legacy_role_field_binding_count} immediate schema-field binding(s), {legacy_schema_field_count} complete role-bounded schema field(s), {legacy_text_field_count} complete schema text field(s), including {legacy_e3_role_tail_text_field_count} with E3 paged-role tails and {legacy_role_text_field_count} role-bound text field(s), {legacy_relation_count} typed expression/signature pair(s), including {legacy_parameter_relation_count} with exact parameter identities, {legacy_synchronous_state_count} relation update-state field(s), comprising {legacy_synchronous_relation_count} synchronous and {legacy_asynchronous_relation_count} asynchronous state(s), {legacy_type_descriptor_count} type descriptor(s), including {legacy_literal_type_descriptor_count} literal name(s), {legacy_scalar_value_count} typed scalar evaluation(s), including {legacy_named_scalar_value_count} named scalar(s), {legacy_string_value_count} string value(s), including {legacy_named_string_value_count} named string(s), and {legacy_integer_value_count} signed integer value(s), including {legacy_named_integer_value_count} named integer(s); {} uniquely named, literal-typed parameter(s), including {} resolved through descriptor selectors, and {} local-input legacy formula(s) transferred, while remaining selector semantics, unbound relation ownership and parameters, unresolved selector types, feature semantics, and feature history remain unresolved.",
            native.legacy_entity_runs.len(),
            formula_transfer.legacy_parameter_count,
            formula_transfer.legacy_selector_parameter_count,
            formula_transfer.legacy_formula_count,
        )));
    }
    if unresolved_dimension_quantity_count != 0 {
        report.losses.push(
            CatiaLossCode::AttributesDimensionQuantityUnresolved.note(format!(
                "{unresolved_dimension_quantity_count} finite `Range`/`CstAttr_Dimension` \
                 scalar production(s) remain native because the admitted selectors, suffix \
                 framing, interval, and owner incidences do not assign a physical quantity."
            )),
        );
    }
    if !native.value_blocks.is_empty() {
        report.losses.push(CatiaLossCode::AttributesVisualizationUnbound.note(format!(
            "CATIA native data retains {} visualization value block(s), {value_field_count} encoded field(s), and {value_selection_count} schema-selected presentation value(s); {} display-color packet(s) remain without a proven typed face or body target ({} packet(s) transferred), while other visualization fields remain native.",
            native.value_blocks.len(),
            appearance_transfer.unresolved_packets,
            appearance_transfer.transferred_packets,
        )));
    }
    native.store_owned(ir.native.namespace_mut("catia"))?;
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        &mut admitted_entities,
        "admit CATIA entities",
    )?;
    decode_result(scan, matched, ir, report, annotations, unknowns)
}

fn modeling_graph_scope(
    has_outer_declarations: bool,
    graphs: &[CatiaObjectGraph],
) -> Option<HashSet<String>> {
    if !has_outer_declarations {
        return None;
    }
    let mut part_graphs = graphs.iter().filter(|graph| {
        graph
            .outer_container
            .as_ref()
            .is_some_and(|container| container.class_name == "CATPrtCont")
    });
    match (part_graphs.next(), part_graphs.next()) {
        (Some(graph), None) => Some(HashSet::from([graph.id.clone()])),
        _ => Some(HashSet::new()),
    }
}

/// The single site that finishes a decode and charges dialect admission loss.
///
/// Identity is authored once from the match classified at the decode entry;
/// the sealed wrapper stamps it onto the report. This function merges the
/// container-level notes and charges dialect loss from that same match.
fn decode_result(
    scan: &ContainerScan,
    matched: &DialectMatch,
    mut ir: CadIr,
    mut body: DecodeBody,
    annotations: Annotations,
    unknowns: Vec<UnknownRecord>,
) -> Result<Decoded, CodecError> {
    ir.source = Some(crate::assemble::source_meta(scan, matched));
    body.notes = crate::container::notes(scan);
    body.losses.extend(crate::dialect::dialect_loss(matched));
    let mut source_fidelity = SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "catia", unknowns)?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

#[cfg(test)]
mod tests;
