// SPDX-License-Identifier: Apache-2.0
//! Schema-aware STEP-to-IR decoding entry point.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::ids::{Identity, UnknownId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{SourceFidelity, SourceObjectAssociation};

use crate::dialect::StepDialect;
use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{self, Exchange, ParseDiagnostic, RawRecord, Value};

pub(crate) mod dependencies;
mod drawing;
pub(crate) mod geometry;
mod index;
pub(crate) mod pmi;
pub(crate) mod presentation;
pub(crate) mod product;
mod reference;
mod representation;
pub(crate) mod tessellation;
pub(crate) mod topology;
mod validation;

const MAX_RECORD_GRAPH_DEPTH: usize = 256;

/// Container facts available when the STEP source identity is authored.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Packaging {
    /// A bare ISO 10303-21 exchange.
    Bare,
    /// A ZIP-packaged exchange and its selected root entry.
    Zip {
        entry_count: usize,
        root_data_offset: u64,
    },
}

#[derive(Clone, Copy)]
enum DecodeMode {
    Decode(Packaging),
    Inspect,
}

impl Packaging {
    fn add_source_attributes(self, attributes: &mut BTreeMap<NonBlankString, String>) {
        let Self::Zip {
            entry_count,
            root_data_offset,
        } = self
        else {
            return;
        };
        attributes.insert(
            cadmpeg_core::nonblank_literal!("container_kind"),
            "iso-10303-21-zip".into(),
        );
        attributes.insert(
            cadmpeg_core::nonblank_literal!("archive_root"),
            crate::archive::ROOT_NAME.into(),
        );
        attributes.insert(
            cadmpeg_core::nonblank_literal!("archive_entries"),
            entry_count.to_string(),
        );
        attributes.insert(
            cadmpeg_core::nonblank_literal!("archive_root_data_offset"),
            root_data_offset.to_string(),
        );
    }
}

fn record_graph_limit(ctx: Option<&DecodeContext<'_>>) -> usize {
    ctx.and_then(|ctx| usize::try_from(ctx.policy().limits.max_recursion_depth).ok())
        .map_or(MAX_RECORD_GRAPH_DEPTH, |policy| {
            policy.min(MAX_RECORD_GRAPH_DEPTH)
        })
}

struct StageOutcome<T> {
    value: T,
    claims: HashSet<u64>,
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
    matched: DialectMatch,
    source_attributes: BTreeMap<NonBlankString, String>,
    body: DecodeBody,
    typed_records: HashSet<u64>,
    admitted_ir_entities: u64,
    semantic_input_work: u64,
    ctx: &'ctx DecodeContext<'arena>,
}

impl<'ctx, 'arena> StepDecodeSession<'ctx, 'arena> {
    fn new(
        exchange: &Exchange,
        diagnostics: &[ParseDiagnostic],
        ctx: &'ctx DecodeContext<'arena>,
        mode: DecodeMode,
    ) -> Result<Self, CodecError> {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            cadmpeg_core::nonblank_literal!("schema"),
            exchange.joined_schema_identifiers(Some(ctx))?,
        );
        attributes.insert(
            cadmpeg_core::nonblank_literal!("data_sections"),
            exchange.data().len().to_string(),
        );
        attributes.insert(
            cadmpeg_core::nonblank_literal!("entity_instances"),
            exchange.records().len().to_string(),
        );
        if let DecodeMode::Decode(packaging) = mode {
            packaging.add_source_attributes(&mut attributes);
        }
        // The `schema` attribute above stays: it is the joined identifier list,
        // and retiring the ad-hoc attribute keys is a later phase.
        let primary = StepDialect::classify(exchange, Some(ctx))?;
        let dialect_loss = crate::dialect::dialect_loss(&primary, Some(ctx))?;
        let ir = CadIr::empty();

