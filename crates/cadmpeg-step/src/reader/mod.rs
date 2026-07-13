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

mod geometry;

/// Decode a complete clear-text exchange structure.
pub fn decode(input: &[u8], options: &DecodeOptions) -> Result<DecodeResult, CodecError> {
    let exchange = parse::parse(input).map_err(|error| CodecError::Malformed(error.to_string()))?;
    let mut ir = CadIr::empty(Units::default());
    let mut attributes = BTreeMap::new();
    attributes.insert("schema".into(), schema_name(&exchange));
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
        losses: Vec::new(),
        notes: exchange
            .references
            .iter()
            .map(|entry| format!("external reference {} -> {}", entry.name, entry.uri))
            .collect(),
    };
    if options.container_only {
        return Ok(DecodeResult::new(ir, report));
    }

    let geometry = geometry::decode(&exchange, &mut ir);
    report.geometry_transferred =
        !ir.model.points.is_empty() || !ir.model.curves.is_empty() || !ir.model.surfaces.is_empty();
    report
        .losses
        .extend(geometry.warnings.into_iter().map(|message| LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));

    let mut opaque = Vec::with_capacity(exchange.records.len());
    let mut counts = BTreeMap::<String, usize>::new();
    for record in exchange.records.values() {
        if geometry.typed_records.contains(&record.id) {
            continue;
        }
        let kind = record
            .partials
            .iter()
            .map(|partial| partial.name.as_str())
            .collect::<Vec<_>>()
            .join("+");
        *counts.entry(kind.clone()).or_default() += 1;
        let bytes = input[record.span.clone()].to_vec();
        let mut links = BTreeSet::new();
        for partial in &record.partials {
            partial
                .parameters
                .iter()
                .for_each(|value| collect_references(value, &mut links));
        }
        opaque.push(UnknownRecord {
            id: UnknownId(format!("step:{kind}:#{}", record.id)),
            offset: record.span.start as u64,
            byte_len: record.span.len() as u64,
            sha256: sha256_hex(&bytes),
            data: Some(bytes),
            links: links.into_iter().map(|id| format!("step:#{id}")).collect(),
        });
    }
    ir.set_native_unknowns("step", &opaque)?;
    report
        .losses
        .extend(counts.into_iter().map(|(name, count)| LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!("preserved {count} {name} instance(s) as named opaque STEP records"),
            provenance: None,
        }));
    Ok(DecodeResult::new(ir, report))
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
