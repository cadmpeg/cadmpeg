// SPDX-License-Identifier: Apache-2.0
//! Schema-aware STEP-to-IR decoding entry point.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_codec_core::CodecError;
use cadmpeg_ir::codec::{DecodeOptions, DecodeResult};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossKind, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;

use crate::parse::{self, Exchange, ParseDiagnostic, Value};

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
    let (exchange, diagnostics) =
        parse::parse(input).map_err(|error| CodecError::Malformed(error.to_string()))?;
    Ok(decode_exchange(input, options, &exchange, &diagnostics))
}

pub(super) fn decode_exchange(
    input: &[u8],
    options: DecodeOptions,
    exchange: &Exchange,
    diagnostics: &[ParseDiagnostic],
) -> DecodeResult {
    decode_exchange_mode(input, options, exchange, diagnostics, true).0
}

pub(super) fn inspect_exchange(
    input: &[u8],
    exchange: &Exchange,
    diagnostics: &[ParseDiagnostic],
) -> (DecodeResult, BTreeSet<usize>) {
    decode_exchange_mode(
        input,
        DecodeOptions::default(),
        exchange,
        diagnostics,
        false,
    )
}

fn decode_exchange_mode(
    input: &[u8],
    options: DecodeOptions,
    exchange: &Exchange,
    diagnostics: &[ParseDiagnostic],
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
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses: Vec::new(),
        notes: exchange
            .references
            .iter()
            .map(|entry| format!("external reference {} -> {}", entry.name, entry.uri))
            .collect(),
    };
    report.losses.extend(diagnostics.iter().map(|diagnostic| {
        LossNote::new(
            LossKind::NoncanonicalSourceSyntax,
            diagnostic.message.clone(),
        )
        .with_provenance(cadmpeg_ir::LossProvenance {
            format: "step".into(),
            stream: String::new(),
            offset: diagnostic.offset as u64,
            tag: Some("complex_entity".into()),
        })
    }));
    if options.container_only {
        return (
            DecodeResult::new(ir, report, cadmpeg_ir::SourceFidelity::default()),
            BTreeSet::new(),
        );
    }

    let mut geometry = geometry::decode(exchange, &mut ir);
    let dependencies = dependencies::decode(exchange);
    let topology = topology::decode(exchange, &mut ir);
    geometry::associate_topology_carriers(exchange, &mut ir);
    geometry::associate_free_geometric_set_members(exchange, &mut ir);
    geometry::associate_free_representation_members(exchange, &mut ir);
    retain_unowned_pcurves(exchange, &mut geometry, &mut ir);
    let product = product::decode(exchange, &geometry, &topology, &mut ir);
    let tessellation = tessellation::decode(exchange, &geometry, &topology, &mut ir);
    let pmi = pmi::decode(exchange, &geometry, &mut ir);
    let presentation = presentation::decode(exchange, &topology, &mut ir);
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
            code: cadmpeg_ir::LossKind::DecodeDiagnostic,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(topology.warnings.into_iter().map(|message| LossNote {
            code: cadmpeg_ir::LossKind::DecodeDiagnostic,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(presentation.warnings.into_iter().map(|message| LossNote {
            code: cadmpeg_ir::LossKind::DecodeDiagnostic,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(product.warnings.into_iter().map(|message| LossNote {
            code: cadmpeg_ir::LossKind::DecodeDiagnostic,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(tessellation.warnings.into_iter().map(|message| LossNote {
            code: cadmpeg_ir::LossKind::DecodeDiagnostic,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report.losses.extend(tessellation.losses);
    report
        .losses
        .extend(pmi.warnings.into_iter().map(|message| LossNote {
            code: cadmpeg_ir::LossKind::DecodeDiagnostic,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));
    report
        .losses
        .extend(validation.warnings.into_iter().map(|message| LossNote {
            code: cadmpeg_ir::LossKind::DecodeDiagnostic,
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
            .map(|record| (record.id, opaque_record_id(record).0))
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
            code: cadmpeg_ir::LossKind::RecordNotTyped,
            severity: Severity::Warning,
            message: format!("preserved {count} {name} instance(s) as named opaque STEP records"),
            provenance: None,
        }));
    (
        DecodeResult::new(ir, report, cadmpeg_ir::SourceFidelity::default()),
        opaque_offsets,
    )
}

fn retain_unowned_pcurves(
    exchange: &Exchange,
    geometry: &mut geometry::GeometryResult,
    ir: &mut CadIr,
) {
    let owned = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.0.clone()))
        .chain(ir.model.loops.iter().flat_map(|loop_| {
            loop_
                .vertex_uses
                .iter()
                .flat_map(|use_| use_.pcurves.iter().map(|pcurve| pcurve.pcurve.0.clone()))
        }))
        .collect::<BTreeSet<_>>();
    let unowned = exchange
        .records
        .iter()
        .filter(|(_, record)| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "PCURVE")
        })
        .map(|(&id, _)| id)
        .filter(|id| !owned.contains(&format!("step:data:pcurve#{id}")))
        .collect::<BTreeSet<_>>();
    if unowned.is_empty() {
        return;
    }
    let mut roots = BTreeSet::new();
    for identity in ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.0.as_str())
        .chain(
            ir.model
                .edges
                .iter()
                .filter_map(|edge| edge.curve.as_ref().map(|curve| curve.0.as_str())),
        )
        .chain(ir.model.faces.iter().map(|face| face.surface.0.as_str()))
        .chain(
            ir.model
                .coedges
                .iter()
                .filter_map(|coedge| coedge.use_curve.as_ref().map(|curve| curve.0.as_str())),
        )
        .chain(
            ir.model
                .pcurves
                .iter()
                .filter(|pcurve| owned.contains(&pcurve.id.0))
                .map(|pcurve| pcurve.id.0.as_str()),
        )
        .chain(
            ir.model
                .points
                .iter()
                .filter(|point| point.source_object.is_some())
                .map(|point| point.id.0.as_str()),
        )
        .chain(
            ir.model
                .curves
                .iter()
                .filter(|curve| curve.source_object.is_some())
                .map(|curve| curve.id.0.as_str()),
        )
        .chain(
            ir.model
                .surfaces
                .iter()
                .filter(|surface| surface.source_object.is_some())
                .map(|surface| surface.id.0.as_str()),
        )
        .chain(
            ir.model
                .procedural_curves
                .iter()
                .map(|curve| curve.id.0.as_str()),
        )
        .chain(
            ir.model
                .procedural_surfaces
                .iter()
                .map(|surface| surface.id.0.as_str()),
        )
        .filter_map(step_id_from_ir)
    {
        roots.insert(identity);
    }
    let protected_roots = roots
        .into_iter()
        .filter(|id| !unowned.contains(id))
        .collect::<BTreeSet<_>>();
    let protected = record_closure(&protected_roots, exchange);
    let removed_closure = record_closure(&unowned, exchange);
    let removed = unowned.len();
    ir.model
        .pcurves
        .retain(|pcurve| owned.contains(&pcurve.id.0));
    ir.model.points.retain(|point| {
        step_id_from_ir(&point.id.0)
            .is_none_or(|id| !removed_closure.contains(&id) || protected.contains(&id))
    });
    ir.model.curves.retain(|curve| {
        step_id_from_ir(&curve.id.0)
            .is_none_or(|id| !removed_closure.contains(&id) || protected.contains(&id))
    });
    ir.model.surfaces.retain(|surface| {
        step_id_from_ir(&surface.id.0)
            .is_none_or(|id| !removed_closure.contains(&id) || protected.contains(&id))
    });
    ir.model.procedural_curves.retain(|curve| {
        step_id_from_ir(&curve.id.0)
            .is_none_or(|id| !removed_closure.contains(&id) || protected.contains(&id))
    });
    ir.model.procedural_surfaces.retain(|surface| {
        step_id_from_ir(&surface.id.0)
            .is_none_or(|id| !removed_closure.contains(&id) || protected.contains(&id))
    });
    geometry
        .typed_records
        .retain(|id| !removed_closure.contains(id) || protected.contains(id));
    geometry.warnings.push(format!(
        "retained {removed} unowned pcurve carrier(s) as opaque source records"
    ));
}

fn step_id_from_ir(identity: &str) -> Option<u64> {
    identity.rsplit_once('#')?.1.parse().ok()
}

fn record_closure(roots: &BTreeSet<u64>, exchange: &Exchange) -> BTreeSet<u64> {
    let mut closure = BTreeSet::new();
    let mut pending = roots.iter().copied().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if !closure.insert(id) {
            continue;
        }
        let Some(record) = exchange.records.get(&id) else {
            continue;
        };
        let mut references = BTreeSet::new();
        record
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
            .for_each(|value| collect_references(value, &mut references));
        pending.extend(references);
    }
    closure
}

fn opaque_record_id(record: &parse::RawRecord) -> UnknownId {
    let kind = record
        .partials
        .iter()
        .map(|partial| partial.name.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("_");
    UnknownId(format!("step:data:{kind}#{}", record.id))
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
