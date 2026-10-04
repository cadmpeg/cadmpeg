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

fn record_graph_limit(ctx: &DecodeContext<'_>) -> usize {
    usize::try_from(ctx.policy().limits.max_recursion_depth)
        .ok()
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
            exchange.joined_schema_identifiers(ctx)?,
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
        let primary = StepDialect::classify(exchange, ctx)?;
        let dialect_loss = crate::dialect::dialect_loss(&primary, ctx)?;
        let ir = CadIr::empty();

        let mut body = DecodeBody::new(if ctx.container_only() {
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {}
        } else {
            cadmpeg_ir::report::decode::DecodeTransfer::full(false)
        });
        for entry in ctx.admit_iter(exchange.references(), "STEP new borrowed traversal").map_err(cadmpeg_core::CodecError::from)? {
            ctx.reserve_vec(&mut body.notes, 1, "step_decode_reference_notes")?;
            body.notes.push(ctx.format_retained(
                format_args!("external reference {} -> {}", entry.name, entry.uri),
                "step_decode_reference_note_text",
            )?);
        }
        if let Some(loss) = dialect_loss {
            ctx.push_vec(&mut body.losses, loss, "step_decode_loss_notes")?;
        }
        for diagnostic in ctx.admit_iter(diagnostics, "STEP new traversal").map_err(cadmpeg_core::CodecError::from)? {
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
            let message = ctx.format_retained(
                format_args!("{}", diagnostic.message),
                "step_decode_diagnostic_message",
            )?;
            let loss = code.note(message).with_provenance(
                cadmpeg_ir::SourceProvenance::root(
                    crate::dialect::FORMAT,
                    u64_from_index(diagnostic.offset),
                )
                .with_tag(tag),
            );
            ctx.push_vec(&mut body.losses, loss, "step_decode_loss_notes")?;
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
        let units = self
            .semantic_input_work
            .checked_add(output_work)
            .ok_or_else(|| self.ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.ctx.charge_work(units, operation)
    }

    fn charge_pending_ir_entities(&mut self, operation: &'static str) -> Result<(), CodecError> {
        let current_entities = u64_from_index(self.ir.model.entity_count());
        if current_entities < self.admitted_ir_entities {
            self.admitted_ir_entities = current_entities;
            return Ok(());
        }
        let additional_entities = current_entities - self.admitted_ir_entities;
        self.ctx.charge_entities(additional_entities, operation)?;
        self.admitted_ir_entities = current_entities;
        Ok(())
    }

    fn absorb<T>(&mut self, outcome: &mut StageOutcome<T>) -> Result<(), CodecError> {
        let new_claims = self.ctx.admit_iter(&outcome
            .claims, "STEP absorb traversal").map_err(CodecError::from)?
            .filter(|id| !self.typed_records.contains(id))
            .count();
        self.ctx
            .reserve_set(&mut self.typed_records, new_claims, "step_stage_claims")?;
        self.typed_records.extend(outcome.claims.drain());
        self.ctx.reserve_vec(
            &mut self.body.losses,
            outcome.losses.len(),
            "step_stage_losses",
        )?;
        self.body.losses.append(&mut outcome.losses);
        self.ctx.reserve_vec(
            &mut self.body.notes,
            outcome.notes.len(),
            "step_stage_notes",
        )?;
        self.body.notes.append(&mut outcome.notes);
        Ok(())
    }

    fn into_result(
        mut self,
        source_fidelity: SourceFidelity,
        opaque_offsets: BTreeSet<usize>,
    ) -> Result<AnalyzedExchange, CodecError> {
        self.ir.source = Some(SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(
                self.matched
                    .try_clone_for_decode(self.ctx, "copy STEP dialect layer")?,
            ),
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

    session.semantic_input_work = semantic_input_work(exchange, session.ctx)?;
    session.charge_stage("step_geometry_decode")?;
    let mut geometry = geometry::decode(exchange, &mut session.ir, session.ctx)?;
    session.charge_stage("step_dependency_decode")?;
    let mut dependencies = dependencies::decode(exchange, session.ctx)?;
    session.charge_stage("step_carrier_index")?;
    let carrier_index = index::CarrierIndex::from_ir(&session.ir, session.ctx)?;
    session.charge_stage("step_topology_decode")?;
    session.ctx.charge_work(
        implicit_face_plane_work(exchange, session.ctx)?,
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
        session.ctx,
    )?;
    session.charge_stage("step_replica_association")?;
    geometry::associate_replica_bases(exchange, &mut session.ir, &carrier_index, session.ctx)?;
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
        session.ctx,
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
        session.ctx,
    )?;
    session.charge_stage("step_presentation_decode")?;
    let mut presentation = presentation::decode(
        exchange,
        &topology.value,
        &mut session.ir,
        &product.value.product_definition_ids_by_source,
        session.ctx,
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
    session.ctx.reserve_vec(
        &mut session.body.losses,
        post_decode_losses.len(),
        "step_carrier_retention_losses",
    )?;
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
        for (&id, record) in ctx.admit_iter(exchange.records(), "STEP decode exchange mode traversal").map_err(cadmpeg_core::CodecError::from)? {
            if session.typed_records.contains(&id) {
                continue;
            }
            let unknown_id = opaque_record_id(id, record, session.ctx)?;
            session
                .ctx
                .insert_btree_map(&mut opaque_ids, id, unknown_id, "step_opaque_ids")?;
        }
        session
            .ctx
            .reserve_vec(&mut opaque_sources, opaque_ids.len(), "step_opaque_sources")?;
        for (&id, record) in ctx.admit_iter(exchange.records(), "STEP decode exchange mode traversal").map_err(cadmpeg_core::CodecError::from)? {
            if session.typed_records.contains(&id) {
                continue;
            }
            count_unknown_kind(&mut counts, record, session.ctx)?;
            let mut links = BTreeSet::new();
            let mut reference_work = 0;
            for partial in ctx.admit_iter(&record.partials[..], "STEP opaque reference work partial traversal")? {
                reference_work = add_work(reference_work, work_sum(ctx.admit_iter(partial.parameters.as_slice(), "STEP opaque reference work parameter traversal")?.map(|value| reference_work_units(value, session.ctx)))?)?;
            }
            for partial in ctx.admit_iter(&(record.partials)[..], "STEP decode exchange mode traversal").map_err(cadmpeg_core::CodecError::from)? {
                for value in ctx.admit_iter(&(partial.parameters)[..], "STEP decode exchange mode traversal").map_err(cadmpeg_core::CodecError::from)? {
                    collect_references(value, &mut links, session.ctx)?;
                }
            }
            let unknown_id = &opaque_ids[&id];

            opaque_sources.push(OpaqueSourceRecord {
                unknown_id: unknown_id
                    .try_clone_for_decode(session.ctx, "step_opaque_source_identity")?,
                span: record.span.clone(),
                links,
                reference_work,
            });
        }
        let mut target_ids = BTreeSet::new();
        for source in ctx.admit_iter(&opaque_sources[..], "STEP decode exchange mode traversal")? {
            for &id in ctx.admit_iter(&source.links, "STEP opaque source target traversal")? {
                session.ctx.insert_btree_set(&mut target_ids, id, "step_opaque_target_ids_index")?;
            }
        }
        source_targets = record_targets(
            &session.ir,
            |record_id| target_ids.contains(&record_id),
            session.ctx,
        )?;
    } else {
        for (&id, record) in ctx.admit_iter(exchange.records(), "STEP decode exchange mode traversal").map_err(cadmpeg_core::CodecError::from)? {
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
            .reserve_scoped(u64_from_index(input.len()), "step_byte_accounting")?;
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
        let mut opaque = session
            .ctx
            .collection_vec(opaque_count, "step_opaque_records")?;
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
                    (session.ctx).push_formatted_retained(
                        &mut links,
                        format_args!("{}", unknown_id.as_str()),
                        "step_opaque_links",
                        "step_opaque_link_text",
                    )?;
                }
                if let Some(targets) = source_targets.get(&id) {
                    for target in targets {
                        (session.ctx).push_formatted_retained(
                            &mut links,
                            format_args!("{target}"),
                            "step_opaque_links",
                            "step_opaque_link_text",
                        )?;
                    }
                }
            }
            opaque.push(UnknownRecord::retained(
                source.unknown_id,
                u64_from_index(source.span.start),
                bytes,
                links,
            ));
        }
        for (index, signature) in signature_spans.into_iter().enumerate() {
            let bytes = session
                .ctx
                .copy_retained(&input[signature.clone()], "step_signature_record")?;
            let signature_kind = String::from("SIGNATURE");
            session
                .ctx
                .admit_btree_entry(&counts, &signature_kind, "step_opaque_kind_counts")?;
            *counts.entry(signature_kind).or_default() += 1;
            opaque.push(UnknownRecord::retained(
                ids::signature(index),
                u64_from_index(signature.start),
                bytes,
                Vec::new(),
            ));
        }
        source_fidelity.attach_native_unknown_records(
            &mut session.ir,
            "step",
            opaque,
            session.ctx,
        )?;
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
        (session.ctx).push_vec(
            &mut session.body.losses,
            StepLossCode::ByteAccountingUnclassified.note(format!(
                "STEP byte accounting left {} byte(s) unclassified",
                accounting.unclassified
            )),
            "step_decode_loss_notes",
        )?;
    }
    let accounting_note = format!(
        "byte accounting: {} structural, {} typed, {} named opaque, {} unclassified",
        accounting.structural, accounting.typed, accounting.opaque, accounting.unclassified
    );
    session
        .ctx
        .reserve_vec(&mut session.body.notes, 1, "step_byte_accounting_note")?;
    session.body.notes.push(accounting_note);
    for (name, count) in counts {
        let message = session.ctx.format_retained(
            format_args!("preserved {count} {name} instance(s) as named opaque STEP records"),
            "step_opaque_preservation_loss_text",
        )?;
        (session.ctx).push_vec(
            &mut session.body.losses,
            StepLossCode::OpaqueRecordPreserved.note(message),
            "step_decode_loss_notes",
        )?;
    }
    session.charge_pending_ir_entities("step_admit_ir_entities")?;
    session.into_result(source_fidelity, opaque_offsets)
}