        let mut body = DecodeBody::new(if ctx.container_only() {
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {}
        } else {
            cadmpeg_ir::report::decode::DecodeTransfer::full(false)
        });
        for entry in exchange.references() {
            ctx.charge_collection_items(1, "step_decode_reference_notes")?;
            body.notes
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("step_decode_reference_notes", 0, 1))?;
            body.notes.push(crate::decode_alloc::charged_format(
                ctx,
                "step_decode_reference_note_text",
                format_args!("external reference {} -> {}", entry.name, entry.uri),
            )?);
        }
        if let Some(loss) = dialect_loss {
            push_decode_loss(&mut body.losses, loss, ctx)?;
        }
        for diagnostic in diagnostics {
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
                crate::parse::ParseDiagnosticKind::ImplementationLevelUnverified => (
                    StepLossCode::ImplementationLevelUnverified,
                    "implementation_level",
                ),
            };
            let message = crate::decode_alloc::charged_format(
                ctx,
                "step_decode_diagnostic_message",
                format_args!("{}", diagnostic.message),
            )?;
            let loss = code.note(message).with_provenance(
                cadmpeg_ir::SourceProvenance::root(
                    crate::dialect::FORMAT,
                    diagnostic.offset as u64,
                )
                .with_tag(tag),
            );
            push_decode_loss(&mut body.losses, loss, ctx)?;
        }

        Ok(Self {
            ir,
            matched: primary,
            source_attributes: attributes,
            body,
            typed_records: HashSet::new(),
            admitted_ir_entities: 0,
            semantic_input_work: 0,
            ctx,
        })
    }

    fn charge_stage(&mut self, operation: &'static str) -> Result<(), CodecError> {
        self.charge_pending_ir_entities(operation)?;
        let output_work = u64_from_index(self.ir.model.entity_count());
        let units = self.semantic_input_work.saturating_add(output_work);
        self.ctx.charge_work(units, operation)
    }

    fn charge_pending_ir_entities(&mut self, operation: &'static str) -> Result<(), CodecError> {
        let current_entities = u64_from_index(self.ir.model.entity_count());
        let additional_entities = current_entities.saturating_sub(self.admitted_ir_entities);
        self.ctx.charge_entities(additional_entities, operation)?;
        self.admitted_ir_entities = current_entities;
        Ok(())
    }

    fn absorb<T>(&mut self, outcome: &mut StageOutcome<T>) -> Result<(), CodecError> {
        let new_claims = outcome
            .claims
            .iter()
            .filter(|id| !self.typed_records.contains(id))
            .count();
        self.ctx
            .charge_collection_items(u64_from_index(new_claims), "step_stage_claims")?;
        self.typed_records.try_reserve(new_claims).map_err(|_| {
            self.ctx
                .refuse_codec_limit("step_stage_claims", 0, u64_from_index(new_claims))
        })?;
        self.typed_records.extend(outcome.claims.drain());
        self.ctx
            .charge_collection_items(u64_from_index(outcome.losses.len()), "step_stage_losses")?;
        self.body
            .losses
            .try_reserve(outcome.losses.len())
            .map_err(|_| {
                self.ctx.refuse_codec_limit(
                    "step_stage_losses",
                    0,
                    u64_from_index(outcome.losses.len()),
                )
            })?;
        self.body.losses.append(&mut outcome.losses);
        self.ctx
            .charge_collection_items(u64_from_index(outcome.notes.len()), "step_stage_notes")?;
        self.body
            .notes
            .try_reserve(outcome.notes.len())
            .map_err(|_| {
                self.ctx.refuse_codec_limit(
                    "step_stage_notes",
                    0,
                    u64_from_index(outcome.notes.len()),
                )
            })?;
        self.body.notes.append(&mut outcome.notes);
        Ok(())
    }

    fn into_result(
        mut self,
        source_fidelity: SourceFidelity,
        opaque_offsets: BTreeSet<usize>,
    ) -> Result<AnalyzedExchange, CodecError> {
        for (name, value) in self.matched.declared() {
            self.ctx
                .charge_collection_items(1, "step_dialect_match_copy_items")?;
            let bytes = u64_from_index(name.as_str().len())
                .checked_add(u64_from_index(value.len()))
                .ok_or_else(|| {
                    self.ctx
                        .refuse_codec_limit("step_dialect_match_copy_text", 0, u64::MAX)
                })?;
            self.ctx
                .charge_retained(bytes, "step_dialect_match_copy_text")?;
        }
        if let Some(instance) = self.matched.instance() {
            self.ctx.charge_retained(
                u64_from_index(instance.len()),
                "step_dialect_match_copy_text",
            )?;
        }
        self.ir.source = Some(SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(self.matched.clone()),
            self.source_attributes,
        ));
        Ok(AnalyzedExchange {
            decoded: Decoded {
                ir: self.ir,
                body: self.body,
                source_fidelity,
            },
            matched: self.matched,
            opaque_offsets,
        })
    }
}

fn push_decode_loss(
    losses: &mut Vec<LossNote>,
    loss: LossNote,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "step_decode_loss_notes")?;
    losses
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("step_decode_loss_notes", 0, 1))?;
    losses.push(loss);
    Ok(())
}

pub(crate) struct AnalyzedExchange {
    pub(crate) decoded: Decoded,
    pub(crate) matched: DialectMatch,
    pub(crate) opaque_offsets: BTreeSet<usize>,
}

struct OpaqueSourceRecord {
    unknown_id: UnknownId,
    span: std::ops::Range<usize>,
    links: BTreeSet<u64>,
    reference_work: u64,
}

/// Decode a complete clear-text exchange structure.
pub(crate) fn decode(
    input: &[u8],
    ctx: &DecodeContext<'_>,
    packaging: Packaging,
) -> Result<Decoded, CodecError> {
    let (exchange, diagnostics) = parse::parse_with_context(input, ctx)?;
    decode_exchange(input, exchange, &diagnostics, ctx, packaging)
}

pub(crate) fn decode_exchange(
    input: &[u8],
    mut exchange: Exchange,
    diagnostics: &[ParseDiagnostic],
    ctx: &DecodeContext<'_>,
    packaging: Packaging,
) -> Result<Decoded, CodecError> {
    decode_exchange_mode(
        input,
        &mut exchange,
        diagnostics,
        DecodeMode::Decode(packaging),
        ctx,
    )
    .map(|result| result.decoded)
}

/// Deep semantic analysis used by STEP `inspect`.
///
/// Runs the semantic decode path (discarding the IR at the inspect boundary)
/// so `unknown_entities` and related attributes stay accurate. This is not a
/// cheap syntactic census.
pub(crate) fn analyze_exchange(
    input: &[u8],
    exchange: &mut Exchange,
    diagnostics: &[ParseDiagnostic],
    ctx: &DecodeContext<'_>,
) -> Result<AnalyzedExchange, CodecError> {
    decode_exchange_mode(input, exchange, diagnostics, DecodeMode::Inspect, ctx)
}

