// SPDX-License-Identifier: Apache-2.0
//! Physical graph to CADIR native preservation and loss reporting.

use crate::loss::IgesLossCode;
use crate::representation::Representation;
use crate::{card, directory, entities, global, graph, native, parameter};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
#[cfg(test)]
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::hash::{document_local_sha256, DOCUMENT_FINGERPRINT_METADATA_KEY};
use cadmpeg_ir::report::{
    decode::{TransferLedger, TransferOutcome},
    loss::LossNote,
    Severity,
};
use cadmpeg_ir::source_fidelity::parts::{ParserPartOutput, PartOutput};
use cadmpeg_ir::ContainerSummary;
use cadmpeg_ir::{CadIr, RetainedSourceRecord, SourceFidelity, SourceMeta};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

struct ParsingState<'a> {
    parts: ParserPartOutput,
    binary_identities: Option<&'a crate::binary::BinaryIdentities>,
    binary_admission: Option<crate::binary::BinaryAdmission>,
    compressed: Option<crate::compressed::parts::Admission>,
}

impl ParsingState<'_> {
    fn new() -> Self {
        Self {
            parts: ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges),
            binary_identities: None,
            binary_admission: None,
            compressed: None,
        }
    }
}

fn source_meta(
    global: &global::ResolvedGlobal,
    representation: Representation,
    primary: cadmpeg_core::dialect::DialectMatch,
) -> SourceMeta {
    let mut attributes = BTreeMap::from([
        (
            cadmpeg_core::nonblank_const!("representation"),
            representation.as_str().to_owned(),
        ),
        (
            cadmpeg_core::nonblank_const!("parameter_delimiter"),
            char::from(global.parameter_delimiter).to_string(),
        ),
        (
            cadmpeg_core::nonblank_const!("record_delimiter"),
            char::from(global.record_delimiter).to_string(),
        ),
    ]);
    if let Some(value) = global.units_name() {
        attributes.insert(
            cadmpeg_core::nonblank_const!("native_units"),
            value.to_owned(),
        );
    }
    if let Some(value) = global.sender_product() {
        attributes.insert(
            cadmpeg_core::nonblank_const!("sender_product"),
            value.to_owned(),
        );
    }
    if let Some(value) = global.native_file_name() {
        attributes.insert(
            cadmpeg_core::nonblank_const!("native_file_name"),
            value.to_owned(),
        );
    }
    SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(primary),
        attributes,
    )
}

fn push_occurrence_loss(
    losses: &mut Vec<LossNote>,
    code: IgesLossCode,
    message: fmt::Arguments<'_>,
    source_sequence: u32,
    directory: &[directory::DirectoryEntry],
) {
    let note = code.note(message.to_string());
    if let Some(entry) = directory
        .iter()
        .find(|entry| entry.sequence == source_sequence)
    {
        losses.push(note.with_provenance(entry.loss_provenance()));
    } else {
        losses.push(note);
    }
}

fn attributed_sequences(losses: &[LossNote]) -> BTreeSet<u32> {
    fn rendered_sequence(rendered: &str) -> Option<u32> {
        if rendered.len() > 1 && rendered.starts_with('0')
            || !rendered.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        rendered.parse::<u32>().ok()
    }

    let sequences = losses
        .iter()
        .filter_map(|loss| loss.provenance.as_ref()?.tag.as_deref())
        .filter_map(|tag| {
            tag.strip_prefix("directory_entry:D")
                .and_then(rendered_sequence)
                .or_else(|| {
                    let (head, _) = tag.split_once(':')?;
                    rendered_sequence(head.strip_prefix('D')?)
                })
        });
    let mut attributed = BTreeSet::new();
    for sequence in sequences {
        attributed.insert(sequence);
    }
    attributed
}

fn projection_directory(
    directory: &[directory::DirectoryEntry],
    quarantined: &BTreeSet<u32>,
) -> Option<Vec<directory::DirectoryEntry>> {
    if quarantined.is_empty() {
        return None;
    }
    let mut projected = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| !quarantined.contains(&entry.sequence))
    {
        projected.push(entry.clone());
    }
    Some(projected)
}

