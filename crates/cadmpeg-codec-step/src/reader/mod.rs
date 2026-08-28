// SPDX-License-Identifier: Apache-2.0
//! Schema-aware STEP-to-IR decoding entry point.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::{alloc_filled, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeOptions, DecodeResult};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossNote};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{SourceFidelity, SourceObjectAssociation};

use crate::dialect::StepDialect;
use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::parse::{self, Exchange, ParseDiagnostic, Value};

pub(crate) mod dependencies;
mod drawing;
pub(crate) mod geometry;
mod index;
pub(crate) mod pmi;
pub(crate) mod presentation;
pub(crate) mod product;
mod representation;
pub(crate) mod tessellation;
pub(crate) mod topology;
mod validation;

pub(super) const MAX_RECORD_GRAPH_DEPTH: usize = 256;

pub(super) fn record_graph_limit(ctx: Option<&DecodeContext<'_>>) -> usize {
    ctx.and_then(|ctx| usize::try_from(ctx.policy().limits.max_recursion_depth).ok())
        .map_or(MAX_RECORD_GRAPH_DEPTH, |policy| {
            policy.min(MAX_RECORD_GRAPH_DEPTH)
        })
}

struct StageOutcome<T> {
    value: T,
    claims: HashSet<u64>,
    warnings: Vec<String>,
    losses: Vec<LossNote>,
    notes: Vec<String>,
}

impl<T> std::ops::Deref for StageOutcome<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> std::ops::DerefMut for StageOutcome<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

struct StepDecodeSession<'ctx, 'arena> {
    ir: CadIr,
    report: DecodeReport,
    typed_records: HashSet<u64>,
    admitted_ir_entities: u64,
    semantic_input_work: u64,
    ctx: Option<&'ctx DecodeContext<'arena>>,
}

impl<'ctx, 'arena> StepDecodeSession<'ctx, 'arena> {
    fn new(
        exchange: &Exchange,
        diagnostics: &[ParseDiagnostic],
        container_only: bool,
        ctx: Option<&'ctx DecodeContext<'arena>>,
    ) -> Self {
        let mut ir = CadIr::empty(Units::default());
        let mut attributes = BTreeMap::new();
        attributes.insert("schema".into(), schema_name(exchange));
        attributes.insert("data_sections".into(), exchange.data.len().to_string());
        attributes.insert(
            "entity_instances".into(),
            exchange.records.len().to_string(),
        );
        // The `schema` attribute above stays: it is the joined identifier list,
        // and retiring the ad-hoc attribute keys is a later phase.
        let primary = StepDialect::classify(exchange);
        let dialect_loss = crate::dialect::dialect_loss(&primary);
        ir.source = Some(SourceMeta {
            format: crate::dialect::FORMAT.into(),
            attributes,
            ..Default::default()
        });

        let dialects = vec![primary];
        let mut report = DecodeReport {
            dialects,
            format: crate::dialect::FORMAT.into(),
            container_only,
            geometry_transferred: false,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: exchange
                .references
                .iter()
                .map(|entry| format!("external reference {} -> {}", entry.name, entry.uri))
                .collect(),
        };
        report.losses.extend(dialect_loss);
        report.losses.extend(diagnostics.iter().map(|diagnostic| {
            let (code, tag) = match diagnostic.kind {
                crate::parse::ParseDiagnosticKind::ComplexPartialsNotAlphabetical => {
                    (StepLossCode::ParseNoncanonicalSyntax, "complex_entity")
                }
                crate::parse::ParseDiagnosticKind::OmittedEntityName => {
                    (StepLossCode::ParseNoncanonicalSyntax, "entity_name")
                }
                crate::parse::ParseDiagnosticKind::SchemaObjectIdentifierOutOfRange => (
                    StepLossCode::SchemaObjectIdentifierOutOfRange,
                    "schema_identifier",
                ),
            };
            code.note(diagnostic.message.clone())
                .with_provenance(cadmpeg_ir::SourceProvenance {
                    format: crate::dialect::FORMAT.into(),
                    stream: String::new(),
                    offset: diagnostic.offset as u64,
                    tag: Some(tag.into()),
                })
        }));

        Self {
            ir,
            report,
            typed_records: HashSet::new(),
            admitted_ir_entities: 0,
            semantic_input_work: 0,
            ctx,
        }
    }

    fn charge_stage(&mut self, operation: &'static str) -> Result<(), CodecError> {
        self.charge_pending_ir_entities(operation)?;
        let output_work = u64::try_from(self.ir.model.entity_count()).unwrap_or(u64::MAX);
        let units = self.semantic_input_work.saturating_add(output_work);
        self.ctx
            .map_or(Ok(()), |ctx| ctx.charge_work(units, operation))
    }