/// Count the source graph nodes that each semantic pass may inspect.
fn work_overflow() -> CodecError {
    cadmpeg_core::decode::refuse_local_limit("step semantic work", u64::MAX, u64::MAX)
}

fn add_work(total: u64, additional: u64) -> Result<u64, CodecError> {
    total.checked_add(additional).ok_or_else(work_overflow)
}

fn work_sum(values: impl IntoIterator<Item = Result<u64, CodecError>>) -> Result<u64, CodecError> {
    values
        .into_iter()
        .try_fold(0_u64, |total, value| add_work(total, value?))
}

fn semantic_input_work(exchange: &Exchange, ctx: &DecodeContext<'_>) -> Result<u64, CodecError> {
    let mut total = u64_from_index(exchange.references().len());
    for (_, record) in ctx.admit_iter(exchange.records(), "STEP semantic work record traversal")? {
        let mut partials = 0;
        for partial in ctx.admit_iter(&record.partials[..], "STEP semantic work partial traversal")? {
            let parameters = work_sum(ctx.admit_iter(partial.parameters.as_slice(), "STEP semantic work parameter traversal")?.map(|value| value_work_units(value, ctx)))?;
            partials = add_work(partials, add_work(1, parameters)?)?;
        }
        total = add_work(total, add_work(1, partials)?)?;
    }
    for record in ctx.admit_iter(exchange.header(), "STEP semantic work header traversal")? {
        let parameters = work_sum(ctx.admit_iter(record.parameters.as_slice(), "STEP semantic work header parameter traversal")?.map(|value| value_work_units(value, ctx)))?;
        total = add_work(total, add_work(1, parameters)?)?;
    }
    for anchor in ctx.admit_iter(exchange.anchors(), "STEP semantic work anchor traversal")? {
        total = add_work(total, add_work(1, value_work_units(&anchor.value, ctx)?)?)?;
    }
    for section in ctx.admit_iter(exchange.data(), "STEP semantic work data traversal")? {
        let parameters = work_sum(ctx.admit_iter(section.parameters.as_slice(), "STEP semantic work data parameter traversal")?.map(|value| value_work_units(value, ctx)))?;
        total = add_work(total, add_work(add_work(1, parameters)?, u64_from_index(section.records.len()))?)?;
    }
    Ok(total)
}