fn decode_exchange_mode(
    input: &[u8],
    exchange: &mut Exchange,
    diagnostics: &[ParseDiagnostic],
    mode: DecodeMode,
    ctx: &DecodeContext<'_>,
) -> Result<AnalyzedExchange, CodecError> {
    let mut session = StepDecodeSession::new(exchange, diagnostics, ctx, mode)?;
    if ctx.container_only() {
        return session.into_result(SourceFidelity::default(), BTreeSet::new());
    }

    session.semantic_input_work = semantic_input_work(exchange);
    session.charge_stage("step_geometry_decode")?;
    let mut geometry = geometry::decode(exchange, &mut session.ir, session.ctx)?;
    session.charge_stage("step_dependency_decode")?;
    let mut dependencies = dependencies::decode(exchange, session.ctx)?;
    session.charge_stage("step_carrier_index")?;
    let carrier_index = index::CarrierIndex::from_ir(&session.ir, session.ctx)?;
    session.charge_stage("step_topology_decode")?;
    session.ctx.charge_work(
        implicit_face_plane_work(exchange),
        "step_implicit_face_plane",
    )?;
    let mut topology = topology::decode(exchange, &mut session.ir, &carrier_index, session.ctx)?;
    geometry::infer_edge_parameter_ranges(&mut session.ir, session.ctx)?;
    let owned_carriers =
        geometry::topology_owned_carriers(&session.ir, &carrier_index, session.ctx)?;
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
    geometry::associate_pcurve_supports(exchange, &mut session.ir, &carrier_index, session.ctx)?;
    session.charge_stage("step_geometric_set_association")?;
    geometry::associate_free_geometric_set_members(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
        session.ctx,
    )?;
    session.charge_stage("step_representation_association")?;
    geometry::associate_free_representation_members(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
        session.ctx,
    )?;
    session.charge_stage("step_presentation_carrier_association")?;
    geometry::associate_free_presentation_carriers(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
        session.ctx,
    )?;
    session.charge_stage("step_surface_curve_association")?;
    geometry::associate_surface_curve_supports(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        session.ctx,
    )?;
    session.charge_stage("step_product_decode")?;
    let mut product = product::decode(
        exchange,
        &geometry.value,
        &topology.value,
        &mut session.ir,
        Some(session.ctx),
        &mut session.admitted_ir_entities,
    )?;
    session.charge_stage("step_tessellation_decode")?;
    let mut tessellation = tessellation::decode(
        exchange,
        &geometry.value,
        &topology.value,
        &mut session.ir,
        session.ctx,
    )?;
    session.charge_stage("step_pmi_decode")?;
    let mut pmi = pmi::decode(
        exchange,
        &geometry.value,
        &topology.value,
        &mut session.ir,
        Some(session.ctx),
    )?;
    session.charge_stage("step_presentation_decode")?;
    let mut presentation = presentation::decode(
        exchange,
        &topology.value,
        &mut session.ir,
        &product.value.product_definition_ids_by_source,
        Some(session.ctx),
    )?;
    session.charge_stage("step_validation_decode")?;
    let mut validation =
        validation::decode(exchange, &geometry.value, &mut session.ir, session.ctx)?;
    if !session.ir.model.points.is_empty()
        || !session.ir.model.curves.is_empty()
        || !session.ir.model.surfaces.is_empty()
        || !session.ir.model.bodies.is_empty()
        || !session.ir.model.tessellations.is_empty()
    {
        session.body.transfer = cadmpeg_ir::report::decode::DecodeTransfer::full(true);
    }

    // Keep the established report order while every pass contributes through
    // the same accumulator.
    session.absorb(&mut dependencies)?;
    session.absorb(&mut presentation)?;
    session.absorb(&mut product)?;
    session.absorb(&mut tessellation)?;
    session.absorb(&mut topology)?;
    session.absorb(&mut geometry)?;
    session.absorb(&mut pmi)?;
    session.absorb(&mut validation)?;

    session.charge_stage("step_drawing_decode")?;
    let mut drawing = drawing::decode(
        exchange,
        &mut session.ir,
        &session.typed_records,
        &product.value.product_definition_ids_by_shape,
        session.ctx,
    )?;
    session.absorb(&mut drawing)?;
    let mut post_decode_losses = Vec::new();
    session.charge_stage("step_carrier_retention")?;
    retain_unowned_carriers(
        exchange,
        &mut session.ir,
        &mut session.typed_records,
        &mut post_decode_losses,
        session.ctx,
    )?;
    session.ctx.charge_collection_items(
        u64_from_index(post_decode_losses.len()),
        "step_carrier_retention_losses",
    )?;
    session
        .body
        .losses
        .try_reserve(post_decode_losses.len())
        .map_err(|_| {
            session.ctx.refuse_codec_limit(
                "step_carrier_retention_losses",
                0,
                u64_from_index(post_decode_losses.len()),
            )
        })?;
    session.body.losses.append(&mut post_decode_losses);

    session.charge_stage("step_opaque_record_retention")?;
    let opaque_offsets = match mode {
        DecodeMode::Decode(_) => BTreeSet::new(),
        DecodeMode::Inspect => {
            inspect_opaque_offsets(exchange, &session.typed_records, session.ctx)?
        }
    };
    let mut counts = BTreeMap::<String, usize>::new();
    let mut opaque_ids = BTreeMap::new();
    let mut source_targets = BTreeMap::new();
    let mut opaque_sources = Vec::new();
    let mut source_fidelity = SourceFidelity::default();
    if matches!(mode, DecodeMode::Decode(_)) {
        for (&id, record) in exchange.records() {
            if session.typed_records.contains(&id) {
                continue;
            }
            let unknown_id = opaque_record_id(id, record, session.ctx)?;
            session.ctx.charge_collection_items(1, "step_opaque_ids")?;
            opaque_ids.insert(id, unknown_id);
        }
        session
            .ctx
            .charge_collection_items(u64_from_index(opaque_ids.len()), "step_opaque_sources")?;
        opaque_sources
            .try_reserve_exact(opaque_ids.len())
            .map_err(|_| {
                session.ctx.refuse_codec_limit(
                    "step_opaque_sources",
                    0,
                    u64_from_index(opaque_ids.len()),
                )
            })?;
        for (&id, record) in exchange.records() {
            if session.typed_records.contains(&id) {
                continue;
            }
            count_unknown_kind(&mut counts, record, session.ctx)?;
            let mut links = BTreeSet::new();
            let reference_work = record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .map(reference_work_units)
                .fold(0, u64::saturating_add);
            for partial in &record.partials {
                for value in &partial.parameters {
                    collect_references(value, &mut links, session.ctx)?;
                }
            }
            let unknown_id = &opaque_ids[&id];
            session.ctx.charge_retained(
                u64_from_index(unknown_id.as_str().len()),
                "step_opaque_source_identity",
            )?;
            opaque_sources.push(OpaqueSourceRecord {
                unknown_id: unknown_id.clone(),
                span: record.span.clone(),
                links,
                reference_work,
            });
        }
        let mut target_ids = BTreeSet::new();
        for id in opaque_sources
            .iter()
            .flat_map(|source| source.links.iter().copied())
        {
            if !target_ids.contains(&id) {
                session
                    .ctx
                    .charge_collection_items(1, "step_opaque_target_ids_index")?;
                target_ids.insert(id);
            }
        }
        source_targets = record_targets(
            &session.ir,
            |record_id| target_ids.contains(&record_id),
            session.ctx,
        )?;
    } else {
        for (&id, record) in exchange.records() {
            if session.typed_records.contains(&id) {
                continue;
            }
            count_unknown_kind(&mut counts, record, session.ctx)?;
        }
    }
    let accounting = {
        session
            .ctx
            .charge_work(u64_from_index(input.len()), "step_byte_accounting")?;
        let _reservation = session
            .ctx
            .reserve_scoped(input.len() as u64, "step_byte_accounting")?;
        byte_accounting(input, exchange, &session.typed_records, session.ctx)?
    };
    if matches!(mode, DecodeMode::Decode(_)) {
        let signature_spans = exchange.release_source_graph();
        let opaque_count = opaque_sources
            .len()
            .checked_add(signature_spans.len())
            .ok_or_else(|| {
                session
                    .ctx
                    .refuse_codec_limit("step_opaque_records", 0, u64::MAX)
            })?;
        session
            .ctx
            .charge_collection_items(u64_from_index(opaque_count), "step_opaque_records")?;
        let mut opaque = Vec::new();
        opaque.try_reserve_exact(opaque_count).map_err(|_| {
            session
                .ctx
                .refuse_codec_limit("step_opaque_records", 0, u64_from_index(opaque_count))
        })?;
        for source in opaque_sources {
            session
                .ctx
                .charge_collection_items(source.reference_work, "step_opaque_record_links")?;
            let bytes = session
                .ctx
                .copy_retained(&input[source.span.clone()], "step_opaque_record")?;
            let mut links = Vec::new();
            for id in source.links {
                if let Some(unknown_id) = opaque_ids.get(&id) {
                    push_opaque_link(&mut links, unknown_id.as_str(), session.ctx)?;
                }
                if let Some(targets) = source_targets.get(&id) {
                    for target in targets {
                        push_opaque_link(&mut links, target, session.ctx)?;
                    }
                }
            }
            opaque.push(UnknownRecord::retained(
                source.unknown_id,
                source.span.start as u64,
                bytes,
                links,
            ));
        }
        for (index, signature) in signature_spans.into_iter().enumerate() {
            let bytes = session
                .ctx
                .copy_retained(&input[signature.clone()], "step_signature_record")?;
            if !counts.contains_key("SIGNATURE") {
                session
                    .ctx
                    .charge_collection_items(1, "step_opaque_kind_counts")?;
            }
            *counts.entry("SIGNATURE".into()).or_default() += 1;
            opaque.push(UnknownRecord::retained(
                ids::signature(index),
                signature.start as u64,
                bytes,
                Vec::new(),
            ));
        }
        source_fidelity.attach_native_unknown_records(&mut session.ir, "step", opaque)?;
    }
    session.source_attributes.insert(
        cadmpeg_core::nonblank_literal!("bytes_structural"),
        accounting.structural.to_string(),
    );
    session.source_attributes.insert(
        cadmpeg_core::nonblank_literal!("bytes_typed"),
        accounting.typed.to_string(),
    );
    session.source_attributes.insert(
        cadmpeg_core::nonblank_literal!("bytes_named_opaque"),
        accounting.opaque.to_string(),
    );
    session.source_attributes.insert(
        cadmpeg_core::nonblank_literal!("bytes_unclassified"),
        accounting.unclassified.to_string(),
    );
    if accounting.unclassified > 0 {
        push_decode_loss(
            &mut session.body.losses,
            StepLossCode::ByteAccountingUnclassified.note(format!(
                "STEP byte accounting left {} byte(s) unclassified",
                accounting.unclassified
            )),
            session.ctx,
        )?;
    }
    let accounting_note = format!(
        "byte accounting: {} structural, {} typed, {} named opaque, {} unclassified",
        accounting.structural, accounting.typed, accounting.opaque, accounting.unclassified
    );
    session
        .ctx
        .charge_collection_items(1, "step_byte_accounting_note")?;
    session.body.notes.try_reserve(1).map_err(|_| {
        session
            .ctx
            .refuse_codec_limit("step_byte_accounting_note", 0, 1)
    })?;
    session.body.notes.push(accounting_note);
    for (name, count) in counts {
        let message = crate::decode_alloc::charged_format(
            session.ctx,
            "step_opaque_preservation_loss_text",
            format_args!("preserved {count} {name} instance(s) as named opaque STEP records"),
        )?;
        push_decode_loss(
            &mut session.body.losses,
            StepLossCode::OpaqueRecordPreserved.note(message),
            session.ctx,
        )?;
    }
    session.charge_pending_ir_entities("step_admit_ir_entities")?;
    session.into_result(source_fidelity, opaque_offsets)
}

