// SPDX-License-Identifier: Apache-2.0
//! Schema-aware STEP-to-IR decoding entry point.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;

use crate::parse::{self, Exchange, Value};

mod dependencies;
mod geometry;
mod pmi;
mod presentation;
mod product;
mod tessellation;
mod topology;
mod validation;

pub(super) const MAX_RECORD_GRAPH_DEPTH: usize = 256;

/// Decode a complete clear-text exchange structure.
pub fn decode(input: &[u8], options: DecodeOptions) -> Result<DecodeResult, CodecError> {
    let exchange = parse::parse(input).map_err(|error| CodecError::Malformed(error.to_string()))?;
    Ok(decode_exchange(input, options, &exchange))
}

pub(super) fn decode_exchange(
    input: &[u8],
    options: DecodeOptions,
    exchange: &Exchange,
) -> DecodeResult {
    decode_exchange_mode(input, options, exchange, true).0
}

pub(super) fn inspect_exchange(
    input: &[u8],
    exchange: &Exchange,
) -> (DecodeResult, BTreeSet<usize>) {
    decode_exchange_mode(input, DecodeOptions::default(), exchange, false)
}

fn decode_exchange_mode(
    input: &[u8],
    options: DecodeOptions,
    exchange: &Exchange,
    retain_opaque: bool,
) -> (DecodeResult, BTreeSet<usize>) {
    let mut ir = CadIr::empty(Units::default());
    let mut attributes = BTreeMap::new();
    attributes.insert("schema".into(), schema_name(exchange));
    attributes.insert("data_sections".into(), exchange.data.len().to_string());
    attributes.insert(
        "entity_instances".into(),
        exchange.records.len().to_string(),
    );
    ir.source = Some(SourceMeta {
        format: "step".into(),
        attributes,
    });

    let mut report = DecodeReport {
        format: "step".into(),
        container_only: options.container_only,
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
        losses: Vec::new(),
        notes: exchange
            .references
            .iter()
            .map(|entry| format!("external reference {} -> {}", entry.name, entry.uri))
            .collect(),
    };
    if options.container_only {
        return (DecodeResult::new(ir, report), BTreeSet::new());
    }

    let geometry = geometry::decode(exchange, &mut ir);
    let dependencies = dependencies::decode(exchange);
    let topology = topology::decode(exchange, &mut ir);
    let product = product::decode(exchange, &geometry, &mut ir);
    let tessellation = tessellation::decode(exchange, &geometry, &mut ir);
    let pmi = pmi::decode(exchange, &geometry, &mut ir);
    let presentation = presentation::decode(exchange, &mut ir);
    let validation = validation::decode(exchange, &geometry, &mut ir);
    report.notes.extend(dependencies.notes);
    report.notes.extend(validation.notes);
    report.geometry_transferred = !ir.model.points.is_empty()
        || !ir.model.curves.is_empty()
        || !ir.model.surfaces.is_empty()
        || !ir.model.bodies.is_empty()
        || !ir.model.tessellations.is_empty();
    report
        .losses
        .extend(geometry.warnings.into_iter().map(|message| LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(topology.warnings.into_iter().map(|message| LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(presentation.warnings.into_iter().map(|message| LossNote {
            category: LossCategory::Material,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(product.warnings.into_iter().map(|message| LossNote {
            category: LossCategory::Metadata,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(tessellation.warnings.into_iter().map(|message| LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(pmi.warnings.into_iter().map(|message| LossNote {
            category: LossCategory::Metadata,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(validation.warnings.into_iter().map(|message| LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    let mut typed_records = geometry.typed_records;
    typed_records.extend(topology.typed_records);
    typed_records.extend(presentation.typed_records);
    typed_records.extend(product.typed_records);
    typed_records.extend(tessellation.typed_records);
    typed_records.extend(pmi.typed_records);
    typed_records.extend(dependencies.typed_records);
    typed_records.extend(validation.typed_records);

    let opaque_offsets = if retain_opaque {
        BTreeSet::new()
    } else {
        exchange
            .records
            .values()
            .filter(|record| !typed_records.contains(&record.id))
            .map(|record| record.span.start)
            .collect()
    };
    let mut counts = BTreeMap::<String, usize>::new();
    if retain_opaque {
        let opaque_ids = exchange
            .records
            .values()
            .filter(|record| !typed_records.contains(&record.id))
            .map(|record| {
                let kind = record
                    .partials
                    .iter()
                    .map(|partial| partial.name.to_ascii_lowercase())
                    .collect::<Vec<_>>()
                    .join("_");
                (record.id, format!("step:data:{kind}#{}", record.id))
            })
            .collect::<BTreeMap<_, _>>();
        let mut opaque = Vec::with_capacity(exchange.records.len());
        for record in exchange.records.values() {
            if typed_records.contains(&record.id) {
                continue;
            }
            let kind = record
                .partials
                .iter()
                .map(|partial| partial.name.as_str())
                .collect::<Vec<_>>()
                .join("+");
            *counts.entry(kind).or_default() += 1;
            let bytes = input[record.span.clone()].to_vec();
            let mut links = BTreeSet::new();
            for partial in &record.partials {
                partial
                    .parameters
                    .iter()
                    .for_each(|value| collect_references(value, &mut links));
            }
            opaque.push(UnknownRecord {
                id: UnknownId(opaque_ids[&record.id].clone()),
                offset: record.span.start as u64,
                byte_len: record.span.len() as u64,
                sha256: sha256_hex(&bytes),
                data: Some(bytes),
                links: links
                    .into_iter()
                    .filter_map(|id| opaque_ids.get(&id).cloned())
                    .collect(),
            });
        }
        ir.set_native_unknowns_owned("step", opaque);
    } else {
        for record in exchange.records.values() {
            if typed_records.contains(&record.id) {
                continue;
            }
            let kind = record
                .partials
                .iter()
                .map(|partial| partial.name.as_str())
                .collect::<Vec<_>>()
                .join("+");
            *counts.entry(kind).or_default() += 1;
        }
    }
    let accounting = byte_accounting(input.len(), exchange, &typed_records);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("bytes_structural".into(), accounting.structural.to_string());
        source
            .attributes
            .insert("bytes_typed".into(), accounting.typed.to_string());
        source
            .attributes
            .insert("bytes_named_opaque".into(), accounting.opaque.to_string());
        source.attributes.insert(
            "bytes_unclassified".into(),
            accounting.unclassified.to_string(),
        );
    }
    report.notes.push(format!(
        "byte accounting: {} structural, {} typed, {} named opaque, {} unclassified",
        accounting.structural, accounting.typed, accounting.opaque, accounting.unclassified
    ));
    report
        .losses
        .extend(counts.into_iter().map(|(name, count)| LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!("preserved {count} {name} instance(s) as named opaque STEP records"),
            provenance: None,
        }));
    (DecodeResult::new(ir, report), opaque_offsets)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ByteAccounting {
    structural: usize,
    typed: usize,
    opaque: usize,
    unclassified: usize,
}

fn byte_accounting(
    input_len: usize,
    exchange: &Exchange,
    typed_records: &BTreeSet<u64>,
) -> ByteAccounting {
    let mut counts = ByteAccounting::default();
    for record in exchange.records.values() {
        if typed_records.contains(&record.id) {
            counts.typed += record.span.len();
        } else {
            counts.opaque += record.span.len();
        }
    }
    counts.structural = input_len.saturating_sub(counts.typed + counts.opaque);
    counts
}

fn schema_name(exchange: &Exchange) -> String {
    let mut names = Vec::new();
    if let Some(record) = exchange
        .header
        .iter()
        .find(|record| record.name == "FILE_SCHEMA")
    {
        record
            .parameters
            .iter()
            .for_each(|value| collect_strings(value, &mut names));
    }
    names.join(",")
}

fn collect_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(bytes) => output.push(String::from_utf8_lossy(bytes).into_owned()),
        Value::List(values) => values
            .iter()
            .for_each(|value| collect_strings(value, output)),
        Value::Typed(_, value) => collect_strings(value, output),
        _ => {}
    }
}

fn collect_references(value: &Value, output: &mut BTreeSet<u64>) {
    match value {
        Value::Reference(id) => {
            output.insert(*id);
        }
        Value::List(values) => values
            .iter()
            .for_each(|value| collect_references(value, output)),
        Value::Typed(_, value) => collect_references(value, output),
        _ => {}
    }
}
