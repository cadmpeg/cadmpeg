// SPDX-License-Identifier: Apache-2.0
//! Physical graph to CADIR native preservation and loss reporting.

use crate::loss::IgesLossCode;
use crate::{card, directory, entities, global, graph, native, parameter};
use cadmpeg_core::decode::{DecodeContext, DecodeMode};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeOptions, DecodeResult};
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::report::{DecodeReport, LossNote, Severity, TransferDisposition, TransferLedger};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::{CadIr, RetainedSourceRecord, SourceFidelity, SourceMeta};
use std::collections::{BTreeMap, BTreeSet};

fn bytes_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn source_meta(global: &global::Global) -> SourceMeta {
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
        global.version_flag().to_string(),
    );
    if let Some(value) = global.units_name() {
        attributes.insert("native_units".into(), value);
    } else if let Some(value) = global.units_name_bytes() {
        attributes.insert("native_units_bytes_hex".into(), bytes_hex(value));
    }
    if let Some(value) = global.sender_product() {
        attributes.insert("sender_product".into(), value);
    } else if let Some(value) = global.sender_product_bytes() {
        attributes.insert("sender_product_bytes_hex".into(), bytes_hex(value));
    }
    if let Some(value) = global.native_file_name() {
        attributes.insert("native_file_name".into(), value);
    } else if let Some(value) = global.native_file_name_bytes() {
        attributes.insert("native_file_name_bytes_hex".into(), bytes_hex(value));
    }
    SourceMeta {
        format: "iges".into(),
        attributes,
    }
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
    if !options.container_only && scan.noncanonical_before_terminate {
        return Err(crate::error::malformed(
            "IGES Fixed ASCII has a short or overlong line before Terminate",
        ));
    }
    let global = global::parse(&scan)?;
    if !matches!(global.version(), "5.1" | "5.2" | "5.3") {
        return Err(CodecError::NotImplemented(format!(
            "IGES Fixed ASCII version {} decode; target envelope is 5.1, 5.2, or 5.3",
            global.version()
        )));
    }
    if !options.container_only {
        if let Some(field) = global.invalid_string_fields().next() {
            return Err(crate::error::malformed(format!(
                "IGES Global field {field} contains a non-ASCII or control byte"
            )));
        }
    }
    if !options.container_only && !global.has_supported_length_factor() {
        return Err(CodecError::NotImplemented(
            "IGES units flag 3 names a unit without a known millimetre factor".into(),
        ));
    }
    let directory = directory::parse(&scan)?;
    charge_entities(ctx, directory.len() as u64, "iges_directory_entries")?;
    entities::geometry::enforce_transform_depth(&directory, ctx)?;
    let parameters = parameter::assemble_with_context(&scan, &directory, &global, ctx)?;
    let parameter_tokens = parameters
        .iter()
        .map(|record| record.tokens.len() as u64)
        .sum();
    charge_work(ctx, parameter_tokens, "iges_parameter_parse")?;
    let directory_by_sequence = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let parameter_by_sequence = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
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
    ir.source = Some(source_meta(&global));
    let projection = if options.container_only {
        entities::geometry::Projection {
            handled: BTreeSet::default(),
            decoded: BTreeSet::default(),
            losses: Vec::new(),
        }
    } else {
        charge_work(ctx, parameter_tokens, "iges_geometry_projection")?;
        entities::geometry::project_geometry(&mut ir, &directory, &parameters, &global, ctx)?
    };
    let structure_decoded = projection
        .decoded
        .iter()
        .filter(|sequence| {
            directory_by_sequence
                .get(sequence)
                .is_some_and(|entry| matches!(entry.entity_type, 308 | 320 | 408 | 420))
        })
        .copied()
        .collect::<BTreeSet<_>>();
    charge_work(ctx, parameter_tokens, "iges_native_projection")?;
    let product_occurrence_expansion = native::store(
        &mut ir,
        &scan,
        &directory,
        &parameters,
        &mut references,
        &global,
        (!options.container_only).then_some(&structure_decoded),
        native::ProductOccurrenceLimits::new(
            product_occurrence_output_limit,
            product_occurrence_depth_limit,
        ),
        ctx,
    )?;
    ir.finalize();
    let document_digest = crate::document_digest(&ir);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.into(), document_digest);
    }
    source_fidelity.finalize();

    let geometry_transferred = !projection.decoded.is_empty();
    let mut losses = projection.losses;
    losses.extend(graph::losses(&references, &scan, &parameters));
    losses.extend(parameters.iter().filter_map(|record| {
        let count = parameter::ambiguous_trailing_pointer_group_count_with_records(
            record,
            &directory_by_sequence,
            &parameter_by_sequence,
        )?;
        let entry = directory_by_sequence.get(&record.directory_sequence)?;
        Some(
            IgesLossCode::AmbiguousTrailingPointerGroups
                .note(format!(
                    "IGES Directory Entry D{} has {count} fully valid trailing pointer-group boundaries; entity-specific parameter arity is required",
                    entry.sequence
                ))
                .with_provenance(entry.loss_provenance()),
        )
    }));
    if product_occurrence_expansion.output_truncated {
        losses.push(
            IgesLossCode::OccurrenceExpansionOutputTruncated
                .note("IGES product occurrence expansion reached its configured output limit"),
        );
    }
    if product_occurrence_expansion.depth_truncated {
        losses.push(
            IgesLossCode::OccurrenceExpansionDepthTruncated.note(
                "IGES product occurrence expansion reached its configured nesting-depth limit",
            ),
        );
    }
    if product_occurrence_expansion
        .diagnostics
        .root_inference_blocked()
    {
        losses.push(IgesLossCode::OccurrenceRootInferenceBlocked.note(
            "IGES product occurrence root inference was suppressed because a definition member list is malformed",
        ));
    }
    if product_occurrence_expansion.diagnostics.invalid_structure() {
        losses.push(IgesLossCode::OccurrenceInvalidStructure.note(
            "IGES product occurrence expansion omitted a definition or instance rejected by structure validation",
        ));
    }
    if !options.container_only {
        losses.extend(
            directory
                .iter()
                .filter(|entry| {
                    entry.entity_type != 0
                        && (!crate::profile::envelope_a_admits(entry.entity_type, entry.form)
                            || !projection.handled.contains(&entry.sequence))
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
                }),
        );
        charge_work(
            ctx,
            ir.model.entity_count() as u64,
            "iges_semantic_validation",
        )?;
        reject_invalid_semantic_ir(&ir, &losses)?;
        if options.policy.mode == DecodeMode::Strict {
            if let Some(loss) = losses
                .iter()
                .find(|loss| loss.severity >= Severity::Warning)
            {
                return Err(CodecError::Malformed(format!(
                    "strict mode rejects {}: {}",
                    loss.code, loss.message
                )));
            }
        }
    }
    let mut transfer_ledger = TransferLedger::default();
    for entry in directory.iter().filter(|entry| entry.entity_type != 0) {
        let note = if options.container_only {
            "native record retained; semantic projection was not requested"
        } else if projection.decoded.contains(&entry.sequence) {
            "native record retained; semantic projection emitted"
        } else if crate::profile::envelope_a_admits(entry.entity_type, entry.form) {
            "native record retained; semantic projection omitted with an attributed loss"
        } else {
            "native record retained; entity is outside the declared read envelope"
        };
        transfer_ledger.record(
            format!("D{}", entry.sequence),
            Some(format!("iges:entity:directory#{}", entry.sequence)),
            TransferDisposition::Retained,
            Some(note.into()),
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
pub(crate) fn reject_invalid_semantic_ir(
    ir: &CadIr,
    losses: &[LossNote],
) -> Result<(), CodecError> {
    let validation = cadmpeg_ir::validate_neutral(ir, losses.to_vec());
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
