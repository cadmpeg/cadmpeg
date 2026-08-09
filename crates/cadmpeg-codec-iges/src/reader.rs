// SPDX-License-Identifier: Apache-2.0
//! Physical graph to CADIR native preservation and loss reporting.

use crate::{card, directory, entities, global, graph, native, parameter};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeOptions, DecodeResult};
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::report::{DecodeReport, LossNote, Severity};
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

pub(crate) fn decode(bytes: &[u8], options: DecodeOptions) -> Result<DecodeResult, CodecError> {
    let scan = card::scan(bytes)?;
    let global = global::parse(&scan)?;
    if !matches!(global.version(), "5.1" | "5.2" | "5.3") {
        return Err(CodecError::NotImplemented(format!(
            "IGES Fixed ASCII version {} decode; target envelope is 5.1, 5.2, or 5.3",
            global.version()
        )));
    }
    let directory = directory::parse(&scan)?;
    let parameters = parameter::assemble(&scan, &directory, &global)?;
    let references = graph::build(&directory);
    let mut source_fidelity = SourceFidelity::default();
    source_fidelity.retained_records.push(RetainedSourceRecord {
        id: crate::SOURCE_IMAGE_ID.into(),
        stream: "iges".into(),
        offset: 0,
        byte_len: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        data: Some(bytes.to_vec()),
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
        entities::geometry::project_geometry(&mut ir, &directory, &parameters, &global)
    };
    let product_occurrences_truncated = native::store(
        &mut ir,
        &scan,
        &directory,
        &parameters,
        &references,
        &global,
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
    if product_occurrences_truncated {
        losses.push(LossNote {
            code: cadmpeg_ir::LossKind::DecodeDiagnostic,
            severity: Severity::Warning,
            message: "IGES product occurrence expansion reached its configured output limit".into(),
            provenance: None,
        });
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
                .map(|entry| LossNote {
                    code: cadmpeg_ir::LossKind::RecordNotTyped,
                    severity: Severity::Warning,
                    message: if crate::profile::envelope_a_admits(entry.entity_type, entry.form) {
                        format!(
                            "IGES entity type {} form {} retained without neutral projection",
                            entry.entity_type, entry.form
                        )
                    } else {
                        format!(
                            "IGES entity type {} form {} is outside the Fixed ASCII mechanical/document envelope",
                            entry.entity_type, entry.form
                        )
                    },
                    provenance: None,
                }),
        );
    }
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
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses,
            notes,
        },
        source_fidelity,
    ))
}