/// Count the source graph nodes that each semantic pass may inspect.
fn semantic_input_work(exchange: &Exchange) -> u64 {
    let records = exchange.records().values().map(|record| {
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
    let headers = exchange.header().iter().map(|record| {
        1_u64.saturating_add(
            record
                .parameters
                .iter()
                .map(value_work_units)
                .fold(0, u64::saturating_add),
        )
    });
    let anchors = exchange
        .anchors()
        .iter()
        .map(|anchor| 1_u64.saturating_add(value_work_units(&anchor.value)));
    let data = exchange.data().iter().map(|section| {
        1_u64
            .saturating_add(
                section
                    .parameters
                    .iter()
                    .map(value_work_units)
                    .fold(0, u64::saturating_add),
            )
            .saturating_add(u64_from_index(section.records.len()))
    });
    let references = u64_from_index(exchange.references().len());
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
        .records()
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
                .map(|points| u64_from_index(points.len()))
        })
        .fold(0, u64::saturating_add)
}

fn insert_retained_identity(
    identities: &mut BTreeSet<String>,
    identity: &str,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if !identities.contains(identity) {
        ctx.charge_collection_items(1, "step_owned_pcurve_ids")?;
        let copy = crate::decode_alloc::charged_format(
            ctx,
            "step_owned_pcurve_identity",
            format_args!("{identity}"),
        )?;
        identities.insert(copy);
    }
    Ok(())
}