    fn charge_pending_ir_entities(&mut self, operation: &'static str) -> Result<(), CodecError> {
        let current_entities = u64::try_from(self.ir.model.entity_count()).unwrap_or(u64::MAX);
        let additional_entities = current_entities.saturating_sub(self.admitted_ir_entities);
        if let Some(ctx) = self.ctx {
            ctx.charge_entities(additional_entities, operation)?;
        }
        self.admitted_ir_entities = current_entities;
        Ok(())
    }

    fn absorb<T>(&mut self, outcome: &mut StageOutcome<T>) {
        self.typed_records.extend(outcome.claims.drain());
        self.report.losses.append(&mut outcome.losses);
        self.report.notes.append(&mut outcome.notes);
    }

    fn absorb_warnings(&mut self, warnings: impl IntoIterator<Item = String>) {
        self.report.losses.extend(
            warnings
                .into_iter()
                .map(|message| StepLossCode::DecodeWarning.note(message)),
        );
    }

    fn into_result(self, source_fidelity: SourceFidelity) -> DecodeResult {
        DecodeResult::new(self.ir, self.report, source_fidelity)
    }
}

struct OpaqueSourceRecord {
    unknown_id: String,
    span: std::ops::Range<usize>,
    links: BTreeSet<u64>,
    reference_work: u64,
}

/// Decode a complete clear-text exchange structure.
pub fn decode(
    input: &[u8],
    options: DecodeOptions,
    ctx: &DecodeContext<'_>,
) -> Result<DecodeResult, CodecError> {
    let (exchange, diagnostics) = parse::parse_with_context(input, ctx)?;
    decode_exchange(input, options, exchange, &diagnostics, Some(ctx))
}

