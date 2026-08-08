// SPDX-License-Identifier: Apache-2.0
//! Schema-aware STEP-to-IR decoding entry point.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeOptions, DecodeResult};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossKind, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;

use crate::parse::{self, Exchange, ParseDiagnostic, Value};

mod dependencies;
mod drawing;
mod geometry;
mod index;
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
            tag: Some(
                match diagnostic.kind {
                    crate::parse::ParseDiagnosticKind::ComplexPartialsNotAlphabetical => {
                        "complex_entity"
                    }
                    crate::parse::ParseDiagnosticKind::OmittedEntityName => "entity_name",
                }
                .into(),
            ),
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
    let carrier_index = index::CarrierIndex::from_ir(&ir);
    let topology = topology::decode(
        exchange,
        &mut ir,
        &carrier_index,
        geometry.plane_angle_scale,
    );
    geometry::repair_angular_pcurve_units(
        &mut ir,
        geometry.plane_angle_scale,
        &mut geometry.warnings,
    );
    let owned_carriers = geometry::topology_owned_carriers(&ir, &carrier_index);
    geometry::associate_topology_carriers(exchange, &mut ir, &carrier_index, &owned_carriers);
    geometry::associate_replica_bases(exchange, &mut ir, &carrier_index);
    geometry::associate_pcurve_supports(exchange, &mut ir, &carrier_index);
    geometry::associate_free_geometric_set_members(
        exchange,
        &mut ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    geometry::associate_free_representation_members(
        exchange,
        &mut ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    geometry::associate_free_presentation_carriers(
        exchange,
        &mut ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    geometry::associate_surface_curve_supports(exchange, &mut ir, &carrier_index, &owned_carriers);
    let product = product::decode(exchange, &geometry, &topology, &mut ir);
    let tessellation = tessellation::decode(exchange, &geometry, &topology, &mut ir);
    let pmi = pmi::decode(exchange, &geometry, &mut ir);
    let presentation = presentation::decode(exchange, &topology, &mut ir);
    let validation = validation::decode(exchange, &geometry, &mut ir);
    report.notes.extend(dependencies.notes);
    report.notes.extend(validation.notes);
    report.losses.extend(dependencies.losses);
    report.losses.extend(presentation.losses);
    report.losses.extend(product.losses);
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
    report.losses.extend(topology.losses);
    report.losses.extend(geometry.losses);
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
    report.losses.extend(pmi.losses);
    report.losses.extend(validation.losses);
    let mut typed_records = geometry.typed_records;
    typed_records.extend(topology.typed_records);
    typed_records.extend(presentation.typed_records);
    typed_records.extend(product.typed_records);
    typed_records.extend(tessellation.typed_records);
    typed_records.extend(pmi.typed_records);
    typed_records.extend(dependencies.typed_records);
    typed_records.extend(validation.typed_records);
    let drawing = drawing::decode(exchange, &mut ir, &typed_records);
    report.losses.extend(drawing.losses);
    typed_records.extend(drawing.typed_records);
    let mut post_decode_warnings = Vec::new();
    retain_unowned_pcurves(
        exchange,
        &mut ir,
        &mut typed_records,
        &mut post_decode_warnings,
    );
    report
        .losses
        .extend(post_decode_warnings.into_iter().map(|message| LossNote {
            code: cadmpeg_ir::LossKind::DecodeDiagnostic,
            severity: Severity::Warning,
            message,
            provenance: None,
        }));

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
        let typed_targets = typed_record_targets(&ir, &typed_records);
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
                    .flat_map(|id| {
                        opaque_ids
                            .get(&id)
                            .cloned()
                            .into_iter()
                            .chain(typed_targets.get(&id).into_iter().flatten().cloned())
                    })
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
    let accounting = byte_accounting(input, exchange, &typed_records);
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
    if accounting.unclassified > 0 {
        report.losses.push(LossNote {
            code: LossKind::DecodeDiagnostic,
            severity: Severity::Error,
            message: format!(
                "STEP byte accounting left {} byte(s) unclassified",
                accounting.unclassified
            ),
            provenance: None,
        });
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
    ir: &mut CadIr,
    typed_records: &mut BTreeSet<u64>,
    warnings: &mut Vec<String>,
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
    let deleted_pcurves = ir
        .model
        .pcurves
        .iter()
        .filter(|pcurve| !retains_carrier(&pcurve.id.0, &removed_closure, &protected))
        .count();
    let deleted_points = ir
        .model
        .points
        .iter()
        .filter(|point| !retains_carrier(&point.id.0, &removed_closure, &protected))
        .count();
    let deleted_curves = ir
        .model
        .curves
        .iter()
        .filter(|curve| !retains_carrier(&curve.id.0, &removed_closure, &protected))
        .count();
    let deleted_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| !retains_carrier(&surface.id.0, &removed_closure, &protected))
        .count();
    let deleted_procedural_curves = ir
        .model
        .procedural_curves
        .iter()
        .filter(|curve| !retains_carrier(&curve.id.0, &removed_closure, &protected))
        .count();
    let deleted_procedural_surfaces = ir
        .model
        .procedural_surfaces
        .iter()
        .filter(|surface| !retains_carrier(&surface.id.0, &removed_closure, &protected))
        .count();
    ir.model
        .pcurves
        .retain(|pcurve| owned.contains(&pcurve.id.0));
    ir.model
        .points
        .retain(|point| retains_carrier(&point.id.0, &removed_closure, &protected));
    ir.model
        .curves
        .retain(|curve| retains_carrier(&curve.id.0, &removed_closure, &protected));
    ir.model
        .surfaces
        .retain(|surface| retains_carrier(&surface.id.0, &removed_closure, &protected));
    ir.model
        .procedural_curves
        .retain(|curve| retains_carrier(&curve.id.0, &removed_closure, &protected));
    ir.model
        .procedural_surfaces
        .retain(|surface| retains_carrier(&surface.id.0, &removed_closure, &protected));
    typed_records.retain(|id| {
        !unowned.contains(id) && (!removed_closure.contains(id) || protected.contains(id))
    });
    let protected_pcurves = unowned.iter().filter(|id| protected.contains(id)).count();
    let opaque_pcurves = unowned.len() - protected_pcurves;
    warnings.push(format!(
        "unowned STEP carrier retention: opaque_pcurves={opaque_pcurves}, protected_pcurves={protected_pcurves}, deleted pcurves={deleted_pcurves}, points={deleted_points}, curves={deleted_curves}, surfaces={deleted_surfaces}, procedural_curves={deleted_procedural_curves}, procedural_surfaces={deleted_procedural_surfaces}"
    ));
}

fn retains_carrier(
    identity: &str,
    removed_closure: &BTreeSet<u64>,
    protected: &BTreeSet<u64>,
) -> bool {
    step_id_from_ir(identity)
        .is_none_or(|id| !removed_closure.contains(&id) || protected.contains(&id))
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

fn typed_record_targets(
    ir: &CadIr,
    typed_records: &BTreeSet<u64>,
) -> BTreeMap<u64, BTreeSet<String>> {
    cadmpeg_ir::index::ModelIndex::new(ir)
        .identities()
        .filter_map(|identity| {
            let record_id = source_record_id(identity)?;
            typed_records
                .contains(&record_id)
                .then(|| (record_id, identity.to_owned()))
        })
        .fold(BTreeMap::new(), |mut targets, (record_id, identity)| {
            targets.entry(record_id).or_default().insert(identity);
            targets
        })
}

fn source_record_id(identity: &str) -> Option<u64> {
    identity.rsplit_once('#')?.1.split('-').next()?.parse().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteClass {
    Unclassified,
    Structural,
    Typed,
    Opaque,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ByteAccounting {
    structural: usize,
    typed: usize,
    opaque: usize,
    unclassified: usize,
}

fn byte_accounting(
    input: &[u8],
    exchange: &Exchange,
    typed_records: &BTreeSet<u64>,
) -> ByteAccounting {
    let mut classes = vec![ByteClass::Unclassified; input.len()];
    for record in exchange.records.values() {
        let class = if typed_records.contains(&record.id) {
            ByteClass::Typed
        } else {
            ByteClass::Opaque
        };
        claim_range(&mut classes, &record.span, class);
    }
    if let Some(signature) = &exchange.signature {
        claim_range(&mut classes, signature, ByteClass::Structural);
    }
    let tokens = match crate::lex::lex(input) {
        Ok(tokens) => tokens,
        Err(error) => crate::lex::lex(&input[..error.offset]).unwrap_or_default(),
    };
    for token in &tokens {
        claim_range(&mut classes, &token.span, ByteClass::Structural);
    }
    let mut cursor = 0;
    for token in &tokens {
        claim_trivia(input, cursor..token.span.start, &mut classes);
        cursor = token.span.end;
    }
    claim_trivia(input, cursor..input.len(), &mut classes);

    classes
        .into_iter()
        .fold(ByteAccounting::default(), |mut counts, class| {
            match class {
                ByteClass::Unclassified => counts.unclassified += 1,
                ByteClass::Structural => counts.structural += 1,
                ByteClass::Typed => counts.typed += 1,
                ByteClass::Opaque => counts.opaque += 1,
            }
            counts
        })
}

fn claim_range(classes: &mut [ByteClass], range: &std::ops::Range<usize>, class: ByteClass) {
    let end = range.end.min(classes.len());
    for claimed in &mut classes[range.start.min(end)..end] {
        if *claimed == ByteClass::Unclassified {
            *claimed = class;
        }
    }
}

fn claim_trivia(input: &[u8], range: std::ops::Range<usize>, classes: &mut [ByteClass]) {
    let end = range.end.min(input.len());
    let mut at = range.start.min(end);
    while at < end {
        if classes[at] != ByteClass::Unclassified {
            at += 1;
        } else if input[at].is_ascii_whitespace() {
            classes[at] = ByteClass::Structural;
            at += 1;
        } else if input[at..end].starts_with(b"/*") {
            let Some(relative_end) = input[at + 2..end]
                .windows(2)
                .position(|window| window == b"*/")
            else {
                return;
            };
            claim_range(classes, &(at..at + relative_end + 4), ByteClass::Structural);
            at += relative_end + 4;
        } else {
            return;
        }
    }
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

pub(super) fn decode_text(
    value: &Value,
    losses: &mut Vec<LossNote>,
    record_id: u64,
    field: &str,
    code: LossKind,
) -> Option<String> {
    let Value::String(bytes) = value else {
        return None;
    };
    match crate::strings::decode(bytes) {
        Ok(text) => Some(text),
        Err(error) => {
            losses.push(LossNote {
                code,
                severity: Severity::Warning,
                message: format!(
                    "STEP record #{record_id} has an invalid {field} string escape: {error}"
                ),
                provenance: None,
            });
            None
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_accounting_reports_an_unrecognized_suffix() {
        let input = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
        let (exchange, _) = crate::parse::parse(input).expect("parse accounting fixture");
        let mut extended = input.to_vec();
        extended.push(0x01);

        let accounting = byte_accounting(&extended, &exchange, &BTreeSet::new());

        assert_eq!(accounting.unclassified, 1);
        assert_eq!(
            accounting.structural + accounting.typed + accounting.opaque + accounting.unclassified,
            extended.len()
        );

        let result = decode_exchange_mode(
            &extended,
            cadmpeg_ir::codec::DecodeOptions::default(),
            &exchange,
            &[],
            true,
        )
        .0;
        assert!(result.report.losses.iter().any(|loss| {
            loss.code == LossKind::DecodeDiagnostic
                && loss.severity == Severity::Error
                && loss.message.contains("1 byte(s) unclassified")
        }));
    }
}