fn retain_unowned_carriers(
    exchange: &Exchange,
    ir: &mut CadIr,
    typed_records: &mut HashSet<u64>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut owned = BTreeSet::new();
    for coedge in &ir.model.coedges {
        for use_ in &coedge.pcurves {
            insert_retained_identity(&mut owned, use_.pcurve.as_str(), ctx)?;
        }
    }
    for loop_ in &ir.model.loops {
        for pcurve in loop_.vertex_pcurves() {
            insert_retained_identity(&mut owned, pcurve.pcurve.as_str(), ctx)?;
        }
    }
    for surface in &ir.model.procedural_surfaces {
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
            boundary_pcurves,
            ..
        } = surface.definition()
        else {
            continue;
        };
        for pcurve in boundary_pcurves {
            insert_retained_identity(&mut owned, pcurve.as_str(), ctx)?;
        }
    }
    let mut unowned_pcurves = BTreeSet::new();
    for (&id, record) in exchange.records() {
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "PCURVE")
            && !owned.contains(ids::data(kind!("pcurve"), id).as_str())
        {
            ctx.charge_collection_items(1, "step_unowned_pcurves")?;
            unowned_pcurves.insert(id);
        }
    }
    let referenced = referenced_record_ids(exchange, ctx)?;
    let direct_carriers = ir
        .model
        .points
        .iter()
        .filter(|point| point.source_object.is_none())
        .map(|point| point.id.as_str())
        .chain(
            ir.model
                .curves
                .iter()
                .filter(|curve| curve.source_object.is_none())
                .map(|curve| curve.id.as_str()),
        )
        .chain(
            ir.model
                .surfaces
                .iter()
                .filter(|surface| surface.source_object.is_none())
                .map(|surface| surface.id.as_str()),
        )
        .filter_map(step_instance_id)
        .filter(|id| exchange.records().contains_key(id) && !referenced.contains(id));
    let mut unowned_direct_carriers = BTreeSet::new();
    for id in direct_carriers {
        if !unowned_direct_carriers.contains(&id) {
            ctx.charge_collection_items(1, "step_unowned_direct_carriers")?;
            unowned_direct_carriers.insert(id);
        }
    }
    associate_unowned_direct_carriers(ir, &unowned_direct_carriers);
    if unowned_pcurves.is_empty() {
        return Ok(());
    }
    let mut roots = BTreeSet::new();
    for identity in ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.as_str())
        .chain(
            ir.model
                .edges
                .iter()
                .filter_map(|edge| edge.curve().map(cadmpeg_ir::ids::CurveId::as_str)),
        )
        .chain(ir.model.faces.iter().map(|face| face.surface.as_str()))
        .chain(
            ir.model
                .coedges
                .iter()
                .filter_map(|coedge| coedge.use_curve.as_ref().map(|use_| use_.curve.as_str())),
        )
        .chain(
            ir.model
                .pcurves
                .iter()
                .filter(|pcurve| owned.contains(pcurve.id.as_str()))
                .map(|pcurve| pcurve.id.as_str()),
        )
        .chain(
            ir.model
                .points
                .iter()
                .filter(|point| point.source_object.is_some())
                .map(|point| point.id.as_str()),
        )
        .chain(
            ir.model
                .curves
                .iter()
                .filter(|curve| curve.source_object.is_some())
                .map(|curve| curve.id.as_str()),
        )
        .chain(
            ir.model
                .surfaces
                .iter()
                .filter(|surface| surface.source_object.is_some())
                .map(|surface| surface.id.as_str()),
        )
        .chain(
            ir.model
                .procedural_curves
                .iter()
                .map(|curve| curve.id.as_str()),
        )
        .chain(
            ir.model
                .procedural_surfaces
                .iter()
                .map(|surface| surface.id.as_str()),
        )
        .filter_map(step_instance_id)
    {
        if !roots.contains(&identity) {
            ctx.charge_collection_items(1, "step_unowned_protected_roots")?;
            roots.insert(identity);
        }
    }
    let mut protected_roots = BTreeSet::new();
    for id in roots.into_iter().filter(|id| !unowned_pcurves.contains(id)) {
        ctx.charge_collection_items(1, "step_unowned_protected_root_copy")?;
        protected_roots.insert(id);
    }
    let protected = record_closure(&protected_roots, exchange, ctx)?;
    let removed_closure = record_closure(&unowned_pcurves, exchange, ctx)?;
    let deleted_pcurves = ir
        .model
        .pcurves
        .iter()
        .filter(|pcurve| !retains_carrier(pcurve.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_points = ir
        .model
        .points
        .iter()
        .filter(|point| !retains_carrier(point.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_curves = ir
        .model
        .curves
        .iter()
        .filter(|curve| !retains_carrier(curve.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| !retains_carrier(surface.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_procedural_curves = ir
        .model
        .procedural_curves
        .iter()
        .filter(|curve| !retains_carrier(curve.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_procedural_surfaces = ir
        .model
        .procedural_surfaces
        .iter()
        .filter(|surface| !retains_carrier(surface.id.as_str(), &removed_closure, &protected))
        .count();
    ir.model
        .pcurves
        .retain(|pcurve| owned.contains(pcurve.id.as_str()));
    ir.model
        .points
        .retain(|point| retains_carrier(point.id.as_str(), &removed_closure, &protected));
    ir.model
        .curves
        .retain(|curve| retains_carrier(curve.id.as_str(), &removed_closure, &protected));
    ir.model
        .surfaces
        .retain(|surface| retains_carrier(surface.id.as_str(), &removed_closure, &protected));
    ir.model
        .procedural_curves
        .retain(|curve| retains_carrier(curve.id.as_str(), &removed_closure, &protected));
    ir.model
        .procedural_surfaces
        .retain(|surface| retains_carrier(surface.id.as_str(), &removed_closure, &protected));
    typed_records.retain(|id| {
        !unowned_pcurves.contains(id) && (!removed_closure.contains(id) || protected.contains(id))
    });
    let protected_pcurves = unowned_pcurves
        .iter()
        .filter(|id| protected.contains(id))
        .count();
    let opaque_pcurves = unowned_pcurves.len() - protected_pcurves;
    push_decode_loss(losses, StepLossCode::DecodeWarning.note(format!(
        "unowned STEP carrier retention: opaque_pcurves={opaque_pcurves}, protected_pcurves={protected_pcurves}, deleted pcurves={deleted_pcurves}, points={deleted_points}, curves={deleted_curves}, surfaces={deleted_surfaces}, procedural_curves={deleted_procedural_curves}, procedural_surfaces={deleted_procedural_surfaces}"
    )), ctx)?;
    Ok(())
}

fn associate_unowned_direct_carriers(ir: &mut CadIr, ids: &BTreeSet<u64>) {
    for point in &mut ir.model.points {
        let Some(id) = step_instance_id(point.id.as_str()) else {
            continue;
        };
        if ids.contains(&id) && point.source_object.is_none() {
            point.source_object = Some(step_source_association(id, None));
        }
    }
    for curve in &mut ir.model.curves {
        let Some(id) = step_instance_id(curve.id.as_str()) else {
            continue;
        };
        if ids.contains(&id) && curve.source_object.is_none() {
            curve.source_object = Some(step_source_association(id, None));
        }
    }
    for surface in &mut ir.model.surfaces {
        let Some(id) = step_instance_id(surface.id.as_str()) else {
            continue;
        };
        if ids.contains(&id) && surface.source_object.is_none() {
            surface.source_object = Some(step_source_association(id, None));
        }
    }
}

/// A non-blank STEP record reference.
fn step_source_id(id: u64) -> cadmpeg_core::text::NonBlankString {
    cadmpeg_core::nonblank_literal!("#{id}")
}

/// A source association for a STEP record.
fn step_source_association(id: u64, name: Option<String>) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: cadmpeg_ir::codec_format!(crate::dialect::FORMAT),
        object_id: step_source_id(id),
        name,
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
    step_instance_id(identity)
        .is_none_or(|id| !removed_closure.contains(&id) || protected.contains(&id))
}

/// Extract the numeric STEP instance id from a canonical IR identity.
fn step_instance_id(identity: &str) -> Option<u64> {
    identity.rsplit_once('#')?.1.parse().ok()
}

fn record_closure(
    roots: &BTreeSet<u64>,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut closure = BTreeSet::new();
    ctx.charge_collection_items(u64_from_index(roots.len()), "step_record_closure_pending")?;
    let mut pending = Vec::new();
    pending.try_reserve_exact(roots.len()).map_err(|_| {
        ctx.refuse_codec_limit(
            "step_record_closure_pending",
            0,
            u64_from_index(roots.len()),
        )
    })?;
    pending.extend(roots.iter().copied());
    while let Some(id) = pending.pop() {
        if closure.contains(&id) {
            continue;
        }
        ctx.charge_collection_items(1, "step_record_closure_ids")?;
        closure.insert(id);
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let mut references = BTreeSet::new();
        for value in record
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            collect_references(value, &mut references, ctx)?;
        }
        ctx.charge_collection_items(
            u64_from_index(references.len()),
            "step_record_closure_pending",
        )?;
        pending.try_reserve(references.len()).map_err(|_| {
            ctx.refuse_codec_limit(
                "step_record_closure_pending",
                0,
                u64_from_index(references.len()),
            )
        })?;
        pending.extend(references);
    }
    Ok(closure)
}

fn referenced_record_ids(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut references = BTreeSet::new();
    for record in exchange.records().values() {
        for parameter in record
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            collect_references(parameter, &mut references, ctx)?;
        }
    }
    Ok(references)
}

fn count_unknown_kind(
    counts: &mut BTreeMap<String, usize>,
    record: &parse::RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let kind = crate::decode_alloc::charged_join(
        ctx,
        "step_opaque_kind_text",
        record.partials.iter().map(|partial| partial.name.as_str()),
        "+",
    )?;
    if !counts.contains_key(&kind) {
        ctx.charge_collection_items(1, "step_opaque_kind_counts")?;
    }
    *counts.entry(kind).or_default() += 1;
    Ok(())
}

fn push_opaque_link(
    links: &mut Vec<String>,
    identity: &str,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let copy = crate::decode_alloc::charged_format(
        ctx,
        "step_opaque_link_text",
        format_args!("{identity}"),
    )?;
    ctx.charge_collection_items(1, "step_opaque_links")?;
    links
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("step_opaque_links", 0, 1))?;
    links.push(copy);
    Ok(())
}

fn opaque_record_id(
    id: u64,
    record: &parse::RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<UnknownId, CodecError> {
    let operation = "step_opaque_kind_name";
    let len = record
        .partials
        .iter()
        .enumerate()
        .try_fold(0usize, |length, (index, partial)| {
            length
                .checked_add(usize::from(index > 0))?
                .checked_add(partial.name.len())
        })
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    ctx.charge_retained(u64_from_index(len), operation)?;
    let mut kind = String::new();
    kind.try_reserve_exact(len)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, u64_from_index(len)))?;
    for (index, partial) in record.partials.iter().enumerate() {
        if index > 0 {
            kind.push('_');
        }
        for byte in partial.name.bytes() {
            kind.push(char::from(byte.to_ascii_lowercase()));
        }
    }
    let derived = crate::ids::IdentityKind::try_new(kind).ok();
    let kind = derived
        .as_ref()
        .map_or("record", crate::ids::IdentityKind::as_str);
    let text = crate::decode_alloc::charged_format(
        ctx,
        "step_opaque_identity_text",
        format_args!("step:data:{kind}#{id}"),
    )?;
    let identity = Identity::new(text)
        .map_err(|_| CodecError::WrongFormat("invalid STEP opaque identity".into()))?;
    Ok(UnknownId::from(identity))
}

fn record_targets(
    ir: &CadIr,
    include_record: impl Fn(u64) -> bool,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, BTreeSet<String>>, CodecError> {
    let mut targets = BTreeMap::<u64, BTreeSet<String>>::new();
    for identity in cadmpeg_ir::index::ModelIndex::new(ir).identities() {
        let Some(record_id) = source_record_id(identity) else {
            continue;
        };
        if !include_record(record_id) {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = targets.entry(record_id) {
            ctx.charge_collection_items(1, "step_opaque_target_records")?;
            entry.insert(BTreeSet::new());
        }
        let values = targets
            .get_mut(&record_id)
            .ok_or_else(|| ctx.refuse_codec_limit("step_opaque_target_records", 0, 1))?;
        if !values.contains(identity) {
            ctx.charge_collection_items(1, "step_opaque_target_ids")?;
            values.insert(crate::decode_alloc::charged_format(
                ctx,
                "step_opaque_target_identity",
                format_args!("{identity}"),
            )?);
        }
    }
    Ok(targets)
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
    ctx: &DecodeContext<'_>,
) -> Result<ByteAccounting, CodecError> {
    let mut classes =
        ctx.alloc_filled(input.len(), ByteClass::Unclassified, "step byte classes")?;
    for (&id, record) in exchange.records() {
        let class = if typed_records.contains(&id) {
            ByteClass::Typed
        } else {
            ByteClass::Opaque
        };
        claim_range(
            &mut classes,
            &record.span,
            class,
            format_args!("record #{id}"),
        )?;
    }
    for signature in exchange.signatures() {
        claim_range(
            &mut classes,
            signature,
            ByteClass::Structural,
            format_args!("file signature at byte {}", signature.start),
        )?;
    }
    let mut lexer = crate::lex::Lexer::with_context(input, ctx);
    lexer.set_transient_literals();
    let mut cursor = 0;
    loop {
        let token = match lexer.next_token() {
            Ok(Some(token)) => token,
            Ok(None) => break,
            Err(error) => {
                if let Some(resource) = error.into_resource_error() {
                    return Err(resource);
                }
                break;
            }
        };
        claim_range(
            &mut classes,
            &token.span,
            ByteClass::Structural,
            format_args!("token at byte {}", token.span.start),
        )?;
        claim_trivia(input, cursor..token.span.start, &mut classes)?;
        cursor = token.span.end;
    }
    claim_trivia(input, cursor..input.len(), &mut classes)?;

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

/// Mark every byte of `range` that no earlier claim took.
///
/// `owner` names the source construct that states the span. A span that ends
/// past the classified input, or that ends before it starts, names bytes the
/// input does not hold, so it is refused with the declared end and the
/// available length.
fn claim_range(
    classes: &mut [ByteClass],
    range: &std::ops::Range<usize>,
    class: ByteClass,
    owner: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let available = classes.len();
    let claimed = classes.get_mut(range.start..range.end).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "STEP {owner} claims bytes {}..{}, but the input holds {available} bytes",
            range.start, range.end
        ))
    })?;
    for byte_class in claimed {
        if *byte_class == ByteClass::Unclassified {
            *byte_class = class;
        }
    }
    Ok(())
}

/// Mark the whitespace, comments and print-control directives of `range`.
///
/// The walk stops at the first byte that is none of those. A range that ends
/// past the input, or a directive that ends past the range, states bytes the
/// range does not hold, so both are refused with the declared end and the
/// available length.
fn claim_trivia(
    input: &[u8],
    range: std::ops::Range<usize>,
    classes: &mut [ByteClass],
) -> Result<(), CodecError> {
    let end = range.end;
    if end > input.len() || range.start > end {
        return Err(CodecError::malformed(format_args!(
            "STEP trivia claims bytes {}..{end}, but the input holds {} bytes",
            range.start,
            input.len()
        )));
    }
    let mut at = range.start;
    while at < end {
        let Some(&byte) = input.get(at) else {
            return Ok(());
        };
        if classes.get(at) != Some(&ByteClass::Unclassified) {
            at += 1;
        } else if byte.is_ascii_control() || byte == b' ' {
            claim_range(
                classes,
                &(at..at + 1),
                ByteClass::Structural,
                format_args!("trivia byte at {at}"),
            )?;
            at += 1;
        } else if let Some(after_print_control) = crate::lex::print_control_end(input, at) {
            if after_print_control > end {
                return Err(CodecError::malformed(format_args!(
                    "STEP print control directive at byte {at} ends at {after_print_control}, but its trivia run ends at {end}"
                )));
            }
            claim_range(
                classes,
                &(at..after_print_control),
                ByteClass::Structural,
                format_args!("print control directive at byte {at}"),
            )?;
            at = after_print_control;
        } else if input[at..end].starts_with(b"/*") {
            let Some(relative_end) = input[at + 2..end]
                .windows(2)
                .position(|window| window == b"*/")
            else {
                return Ok(());
            };
            claim_range(
                classes,
                &(at..at + relative_end + 4),
                ByteClass::Structural,
                format_args!("comment at byte {at}"),
            )?;
            at += relative_end + 4;
        } else {
            return Ok(());
        }
    }
    Ok(())
}

fn decode_text_charged(
    exchange: &Exchange,
    value: &Value,
    losses: &mut Vec<LossNote>,
    record_id: u64,
    field: &str,
    code: StepLossCode,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<String>, CodecError> {
    let Value::String(bytes) = value else {
        return Ok(None);
    };
    match exchange.decode_string_with_context(bytes, ctx) {
        Ok(text) => Ok(Some(text)),
        Err(crate::strings::StringDecodeFailure::Invalid(error)) => {
            let message = if let Some(ctx) = ctx {
                crate::decode_alloc::charged_format(
                    ctx,
                    "step_invalid_string_loss_text",
                    format_args!("STEP record #{record_id} has an invalid {field} string: {error}"),
                )?
            } else {
                format!("STEP record #{record_id} has an invalid {field} string: {error}")
            };
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "step_invalid_string_losses")?;
            }
            losses.try_reserve(1).map_err(|_| match ctx {
                Some(ctx) => ctx.refuse_codec_limit("step_invalid_string_losses", 0, 1),
                None => {
                    cadmpeg_core::decode::refuse_local_limit("step_invalid_string_losses", 0, 1)
                }
            })?;
            losses.push(code.note(message));
            Ok(None)
        }
        Err(crate::strings::StringDecodeFailure::Resource(error)) => Err(error),
    }
}

fn collect_references(
    value: &Value,
    output: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_reference_walk")?;
    match value {
        Value::Reference(id) => {
            if !output.contains(id) {
                ctx.charge_collection_items(1, "step_reference_walk_ids")?;
                output.insert(*id);
            }
        }
        Value::List(values) => {
            for value in values {
                collect_references(value, output, ctx)?;
            }
        }
        Value::Typed(_, value) => collect_references(value, output, ctx)?,
        _ => {}
    }
    Ok(())
}

/// Record accessors shared by the reader submodules.
trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn display_name(&self, ctx: Option<&DecodeContext<'_>>) -> Result<String, CodecError>;
    fn parameters(&self) -> &[Value];
    fn parameter(&self, index: usize) -> Option<&Value>;
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
}

impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn display_name(&self, ctx: Option<&DecodeContext<'_>>) -> Result<String, CodecError> {
        let names = self.partials.iter().map(|partial| partial.name.as_str());
        match ctx {
            Some(ctx) => {
                crate::decode_alloc::charged_join(ctx, "step_record_display_name", names, "+")
            }
            None => Ok(names.collect::<Vec<_>>().join("+")),
        }
    }
    fn parameters(&self) -> &[Value] {
        self.partials.first().parameters.as_slice()
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials.first().parameters.get(index)
    }
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials.iter().find(|partial| partial.name == name)
    }
}

