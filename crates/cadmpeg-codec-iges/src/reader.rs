// SPDX-License-Identifier: Apache-2.0
//! Physical graph to CADIR native preservation and loss reporting.

use crate::loss::IgesLossCode;
use crate::{card, directory, entities, global, graph, loss, native, parameter};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::{CodecError, ContainerSummary};
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

fn attributed_sequences(losses: &[LossNote]) -> BTreeSet<u32> {
    fn rendered_sequence(rendered: &str) -> Option<u32> {
        let sequence = rendered.parse::<u32>().ok()?;
        (sequence.to_string() == rendered).then_some(sequence)
    }

    losses
        .iter()
        .filter_map(|loss| loss.provenance.as_ref()?.tag.as_deref())
        .filter_map(|tag| {
            tag.strip_prefix("directory_entry:D")
                .and_then(rendered_sequence)
                .or_else(|| {
                    let (head, _) = tag.split_once(':')?;
                    rendered_sequence(head.strip_prefix('D')?)
                })
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseMode {
    Decode,
    Inspect,
}

fn parameter_tokens(records: &[parameter::ParameterRecord]) -> u64 {
    records
        .iter()
        .map(|record| record.tokens.len() as u64)
        .sum()
}

pub(crate) struct PhysicalParse<'a, 'ctx> {
    scan: card::CardScan<'a>,
    global: global::ResolvedGlobal,
    global_losses: Vec<LossNote>,
    directory: Vec<directory::DirectoryEntry>,
    quarantined_directory: Vec<directory::QuarantinedDirectoryRecord>,
    parameters: Vec<parameter::ParameterRecord>,
    quarantined_parameters: Vec<parameter::QuarantinedParameterRecord>,
    framing_recoveries: card::FramingRecoveries,
    references: BTreeMap<u32, Vec<graph::ReferenceEdge>>,
    _scan_storage: Option<ScopedReservation<'ctx>>,
}

impl<'a, 'ctx> PhysicalParse<'a, 'ctx> {
    fn run(
        bytes: &'a [u8],
        ctx: Option<&'ctx DecodeContext<'_>>,
        mode: ParseMode,
    ) -> Result<Self, CodecError> {
        let (card_scan, card_storage, directory_entries, parameter_parse) = match mode {
            ParseMode::Decode => (
                "iges_card_scan",
                "iges_card_storage",
                "iges_directory_entries",
                "iges_parameter_parse",
            ),
            ParseMode::Inspect => (
                "iges_inspect_card_scan",
                "iges_inspect_card_storage",
                "iges_inspect_directory_entries",
                "iges_inspect_parameter_parse",
            ),
        };
        charge_work(ctx, bytes.len() as u64, card_scan)?;
        let scan_storage = ctx
            .map(|ctx| ctx.reserve_scoped(bytes.len() as u64, card_storage, None))
            .transpose()?;
        let scan = card::scan_with_context(bytes, ctx)?;
        let (global, global_losses) = global::parse(&scan)?;
        let (directory, quarantined_directory) = directory::parse(&scan);
        charge_entities(
            ctx,
            (directory.len() + quarantined_directory.len()) as u64,
            directory_entries,
        )?;
        if mode == ParseMode::Decode {
            entities::geometry::enforce_transform_depth(&directory, ctx)?;
        }
        let parameter::ParameterAssembly {
            records: parameters,
            quarantined: quarantined_parameters,
            recoveries: parameter_recoveries,
        } = parameter::assemble_with_context(
            &scan,
            &directory,
            &quarantined_directory,
            &global,
            ctx,
        )?;
        charge_work(ctx, parameter_tokens(&parameters), parameter_parse)?;
        let references = graph::build(&directory);
        let mut framing_recoveries = scan.recoveries.clone();
        framing_recoveries.merge(parameter_recoveries);
        Ok(Self {
            scan,
            global,
            global_losses,
            directory,
            quarantined_directory,
            parameters,
            quarantined_parameters,
            framing_recoveries,
            references,
            _scan_storage: scan_storage,
        })
    }

    fn admission_losses(&self) -> Vec<LossNote> {
        let mut losses = Vec::new();
        losses.extend(self.global.dialect_loss());
        losses.extend(self.global_losses.iter().cloned());
        losses
    }

    fn record_losses(&self) -> Vec<LossNote> {
        let mut losses = self.framing_recoveries.notes();
        losses.extend(
            self.quarantined_directory
                .iter()
                .map(directory::QuarantinedDirectoryRecord::loss_note),
        );
        losses.extend(
            self.quarantined_parameters
                .iter()
                .map(parameter::QuarantinedParameterRecord::loss_note),
        );
        losses
    }
}

pub(crate) fn inspect(
    ctx: &DecodeContext<'_>,
    window: &[u8],
) -> Result<ContainerSummary, CodecError> {
    let parse = PhysicalParse::run(window, Some(ctx), ParseMode::Inspect)?;
    let mut losses = parse.admission_losses();
    losses.extend(parse.record_losses());
    let mut summary = card::summarize(&parse.scan);
    summary.notes.extend(parse.global.summary_notes());
    summary
        .notes
        .extend(directory::summary_notes(&parse.directory));
    summary
        .notes
        .extend(parameter::summary_notes(&parse.parameters));
    summary
        .notes
        .extend(graph::summary_notes(&parse.references));
    summary.notes.extend(loss::census(&losses));
    Ok(summary)
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
    let mut parse = PhysicalParse::run(bytes, ctx, ParseMode::Decode)?;
    let length_context = parse.global.length_context();
    let quarantined_parameter_sequences = parse
        .quarantined_parameters
        .iter()
        .map(|record| record.sequence)
        .collect::<BTreeSet<_>>();
    let projected_directory = (!quarantined_parameter_sequences.is_empty()).then(|| {
        parse
            .directory
            .iter()
            .filter(|entry| !quarantined_parameter_sequences.contains(&entry.sequence))
            .cloned()
            .collect::<Vec<_>>()
    });
    let projected_directory = projected_directory.as_deref().unwrap_or(&parse.directory);
    let parameter_tokens = parameter_tokens(&parse.parameters);
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
    ir.source = Some(source_meta(&parse.global));
    let projection = match length_context.filter(|_| !options.container_only) {
        Some(context) => {
            charge_work(ctx, parameter_tokens, "iges_geometry_projection")?;
            entities::geometry::project_geometry(
                &mut ir,
                projected_directory,
                &parse.parameters,
                &context,
                ctx,
            )?
        }
        None => entities::geometry::Projection {
            decoded: BTreeSet::default(),
            consumed: BTreeSet::default(),
            losses: Vec::new(),
        },
    };
    let semantic_structure_admitted = (!options.container_only).then_some(&projection.decoded);
    charge_work(ctx, parameter_tokens, "iges_native_projection")?;
    let native::NativeStoreResult {
        occurrence_expansion: product_occurrence_expansion,
        ambiguous_parameter_boundaries,
        overdeclared_counts,
    } = native::store(
        &mut ir,
        &parse.scan,
        &parse.directory,
        &parse.parameters,
        native::QuarantinedRecords {
            directory: &parse.quarantined_directory,
            parameters: &parse.quarantined_parameters,
        },
        semantic_structure_admitted,
        &mut parse.references,
        &parse.global,
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
    let mut losses = parse.admission_losses();
    losses.extend(projection.losses);
    losses.extend(graph::losses(
        &parse.references,
        &parse.scan,
        &parse.parameters,
    ));
    losses.extend(parse.record_losses());
    if let Some(source_sequence) = product_occurrence_expansion.output_truncated_at {
        losses.push(occurrence_loss(
            IgesLossCode::OccurrenceExpansionOutputTruncated,
            "IGES product occurrence expansion reached its configured output limit",
            source_sequence,
            &parse.directory,
        ));
    }
    if let Some(source_sequence) = product_occurrence_expansion.depth_truncated_at {
        losses.push(occurrence_loss(
            IgesLossCode::OccurrenceExpansionDepthTruncated,
            "IGES product occurrence expansion reached its configured nesting-depth limit",
            source_sequence,
            &parse.directory,
        ));
    }
    for source_sequence in product_occurrence_expansion.malformed_definition_sequences {
        losses.push(occurrence_loss(
            IgesLossCode::OccurrenceRootInferenceBlocked,
            "IGES product occurrence root inference was suppressed because a definition member list is malformed",
            source_sequence,
            &parse.directory,
        ));
    }
    for source_sequence in product_occurrence_expansion.malformed_placement_sequences {
        losses.push(occurrence_loss(
            IgesLossCode::OccurrencePlacementMalformed,
            "IGES product occurrence expansion omitted an instance or member with malformed placement data",
            source_sequence,
            &parse.directory,
        ));
    }
    for native::AmbiguousParameterBoundary {
        sequence: source_sequence,
        candidate_count,
        equally_valid,
    } in ambiguous_parameter_boundaries
    {
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
            &parse.directory,
        ));
    }
    for (source_sequence, native::OverdeclaredCount { declared, present }) in overdeclared_counts {
        losses.push(occurrence_loss(
            IgesLossCode::ParameterCountOverdeclared,
            format!(
                "IGES entity D{source_sequence} declares a counted list of {declared} items; its Parameter Data record holds {present} in whole or in part, so the list was not read"
            ),
            source_sequence,
            &parse.directory,
        ));
    }
    if !options.container_only {
        let attributed_before_generic = attributed_sequences(&losses);
        let generic_losses = parse
            .directory
            .iter()
            .filter(|entry| entry.entity_type != 0)
            .filter(|entry| {
                if !crate::profile::envelope_a_admits(entry.entity_type, entry.form) {
                    return true;
                }
                !projection.decoded.contains(&entry.sequence)
                    && !projection.consumed.contains(&entry.sequence)
                    && !attributed_before_generic.contains(&entry.sequence)
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
    let attributed = if options.container_only {
        BTreeSet::new()
    } else {
        attributed_sequences(&losses)
    };
    let mut transfer_ledger = TransferLedger::default();
    for entry in parse
        .directory
        .iter()
        .filter(|entry| entry.entity_type != 0)
    {
        let attributed_loss = attributed.contains(&entry.sequence);
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
        } else {
            "native record retained; no standalone neutral projection was required"
        };
        transfer_ledger.record(
            format!("D{}", entry.sequence),
            Some(format!("iges:entity:directory#{}", entry.sequence)),
            TransferDisposition::Retained,
            Some(note.into()),
        );
    }
    for record in &parse.quarantined_directory {
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
    for record in &parse.quarantined_parameters {
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
    let mut notes = directory::summary_notes(&parse.directory);
    notes.extend(parameter::summary_notes(&parse.parameters));
    notes.extend(graph::summary_notes(&parse.references));
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