fn value_work_units(value: &Value, ctx: &DecodeContext<'_>) -> Result<u64, CodecError> {
    let _depth = ctx.enter_nested("STEP semantic value nesting")?;
    match value {
        Value::List(values) => add_work(1, work_sum(ctx.admit_iter(values.as_slice(), "STEP semantic list traversal")?.map(|value| value_work_units(value, ctx)))?),
        Value::Typed(_, value) => add_work(1, value_work_units(value, ctx)?),
        _ => Ok(1),
    }
}

fn reference_work_units(value: &Value, ctx: &DecodeContext<'_>) -> Result<u64, CodecError> {
    let _depth = ctx.enter_nested("STEP reference work value nesting")?;
    match value {
        Value::Reference(_) => Ok(1),
        Value::List(values) => work_sum(ctx.admit_iter(values.as_slice(), "STEP reference work list traversal")?.map(|value| reference_work_units(value, ctx))),
        Value::Typed(_, value) => reference_work_units(value, ctx),
        _ => Ok(0),
    }
}

/// Reserve the linear scan used to derive a plane for an implicit face.
fn implicit_face_plane_work(exchange: &Exchange, ctx: &DecodeContext<'_>) -> Result<u64, CodecError> {
    let mut total = 0;
    for (_, record) in ctx.admit_iter(exchange.records(), "STEP implicit plane work record traversal")? {
        if let Some(points) = ctx.admit_iter(&record.partials[..], "STEP implicit plane work partial traversal")?.find(|partial| partial.name == "POLY_LOOP").and_then(|partial| partial.parameters.get(1)).and_then(Value::list) {
            total = add_work(total, u64_from_index(points.len()))?;
        }
    }
    Ok(total)
}