/// Value accessors shared by the reader submodules.
trait ValueExt {
    fn number(&self) -> Option<f64>;
    fn typed_number(&self) -> Option<f64>;
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn enumeration(&self) -> Option<&str>;
    fn integer(&self) -> Option<i64>;
    fn logical(&self) -> Option<bool>;
}

impl ValueExt for Value {
    fn typed_number(&self) -> Option<f64> {
        match self {
            Value::Typed(_, value) => value.typed_number(),
            _ => self.number(),
        }
    }
    fn number(&self) -> Option<f64> {
        match self {
            Value::Real(value) => Some(*value),
            Value::Integer(value) => Some(*value as f64),
            _ => None,
        }
    }
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }
    fn list(&self) -> Option<&[Value]> {
        if let Value::List(values) = self {
            Some(values)
        } else {
            None
        }
    }
    fn enumeration(&self) -> Option<&str> {
        if let Value::Enumeration(value) = self {
            Some(value)
        } else {
            None
        }
    }
    fn integer(&self) -> Option<i64> {
        if let Value::Integer(value) = self {
            Some(*value)
        } else {
            None
        }
    }
    fn logical(&self) -> Option<bool> {
        match self {
            Value::Enumeration(value) if value == "T" => Some(true),
            Value::Enumeration(value) if value == "F" => Some(false),
            _ => None,
        }
    }
}

fn named_parameter<'a>(record: &'a RawRecord, name: &str, index: usize) -> Option<&'a Value> {
    record.partial(name)?.parameters.get(index)
}

fn record_values(record: &RawRecord) -> impl Iterator<Item = &Value> {
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
}

fn source_numeric_id(identity: &str, kind: &str) -> Option<u64> {
    let suffix = identity
        .strip_prefix("step:data:")?
        .strip_prefix(kind)?
        .strip_prefix('#')?;
    let suffix = suffix.strip_prefix("poly-point-").unwrap_or(suffix);
    suffix.split('-').next()?.parse().ok()
}

fn inspect_opaque_offsets(
    exchange: &Exchange,
    typed_records: &HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<usize>, CodecError> {
    let mut offsets = BTreeSet::new();
    for (id, record) in exchange.records() {
        if typed_records.contains(id) || offsets.contains(&record.span.start) {
            continue;
        }
        ctx.charge_collection_items(1, "step_inspect_opaque_offsets")?;
        offsets.insert(record.span.start);
    }
    Ok(offsets)
}

#[cfg(test)]
pub(crate) mod tests;