fn quarantined_parameter_sequences(
    records: &[parameter::QuarantinedParameterRecord],
) -> BTreeSet<u32> {
    let mut sequences = BTreeSet::new();
    for record in records {
        sequences.insert(record.sequence);
    }
    sequences
}

fn source_fidelity(
    source_bytes: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<SourceFidelity, CodecError> {
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(source_bytes.len()),
        "iges_source_image",
    )?;
    let retained_source = source_bytes.to_vec();
    let id = crate::SOURCE_IMAGE_ID.to_owned();
    let id = cadmpeg_ir::ids::UnknownId::try_from(id)
        .map_err(|_| CodecError::Malformed("IGES source image id is invalid".into()))?;
    let owner = "iges".to_owned();

    let mut fidelity = SourceFidelity::default();
    fidelity.insert_retained_record(id, RetainedSourceRecord::whole(owner, retained_source))?;
    Ok(fidelity)
}

fn append_generic_losses(
    losses: &mut Vec<LossNote>,
    directory: &[directory::DirectoryEntry],
    projection: &entities::geometry::Projection,
    attributed: &BTreeSet<u32>,
    global_table: global::GlobalTable,
) {
    for entry in directory.iter().filter(|entry| entry.entity_type != 0) {
        let admitted =
            crate::profile::envelope_a_admits(entry.entity_type, entry.form, global_table);
        if admitted
            && (projection.decoded.contains(&entry.sequence)
                || projection.consumed.contains(&entry.sequence)
                || attributed.contains(&entry.sequence))
        {
            continue;
        }

        let (code, message) = if admitted {
            (
                IgesLossCode::EntityRetainedUnprojected,
                format!(
                    "IGES entity type {} form {} retained without neutral projection",
                    entry.entity_type, entry.form
                ),
            )
        } else {
            (
                IgesLossCode::EntityOutsideEnvelope,
                format!(
                    "IGES entity type {} form {} is outside the Fixed ASCII mechanical/document envelope",
                    entry.entity_type, entry.form
                ),
            )
        };

        losses.push(code.note(message).with_provenance(entry.loss_provenance()));
    }
}

fn record_retained_transfer(
    ledger: &mut TransferLedger,
    source: String,
    target: String,
    note: &'static str,
) {
    ledger.record(
        source,
        TransferOutcome::Retained {
            target,
            note: Some(note.to_owned()),
        },
    );
}

fn annotate_representation(
    summary: &mut ContainerSummary,
    representation: Representation,
    source_size: usize,
) {
    if representation == Representation::FixedAscii {
        return;
    }
    summary.container_kind = representation.container_kind();
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

fn mark_quarantined_placements(
    projection: &mut entities::geometry::Projection,
    directory: &[directory::DirectoryEntry],
    quarantined: &BTreeSet<u32>,
) {
    for entry in directory.iter().filter(|entry| {
        matches!(entry.entity_type, 408 | 420)
            && entry.form == 0
            && quarantined.contains(&entry.sequence)
    }) {
        projection.placement_rejections.insert(
            entry.sequence,
            entities::structure::PlacementRejection::MissingRecord,
        );
    }
}

struct PhysicalParse<'a> {
    scan: card::CardScan<'a>,
    global: global::ResolvedGlobal,
    global_losses: Vec<LossNote>,
    directory: Vec<directory::DirectoryEntry>,
    quarantined_directory: Vec<directory::QuarantinedDirectoryRecord>,
    parameters: Vec<parameter::ParameterRecord>,
    trailing_pointer_analysis: BTreeMap<u32, parameter::TrailingPointerAnalysis>,
    quarantined_parameters: Vec<parameter::QuarantinedParameterRecord>,
    given_up_parameters: BTreeSet<u32>,
    framing_recoveries: card::FramingRecoveries,
    references: BTreeMap<u32, Vec<graph::ReferenceEdge>>,
    parser_parts: ParserPartOutput,
    parts: PartOutput,
}

