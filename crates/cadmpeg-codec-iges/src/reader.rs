// SPDX-License-Identifier: Apache-2.0
//! Physical graph to CADIR native preservation and loss reporting.

use crate::loss::IgesLossCode;
use crate::{card, directory, entities, global, graph, native, parameter};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeOptions, DecodeResult};
use cadmpeg_ir::hash::{
    document_local_sha256_with_charge, sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE,
};
use cadmpeg_ir::report::{DecodeReport, LossNote, Severity, TransferDisposition, TransferLedger};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::{CadIr, RetainedSourceRecord, SourceFidelity, SourceMeta};
use std::collections::{BTreeMap, BTreeSet};

fn source_meta(global: &global::ResolvedGlobal) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert("representation".into(), "fixed-ascii".into());
    attributes.insert(
        "parameter_delimiter".into(),
        char::from(global.parameter_delimiter).to_string(),
    );
    attributes.insert(
        "record_delimiter".into(),
        char::from(global.record_delimiter).to_string(),
    );
    attributes.insert("iges_version".into(), global.version().into());
    attributes.insert(
        "iges_version_flag".into(),
        global.declared_version_flag().to_string(),
    );
    if let Some(value) = global.units_name() {
        attributes.insert("native_units".into(), value);
    }
    if let Some(value) = global.sender_product() {
        attributes.insert("sender_product".into(), value);
    }
    if let Some(value) = global.native_file_name() {
        attributes.insert("native_file_name".into(), value);
    }
    SourceMeta {
        format: "iges".into(),
        attributes,
    }
}

fn occurrence_loss(
    code: IgesLossCode,
    message: impl Into<String>,
    source_sequence: u32,
    directory: &[directory::DirectoryEntry],
) -> LossNote {
    let note = code.note(message);
    if let Some(entry) = directory
        .iter()
        .find(|entry| entry.sequence == source_sequence)
    {
        note.with_provenance(entry.loss_provenance())
    } else {
        note
    }
}

fn loss_is_attributed_to(losses: &[LossNote], source_sequence: u32) -> bool {
    let directory_tag = format!("directory_entry:D{source_sequence}");
    let parameter_prefix = format!("D{source_sequence}:");
    losses.iter().any(|loss| {
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref())
            .is_some_and(|tag| tag == directory_tag || tag.starts_with(&parameter_prefix))
    })
}

pub(crate) fn decode(
    bytes: &[u8],
    options: DecodeOptions,
    ctx: &DecodeContext<'_>,
) -> Result<DecodeResult, CodecError> {
    let output = usize::try_from(ctx.policy().limits.max_collection_items)
        .ok()
        .map_or(native::MAX_PRODUCT_OCCURRENCES, |policy| {
            policy.min(native::MAX_PRODUCT_OCCURRENCES)
        });
    let depth = usize::try_from(ctx.policy().limits.max_recursion_depth)
        .ok()
        .map_or(native::MAX_PRODUCT_OCCURRENCE_DEPTH, |policy| {
            policy.min(native::MAX_PRODUCT_OCCURRENCE_DEPTH)
        });
    decode_with_occurrence_limits(bytes, options, output, depth, Some(ctx))
}

