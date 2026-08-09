// SPDX-License-Identifier: Apache-2.0
//! Schema-aware STEP-to-IR decoding entry point.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeOptions, DecodeResult};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossKind, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::SourceObjectAssociation;

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
pub fn decode(
    input: &[u8],
    options: DecodeOptions,
    ctx: &DecodeContext<'_>,
) -> Result<DecodeResult, CodecError> {
    let (exchange, diagnostics) = parse::parse_with_context(input, ctx)?;
    decode_exchange(input, options, &exchange, &diagnostics, Some(ctx))
}

pub(super) fn decode_exchange(
    input: &[u8],
    options: DecodeOptions,
    exchange: &Exchange,
    diagnostics: &[ParseDiagnostic],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<DecodeResult, CodecError> {
    decode_exchange_mode(input, options, exchange, diagnostics, true, ctx).map(|(result, _)| result)
}

pub(super) fn inspect_exchange(
    input: &[u8],
    exchange: &Exchange,
    diagnostics: &[ParseDiagnostic],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<(DecodeResult, BTreeSet<usize>), CodecError> {
    decode_exchange_mode(
        input,
        DecodeOptions::default(),
        exchange,
        diagnostics,
        false,
        ctx,
    )
}

fn decode_exchange_mode(
    input: &[u8],
    options: DecodeOptions,
    exchange: &Exchange,
    diagnostics: &[ParseDiagnostic],
    retain_opaque: bool,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<(DecodeResult, BTreeSet<usize>), CodecError> {
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
        return Ok((
            DecodeResult::new(ir, report, cadmpeg_ir::SourceFidelity::default()),
            BTreeSet::new(),
        ));
    }

    let semantic_input_work = semantic_input_work(exchange);
    let mut admitted_ir_entities = 0;
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_geometry_decode",
    )?;
    let mut geometry = geometry::decode(exchange, &mut ir);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_dependency_decode",
    )?;
    let dependencies = dependencies::decode(exchange);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_carrier_index",
    )?;
    let carrier_index = index::CarrierIndex::from_ir(&ir);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_topology_decode",
    )?;
    if let Some(ctx) = ctx {
        ctx.charge_work(
            implicit_face_plane_work(exchange),
            "step_implicit_face_plane",
        )?;
    }
    let topology = topology::decode(exchange, &mut ir, &carrier_index);
    let owned_carriers = geometry::topology_owned_carriers(&ir, &carrier_index);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_topology_association",
    )?;
    geometry::associate_topology_carriers(exchange, &mut ir, &carrier_index, &owned_carriers);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_replica_association",
    )?;
    geometry::associate_replica_bases(exchange, &mut ir, &carrier_index);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_pcurve_association",
    )?;
    geometry::associate_pcurve_supports(exchange, &mut ir, &carrier_index);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_geometric_set_association",
    )?;
    geometry::associate_free_geometric_set_members(
        exchange,
        &mut ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_representation_association",
    )?;
    geometry::associate_free_representation_members(
        exchange,
        &mut ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_presentation_carrier_association",
    )?;
    geometry::associate_free_presentation_carriers(
        exchange,
        &mut ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_surface_curve_association",
    )?;
    geometry::associate_surface_curve_supports(exchange, &mut ir, &carrier_index, &owned_carriers);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_product_decode",
    )?;
    let product = product::decode(exchange, &geometry, &topology, &mut ir);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_tessellation_decode",
    )?;
    let tessellation = tessellation::decode(exchange, &geometry, &topology, &mut ir);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_pmi_decode",
    )?;
    let pmi = pmi::decode(exchange, &geometry, &mut ir);
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_presentation_decode",
    )?;
    let presentation = presentation::decode(
        exchange,
        &topology,
        &mut ir,
        &product.product_definition_ids_by_source,
    );
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_validation_decode",
    )?;
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
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_drawing_decode",
    )?;
    let drawing = drawing::decode(exchange, &mut ir, &typed_records);
    report.losses.extend(drawing.losses);
    typed_records.extend(drawing.typed_records);
    let mut post_decode_warnings = Vec::new();
    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_carrier_retention",
    )?;
    retain_unowned_carriers(
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

    charge_semantic_stage(
        ctx,
        semantic_input_work,
        &ir,
        &mut admitted_ir_entities,
        "step_opaque_record_retention",
    )?;
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
            let mut links = BTreeSet::new();
            let reference_work = record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .map(reference_work_units)
                .fold(0, u64::saturating_add);
            let bytes = if let Some(ctx) = ctx {
                ctx.charge_collection_items(reference_work, "step_opaque_record_links")?;
                ctx.copy_retained(&input[record.span.clone()], "step_opaque_record", None)?
            } else {
                input[record.span.clone()].to_vec()
            };
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
        for (index, signature) in exchange.signatures.iter().enumerate() {
            let bytes = if let Some(ctx) = ctx {
                ctx.copy_retained(&input[signature.clone()], "step_signature_record", None)?
            } else {
                input[signature.clone()].to_vec()
            };
            *counts.entry("SIGNATURE".into()).or_default() += 1;
            opaque.push(UnknownRecord {
                id: UnknownId(format!("step:signature#{index}")),
                offset: signature.start as u64,
                byte_len: signature.len() as u64,
                sha256: sha256_hex(&bytes),
                data: Some(bytes),
                links: Vec::new(),
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
    let accounting = {
        if let Some(ctx) = ctx {
            ctx.charge_work(
                u64::try_from(input.len()).unwrap_or(u64::MAX),
                "step_byte_accounting",
            )?;
        }
        let _reservation = ctx
            .map(|ctx| ctx.reserve_scoped(input.len() as u64, "step_byte_accounting", None))
            .transpose()?;
        byte_accounting(input, exchange, &typed_records)
    };
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
    charge_pending_ir_entities(
        ctx,
        &ir,
        &mut admitted_ir_entities,
        "step_admit_ir_entities",
    )?;
    Ok((
        DecodeResult::new(ir, report, cadmpeg_ir::SourceFidelity::default()),
        opaque_offsets,
    ))
}

fn charge_semantic_stage(
    ctx: Option<&DecodeContext<'_>>,
    input_work: u64,
    ir: &CadIr,
    admitted_ir_entities: &mut u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    charge_pending_ir_entities(ctx, ir, admitted_ir_entities, operation)?;
    let output_work = u64::try_from(ir.model.entity_count()).unwrap_or(u64::MAX);
    let units = input_work.saturating_add(output_work);
    ctx.map_or(Ok(()), |ctx| ctx.charge_work(units, operation))
}

fn charge_pending_ir_entities(
    ctx: Option<&DecodeContext<'_>>,
    ir: &CadIr,
    admitted_ir_entities: &mut u64,
    operation: &'static str,
) -> Result<(), CodecError> {
    let current_entities = u64::try_from(ir.model.entity_count()).unwrap_or(u64::MAX);
    let additional_entities = current_entities.saturating_sub(*admitted_ir_entities);
    if let Some(ctx) = ctx {
        ctx.charge_entities(additional_entities, operation)?;
    }
    *admitted_ir_entities = current_entities;
    Ok(())
}

/// Count the source graph nodes that each semantic pass may inspect.
fn semantic_input_work(exchange: &Exchange) -> u64 {
    let records = exchange.records.values().map(|record| {
        1_u64.saturating_add(
            record
                .partials
                .iter()
                .map(|partial| {
                    1_u64.saturating_add(
                        partial
                            .parameters
                            .iter()
                            .map(value_work_units)
                            .fold(0, u64::saturating_add),
                    )
                })
                .fold(0, u64::saturating_add),
        )
    });
    let headers = exchange.header.iter().map(|record| {
        1_u64.saturating_add(
            record
                .parameters
                .iter()
                .map(value_work_units)
                .fold(0, u64::saturating_add),
        )
    });
    let anchors = exchange
        .anchors
        .iter()
        .map(|anchor| 1_u64.saturating_add(value_work_units(&anchor.value)));
    let data = exchange.data.iter().map(|section| {
        1_u64
            .saturating_add(
                section
                    .parameters
                    .iter()
                    .map(value_work_units)
                    .fold(0, u64::saturating_add),
            )
            .saturating_add(u64::try_from(section.records.len()).unwrap_or(u64::MAX))
    });
    let references = u64::try_from(exchange.references.len()).unwrap_or(u64::MAX);
    records
        .chain(headers)
        .chain(anchors)
        .chain(data)
        .fold(references, u64::saturating_add)
}

fn value_work_units(value: &Value) -> u64 {
    match value {
        Value::List(values) => 1_u64.saturating_add(
            values
                .iter()
                .map(value_work_units)
                .fold(0, u64::saturating_add),
        ),
        Value::Typed(_, value) => 1_u64.saturating_add(value_work_units(value)),
        _ => 1,
    }
}

fn reference_work_units(value: &Value) -> u64 {
    match value {
        Value::Reference(_) => 1,
        Value::List(values) => values
            .iter()
            .map(reference_work_units)
            .fold(0, u64::saturating_add),
        Value::Typed(_, value) => reference_work_units(value),
        _ => 0,
    }
}

/// Reserve the linear scan used to derive a plane for an implicit face.
fn implicit_face_plane_work(exchange: &Exchange) -> u64 {
    exchange
        .records
        .values()
        .filter_map(|record| {
            record
                .partials
                .iter()
                .find(|partial| partial.name == "POLY_LOOP")
                .and_then(|partial| partial.parameters.get(1))
                .and_then(|value| match value {
                    Value::List(values) => Some(values),
                    _ => None,
                })
                .map(|points| u64::try_from(points.len()).unwrap_or(u64::MAX))
        })
        .fold(0, u64::saturating_add)
}

fn retain_unowned_carriers(
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
    let unowned_pcurves = exchange
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
    let referenced = referenced_record_ids(exchange);
    let unowned_direct_carriers = ir
        .model
        .points
        .iter()
        .filter(|point| point.source_object.is_none())
        .map(|point| point.id.0.as_str())
        .chain(
            ir.model
                .curves
                .iter()
                .filter(|curve| curve.source_object.is_none())
                .map(|curve| curve.id.0.as_str()),
        )
        .chain(
            ir.model
                .surfaces
                .iter()
                .filter(|surface| surface.source_object.is_none())
                .map(|surface| surface.id.0.as_str()),
        )
        .filter_map(step_id_from_ir)
        .filter(|id| exchange.records.contains_key(id) && !referenced.contains(id))
        .collect::<BTreeSet<_>>();
    associate_unowned_direct_carriers(ir, &unowned_direct_carriers);
    if unowned_pcurves.is_empty() {
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
        .filter(|id| !unowned_pcurves.contains(id))
        .collect::<BTreeSet<_>>();
    let protected = record_closure(&protected_roots, exchange);
    let removed_closure = record_closure(&unowned_pcurves, exchange);
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
        !unowned_pcurves.contains(id) && (!removed_closure.contains(id) || protected.contains(id))
    });
    let protected_pcurves = unowned_pcurves
        .iter()
        .filter(|id| protected.contains(id))
        .count();
    let opaque_pcurves = unowned_pcurves.len() - protected_pcurves;
    warnings.push(format!(
        "unowned STEP carrier retention: opaque_pcurves={opaque_pcurves}, protected_pcurves={protected_pcurves}, deleted pcurves={deleted_pcurves}, points={deleted_points}, curves={deleted_curves}, surfaces={deleted_surfaces}, procedural_curves={deleted_procedural_curves}, procedural_surfaces={deleted_procedural_surfaces}"
    ));
}

fn associate_unowned_direct_carriers(ir: &mut CadIr, ids: &BTreeSet<u64>) {
    for point in &mut ir.model.points {
        let Some(id) = step_id_from_ir(&point.id.0) else {
            continue;
        };
        if ids.contains(&id) {
            point
                .source_object
                .get_or_insert_with(|| direct_carrier_association(id));
        }
    }
    for curve in &mut ir.model.curves {
        let Some(id) = step_id_from_ir(&curve.id.0) else {
            continue;
        };
        if ids.contains(&id) {
            curve
                .source_object
                .get_or_insert_with(|| direct_carrier_association(id));
        }
    }
    for surface in &mut ir.model.surfaces {
        let Some(id) = step_id_from_ir(&surface.id.0) else {
            continue;
        };
        if ids.contains(&id) {
            surface
                .source_object
                .get_or_insert_with(|| direct_carrier_association(id));
        }
    }
}

fn direct_carrier_association(id: u64) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: "step".into(),
        object_id: format!("#{id}"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    }
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

fn referenced_record_ids(exchange: &Exchange) -> BTreeSet<u64> {
    let mut references = BTreeSet::new();
    for record in exchange.records.values() {
        for parameter in record
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            collect_references(parameter, &mut references);
        }
    }
    references
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
    for signature in &exchange.signatures {
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
        } else if input[at].is_ascii_control() || input[at] == b' ' {
            classes[at] = ByteClass::Structural;
            at += 1;
        } else if let Some(after_print_control) = crate::lex::print_control_end(input, at) {
            claim_range(
                classes,
                &(at..after_print_control.min(end)),
                ByteClass::Structural,
            );
            at = after_print_control;
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

pub(super) fn schema_identifiers(exchange: &Exchange) -> Vec<String> {
    let Some(record) = exchange
        .header
        .iter()
        .find(|record| record.name == "FILE_SCHEMA")
    else {
        return Vec::new();
    };
    let Some(Value::List(identifiers)) = record.parameters.first() else {
        return Vec::new();
    };
    identifiers
        .iter()
        .filter_map(|value| match value {
            Value::String(bytes) => Some(
                crate::strings::decode(bytes)
                    .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned()),
            ),
            _ => None,
        })
        .collect()
}

fn schema_name(exchange: &Exchange) -> String {
    schema_identifiers(exchange).join(",")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringEncoding {
    Iso8859_1,
    Utf8,
}

fn string_encoding(exchange: &Exchange) -> StringEncoding {
    let edition = exchange
        .header
        .iter()
        .find(|record| record.name == "FILE_DESCRIPTION")
        .and_then(|record| record.parameters.get(1))
        .and_then(|value| match value {
            Value::String(bytes) => bytes
                .split(|byte| *byte == b';')
                .next()
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .and_then(|edition| edition.parse::<u8>().ok()),
            _ => None,
        });
    if edition == Some(4) {
        StringEncoding::Utf8
    } else {
        StringEncoding::Iso8859_1
    }
}

pub(super) fn decode_text(
    exchange: &Exchange,
    value: &Value,
    losses: &mut Vec<LossNote>,
    record_id: u64,
    field: &str,
    code: LossKind,
) -> Option<String> {
    let Value::String(bytes) = value else {
        return None;
    };
    let decoded = match string_encoding(exchange) {
        StringEncoding::Iso8859_1 => crate::strings::decode(bytes),
        StringEncoding::Utf8 => crate::strings::decode_utf8(bytes),
    };
    match decoded {
        Ok(text) => Some(text),
        Err(error) => {
            losses.push(LossNote {
                code,
                severity: Severity::Warning,
                message: format!("STEP record #{record_id} has an invalid {field} string: {error}"),
                provenance: None,
            });
            None
        }
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
        extended.push(0xc3);

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
            None,
        )
        .expect("synthesized unknown record conversion")
        .0;
        assert!(result.report.losses.iter().any(|loss| {
            loss.code == LossKind::DecodeDiagnostic
                && loss.severity == Severity::Error
                && loss.message.contains("1 byte(s) unclassified")
        }));
    }

    #[test]
    fn byte_accounting_claims_controls_inside_print_directives() {
        let input = b"1\\\x01N\x02\\2";
        let mut classes = vec![ByteClass::Unclassified; input.len()];

        claim_trivia(input, 1..input.len(), &mut classes);

        assert!(classes[1..6]
            .iter()
            .all(|class| *class == ByteClass::Structural));
    }

    #[test]
    fn semantic_work_counts_nested_source_graph_nodes() {
        let simple = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
        let nested = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(((1,2),TYPE((3,4))));ENDSEC;END-ISO-10303-21;";
        let (simple_exchange, _) = crate::parse::parse(simple).expect("simple exchange");
        let (nested_exchange, _) = crate::parse::parse(nested).expect("nested exchange");

        assert!(semantic_input_work(&nested_exchange) > semantic_input_work(&simple_exchange));
    }

    #[test]
    fn implicit_face_plane_work_scales_with_point_count() {
        let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POLY_LOOP('',(#2,#3,#4,#5));#2=ITEM();#3=ITEM();#4=ITEM();#5=ITEM();ENDSEC;END-ISO-10303-21;";
        let (exchange, _) = crate::parse::parse(source).expect("polygon exchange");

        assert_eq!(implicit_face_plane_work(&exchange), 4);
    }
}