impl<'a> PhysicalParse<'a> {
    fn run_with_source(
        bytes: &'a [u8],
        source: Option<View<'a>>,
        ctx: &DecodeContext<'_>,
        parsing: ParsingState<'_>,
    ) -> Result<Self, CodecError> {
        let ParsingState {
            parts: mut parser_parts,
            binary_identities,
            binary_admission,
            compressed,
        } = parsing;
        let scan = match source {
            Some(source) => card::scan_source(source, ctx)?,
            None => card::scan(bytes)?,
        };
        let (prepared_global, prepared_directory, prepared_parameters) = match compressed {
            Some(admission) => (
                Some((admission.global, admission.losses)),
                Some(admission.directory),
                Some(admission.parameters),
            ),
            None => (None, None, None),
        };
        let (binary_global, binary_parameters) = match binary_admission {
            Some(admission) => (
                Some((admission.global, admission.losses)),
                Some(admission.parameters),
            ),
            None => (None, None),
        };
        let (global, mut global_losses) = if let Some(global) = prepared_global.or(binary_global) {
            global
        } else if scan.source_view().is_some() {
            global::parts::parse(&scan, ctx, &mut parser_parts)?
        } else {
            global::parse(&scan)?
        };
        // A given-up Directory Entry owns its Parameter Data cards, as a
        // quarantined one does: they are not unclaimed (#215 rules 2-3).
        let (directory, quarantined_directory, given_up_directory) =
            if let Some(directory) = prepared_directory {
                (directory, Vec::new(), Vec::new())
            } else if scan.source_view().is_some() {
                let parsed =
                    directory::parts::parse(&scan, global.global_table(), ctx, &mut parser_parts)?;
                (parsed.entries, Vec::new(), parsed.given_up)
            } else if let Some(identities) = binary_identities {
                let (directory, quarantined) =
                    directory::parse_with_keys(&scan, global.global_table(), ctx, |grammar| {
                        identities.semantic_sequence(grammar)
                    })?;
                (directory, quarantined, Vec::new())
            } else {
                let (directory, quarantined) = directory::parse(&scan, global.global_table(), ctx)?;
                (directory, quarantined, Vec::new())
            };
        global_losses.extend(directory::metadata_losses(&directory));
        let mut parts = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
        let parameter::ParameterAssembly {
            records: parameters,
            trailing_pointer_analysis,
            quarantined: quarantined_parameters,
            recoveries: parameter_recoveries,
            given_up: given_up_parameters,
        } = {
            let excluded = given_up_directory
                .iter()
                .copied()
                .chain(quarantined_directory.iter().map(|record| record.sequence));
            if let Some(parameters) = prepared_parameters {
                parameters
            } else if let Some(admitted) = binary_parameters {
                // Each Binary Parameter record is lexed inside its part; no
                // record is assembled from the normalized cards.
                parameter::finish_admitted(&directory, admitted, &global)
            } else if scan.source_view().is_some() {
                parameter::parts::assemble(&scan, &directory, excluded, &global, ctx, &mut parts)?
            } else {
                parameter::assemble(&scan, &directory, excluded, &global)?
            }
        };
        let conditional_losses = global
            .conditional_double_precision_losses(parameter::uses_double_precision(&parameters));

        global_losses.extend(conditional_losses);
        let references = graph::build(&directory);
        let mut scan = scan;
        let mut framing_recoveries = std::mem::take(&mut scan.recoveries);
        framing_recoveries.merge(parameter_recoveries)?;
        Ok(Self {
            scan,
            global,
            global_losses,
            directory,
            quarantined_directory,
            parameters,
            trailing_pointer_analysis,
            quarantined_parameters,
            given_up_parameters,
            framing_recoveries,
            references,
            parser_parts,
            parts,
        })
    }

    fn admission_losses(&mut self) -> Vec<LossNote> {
        let mut losses = Vec::new();
        if let Some(loss) = crate::dialect::dialect_loss(&self.global) {
            losses.push(loss);
        }

        losses.extend(std::mem::take(&mut self.global_losses));
        if matches!(self.global.global_table(), global::GlobalTable::V4_0) {
            let post_terminate_count = self.scan.post_terminate_count();
            if post_terminate_count > 0 {
                let message = format!(
                    "IGES 4.0 requires the Terminate Section to be the last physical line; \
                     retained {post_terminate_count} trailing record(s) as source data"
                );
                let code = IgesLossCode::GlobalNoncanonicalFraming;

                losses.push(code.note(message));
            }
        }
        losses
    }