fn decode_with_occurrence_limits(
    bytes: &[u8],
    options: DecodeOptions,
    product_occurrence_output_limit: usize,
    product_occurrence_depth_limit: usize,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<DecodeResult, CodecError> {
    charge_work(ctx, bytes.len() as u64, "iges_card_scan")?;
    let _scan_storage = ctx
        .map(|ctx| ctx.reserve_scoped(bytes.len() as u64, "iges_card_storage", None))
        .transpose()?;
    let scan = card::scan_with_context(bytes, ctx)?;
    let (global, global_losses) = global::parse(&scan)?;
    let length_context = global.length_context();
    let (directory, quarantined_directory) = directory::parse(&scan);
    charge_entities(
        ctx,
        (directory.len() + quarantined_directory.len()) as u64,
        "iges_directory_entries",
    )?;
    entities::geometry::enforce_transform_depth(&directory, ctx)?;
    let parameter::ParameterAssembly {
        records: parameters,
        quarantined: quarantined_parameters,
        recoveries: parameter_recoveries,
    } = parameter::assemble_with_context(&scan, &directory, &quarantined_directory, &global, ctx)?;
    let mut framing_recoveries = scan.recoveries.clone();
    framing_recoveries.merge(parameter_recoveries);
    let quarantined_parameter_sequences = quarantined_parameters
        .iter()
        .map(|record| record.sequence)
        .collect::<BTreeSet<_>>();
    let projected_directory = (!quarantined_parameter_sequences.is_empty()).then(|| {
        directory
            .iter()
            .filter(|entry| !quarantined_parameter_sequences.contains(&entry.sequence))
            .cloned()
            .collect::<Vec<_>>()
    });
    let projected_directory = projected_directory.as_deref().unwrap_or(&directory);
    let parameter_tokens = parameters
        .iter()
        .map(|record| record.tokens.len() as u64)
        .sum();
    charge_work(ctx, parameter_tokens, "iges_parameter_parse")?;
    let mut references = graph::build(&directory);
    let mut source_fidelity = SourceFidelity::default();
    source_fidelity.retained_records.push(RetainedSourceRecord {
        id: crate::SOURCE_IMAGE_ID.into(),
        stream: "iges".into(),
        offset: 0,
        byte_len: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        data: Some(match ctx {
            Some(ctx) => ctx.copy_retained(bytes, "iges_source_image", None)?,
            None => bytes.to_vec(),
        }),
    });

    let mut ir = CadIr::empty(Units::default());
    if let Some(context) = &length_context {
        ir.tolerances.linear = context.minimum_resolution_mm();
    }
    ir.source = Some(source_meta(&global));
    let projection = match length_context.filter(|_| !options.container_only) {
        Some(context) => {
            charge_work(ctx, parameter_tokens, "iges_geometry_projection")?;
            entities::geometry::project_geometry(
                &mut ir,
                projected_directory,
                &parameters,
                &context,
                ctx,
            )?
        }
        None => entities::geometry::Projection {
            handled: BTreeSet::default(),
            decoded: BTreeSet::default(),
            consumed: BTreeSet::default(),
            losses: Vec::new(),
        },
    };
    let semantic_structure_admitted = (!options.container_only).then_some(&projection.decoded);
    charge_work(ctx, parameter_tokens, "iges_native_projection")?;
    let native::NativeStoreResult {
        occurrence_expansion: product_occurrence_expansion,
        ambiguous_parameter_sequences,
    } = native::store(
        &mut ir,
        &scan,
        &directory,
        &parameters,
        native::QuarantinedRecords {
            directory: &quarantined_directory,
            parameters: &quarantined_parameters,
        },
        semantic_structure_admitted,
        &mut references,
        &global,
        native::ProductOccurrenceLimits::new(
            product_occurrence_output_limit,
            product_occurrence_depth_limit,
        ),
        ctx,
    )?;
    ir.finalize();
    let document_digest = match ctx {
        Some(ctx) => {
            document_local_sha256_with_charge(&ir, "iges", crate::SOURCE_IMAGE_ID, |bytes| {
                ctx.charge_work(bytes, "iges_document_digest")
            })?
        }
        None => crate::document_digest(&ir),
    };
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.into(), document_digest);
    }
    source_fidelity.finalize();

    let geometry_transferred = !projection.decoded.is_empty();
    let mut losses = Vec::new();
    losses.extend(global.dialect_loss());
    losses.extend(global_losses);
    losses.extend(projection.losses);
    losses.extend(graph::losses(&references, &scan, &parameters));
    losses.extend(framing_recoveries.notes());
    losses.extend(
        quarantined_directory
            .iter()
            .map(directory::QuarantinedDirectoryRecord::loss_note),
    );
    losses.extend(
        quarantined_parameters
            .iter()
            .map(parameter::QuarantinedParameterRecord::loss_note),
    );
    if let Some(source_sequence) = product_occurrence_expansion.output_truncated_at {
        losses.push(occurrence_loss(
            IgesLossCode::OccurrenceExpansionOutputTruncated,
            "IGES product occurrence expansion reached its configured output limit",
            source_sequence,
            &directory,
        ));
    }
    if let Some(source_sequence) = product_occurrence_expansion.depth_truncated_at {
        losses.push(occurrence_loss(
            IgesLossCode::OccurrenceExpansionDepthTruncated,
            "IGES product occurrence expansion reached its configured nesting-depth limit",
            source_sequence,
            &directory,
        ));
    }
    for source_sequence in product_occurrence_expansion.malformed_definition_sequences {
        losses.push(occurrence_loss(
            IgesLossCode::OccurrenceRootInferenceBlocked,
            "IGES product occurrence root inference was suppressed because a definition member list is malformed",
            source_sequence,
            &directory,
        ));
    }
    for source_sequence in product_occurrence_expansion.malformed_placement_sequences {
        losses.push(occurrence_loss(
            IgesLossCode::OccurrencePlacementMalformed,
            "IGES product occurrence expansion omitted an instance or member with malformed placement data",
            source_sequence,
            &directory,
        ));
    }
    for (source_sequence, candidate_count, equally_valid) in ambiguous_parameter_sequences {
        losses.push(occurrence_loss(
            IgesLossCode::ParameterBoundaryAmbiguous,
            format!(
                "IGES Parameter Data has {candidate_count} {} trailing pointer-group boundaries; primary parameters and pointer ownership were not guessed",
                if equally_valid {
                    "equally valid"
                } else {
                    "structural"
                }
            ),
            source_sequence,
            &directory,
        ));
    }
    if !options.container_only {
        let generic_losses = directory
            .iter()
            .filter(|entry| entry.entity_type != 0)
            .filter(|entry| {
                if !crate::profile::envelope_a_admits(entry.entity_type, entry.form) {
                    return true;
                }
                !projection.decoded.contains(&entry.sequence)
                    && !projection.consumed.contains(&entry.sequence)
                    && !loss_is_attributed_to(&losses, entry.sequence)
            })
            .map(|entry| {
                let note = if crate::profile::envelope_a_admits(entry.entity_type, entry.form) {
                    IgesLossCode::EntityRetainedUnprojected.note(format!(
                        "IGES entity type {} form {} retained without neutral projection",
                        entry.entity_type, entry.form
                    ))
                } else {
                    IgesLossCode::EntityOutsideEnvelope.note(format!(
                        "IGES entity type {} form {} is outside the Fixed ASCII mechanical/document envelope",
                        entry.entity_type, entry.form
                    ))
                };
                note.with_provenance(entry.loss_provenance())
            })
            .collect::<Vec<_>>();
        losses.extend(generic_losses);
        charge_work(
            ctx,
            ir.model.entity_count() as u64,
            "iges_semantic_validation",
        )?;
        reject_invalid_semantic_ir(&ir)?;
    }
    let mut transfer_ledger = TransferLedger::default();
    for entry in directory.iter().filter(|entry| entry.entity_type != 0) {
        let attributed_loss = loss_is_attributed_to(&losses, entry.sequence);
        let note = if options.container_only {
            "native record retained; semantic projection was not requested"
        } else if !crate::profile::envelope_a_admits(entry.entity_type, entry.form) {
            "native record retained; entity is outside the declared read envelope"
        } else if projection.decoded.contains(&entry.sequence) && attributed_loss {
            "native record retained; semantic projection emitted with an attributed loss"
        } else if projection.decoded.contains(&entry.sequence) {
            "native record retained; semantic projection emitted"
        } else if attributed_loss {
            "native record retained; semantic projection omitted with an attributed loss"
        } else if projection.consumed.contains(&entry.sequence) {
            "native record retained; record was consumed as construction support"
        } else if projection.handled.contains(&entry.sequence) {
            "native record retained; no standalone neutral projection was required"
        } else {
            "native record retained; semantic projection omitted with an attributed loss"
        };
        transfer_ledger.record(
            format!("D{}", entry.sequence),
            Some(format!("iges:entity:directory#{}", entry.sequence)),
            TransferDisposition::Retained,
            Some(note.into()),
        );
    }
    for record in &quarantined_directory {
        transfer_ledger.record(
            format!("D{}", record.sequence),
            Some(record.identity()),
            TransferDisposition::Retained,
            Some(
                "quarantined directory record retained; typed Directory fields were not recovered"
                    .into(),
            ),
        );
    }
    for record in &quarantined_parameters {
        transfer_ledger.record(
            format!("D{}:parameter", record.sequence),
            Some(record.identity()),
            TransferDisposition::Retained,
            Some("quarantined parameter data retained; tokens were not recovered".into()),
        );
    }
    transfer_ledger
        .verify(&cadmpeg_ir::index::ModelIndex::new(&ir))
        .map_err(|message| {
            CodecError::Malformed(format!("IGES transfer ledger is inconsistent: {message}"))
        })?;
    let mut notes = directory::summary_notes(&directory);
    notes.extend(parameter::summary_notes(&parameters));
    notes.extend(graph::summary_notes(&references));
    Ok(DecodeResult::new(
        ir,
        DecodeReport {
            format: "iges".into(),
            container_only: options.container_only,
            geometry_transferred,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger,
            losses,
            notes,
        },
        source_fidelity,
    ))
}

