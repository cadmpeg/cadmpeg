// SPDX-License-Identifier: Apache-2.0
//! Physical graph to CADIR native preservation and loss reporting.

use crate::loss::IgesLossCode;
use crate::representation::Representation;
use crate::{card, directory, entities, global, graph, native, parameter};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
#[cfg(test)]
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::hash::{document_local_sha256_with_charge, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::report::{LossNote, Severity, TransferLedger, TransferOutcome};
use cadmpeg_ir::ContainerSummary;
use cadmpeg_ir::{CadIr, RetainedSourceRecord, SourceFidelity, SourceMeta};
use std::collections::{BTreeMap, BTreeSet};

fn source_meta(
    global: &global::ResolvedGlobal,
    representation: Representation,
    primary: cadmpeg_core::dialect::DialectMatch,
) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert("representation".into(), representation.as_str().into());
    attributes.insert(
        "parameter_delimiter".into(),
        char::from(global.parameter_delimiter).to_string(),
    );
    attributes.insert(
        "record_delimiter".into(),
        char::from(global.record_delimiter).to_string(),
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
    SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(primary),
        attributes,
    )
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
    trailing_pointer_analysis: BTreeMap<u32, parameter::TrailingPointerAnalysis>,
    quarantined_parameters: Vec<parameter::QuarantinedParameterRecord>,
    framing_recoveries: card::FramingRecoveries,
    references: BTreeMap<u32, Vec<graph::ReferenceEdge>>,
    _scan_storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx> PhysicalParse<'a, 'ctx> {
    fn run(
        bytes: &'a [u8],
        ctx: &'ctx DecodeContext<'_>,
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
        let scan_storage = ctx.reserve_scoped(bytes.len() as u64, card_storage, None)?;
        let scan = card::scan_with_context(bytes, Some(ctx))?;
        let (global, mut global_losses) = global::parse(&scan)?;
        let (directory, quarantined_directory) = directory::parse(&scan, global.global_table());
        charge_entities(
            ctx,
            (directory.len() + quarantined_directory.len()) as u64,
            directory_entries,
        )?;
        if mode == ParseMode::Decode {
            entities::geometry::enforce_transform_depth(&directory, Some(ctx))?;
        }
        let parameter::ParameterAssembly {
            records: parameters,
            trailing_pointer_analysis,
            quarantined: quarantined_parameters,
            recoveries: parameter_recoveries,
        } = parameter::assemble_with_context(
            &scan,
            &directory,
            &quarantined_directory,
            &global,
            Some(ctx),
        )?;
        global_losses.extend(
            global
                .conditional_double_precision_losses(parameter::uses_double_precision(&parameters)),
        );
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
            trailing_pointer_analysis,
            quarantined_parameters,
            framing_recoveries,
            references,
            _scan_storage: scan_storage,
        })
    }

    fn admission_losses(&self) -> Vec<LossNote> {
        let mut losses = Vec::new();
        losses.extend(crate::dialect::dialect_loss(&self.global));
        losses.extend(self.global_losses.iter().cloned());
        if matches!(self.global.global_table(), global::GlobalTable::V4_0) {
            let post_terminate_count = self.scan.post_terminate_count();
            if post_terminate_count > 0 {
                losses.push(IgesLossCode::GlobalNoncanonicalFraming.note(format!(
                    "IGES 4.0 requires the Terminate Section to be the last physical line; retained {post_terminate_count} trailing record(s) as source data"
                )));
            }
        }
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
    representation: Representation,
    source_size: usize,
) -> Result<ContainerSummary, CodecError> {
    let parse = PhysicalParse::run(window, ctx, ParseMode::Inspect)?;
    let primary = crate::dialect::classify(representation, &parse.global);
    let mut losses = parse.admission_losses();
    losses.extend(parse.record_losses());
    let mut summary = card::summarize(&parse.scan, primary);
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
    summary.losses = losses;
    if representation != Representation::FixedAscii {
        summary.container_kind = representation.as_str().into();
        if let Some(note) = summary
            .notes
            .iter_mut()
            .find(|note| note.starts_with("source_bytes="))
        {
            *note = format!("source_bytes={source_size}");
        }
        summary.notes.push(format!(
            "normalized_representation={}",
            representation.as_str()
        ));
    }
    Ok(summary)
}

pub(crate) fn decode(
    parse_bytes: &[u8],
    source_bytes: &[u8],
    representation: Representation,
    ctx: &DecodeContext<'_>,
) -> Result<Decoded, CodecError> {
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
    decode_with_occurrence_limits(
        parse_bytes,
        source_bytes,
        representation,
        output,
        depth,
        ctx,
    )
}