    fn record_losses(&self) -> Result<Vec<LossNote>, CodecError> {
        let mut losses = self.framing_recoveries.notes();
        for record in &self.quarantined_directory {
            losses.push(record.loss_note());
        }
        for record in &self.quarantined_parameters {
            losses.push(record.loss_note()?);
        }
        Ok(losses)
    }
}

pub(crate) fn inspect(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    representation: Representation,
) -> Result<ContainerSummary, CodecError> {
    inspect_with_parsing(
        ctx,
        source.window(),
        source,
        representation,
        ParsingState::new(),
    )
}

pub(crate) fn inspect_binary(
    ctx: &DecodeContext<'_>,
    normalized: crate::binary::NormalizedBinary,
    source: View<'_>,
    parts: ParserPartOutput,
) -> Result<ContainerSummary, CodecError> {
    inspect_with_parsing(
        ctx,
        &normalized.bytes,
        source,
        Representation::Binary,
        ParsingState {
            parts,
            binary_identities: Some(&normalized.identities),
            binary_admission: Some(normalized.admission),
            compressed: None,
        },
    )
}

pub(crate) fn inspect_compressed(
    ctx: &DecodeContext<'_>,
    normalized: crate::compressed::parts::Normalized,
    source: View<'_>,
) -> Result<ContainerSummary, CodecError> {
    inspect_with_parsing(
        ctx,
        &normalized.bytes,
        source,
        Representation::CompressedAscii,
        ParsingState {
            parts: normalized.parts,
            binary_identities: None,
            binary_admission: None,
            compressed: Some(normalized.admission),
        },
    )
}

/// Inspect through the parse that decode runs, so both report the same
/// record losses for one file.
fn inspect_with_parsing(
    ctx: &DecodeContext<'_>,
    parse_bytes: &[u8],
    source: View<'_>,
    representation: Representation,
    parsing: ParsingState<'_>,
) -> Result<ContainerSummary, CodecError> {
    let view = (representation == Representation::FixedAscii).then_some(source);
    let mut parse = PhysicalParse::run_with_source(parse_bytes, view, ctx, parsing)?;
    let parser_parts = std::mem::replace(
        &mut parse.parser_parts,
        ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges),
    );
    let (_, _, _, mut parser_losses) = parser_parts.into_parts(ctx)?.into_parts();
    let parts = std::mem::replace(
        &mut parse.parts,
        PartOutput::new(cadmpeg_ir::CodecFormat::Iges),
    );
    let (_, _, _, mut part_losses) = parts.into_parts(ctx)?.into_parts();
    let primary = crate::dialect::classify(representation, &parse.global);
    let mut losses = parse.admission_losses();
    losses.append(&mut parser_losses);
    losses.append(&mut part_losses);
    let record_losses = parse.record_losses()?;

    losses.extend(record_losses);
    let mut summary = card::summarize(&parse.scan, primary)?;
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
    annotate_representation(&mut summary, representation, source.window().len());
    Ok(summary)
}

pub(crate) fn decode(
    parse_bytes: &[u8],
    source: View<'_>,
    representation: Representation,
    ctx: &DecodeContext<'_>,
) -> Result<Decoded, CodecError> {
    decode_with_parsing(
        parse_bytes,
        source,
        representation,
        ctx,
        ParsingState::new(),
    )
}

pub(crate) fn decode_binary(
    normalized: crate::binary::NormalizedBinary,
    source: View<'_>,
    ctx: &DecodeContext<'_>,
    parts: ParserPartOutput,
) -> Result<Decoded, CodecError> {
    decode_with_parsing(
        &normalized.bytes,
        source,
        Representation::Binary,
        ctx,
        ParsingState {
            parts,
            binary_identities: Some(&normalized.identities),
            binary_admission: Some(normalized.admission),
            compressed: None,
        },
    )
}