/// Fail the decode when the projected IR has any error-severity finding.
///
/// Keeps full [`cadmpeg_ir::validate_neutral`]: `DRAFT_CORE_CHECKS` error
/// outcomes match full validation on every IGES golden fixture, so the route
/// stays on the full validator.
pub(crate) fn reject_invalid_semantic_ir(ir: &CadIr) -> Result<(), CodecError> {
    let validation = cadmpeg_ir::validate_neutral(ir, Vec::new());
    let Some(finding) = validation
        .findings
        .iter()
        .find(|finding| finding.severity >= Severity::Error)
    else {
        return Ok(());
    };
    let entity = finding
        .entity
        .as_deref()
        .map_or(String::new(), |entity| format!(" for {entity}"));
    Err(CodecError::Malformed(format!(
        "IGES semantic projection produced invalid CADIR: {}{entity}: {}",
        finding.check, finding.message
    )))
}

#[cfg(test)]
pub(crate) fn decode_with_test_occurrence_limits(
    bytes: &[u8],
    options: DecodeOptions,
    output_limit: usize,
    depth_limit: usize,
) -> Result<DecodeResult, CodecError> {
    decode_with_occurrence_limits(bytes, options, output_limit, depth_limit, None)
}

fn charge_entities(
    ctx: Option<&DecodeContext<'_>>,
    count: u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.map_or(Ok(()), |ctx| ctx.charge_entities(count, operation))
}

fn charge_work(
    ctx: Option<&DecodeContext<'_>>,
    units: u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.map_or(Ok(()), |ctx| ctx.charge_work(units, operation))
}

#[cfg(test)]
mod tests;