pub(super) fn decode_exchange(
    input: &[u8],
    options: DecodeOptions,
    mut exchange: Exchange,
    diagnostics: &[ParseDiagnostic],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<DecodeResult, CodecError> {
    decode_exchange_mode(input, options, &mut exchange, diagnostics, true, ctx)
        .map(|(result, _)| result)
}

/// Deep semantic analysis used by STEP `inspect`.
///
/// Runs the semantic decode path (discarding the IR at the inspect boundary)
/// so `unknown_entities` and related attributes stay accurate. This is not a
/// cheap syntactic census.
pub(super) fn analyze_exchange(
    input: &[u8],
    exchange: &mut Exchange,
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
    exchange: &mut Exchange,
    diagnostics: &[ParseDiagnostic],
    retain_opaque: bool,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<(DecodeResult, BTreeSet<usize>), CodecError> {
    let mut session = StepDecodeSession::new(exchange, diagnostics, options.container_only, ctx);
    if options.container_only {
        return Ok((
            session.into_result(SourceFidelity::default()),
            BTreeSet::new(),
        ));
    }

    session.semantic_input_work = semantic_input_work(exchange);
    session.charge_stage("step_geometry_decode")?;
    let mut geometry = geometry::decode(exchange, &mut session.ir);
    session.charge_stage("step_dependency_decode")?;
    let mut dependencies = dependencies::decode(exchange);
    session.charge_stage("step_carrier_index")?;
    let carrier_index = index::CarrierIndex::from_ir(&session.ir);
    session.charge_stage("step_topology_decode")?;
    if let Some(ctx) = session.ctx {
        ctx.charge_work(
            implicit_face_plane_work(exchange),
            "step_implicit_face_plane",
        )?;
    }
    let mut topology = topology::decode(exchange, &mut session.ir, &carrier_index, session.ctx);
    geometry::infer_edge_parameter_ranges(&mut session.ir, session.ctx)?;
    let owned_carriers = geometry::topology_owned_carriers(&session.ir, &carrier_index);
    session.charge_stage("step_topology_association")?;
    geometry::associate_topology_carriers(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
    );
    session.charge_stage("step_replica_association")?;
    geometry::associate_replica_bases(exchange, &mut session.ir, &carrier_index);
    session.charge_stage("step_pcurve_association")?;
    geometry::associate_pcurve_supports(exchange, &mut session.ir, &carrier_index);
    session.charge_stage("step_geometric_set_association")?;
    geometry::associate_free_geometric_set_members(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    session.charge_stage("step_representation_association")?;
    geometry::associate_free_representation_members(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    session.charge_stage("step_presentation_carrier_association")?;
    geometry::associate_free_presentation_carriers(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    session.charge_stage("step_surface_curve_association")?;
    geometry::associate_surface_curve_supports(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
    );
    session.charge_stage("step_product_decode")?;
    let mut product = product::decode(
        exchange,
        &geometry.value,
        &topology.value,
        &mut session.ir,
        session.ctx,
        &mut session.admitted_ir_entities,
    )?;
    session.charge_stage("step_tessellation_decode")?;
    let mut tessellation =
        tessellation::decode(exchange, &geometry.value, &topology.value, &mut session.ir);
    session.charge_stage("step_pmi_decode")?;
    let mut pmi = pmi::decode(
        exchange,
        &geometry.value,
        &topology.value,
        &mut session.ir,
        session.ctx,
    );
    session.charge_stage("step_presentation_decode")?;
    let mut presentation = presentation::decode(
        exchange,
        &topology.value,
        &mut session.ir,
        &product.value.product_definition_ids_by_source,
        session.ctx,
    );
    session.charge_stage("step_validation_decode")?;
    let mut validation = validation::decode(exchange, &geometry.value, &mut session.ir);
    session.report.geometry_transferred = !session.ir.model.points.is_empty()
        || !session.ir.model.curves.is_empty()
        || !session.ir.model.surfaces.is_empty()
        || !session.ir.model.bodies.is_empty()
        || !session.ir.model.tessellations.is_empty();

    // Keep the established report order while every pass contributes through
    // the same accumulator.
    session.absorb(&mut dependencies);
    session.absorb(&mut presentation);
    session.absorb(&mut product);
    session.absorb_warnings(std::mem::take(&mut geometry.warnings));
    session.absorb_warnings(std::mem::take(&mut topology.warnings));
    session.absorb_warnings(std::mem::take(&mut presentation.warnings));
    session.absorb_warnings(std::mem::take(&mut product.warnings));
    session.absorb_warnings(std::mem::take(&mut tessellation.warnings));
    session.absorb(&mut tessellation);
    session.absorb(&mut topology);
    session.absorb(&mut geometry);
    session.absorb_warnings(std::mem::take(&mut pmi.warnings));
    session.absorb_warnings(std::mem::take(&mut validation.warnings));
    session.absorb(&mut pmi);
    session.absorb(&mut validation);

    session.charge_stage("step_drawing_decode")?;
    let mut drawing = drawing::decode(
        exchange,
        &mut session.ir,
        &session.typed_records,
        &product.value.product_definition_ids_by_shape,
    );
    session.absorb(&mut drawing);
    let mut post_decode_warnings = Vec::new();
    session.charge_stage("step_carrier_retention")?;
    retain_unowned_carriers(
        exchange,
        &mut session.ir,
        &mut session.typed_records,
        &mut post_decode_warnings,
    );
    session.absorb_warnings(post_decode_warnings);

    session.charge_stage("step_opaque_record_retention")?;
    let opaque_offsets = if retain_opaque {
        BTreeSet::new()
    } else {
        exchange
            .records
            .values()
            .filter(|record| !session.typed_records.contains(&record.id))
            .map(|record| record.span.start)
            .collect()
    };
    let mut counts = BTreeMap::<String, usize>::new();
    let mut opaque_ids = BTreeMap::new();
    let mut source_targets = BTreeMap::new();
    let mut opaque_sources = Vec::new();
    let mut source_fidelity = SourceFidelity::default();
    if retain_opaque {
        opaque_ids = exchange
            .records
            .values()
            .filter(|record| !session.typed_records.contains(&record.id))
            .map(|record| (record.id, opaque_record_id(record).0))
            .collect::<BTreeMap<_, _>>();
        opaque_sources.reserve(opaque_ids.len());
        for record in exchange.records.values() {
            if session.typed_records.contains(&record.id) {
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
            for partial in &record.partials {
                partial
                    .parameters
                    .iter()
                    .for_each(|value| collect_references(value, &mut links));
            }
            opaque_sources.push(OpaqueSourceRecord {
                unknown_id: opaque_ids[&record.id].clone(),
                span: record.span.clone(),
                links,
                reference_work,
            });
        }
        let target_ids = opaque_sources
            .iter()
            .flat_map(|source| source.links.iter().copied())
            .collect::<BTreeSet<_>>();
        source_targets = record_targets(&session.ir, |record_id| target_ids.contains(&record_id));
    } else {
        for record in exchange.records.values() {
            if session.typed_records.contains(&record.id) {
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
        if let Some(ctx) = session.ctx {
            ctx.charge_work(
                u64::try_from(input.len()).unwrap_or(u64::MAX),
                "step_byte_accounting",
            )?;
        }
        let _reservation = session
            .ctx
            .map(|ctx| ctx.reserve_scoped(input.len() as u64, "step_byte_accounting", None))
            .transpose()?;
        byte_accounting(input, exchange, &session.typed_records, session.ctx)?
    };
    if retain_opaque {
        let signature_spans = std::mem::take(&mut exchange.signatures);
        exchange.release_source_graph();
        let mut opaque = Vec::with_capacity(opaque_sources.len() + signature_spans.len());
        for source in opaque_sources {
            let bytes = if let Some(ctx) = session.ctx {
                ctx.charge_collection_items(source.reference_work, "step_opaque_record_links")?;
                ctx.copy_retained(&input[source.span.clone()], "step_opaque_record", None)?
            } else {
                input[source.span.clone()].to_vec()
            };
            opaque.push(UnknownRecord {
                id: UnknownId(source.unknown_id),
                offset: source.span.start as u64,
                byte_len: source.span.len() as u64,
                sha256: sha256_hex(&bytes),
                data: Some(bytes),
                links: source
                    .links
                    .into_iter()
                    .flat_map(|id| {
                        opaque_ids
                            .get(&id)
                            .cloned()
                            .into_iter()
                            .chain(source_targets.get(&id).into_iter().flatten().cloned())
                    })
                    .collect(),
            });
        }
        for (index, signature) in signature_spans.into_iter().enumerate() {
            let bytes = if let Some(ctx) = session.ctx {
                ctx.copy_retained(&input[signature.clone()], "step_signature_record", None)?
            } else {
                input[signature.clone()].to_vec()
            };
            *counts.entry("SIGNATURE".into()).or_default() += 1;
            opaque.push(UnknownRecord {
                id: crate::ids::StepIdentity::signature(index),
                offset: signature.start as u64,
                byte_len: signature.len() as u64,
                sha256: sha256_hex(&bytes),
                data: Some(bytes),
                links: Vec::new(),
            });
        }
        source_fidelity.attach_native_unknown_records(&mut session.ir, "step", opaque)?;
    }
    if let Some(source) = &mut session.ir.source {
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
        session
            .report
            .losses
            .push(StepLossCode::ByteAccountingUnclassified.note(format!(
                "STEP byte accounting left {} byte(s) unclassified",
                accounting.unclassified
            )));
    }
    session.report.notes.push(format!(
        "byte accounting: {} structural, {} typed, {} named opaque, {} unclassified",
        accounting.structural, accounting.typed, accounting.opaque, accounting.unclassified
    ));
    session
        .report
        .losses
        .extend(counts.into_iter().map(|(name, count)| {
            StepLossCode::OpaqueRecordPreserved.note(format!(
                "preserved {count} {name} instance(s) as named opaque STEP records"
            ))
        }));
    session.charge_pending_ir_entities("step_admit_ir_entities")?;
    Ok((session.into_result(source_fidelity), opaque_offsets))
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
    typed_records: &mut HashSet<u64>,
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
        .chain(
            ir.model
                .procedural_surfaces
                .iter()
                .filter_map(|surface| {
                    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
                        boundary_pcurves,
                        ..
                    } = &surface.definition
                    else {
                        return None;
                    };
                    Some(boundary_pcurves)
                })
                .flatten()
                .map(|pcurve| pcurve.0.clone()),
        )
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
        .filter(|id| !owned.contains(&StepIdentity::data("pcurve", id)))
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
        format: crate::dialect::FORMAT.into(),
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
    UnknownId(crate::ids::StepIdentity::data(&kind, record.id))
}

fn record_targets(
    ir: &CadIr,
    include_record: impl Fn(u64) -> bool,
) -> BTreeMap<u64, BTreeSet<String>> {
    cadmpeg_ir::index::ModelIndex::new(ir)
        .identities()
        .filter_map(|identity| {
            let record_id = source_record_id(identity)?;
            include_record(record_id).then(|| (record_id, identity.to_owned()))
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
    typed_records: &HashSet<u64>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<ByteAccounting, CodecError> {
    let mut classes = if let Some(ctx) = ctx {
        ctx.alloc_filled(input.len(), ByteClass::Unclassified, "step byte classes")?
    } else {
        alloc_filled(input.len(), ByteClass::Unclassified, "step byte classes")?
    };
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
    let mut lexer = crate::lex::Lexer::new(input);
    let mut cursor = 0;
    while let Ok(Some(token)) = lexer.next_token() {
        claim_range(&mut classes, &token.span, ByteClass::Structural);
        claim_trivia(input, cursor..token.span.start, &mut classes);
        cursor = token.span.end;
    }
    claim_trivia(input, cursor..input.len(), &mut classes);

    Ok(classes
        .into_iter()
        .fold(ByteAccounting::default(), |mut counts, class| {
            match class {
                ByteClass::Unclassified => counts.unclassified += 1,
                ByteClass::Structural => counts.structural += 1,
                ByteClass::Typed => counts.typed += 1,
                ByteClass::Opaque => counts.opaque += 1,
            }
            counts
        }))
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
    code: StepLossCode,
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
            losses.push(code.note(format!(
                "STEP record #{record_id} has an invalid {field} string: {error}"
            )));
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
pub(crate) mod tests;