fn decode_with_occurrence_limits(
    parse_bytes: &[u8],
    source_bytes: &[u8],
    representation: Representation,
    product_occurrence_output_limit: usize,
    product_occurrence_depth_limit: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Decoded, CodecError> {
    let mut parse = PhysicalParse::run(parse_bytes, ctx, ParseMode::Decode)?;
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
    let retained_source = ctx.copy_retained(source_bytes, "iges_source_image", None)?;
    source_fidelity
        .retained_records
        .push(RetainedSourceRecord::retained(
            crate::SOURCE_IMAGE_ID,
            "iges",
            0,
            retained_source,
        ));

    let primary = crate::dialect::classify(representation, &parse.global);
    let mut ir = CadIr::decoded(source_meta(&parse.global, representation, primary));
    if let Some(context) = &length_context {
        ir.tolerances.linear = context.minimum_resolution_mm();
    }
    let projection = match length_context.filter(|_| !ctx.container_only()) {
        Some(context) => {
            charge_work(ctx, parameter_tokens, "iges_geometry_projection")?;
            entities::geometry::project_geometry(
                &mut ir,
                projected_directory,
                &parse.parameters,
                &parse.trailing_pointer_analysis,
                &context,
                Some(ctx),
            )?
        }
        None => entities::geometry::Projection::default(),
    };
    let semantic_structure_admitted = (!ctx.container_only()).then_some(&projection.decoded);
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
        &parse.trailing_pointer_analysis,
        native::QuarantinedRecords {
            directory: &parse.quarantined_directory,
            parameters: &parse.quarantined_parameters,
        },
        semantic_structure_admitted,
        &projection.boundary_vertex_derivations,
        &mut parse.references,
        &parse.global,
        native::ProductOccurrenceLimits::new(
            product_occurrence_output_limit,
            product_occurrence_depth_limit,
        ),
        Some(ctx),
    )?;
    // The transfer ledger is verified before DecodeResult construction, so its
    // identity checks require the same canonical arena order as the result.
    ir.finalize();
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
    let global_table = parse.global.global_table();
    if !ctx.container_only() {
        let attributed_before_generic = attributed_sequences(&losses);
        let generic_losses = parse
            .directory
            .iter()
            .filter(|entry| entry.entity_type != 0)
            .filter(|entry| {
                if !crate::profile::envelope_a_admits(entry.entity_type, entry.form, global_table) {
                    return true;
                }
                !projection.decoded.contains(&entry.sequence)
                    && !projection.consumed.contains(&entry.sequence)
                    && !attributed_before_generic.contains(&entry.sequence)
            })
            .map(|entry| {
                let note = if crate::profile::envelope_a_admits(entry.entity_type, entry.form, global_table)
                {
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
    let attributed = if ctx.container_only() {
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
        let note = if ctx.container_only() {
            "native record retained; semantic projection was not requested"
        } else if !crate::profile::envelope_a_admits(entry.entity_type, entry.form, global_table) {
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
            // The generic pass attributes every envelope-admitted record that
            // is neither decoded nor consumed, so no decode reaches this arm;
            // it names that state truthfully if a later pass admits it.
            "native record retained; no standalone neutral projection was required"
        };
        transfer_ledger.record(
            format!("D{}", entry.sequence),
            TransferOutcome::Retained {
                target: format!("iges:entity:directory#{}", entry.sequence),
                note: Some(note.into()),
            },
        );
    }
    for record in &parse.quarantined_directory {
        transfer_ledger.record(
            format!("D{}", record.sequence),
            TransferOutcome::Retained {
                target: record.identity(),
                note: Some(
                    "quarantined directory record retained; typed Directory fields were not recovered"
                        .into(),
                ),
            },
        );
    }
    for record in &parse.quarantined_parameters {
        transfer_ledger.record(
            format!("D{}:parameter", record.sequence),
            TransferOutcome::Retained {
                target: record.identity(),
                note: Some("quarantined parameter data retained; tokens were not recovered".into()),
            },
        );
    }
    transfer_ledger
        .verify(&cadmpeg_ir::index::ModelIndex::new(&ir))
        .map_err(|message| {
            CodecError::malformed(format_args!(
                "IGES transfer ledger is inconsistent: {message}"
            ))
        })?;
    let mut notes = directory::summary_notes(&parse.directory);
    notes.extend(parameter::summary_notes(&parse.parameters));
    notes.extend(graph::summary_notes(&parse.references));
    let document_digest =
        document_local_sha256_with_charge(&ir, "iges", crate::SOURCE_IMAGE_ID, |bytes| {
            ctx.charge_work(bytes, "iges_document_digest")
        })?;
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.into(), document_digest);
    }
    let mut body = DecodeBody::new(geometry_transferred);
    body.losses = losses;
    body.notes = notes;
    body.transfer_ledger = transfer_ledger;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
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
    Err(CodecError::malformed(format_args!(
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
) -> Result<cadmpeg_ir::codec::DecodeResult, cadmpeg_ir::codec::DecodeFailure> {
    use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence};

    struct OccurrenceLimitCodec {
        output_limit: usize,
        depth_limit: usize,
    }

    impl CodecBackend for OccurrenceLimitCodec {
        const FORMAT: &'static str = crate::dialect::FORMAT;

        fn detect_impl(&self, _prefix: &[u8]) -> Confidence {
            Confidence::High
        }

        fn inspect_impl(
            &self,
            _ctx: &DecodeContext<'_>,
            _root: cadmpeg_core::decode::View<'_>,
        ) -> Result<cadmpeg_ir::ContainerSummary, CodecError> {
            unreachable!("test backend is decode-only")
        }

        fn decode_impl(
            &self,
            ctx: &DecodeContext<'_>,
            root: cadmpeg_core::decode::View<'_>,
        ) -> Result<Decoded, CodecError> {
            decode_with_occurrence_limits(
                root.window(),
                root.window(),
                Representation::FixedAscii,
                self.output_limit,
                self.depth_limit,
                ctx,
            )
        }
    }

    Codec::decode(
        &OccurrenceLimitCodec {
            output_limit,
            depth_limit,
        },
        &mut std::io::Cursor::new(bytes),
        &options,
    )
}

fn charge_entities(
    ctx: &DecodeContext<'_>,
    count: u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_entities(count, operation)
}

fn charge_work(
    ctx: &DecodeContext<'_>,
    units: u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(units, operation)
}

#[cfg(test)]
mod tests;