fn insert_retained_identity(
    identities: &mut BTreeSet<String>,
    identity: &str,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if !ctx.contains_btree_set(identities, identity, "STEP identities membership")? {
        let copy = ctx.format_retained(format_args!("{identity}"), "step_owned_pcurve_identity")?;
        ctx.insert_btree_set(identities, copy, "step_owned_pcurve_ids")?;
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
    for coedge in ctx.admit_iter(&(ir.model.coedges)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)? {
        for use_ in ctx.admit_iter(&(coedge.pcurves)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)? {
            insert_retained_identity(&mut owned, use_.pcurve.as_str(), ctx)?;
        }
    }
    for loop_ in ctx.admit_iter(&(ir.model.loops)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)? {
        if let Some((_, pcurves)) = loop_.singular_vertex() {
            for pcurve in ctx.admit_iter(pcurves, "STEP singular vertex pcurve traversal")? {
                insert_retained_identity(&mut owned, pcurve.pcurve.as_str(), ctx)?;
            }
        }
        for use_ in ctx.admit_iter(loop_.anchored_vertex_uses(), "STEP anchored vertex use traversal")? {
            for pcurve in ctx.admit_iter(use_.pcurves.as_slice(), "STEP anchored vertex pcurve traversal")? {
                insert_retained_identity(&mut owned, pcurve.pcurve.as_str(), ctx)?;
            }
        }
    }
    for surface in ctx.admit_iter(&(ir.model.procedural_surfaces)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)? {
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
            boundary_pcurves,
            ..
        } = surface.definition()
        else {
            continue;
        };
        for pcurve in ctx.admit_iter(boundary_pcurves.as_slice(), "STEP retain unowned carriers view traversal").map_err(cadmpeg_core::CodecError::from)? {
            insert_retained_identity(&mut owned, pcurve.as_str(), ctx)?;
        }
    }
    let mut unowned_pcurves = BTreeSet::new();
    for (&id, record) in ctx.admit_iter(exchange.records(), "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)? {
        if ctx.admit_iter(&(record
            .partials)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)?
            .any(|partial| partial.name == "PCURVE")
            && !ctx.contains_btree_set(&owned, ids::data(kind!("pcurve"), id).as_str(), "STEP owned pcurve identity lookup")?
        {
            ctx.insert_btree_set(&mut unowned_pcurves, id, "step_unowned_pcurves")?;
        }
    }
    let referenced = referenced_record_ids(exchange, ctx)?;
    let direct_carriers = ctx.admit_iter(&(ir
        .model
        .points
        )[..], "STEP retain unowned carriers chain traversal")?
        .filter(|point| point.source_object.is_none())
        .map(|point| point.id.as_str())
        .chain(
            ctx.admit_iter(&(ir.model
                .curves
                )[..], "STEP retain unowned carriers chain traversal")?
                .filter(|curve| curve.source_object.is_none())
                .map(|curve| curve.id.as_str()),
        )
        .chain(
            ctx.admit_iter(&(ir.model
                .surfaces
                )[..], "STEP retain unowned carriers chain traversal")?
                .filter(|surface| surface.source_object.is_none())
                .map(|surface| surface.id.as_str()),
        )
        .filter_map(step_instance_id)
        .filter(|id| exchange.records().contains_key(id) && !referenced.contains(id));
    let mut unowned_direct_carriers = BTreeSet::new();
    for id in direct_carriers {
        ctx.insert_btree_set(
            &mut unowned_direct_carriers,
            id,
            "step_unowned_direct_carriers",
        )?;
    }
    associate_unowned_direct_carriers(ir, &unowned_direct_carriers, ctx)?;
    if unowned_pcurves.is_empty() {
        return Ok(());
    }
    let mut roots = BTreeSet::new();
    for identity in ctx.admit_iter(&(ir
        .model
        .vertices)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)?
        .map(|vertex| Ok(vertex.point.as_str()))
        .chain(
            ctx.admit_iter(&(ir.model
                .edges
                )[..], "STEP retain unowned carriers chain traversal")?
                .filter_map(|edge| edge.curve().map(cadmpeg_ir::ids::CurveId::as_str)).map(Ok),
        )
        .chain(ctx.admit_iter(&(ir.model.faces)[..], "STEP retain unowned carriers chain traversal")?.map(|face| Ok(face.surface.as_str())))
        .chain(
            ctx.admit_iter(&(ir.model
                .coedges
                )[..], "STEP retain unowned carriers chain traversal")?
                .filter_map(|coedge| coedge.use_curve.as_ref().map(|use_| use_.curve.as_str())).map(Ok),
        )
        .chain(
            ctx.admit_iter(&(ir.model
                .pcurves
                )[..], "STEP retain unowned carriers chain traversal")?
                .map(|pcurve| {
                    ctx.contains_btree_set(&owned, pcurve.id.as_str(), "STEP protected pcurve root lookup")
                        .map(|owned| owned.then_some(pcurve.id.as_str()))
                })
                .filter_map(Result::transpose),
        )
        .chain(
            ctx.admit_iter(&(ir.model
                .points
                )[..], "STEP retain unowned carriers chain traversal")?
                .filter(|point| point.source_object.is_some())
                .map(|point| Ok(point.id.as_str())),
        )
        .chain(
            ctx.admit_iter(&(ir.model
                .curves
                )[..], "STEP retain unowned carriers chain traversal")?
                .filter(|curve| curve.source_object.is_some())
                .map(|curve| Ok(curve.id.as_str())),
        )
        .chain(
            ctx.admit_iter(&(ir.model
                .surfaces
                )[..], "STEP retain unowned carriers chain traversal")?
                .filter(|surface| surface.source_object.is_some())
                .map(|surface| Ok(surface.id.as_str())),
        )
        .chain(
            ctx.admit_iter(&(ir.model
                .procedural_curves
                )[..], "STEP retain unowned carriers chain traversal")?
                .map(|curve| Ok(curve.id.as_str())),
        )
        .chain(
            ctx.admit_iter(&(ir.model
                .procedural_surfaces
                )[..], "STEP retain unowned carriers chain traversal")?
                .map(|surface| Ok(surface.id.as_str())),
        )
        .map(|identity: Result<&str, CodecError>| identity.map(step_instance_id))
        .filter_map(Result::transpose)
    {
        ctx.insert_btree_set(&mut roots, identity?, "step_unowned_protected_roots")?;
    }
    let mut protected_roots = BTreeSet::new();
    for id in roots.into_iter().filter(|id| !unowned_pcurves.contains(id)) {
        ctx.insert_btree_set(&mut protected_roots, id, "step_unowned_protected_root_copy")?;
    }
    let protected = record_closure(&protected_roots, exchange, ctx)?;
    let removed_closure = record_closure(&unowned_pcurves, exchange, ctx)?;
    let deleted_pcurves = ctx.admit_iter(&(ir
        .model
        .pcurves)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)?
        .filter(|pcurve| !retains_carrier(pcurve.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_points = ctx.admit_iter(&(ir
        .model
        .points)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)?
        .filter(|point| !retains_carrier(point.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_curves = ctx.admit_iter(&(ir
        .model
        .curves)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)?
        .filter(|curve| !retains_carrier(curve.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_surfaces = ctx.admit_iter(&(ir
        .model
        .surfaces)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)?
        .filter(|surface| !retains_carrier(surface.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_procedural_curves = ctx.admit_iter(&(ir
        .model
        .procedural_curves)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)?
        .filter(|curve| !retains_carrier(curve.id.as_str(), &removed_closure, &protected))
        .count();
    let deleted_procedural_surfaces = ctx.admit_iter(&(ir
        .model
        .procedural_surfaces)[..], "STEP retain unowned carriers traversal").map_err(cadmpeg_core::CodecError::from)?
        .filter(|surface| !retains_carrier(surface.id.as_str(), &removed_closure, &protected))
        .count();
    ctx.retain_vec(&mut ir.model.pcurves, |pcurve| ctx.contains_btree_set(&owned, pcurve.id.as_str(), "STEP owned pcurve identity lookup"), "STEP unowned pcurves retention")?;
    ctx.retain_vec(&mut ir.model.points, |point| Ok(retains_carrier(point.id.as_str(), &removed_closure, &protected)), "STEP unowned points retention")?;
    ctx.retain_vec(&mut ir.model.curves, |curve| Ok(retains_carrier(curve.id.as_str(), &removed_closure, &protected)), "STEP unowned curves retention")?;
    ctx.retain_vec(&mut ir.model.surfaces, |surface| Ok(retains_carrier(surface.id.as_str(), &removed_closure, &protected)), "STEP unowned surfaces retention")?;
    ctx.retain_vec(&mut ir.model.procedural_curves, |curve| Ok(retains_carrier(curve.id.as_str(), &removed_closure, &protected)), "STEP unowned procedural_curves retention")?;
    ctx.retain_vec(&mut ir.model.procedural_surfaces, |surface| Ok(retains_carrier(surface.id.as_str(), &removed_closure, &protected)), "STEP unowned procedural_surfaces retention")?;
    typed_records.retain(|id| {
        !unowned_pcurves.contains(id) && (!removed_closure.contains(id) || protected.contains(id))
    });
    let protected_pcurves = ctx.admit_iter(&unowned_pcurves, "STEP protected pcurve traversal").map_err(cadmpeg_core::CodecError::from)?
        .filter(|id| protected.contains(id))
        .count();
    let opaque_pcurves = unowned_pcurves.len() - protected_pcurves;
    ctx.push_vec(losses, StepLossCode::DecodeWarning.note(format!(
        "unowned STEP carrier retention: opaque_pcurves={opaque_pcurves}, protected_pcurves={protected_pcurves}, deleted pcurves={deleted_pcurves}, points={deleted_points}, curves={deleted_curves}, surfaces={deleted_surfaces}, procedural_curves={deleted_procedural_curves}, procedural_surfaces={deleted_procedural_surfaces}"
    )), "step_decode_loss_notes")?;
    Ok(())
}

fn associate_unowned_direct_carriers(
    ir: &mut CadIr,
    ids: &BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for point in &mut ir.model.points {
        let Some(id) = step_instance_id(point.id.as_str()) else {
            continue;
        };
        if ids.contains(&id) && point.source_object.is_none() {
            point.source_object = Some(step_source_association(ctx, id, None)?);
        }
    }
    for curve in &mut ir.model.curves {
        let Some(id) = step_instance_id(curve.id.as_str()) else {
            continue;
        };
        if ids.contains(&id) && curve.source_object.is_none() {
            curve.source_object = Some(step_source_association(ctx, id, None)?);
        }
    }
    for surface in &mut ir.model.surfaces {
        let Some(id) = step_instance_id(surface.id.as_str()) else {
            continue;
        };
        if ids.contains(&id) && surface.source_object.is_none() {
            surface.source_object = Some(step_source_association(ctx, id, None)?);
        }
    }
    Ok(())
}

/// A non-blank STEP record reference.
fn step_source_id(
    ctx: &DecodeContext<'_>,
    id: u64,
) -> Result<cadmpeg_core::text::NonBlankString, CodecError> {
    cadmpeg_core::nonblank_literal!(ctx, "#{id}")
}

/// A source association for a STEP record.
fn step_source_association(
    ctx: &DecodeContext<'_>,
    id: u64,
    name: Option<String>,
) -> Result<SourceObjectAssociation, CodecError> {
    Ok(SourceObjectAssociation {
        format: cadmpeg_ir::codec_format!(crate::dialect::FORMAT),
        object_id: step_source_id(ctx, id)?,
        name,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    })
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
    let mut pending = ctx.collection_vec(roots.len(), "step_record_closure_pending")?;
    pending.extend(roots.iter().copied());
    while let Some(id) = pending.pop() {
        if closure.contains(&id) {
            continue;
        }
        ctx.insert_btree_set(&mut closure, id, "step_record_closure_ids")?;
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let mut references = BTreeSet::new();
        for partial in ctx.admit_iter(&(record
            .partials)[..], "STEP record closure traversal").map_err(cadmpeg_core::CodecError::from)? {
        for value in ctx.admit_iter(partial.parameters.as_slice(), "STEP record parameter traversal")? {
            collect_references(value, &mut references, ctx)?;
        }
    }
        ctx.reserve_vec(
            &mut pending,
            references.len(),
            "step_record_closure_pending",
        )?;
        pending.extend(references);
    }
    Ok(closure)
}

fn referenced_record_ids(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut references = BTreeSet::new();
    for record in ctx.admit_iter(exchange.records(), "STEP referenced record ids map traversal").map_err(cadmpeg_core::CodecError::from)?.map(|(_, value)| value) {
        for partial in ctx.admit_iter(&(record
            .partials)[..], "STEP referenced record ids traversal").map_err(cadmpeg_core::CodecError::from)? {
        for parameter in ctx.admit_iter(partial.parameters.as_slice(), "STEP record parameter traversal")? {
            collect_references(parameter, &mut references, ctx)?;
        }
    }
    }
    Ok(references)
}

fn count_unknown_kind(
    counts: &mut BTreeMap<String, usize>,
    record: &parse::RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let kind = ctx.join_display_retained(
        record.partials.iter().map(|partial| partial.name.as_str()),
        "+",
        "step_opaque_kind_text",
    )?;
    ctx.admit_btree_entry(counts, &kind, "step_opaque_kind_counts")?;
    *counts.entry(kind).or_default() += 1;
    Ok(())
}

fn opaque_record_id(
    id: u64,
    record: &parse::RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<UnknownId, CodecError> {
    let operation = "step_opaque_kind_name";
    let len = ctx.admit_iter(&(record
        .partials)[..], "STEP opaque record id traversal").map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
        .try_fold(0usize, |length, (index, partial)| {
            length
                .checked_add(usize::from(index > 0))?
                .checked_add(partial.name.len())
        })
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    let mut kind = ctx.retained_string(len, operation)?;

    for (index, partial) in ctx.admit_iter(&(record.partials)[..], "STEP opaque record id traversal").map_err(cadmpeg_core::CodecError::from)?.enumerate() {
        if index > 0 {
            kind.push('_');
        }
        for byte in ctx.admit_iter(partial.name.as_bytes(), "STEP opaque record id traversal").map_err(CodecError::from)?.copied() {
            kind.push(char::from(byte.to_ascii_lowercase()));
        }
    }
    let derived = crate::ids::IdentityKind::try_new(kind).ok();
    let kind = derived
        .as_ref()
        .map_or("record", crate::ids::IdentityKind::as_str);
    let text = ctx.format_retained(
        format_args!("step:data:{kind}#{id}"),
        "step_opaque_identity_text",
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
    for identity in cadmpeg_ir::index::ModelIndex::build(ir, ctx)?.identities(ctx) {
        let identity = identity?;
        let Some(record_id) = source_record_id(identity) else {
            continue;
        };
        if !include_record(record_id) {
            continue;
        }
        if !targets.contains_key(&record_id) {
            ctx.insert_btree_map(
                &mut targets,
                record_id,
                BTreeSet::new(),
                "step_opaque_target_records",
            )?;
        }
        let values = targets
            .get_mut(&record_id)
            .ok_or_else(|| ctx.refuse_codec_limit("step_opaque_target_records", 0, 1))?;
        if !ctx.contains_btree_set(values, identity, "STEP values membership")? {
            let copy =
                ctx.format_retained(format_args!("{identity}"), "step_opaque_target_identity")?;
            ctx.insert_btree_set(values, copy, "step_opaque_target_ids")?;
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
    for (&id, record) in ctx.admit_iter(exchange.records(), "STEP byte accounting traversal").map_err(cadmpeg_core::CodecError::from)? {
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
    for signature in ctx.admit_iter(exchange.signatures(), "STEP byte accounting borrowed traversal").map_err(cadmpeg_core::CodecError::from)? {
        claim_range(
            &mut classes,
            signature,
            ByteClass::Structural,
            format_args!("file signature at byte {}", signature.start),
        )?;
    }
    let mut lexer = crate::lex::Lexer::new(input, ctx);
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let Value::String(bytes) = value else {
        return Ok(None);
    };
    match exchange.decode_string_with_context(bytes, ctx) {
        Ok(text) => Ok(Some(text)),
        Err(crate::strings::StringDecodeFailure::Invalid(error)) => {
            let message = ctx.format_retained(
                format_args!("STEP record #{record_id} has an invalid {field} string: {error}"),
                "step_invalid_string_loss_text",
            )?;
            ctx.reserve_vec(losses, 1, "step_invalid_string_losses")?;
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
            ctx.insert_btree_set(output, *id, "step_reference_walk_ids")?;
        }
        Value::List(values) => {
            for value in ctx.admit_iter(values.as_slice(), "STEP collect references value traversal").map_err(cadmpeg_core::CodecError::from)? {
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
    fn parameters(&self) -> &[Value];
    fn parameter(&self, index: usize) -> Option<&Value>;
    fn partial(&self, ctx: &DecodeContext<'_>, name: &str) -> Result<Option<&crate::parse::PartialRecord>, CodecError>;
}

impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn parameters(&self) -> &[Value] {
        self.partials.first().parameters.as_slice()
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials.first().parameters.get(index)
    }
    fn partial(&self, ctx: &DecodeContext<'_>, name: &str) -> Result<Option<&crate::parse::PartialRecord>, CodecError> {
        Ok(ctx.admit_iter(&self.partials[..], "STEP partial record search")?.map(|partial| -> Result<Option<_>, CodecError> { Ok((ctx.equal(partial.name.as_str(), name, "STEP partial equality")?).then_some(partial)) }).find_map(Result::transpose).transpose()?)
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
            Value::Real(value) => Some(value.get()),
            Value::Integer(value) => cadmpeg_core::convert::f64_from_i64(*value),
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

fn named_parameter<'a>(ctx: &DecodeContext<'_>, record: &'a RawRecord, name: &str, index: usize) -> Result<Option<&'a Value>, CodecError> {
    Ok(ctx.admit_iter(&record.partials[..], "STEP named attribute partial traversal")?.map(|partial| -> Result<Option<_>, CodecError> { Ok((ctx.equal(partial.name.as_str(), name, "STEP named parameter equality")?).then_some(partial)) }).find_map(Result::transpose).transpose()?.and_then(|partial| partial.parameters.get(index)))
}

fn find_record_value<T>(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
    mut predicate: impl FnMut(&Value) -> Result<Option<T>, CodecError>,
) -> Result<Option<T>, CodecError> {
    for partial in ctx.admit_iter(&record.partials[..], "STEP record value partial traversal")? {
        for value in ctx.admit_iter(partial.parameters.as_slice(), "STEP record value parameter traversal")? {
            if let Some(found) = predicate(value)? { return Ok(Some(found)); }
        }
    }
    Ok(None)
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
    for (id, record) in ctx.admit_iter(exchange.records(), "STEP inspect opaque offsets traversal").map_err(cadmpeg_core::CodecError::from)? {
        if typed_records.contains(id) || offsets.contains(&record.span.start) {
            continue;
        }
        ctx.insert_btree_set(
            &mut offsets,
            record.span.start,
            "step_inspect_opaque_offsets",
        )?;
    }
    Ok(offsets)
}

#[cfg(test)]
pub(crate) mod tests;