pub(crate) fn decode_compressed(
    normalized: crate::compressed::parts::Normalized,
    source: View<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Decoded, CodecError> {
    decode_with_parsing(
        &normalized.bytes,
        source,
        Representation::CompressedAscii,
        ctx,
        ParsingState {
            parts: normalized.parts,
            binary_identities: None,
            binary_admission: None,
            compressed: Some(normalized.admission),
        },
    )
}

fn decode_with_parsing(
    parse_bytes: &[u8],
    source: View<'_>,
    representation: Representation,
    ctx: &DecodeContext<'_>,
    parsing: ParsingState<'_>,
) -> Result<Decoded, CodecError> {
    let output = native::MAX_PRODUCT_OCCURRENCES;
    let depth = usize::try_from(ctx.policy().limits.max_recursion_depth)
        .ok()
        .map_or(native::MAX_PRODUCT_OCCURRENCE_DEPTH, |policy| {
            policy.min(native::MAX_PRODUCT_OCCURRENCE_DEPTH)
        });
    decode_with_occurrence_limits(
        parse_bytes,
        source,
        representation,
        output,
        depth,
        ctx,
        parsing,
    )
}

fn decode_with_occurrence_limits(
    parse_bytes: &[u8],
    source: View<'_>,
    representation: Representation,
    product_occurrence_output_limit: usize,
    product_occurrence_depth_limit: usize,
    ctx: &DecodeContext<'_>,
    parsing: ParsingState<'_>,
) -> Result<Decoded, CodecError> {
    let mut source_fidelity = source_fidelity(source.window(), ctx)?;
    let view = (representation == Representation::FixedAscii).then_some(source);
    let mut parse = PhysicalParse::run_with_source(parse_bytes, view, ctx, parsing)?;
    let parser_parts = std::mem::replace(
        &mut parse.parser_parts,
        ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges),
    );
    let (_, parser_native, parser_fidelity, mut parser_losses) =
        parser_parts.into_parts(ctx)?.into_parts();
    let parts = std::mem::replace(
        &mut parse.parts,
        PartOutput::new(cadmpeg_ir::CodecFormat::Iges),
    );
    let (part_model, part_native, part_fidelity, mut part_losses) =
        parts.into_parts(ctx)?.into_parts();
    let length_context = parse.global.length_context();
    let mut unavailable_parameter_sequences =
        quarantined_parameter_sequences(&parse.quarantined_parameters);
    unavailable_parameter_sequences.extend(&parse.given_up_parameters);
    let projected_directory =
        projection_directory(&parse.directory, &unavailable_parameter_sequences);
    let projected_directory = projected_directory.as_deref().unwrap_or(&parse.directory);
    let card_layout = parse
        .scan
        .lines
        .iter()
        .enumerate()
        .map(|(index, card)| {
            let line = card.physical();
            (
                index + 1,
                line.offset,
                line.payload.len(),
                line.line_ending().len(),
            )
        })
        .collect::<Vec<_>>();
    source_fidelity.metadata.insert(
        "iges.card_layout".to_owned(),
        serde_json::to_string(&card_layout).map_err(CodecError::malformed)?,
    );
    let entity_layout = parse
        .directory
        .iter()
        .map(|entry| {
            (
                entry.sequence,
                entry.parameter_start,
                entry.parameter_line_count,
            )
        })
        .collect::<Vec<_>>();
    source_fidelity.metadata.insert(
        "iges.entity_layout".to_owned(),
        serde_json::to_string(&entity_layout).map_err(CodecError::malformed)?,
    );
    let token_layout = parse
        .parameters
        .iter()
        .map(|record| {
            (
                record.directory_sequence,
                record.line_range.clone(),
                record
                    .tokens()
                    .iter()
                    .map(|token| token.as_ref().map(|token| token.span.clone()))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    source_fidelity.metadata.insert(
        "iges.parameter_layout".to_owned(),
        serde_json::to_string(&token_layout).map_err(CodecError::malformed)?,
    );

    let primary = crate::dialect::classify(representation, &parse.global);
    let mut ir = CadIr::decoded(source_meta(&parse.global, representation, primary));
    ir.scope = if ctx.container_only() {
        cadmpeg_ir::ContentScope::container()
    } else {
        cadmpeg_ir::ContentScope::full()
    };
    ir.model = part_model;
    let mut invalid_resolution = false;
    if let Some(context) = &length_context {
        match cadmpeg_ir::scalar::PositiveLength::new(context.minimum_resolution_mm()) {
            Some(value) => ir.tolerances.linear = value,
            None => invalid_resolution = true,
        }
    }
    let mut projection = match length_context.filter(|_| !ctx.container_only()) {
        Some(context) => entities::geometry::project_geometry(
            &mut ir,
            projected_directory,
            &parse.parameters,
            &parse.trailing_pointer_analysis,
            &context,
            ctx,
        )?,
        None => entities::geometry::Projection::default(),
    };
    mark_quarantined_placements(
        &mut projection,
        &parse.directory,
        &unavailable_parameter_sequences,
    );
    let semantic_structure_admitted = (!ctx.container_only()).then_some(&projection);
    let native::NativeStoreResult {
        projection_losses,
        occurrence_expansion: product_occurrence_expansion,
        ambiguous_parameter_boundaries,
        overdeclared_counts,
        unstatable_attribute_tables,
    } = native::store(
        &mut ir,
        native::NativeStoreInputs {
            scan: &parse.scan,
            directory: &parse.directory,
            parameters: &parse.parameters,
            trailing_pointer_analysis: &parse.trailing_pointer_analysis,
            quarantine: native::QuarantinedRecords {
                directory: &parse.quarantined_directory,
                parameters: &parse.quarantined_parameters,
            },
            structure_admitted: semantic_structure_admitted,
            sequences: &projection.sequences,
            boundary_vertex_derivations: &projection.boundary_vertex_derivations,
        },
        &mut parse.references,
        &parse.global,
        native::ProductOccurrenceLimits::new(
            product_occurrence_output_limit,
            product_occurrence_depth_limit,
        ),
        ctx,
    )?;
    for mut native in [parser_native, part_native] {
        let source_arenas = native.namespace_mut("iges").arenas_mut();
        for (name, mut records) in std::mem::take(source_arenas) {
            let destination = ir
                .native
                .namespace_mut("iges")
                .arenas_mut()
                .entry(name)
                .or_default();
            // The part helper charged these records when its root output
            // received them. This move is not a new retained copy.
            let bytes = cadmpeg_core::decode::u64_from_index(
                records
                    .len()
                    .checked_mul(size_of::<cadmpeg_ir::native::NativeRecord>())
                    .ok_or_else(|| {
                        CodecError::malformed("IGES parser record composition overflows")
                    })?,
            );
            destination.try_reserve(records.len()).map_err(|_| {
                cadmpeg_core::decode::ResourceLimit::allocation_failed(
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                    ctx.policy().limits.max_retained_bytes,
                    bytes,
                    "IGES parser record composition",
                )
            })?;
            destination.append(&mut records);
        }
    }
    source_fidelity.append(parser_fidelity)?;
    source_fidelity.append(part_fidelity)?;
    source_fidelity.install_record_captures(ctx, |_, _| Ok(()))?;
    // The transfer ledger is verified before DecodeResult construction, so its
    // identity checks require the same canonical arena order as the result.
    ir.finalize();
    let geometry_transferred = !projection.decoded.is_empty();
    let mut losses = parse.admission_losses();
    losses.append(&mut parser_losses);
    losses.append(&mut part_losses);
    losses.extend(projection_losses);
    if invalid_resolution
        && !losses.iter().any(|loss| {
            loss.code.local_code() == IgesLossCode::GlobalSemanticContextSubstituted.code()
        })
    {
        let message =
            "minimum resolution must be positive and finite; the default linear tolerance is used"
                .to_string();
        let code = IgesLossCode::GlobalSemanticContextSubstituted;

        losses.push(code.note(message));
    }

    losses.extend(std::mem::take(&mut projection.losses));
    entities::ownership::mark_supports(&mut ir, &parse.directory, &parse.parameters);
    let graph_losses = graph::losses(&parse.references, &parse.scan, &parse.parameters);

    losses.extend(graph_losses);
    let record_losses = parse.record_losses()?;

    losses.extend(record_losses);
    if let Some(source_sequence) = product_occurrence_expansion.output_truncated_at {
        push_occurrence_loss(
            &mut losses,
            IgesLossCode::OccurrenceExpansionOutputTruncated,
            format_args!("IGES product occurrence expansion reached its configured output limit"),
            source_sequence,
            &parse.directory,
        );
    }
    if let Some(source_sequence) = product_occurrence_expansion.depth_truncated_at {
        push_occurrence_loss(
            &mut losses,
            IgesLossCode::OccurrenceExpansionDepthTruncated,
            format_args!(
                "IGES product occurrence expansion reached its configured nesting-depth limit"
            ),
            source_sequence,
            &parse.directory,
        );
    }
    for source_sequence in product_occurrence_expansion.malformed_definition_sequences {
        push_occurrence_loss(
            &mut losses,
            IgesLossCode::OccurrenceRootInferenceBlocked,
            format_args!(
                "IGES product occurrence root inference was suppressed because a definition member list is malformed"
            ),
            source_sequence,
            &parse.directory,
        );
    }
    for source_sequence in product_occurrence_expansion.malformed_placement_sequences {
        push_occurrence_loss(
            &mut losses,
            IgesLossCode::OccurrencePlacementMalformed,
            format_args!(
                "IGES product occurrence expansion omitted an instance or member with malformed placement data"
            ),
            source_sequence,
            &parse.directory,
        );
    }
    for native::AmbiguousParameterBoundary {
        sequence: source_sequence,
        ambiguity,
    } in ambiguous_parameter_boundaries
    {
        let (candidate_count, kind) = match ambiguity {
            native::ParameterBoundaryAmbiguity::EquallyValid(count) => (count, "equally valid"),
            native::ParameterBoundaryAmbiguity::Structural(count) => (count, "structural"),
        };
        push_occurrence_loss(
            &mut losses,
            IgesLossCode::ParameterBoundaryAmbiguous,
            format_args!(
                "IGES Parameter Data has {candidate_count} {kind} trailing pointer-group boundaries; primary parameters and pointer ownership were not guessed"
            ),
            source_sequence,
            &parse.directory,
        );
    }
    for (source_sequence, crate::parameter::OverdeclaredCount { declared, present }) in
        overdeclared_counts
    {
        push_occurrence_loss(
            &mut losses,
            IgesLossCode::ParameterCountOverdeclared,
            format_args!(
                "IGES entity D{source_sequence} declares a counted list of {declared} \
                 items; its Parameter Data record holds {present} in whole or in part, \
                 so the list was not read"
            ),
            source_sequence,
            &parse.directory,
        );
    }
    for (source_sequence, refusal) in unstatable_attribute_tables {
        push_occurrence_loss(
            &mut losses,
            IgesLossCode::AttributeTableCountUnstatable,
            format_args!(
                "IGES attribute table instance D{source_sequence} {}, so no attribute row was read",
                refusal.reason()
            ),
            source_sequence,
            &parse.directory,
        );
    }
    let untyped_records = native::untyped_records(&ir, ctx)?;
    for record in &untyped_records {
        losses.push(IgesLossCode::EntityRetainedUnprojected.note(format!(
            "native record {record} contains source-only text or macro statements; their evidence is retained but their content is not typed"
        )));
    }
    if !untyped_records.is_empty() {
        source_fidelity.mark_untyped(crate::SOURCE_IMAGE_ID, [cadmpeg_ir::ArenaClass::Model])?;
    }
    let global_table = parse.global.global_table();
    if !ctx.container_only() {
        let mut attributed_before_generic = attributed_sequences(&losses);
        attributed_before_generic.extend(&parse.given_up_parameters);
        attributed_before_generic.extend(
            parse
                .parameters
                .iter()
                .filter(|record| record.has_absent_fields())
                .map(|record| record.directory_sequence),
        );
        append_generic_losses(
            &mut losses,
            &parse.directory,
            &projection,
            &attributed_before_generic,
            global_table,
        );
        reject_invalid_semantic_ir(ctx, &ir)?;
    }
    let attributed = if ctx.container_only() {
        BTreeSet::new()
    } else {
        let mut attributed = attributed_sequences(&losses);
        attributed.extend(&parse.given_up_parameters);
        attributed.extend(
            parse
                .parameters
                .iter()
                .filter(|record| record.has_absent_fields())
                .map(|record| record.directory_sequence),
        );
        attributed
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
        let source = format!("D{}", entry.sequence);
        let target = format!("iges:entity:directory#{}", entry.sequence);
        record_retained_transfer(&mut transfer_ledger, source, target, note);
    }
    for record in &parse.quarantined_directory {
        let source = format!("D{}", record.sequence);
        record_retained_transfer(
            &mut transfer_ledger,
            source,
            record.identity(),
            "quarantined directory record retained; typed Directory fields were not recovered",
        );
    }
    for record in &parse.quarantined_parameters {
        let source = format!("D{}:parameter", record.sequence);
        record_retained_transfer(
            &mut transfer_ledger,
            source,
            record.identity(),
            "quarantined parameter data retained; tokens were not recovered",
        );
    }
    let verification_index = cadmpeg_ir::index::ModelIndex::build(&ir);
    transfer_ledger
        .verify(&verification_index)
        .map_err(|message| {
            CodecError::malformed(format_args!(
                "IGES transfer ledger is inconsistent: {message}"
            ))
        })?;
    let mut notes = directory::summary_notes(&parse.directory);
    notes.extend(parameter::summary_notes(&parse.parameters));
    notes.extend(graph::summary_notes(&parse.references));
    let document_digest =
        document_local_sha256(ctx, &ir, ir.source.as_ref(), "iges", crate::SOURCE_IMAGE_ID)?;
    drop(verification_index);
    source_fidelity.metadata.insert(
        DOCUMENT_FINGERPRINT_METADATA_KEY.to_owned(),
        document_digest,
    );
    let mut body = DecodeBody::new(if ctx.container_only() {
        cadmpeg_ir::report::decode::DecodeTransfer {
            scope: cadmpeg_ir::ContentScope::container(),
            geometry_transferred: false,
        }
    } else {
        cadmpeg_ir::report::decode::DecodeTransfer {
            scope: cadmpeg_ir::ContentScope::full(),
            geometry_transferred,
        }
    });
    body.transfer.scope = ir.scope;
    // These source records were not delivered as typed geometry, or were
    // quarantined before semantic admission. Declare that at the retention
    // boundary; the independent loss table is only a conformance oracle.
    let untyped_geometry = parse.directory.iter().any(|entry| {
        matches!(
            entry.entity_type,
            100 | 102 | 104 | 108 | 110 | 112 | 114 | 116 | 118 | 120 | 122 | 126 | 128 | 130 | 140
                ..=144 | 186 | 190 | 192 | 194 | 196 | 198 | 502 | 504 | 508 | 510 | 514
        ) && (ctx.container_only()
            || (!projection.decoded.contains(&entry.sequence)
                && !projection.consumed.contains(&entry.sequence)))
    });
    if untyped_geometry
        || !parse.quarantined_directory.is_empty()
        || !parse.quarantined_parameters.is_empty()
    {
        source_fidelity.mark_untyped(crate::SOURCE_IMAGE_ID, [cadmpeg_ir::ArenaClass::Model])?;
    }
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
fn reject_invalid_semantic_ir(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<(), CodecError> {
    let validation = cadmpeg_ir::validate::validate_neutral_for_decode(ctx, ir, Vec::new())?;
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
    use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, FormatId};

    struct OccurrenceLimitCodec {
        output_limit: usize,
        depth_limit: usize,
    }

    impl CodecBackend for OccurrenceLimitCodec {
        const FORMAT: FormatId = FormatId::new(crate::dialect::FORMAT);

        fn detect_impl(
            &self,
            _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
            _prefix: cadmpeg_core::decode::View<'_>,
        ) -> Result<Confidence, cadmpeg_core::CodecError> {
            Ok(Confidence::High)
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
                root,
                Representation::FixedAscii,
                self.output_limit,
                self.depth_limit,
                ctx,
                ParsingState::new(),
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

#[cfg(test)]
mod tests;
