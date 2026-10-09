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
    claims: BTreeSet<u64>,
    losses: Vec<LossNote>,
    notes: Vec<String>,
    value: T,
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

/// Adds each claimed record id to a stage's claim set.
fn claim_records(
    ctx: &DecodeContext<'_>,
    claims: &mut BTreeSet<u64>,
    ids: impl IntoIterator<Item = u64>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let mut ids = ids.into_iter();
    while let Some(id) = ctx.next_charged(&mut ids, operation)? {
        ctx.insert_btree_set(claims, id, operation)?;
    }
    Ok(())
}

struct StepDecodeSession<'ctx, 'arena> {
    ir: CadIr,
    matched: DialectMatch,
    source_attributes: BTreeMap<NonBlankString, String>,
    body: DecodeBody,
    typed_records: HashSet<u64>,
    typed_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    admitted_ir_entities: u64,
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
        let primary = StepDialect::classify(exchange, ctx)?;
        let dialect_loss = crate::dialect::dialect_loss(&primary, ctx)?;
        let ir = CadIr::empty();

        let mut body = DecodeBody::new(if ctx.container_only() {
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {}
        } else {
            cadmpeg_ir::report::decode::DecodeTransfer::full(false)
        });
        {
            let mut reference_entries = (exchange.references()).iter();
            for _ in 0..reference_entries.len() {
                let entry = ctx
                    .next_charged(&mut reference_entries, "STEP new borrowed traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                ctx.reserve_vec(&mut body.notes, 1, "step_decode_reference_notes")?;
                body.notes.push(ctx.format_retained(
                    format_args!("external reference {} -> {}", entry.name, entry.uri),
                    "step_decode_reference_note_text",
                )?);
            }
        }
        if let Some(loss) = dialect_loss {
            ctx.push_vec(&mut body.losses, loss, "step_decode_loss_notes")?;
        }
        {
            let mut parse_diagnostics = (diagnostics).iter();
            for _ in 0..parse_diagnostics.len() {
                let diagnostic = ctx
                    .next_charged(&mut parse_diagnostics, "STEP new traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
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
        }

        Ok(Self {
            ir,
            matched: primary,
            source_attributes: attributes,
            body,
            typed_records: HashSet::new(),
            typed_storage: ctx.reserve_scoped(0, "STEP session typed record index")?,
            admitted_ir_entities: 0,
            ctx,
        })
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
        {
            let mut stage_claims = (std::mem::take(&mut outcome.claims)).into_iter();
            for _ in 0..stage_claims.len() {
                let id = self.ctx
                    .next_charged(&mut stage_claims, "step_stage_claims")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                self.typed_storage.with_storage(|| {
                    self.ctx
                        .insert_hash_set(&mut self.typed_records, id, "step_stage_claims")
                })?;
            }
        }
        self.ctx.reserve_vec(
            &mut self.body.losses,
            outcome.losses.len(),
            "step_stage_losses",
        )?;
        self.body.losses.extend(self.ctx.admit_iter(
            std::mem::take(&mut outcome.losses),
            "STEP stage loss transfer",
        )?);
        self.ctx.reserve_vec(
            &mut self.body.notes,
            outcome.notes.len(),
            "step_stage_notes",
        )?;
        self.body.notes.extend(self.ctx.admit_iter(
            std::mem::take(&mut outcome.notes),
            "STEP stage note transfer",
        )?);
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
}

/// Decode a complete clear-text exchange structure.
pub(crate) fn decode(
    input: &[u8],
    ctx: &DecodeContext<'_>,
    packaging: Packaging,
) -> Result<Decoded, CodecError> {
    let parsed = parse::parse_with_context(input, ctx, "STEP bare parsed graph storage")?;
    decode_exchange(input, parsed.exchange, &parsed.diagnostics, ctx, packaging)
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

    session.charge_pending_ir_entities("step_geometry_decode")?;
    let mut geometry = geometry::decode(exchange, &mut session.ir, session.ctx)?;
    session.charge_pending_ir_entities("step_dependency_decode")?;
    let mut dependencies = dependencies::decode(exchange, session.ctx)?;
    session.charge_pending_ir_entities("step_carrier_index")?;
    let (carrier_index_buffer, carrier_storage) = session
        .ctx
        .with_scoped_storage("STEP carrier index scratch", || {
            index::CarrierIndex::from_ir(&session.ir, session.ctx)
        })?;
    let carrier_index = carrier_index_buffer;
    session.charge_pending_ir_entities("step_topology_decode")?;
    let mut topology = topology::decode(exchange, &mut session.ir, &carrier_index, session.ctx)?;
    geometry::infer_edge_parameter_ranges(&mut session.ir, session.ctx)?;
    let (owned_carriers_buffer, owned_carrier_storage) = session
        .ctx
        .with_scoped_storage("STEP topology owned carrier scratch", || {
            geometry::topology_owned_carriers(&session.ir, &carrier_index, session.ctx)
        })?;
    let owned_carriers = owned_carriers_buffer;
    session.charge_pending_ir_entities("step_topology_association")?;
    geometry::associate_topology_carriers(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        session.ctx,
    )?;
    session.charge_pending_ir_entities("step_replica_association")?;
    geometry::associate_replica_bases(exchange, &mut session.ir, &carrier_index, session.ctx)?;
    session.charge_pending_ir_entities("step_pcurve_association")?;
    geometry::associate_pcurve_supports(exchange, &mut session.ir, &carrier_index, session.ctx)?;
    session.charge_pending_ir_entities("step_geometric_set_association")?;
    geometry::associate_free_geometric_set_members(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
        session.ctx,
    )?;
    session.charge_pending_ir_entities("step_representation_association")?;
    geometry::associate_free_representation_members(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
        session.ctx,
    )?;
    session.charge_pending_ir_entities("step_presentation_carrier_association")?;
    geometry::associate_free_presentation_carriers(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
        session.ctx,
    )?;
    session.charge_pending_ir_entities("step_surface_curve_association")?;
    geometry::associate_surface_curve_supports(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        session.ctx,
    )?;
    drop(owned_carriers);
    drop(owned_carrier_storage);
    drop(carrier_index);
    drop(carrier_storage);
    session.charge_pending_ir_entities("step_product_decode")?;
    let mut product = product::decode(
        exchange,
        &geometry.value,
        &topology.value,
        &mut session.ir,
        session.ctx,
        &mut session.admitted_ir_entities,
    )?;
    session.charge_pending_ir_entities("step_tessellation_decode")?;
    let mut tessellation = tessellation::decode(
        exchange,
        &geometry.value,
        &topology.value,
        &mut session.ir,
        session.ctx,
    )?;
    session.charge_pending_ir_entities("step_pmi_decode")?;
    let mut pmi = pmi::decode(
        exchange,
        &geometry.value,
        &topology.value,
        &mut session.ir,
        session.ctx,
    )?;
    session.charge_pending_ir_entities("step_presentation_decode")?;
    let mut presentation = presentation::decode(
        exchange,
        &topology.value,
        &mut session.ir,
        &product.value.0.product_definition_ids_by_source,
        session.ctx,
    )?;
    session.charge_pending_ir_entities("step_validation_decode")?;
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
    drop(dependencies);
    session.absorb(&mut presentation)?;
    drop(presentation);
    session.absorb(&mut product)?;
    let (product_data, product_claim_storage, product_report_storage) = product.value;
    drop(product_claim_storage);
    drop(product_report_storage);
    session.absorb(&mut tessellation)?;
    session.absorb(&mut topology)?;
    topology.value.release_claim_storage();
    session.absorb(&mut geometry)?;
    session.absorb(&mut pmi)?;
    drop(pmi);
    session.absorb(&mut validation)?;
    drop(validation);

    session.charge_pending_ir_entities("step_drawing_decode")?;
    let mut drawing = drawing::decode(
        exchange,
        &mut session.ir,
        &session.typed_records,
        &product_data.product_definition_ids_by_shape,
        session.ctx,
    )?;
    session.absorb(&mut drawing)?;
    drop(drawing);
    drop(product_data);
    let post_loss_storage = std::cell::RefCell::new(
        session
            .ctx
            .reserve_scoped(0, "STEP carrier report buffer")?,
    );
    let mut post_decode_losses = Vec::new();
    session.charge_pending_ir_entities("step_carrier_retention")?;
    retain_unowned_carriers(
        exchange,
        &mut session.ir,
        &mut session.typed_records,
        (&mut post_decode_losses, &post_loss_storage),
        session.ctx,
    )?;
    session.ctx.reserve_vec(
        &mut session.body.losses,
        post_decode_losses.len(),
        "step_carrier_retention_losses",
    )?;
    session.body.losses.extend(
        session
            .ctx
            .admit_iter(post_decode_losses, "STEP carrier retention loss transfer")?,
    );
    drop(post_loss_storage);

    session.charge_pending_ir_entities("step_opaque_record_retention")?;
    let opaque_offsets = match mode {
        DecodeMode::Decode(_) => BTreeSet::new(),
        DecodeMode::Inspect => {
            inspect_opaque_offsets(exchange, &session.typed_records, session.ctx)?
        }
    };
    let mut opaque_storage = ctx.reserve_scoped(0, "STEP opaque source indices")?;
    let mut counts = BTreeMap::<String, usize>::new();
    let mut opaque_ids = BTreeMap::new();
    let mut source_targets = BTreeMap::new();
    let mut opaque_sources = Vec::new();
    let mut source_fidelity = SourceFidelity::default();
    if matches!(mode, DecodeMode::Decode(_)) {
        {
            let mut opaque_id_records = (exchange.records()).iter();
            for _ in 0..opaque_id_records.len() {
                let (&id, record) = ctx
                    .next_charged(&mut opaque_id_records, "STEP decode exchange mode traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                if ctx.contains_hash_set(
                    &session.typed_records,
                    &id,
                    "STEP mod session.typed_records contains",
                )? {
                    continue;
                }
                opaque_storage.with_storage(|| {
                    let unknown_id = opaque_record_id(id, record, session.ctx)?;
                    session
                        .ctx
                        .insert_btree_map(&mut opaque_ids, id, unknown_id, "step_opaque_ids")
                })?;
            }
        }
        opaque_storage.with_storage(|| {
            session
                .ctx
                .reserve_vec(&mut opaque_sources, opaque_ids.len(), "step_opaque_sources")
        })?;
        {
            let mut opaque_source_records = (exchange.records()).iter();
            for _ in 0..opaque_source_records.len() {
                let (&id, record) = ctx
                    .next_charged(&mut opaque_source_records, "STEP decode exchange mode traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                if ctx.contains_hash_set(
                    &session.typed_records,
                    &id,
                    "STEP mod session.typed_records contains",
                )? {
                    continue;
                }
                opaque_storage.with_storage(|| count_unknown_kind(&mut counts, record, session.ctx))?;
                let mut links = BTreeSet::new();
                {
                    let mut source_partials = ((record.partials)[..]).iter();
                    for _ in 0..source_partials.len() {
                        let partial = ctx
                            .next_charged(&mut source_partials, "STEP decode exchange mode traversal")?
                            .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                        {
                            let mut partial_values = ((partial.parameters)[..]).iter();
                            for _ in 0..partial_values.len() {
                                let value = ctx
                                    .next_charged(&mut partial_values, "STEP decode exchange mode traversal")?
                                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                                opaque_storage
                                    .with_storage(|| collect_references(value, &mut links, session.ctx))?;
                            }
                        }
                    }
                }
                let unknown_id = ctx
                    .get_btree_map(&opaque_ids, &id, "STEP opaque source identity lookup")?
                    .ok_or_else(|| CodecError::malformed("STEP opaque source was not indexed"))?;

                opaque_sources.push(OpaqueSourceRecord {
                    unknown_id: unknown_id
                        .try_clone_for_decode(session.ctx, "step_opaque_source_identity")?,
                    span: record.span.clone(),
                    links,
                });
            }
        }
        let mut target_ids = BTreeSet::new();
        {
            let mut opaque_target_sources = (opaque_sources[..]).iter();
            for _ in 0..opaque_target_sources.len() {
                let source = ctx
                    .next_charged(&mut opaque_target_sources, "STEP decode exchange mode traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                {
                    let mut target_links = (source.links).iter();
                    for _ in 0..target_links.len() {
                        let &id = ctx
                            .next_charged(&mut target_links, "STEP opaque source target traversal")?
                            .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                        opaque_storage.with_storage(|| {
                            session.ctx.insert_btree_set(
                                &mut target_ids,
                                id,
                                "step_opaque_target_ids_index",
                            )
                        })?;
                    }
                }
            }
        }
        source_targets = opaque_storage.with_storage(|| {
            record_targets(
                &session.ir,
                |record_id| {
                    ctx.contains_btree_set(&target_ids, &record_id, "STEP mod target_ids contains")
                },
                session.ctx,
            )
        })?;
    } else {
        {
            let mut opaque_count_records = (exchange.records()).iter();
            for _ in 0..opaque_count_records.len() {
                let (&id, record) = ctx
                    .next_charged(&mut opaque_count_records, "STEP decode exchange mode traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                if ctx.contains_hash_set(
                    &session.typed_records,
                    &id,
                    "STEP mod session.typed_records contains",
                )? {
                    continue;
                }
                opaque_storage.with_storage(|| count_unknown_kind(&mut counts, record, session.ctx))?;
            }
        }
    }
    let accounting = { byte_accounting(input, exchange, &session.typed_records, session.ctx)? };
    if matches!(mode, DecodeMode::Decode(_)) {
        let signature_spans = exchange.release_source_graph();
        // Records and signatures occupy disjoint source spans.
        let opaque_count = opaque_sources.len() + signature_spans.len();
        let mut opaque = session
            .ctx
            .collection_vec(opaque_count, "step_opaque_records")?;
        {
            let mut opaque_source_values = (opaque_sources).into_iter();
            for _ in 0..opaque_source_values.len() {
                let source = ctx
                    .next_charged(&mut opaque_source_values, "STEP mod opaque_sources traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                let bytes = session
                    .ctx
                    .copy_retained(&input[source.span.clone()], "step_opaque_record")?;
                let mut links = Vec::new();
                {
                    let mut source_links = (source.links).into_iter();
                    for _ in 0..source_links.len() {
                        let id = ctx
                            .next_charged(&mut source_links, "STEP mod source.links traversal")?
                            .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                        if let Some(unknown_id) =
                            ctx.get_btree_map(&opaque_ids, &id, "STEP mod opaque_ids get")?
                        {
                            (session.ctx).push_formatted_retained(
                                &mut links,
                                format_args!("{}", unknown_id.as_str()),
                                "step_opaque_links",
                                "step_opaque_link_text",
                            )?;
                        }
                        if let Some(targets) =
                            ctx.get_btree_map(&source_targets, &id, "STEP mod source_targets get")?
                        {
                            {
                                let mut target_identities = (targets).iter();
                                for _ in 0..target_identities.len() {
                                    let target = ctx
                                        .next_charged(&mut target_identities, "STEP mod targets traversal")?
                                        .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                                    (session.ctx).push_formatted_retained(
                                        &mut links,
                                        format_args!("{target}"),
                                        "step_opaque_links",
                                        "step_opaque_link_text",
                                    )?;
                                }
                            }
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
        }
        {
            let mut signature_ranges = (signature_spans).into_iter().enumerate();
            for _ in 0..signature_ranges.len() {
                let (index, signature) = ctx
                    .next_charged(&mut signature_ranges, "STEP signature span traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                let bytes = session
                    .ctx
                    .copy_retained(&input[signature.clone()], "step_signature_record")?;
                let signature_kind = String::from("SIGNATURE");
                *opaque_storage
                    .with_storage(|| {
                        ctx.entry_btree_map(&mut counts, signature_kind, "step_opaque_kind_counts")
                    })?
                    .or_default() += 1;
                opaque.push(UnknownRecord::retained(
                    ids::signature(index),
                    u64_from_index(signature.start),
                    bytes,
                    Vec::new(),
                ));
            }
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
    {
        let mut opaque_kind_counts = (counts).into_iter();
        for _ in 0..opaque_kind_counts.len() {
            let (name, count) = ctx
                .next_charged(&mut opaque_kind_counts, "STEP mod counts traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
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
    }
    session.charge_pending_ir_entities("step_admit_ir_entities")?;
    session.into_result(source_fidelity, opaque_offsets)
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
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx.reserve_scoped(0, "STEP retain_unowned_carriers scratch")?;
    let mut owned = BTreeSet::new();
    {
        let mut coedges = ((ir.model.coedges)[..]).iter();
        for _ in 0..coedges.len() {
            let coedge = ctx
                .next_charged(&mut coedges, "STEP retain unowned carriers traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            {
                let mut curve_uses = ((coedge.pcurves)[..]).iter();
                for _ in 0..curve_uses.len() {
                    let use_ = ctx
                        .next_charged(&mut curve_uses, "STEP retain unowned carriers traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                    scratch_storage
                        .with_storage(|| insert_retained_identity(&mut owned, use_.pcurve.as_str(), ctx))?;
                }
            }
        }
    }
    {
        let mut loops = ((ir.model.loops)[..]).iter();
        for _ in 0..loops.len() {
            let loop_ = ctx
                .next_charged(&mut loops, "STEP retain unowned carriers traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            if let Some((_, pcurves)) = loop_.singular_vertex() {
                {
                    let mut singular_pcurves = (pcurves).iter();
                    for _ in 0..singular_pcurves.len() {
                        let pcurve = ctx
                            .next_charged(&mut singular_pcurves, "STEP singular vertex pcurve traversal")?
                            .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                        scratch_storage.with_storage(|| {
                            insert_retained_identity(&mut owned, pcurve.pcurve.as_str(), ctx)
                        })?;
                    }
                }
            }
            {
                let mut anchored_uses = (loop_.anchored_vertex_uses()).iter();
                for _ in 0..anchored_uses.len() {
                    let use_ = ctx
                        .next_charged(&mut anchored_uses, "STEP anchored vertex use traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                    {
                        let mut anchored_pcurves = (use_.pcurves.as_slice()).iter();
                        for _ in 0..anchored_pcurves.len() {
                            let pcurve = ctx
                                .next_charged(&mut anchored_pcurves, "STEP anchored vertex pcurve traversal")?
                                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                            scratch_storage.with_storage(|| {
                                insert_retained_identity(&mut owned, pcurve.pcurve.as_str(), ctx)
                            })?;
                        }
                    }
                }
            }
        }
    }
    {
        let mut procedural_surfaces = ((ir.model.procedural_surfaces)[..]).iter();
        for _ in 0..procedural_surfaces.len() {
            let surface = ctx
                .next_charged(&mut procedural_surfaces, "STEP retain unowned carriers traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
                boundary_pcurves,
                ..
            } = surface.definition()
            else {
                continue;
            };
            {
                let mut boundary_curves = (boundary_pcurves.as_slice()).iter();
                for _ in 0..boundary_curves.len() {
                    let pcurve = ctx
                        .next_charged(&mut boundary_curves, "STEP retain unowned carriers view traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                    scratch_storage
                        .with_storage(|| insert_retained_identity(&mut owned, pcurve.as_str(), ctx))?;
                }
            }
        }
    }
    let mut unowned_pcurves = BTreeSet::new();
    {
        let mut pcurve_records = (exchange.records()).iter();
        for _ in 0..pcurve_records.len() {
            let (&id, record) = ctx
                .next_charged(&mut pcurve_records, "STEP retain unowned carriers traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            if record.partial(ctx, "PCURVE")?.is_some()
                && !ctx.contains_btree_set(
                    &owned,
                    ids::data(kind!("pcurve"), id).as_str(),
                    "STEP owned pcurve identity lookup",
                )?
            {
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut unowned_pcurves, id, "step_unowned_pcurves")
                })?;
            }
        }
    }
    let (referenced_buffer, _reference_storage) = ctx
        .with_scoped_storage("STEP referenced record scratch", || {
            referenced_record_ids(exchange, ctx)
        })?;
    let referenced = referenced_buffer;
    let mut unowned_direct_carriers = BTreeSet::new();
    {
        let mut points = ir.model.points.iter();
        for _ in 0..points.len() {
            let point = ctx
                .next_charged(&mut points, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP direct carrier source ended early"))?;
            if point.source_object.is_none() {
                insert_unowned_direct_carrier(
                    point.id.as_str(),
                    exchange,
                    &referenced,
                    &mut unowned_direct_carriers,
                    &mut scratch_storage,
                    ctx,
                )?;
            }
        }
    }
    {
        let mut curves = ir.model.curves.iter();
        for _ in 0..curves.len() {
            let curve = ctx
                .next_charged(&mut curves, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP direct carrier source ended early"))?;
            if curve.source_object.is_none() {
                insert_unowned_direct_carrier(
                    curve.id.as_str(),
                    exchange,
                    &referenced,
                    &mut unowned_direct_carriers,
                    &mut scratch_storage,
                    ctx,
                )?;
            }
        }
    }
    {
        let mut surfaces = ir.model.surfaces.iter();
        for _ in 0..surfaces.len() {
            let surface = ctx
                .next_charged(&mut surfaces, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP direct carrier source ended early"))?;
            if surface.source_object.is_none() {
                insert_unowned_direct_carrier(
                    surface.id.as_str(),
                    exchange,
                    &referenced,
                    &mut unowned_direct_carriers,
                    &mut scratch_storage,
                    ctx,
                )?;
            }
        }
    }
    associate_unowned_direct_carriers(ir, &unowned_direct_carriers, ctx)?;
    if unowned_pcurves.is_empty() {
        return Ok(());
    }
    let mut roots = BTreeSet::new();
    {
        let mut vertices = ir.model.vertices.iter();
        for _ in 0..vertices.len() {
            let vertex = ctx
                .next_charged(&mut vertices, "STEP retain unowned carriers traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            insert_protected_carrier_root(
                vertex.point.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    {
        let mut edges = ir.model.edges.iter();
        for _ in 0..edges.len() {
            let edge = ctx
                .next_charged(&mut edges, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            let Some(curve) = edge.curve() else { continue; };
            insert_protected_carrier_root(
                curve.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    {
        let mut faces = ir.model.faces.iter();
        for _ in 0..faces.len() {
            let face = ctx
                .next_charged(&mut faces, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            insert_protected_carrier_root(
                face.surface.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    {
        let mut coedges = ir.model.coedges.iter();
        for _ in 0..coedges.len() {
            let coedge = ctx
                .next_charged(&mut coedges, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            let Some(use_curve) = coedge.use_curve.as_ref() else { continue; };
            insert_protected_carrier_root(
                use_curve.curve.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    {
        let mut pcurves = ir.model.pcurves.iter();
        for _ in 0..pcurves.len() {
            let pcurve = ctx
                .next_charged(&mut pcurves, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            if !ctx.contains_btree_set(&owned, pcurve.id.as_str(), "STEP protected pcurve root lookup")? { continue; }
            insert_protected_carrier_root(
                pcurve.id.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    {
        let mut points = ir.model.points.iter();
        for _ in 0..points.len() {
            let point = ctx
                .next_charged(&mut points, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            if point.source_object.is_none() { continue; }
            insert_protected_carrier_root(
                point.id.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    {
        let mut curves = ir.model.curves.iter();
        for _ in 0..curves.len() {
            let curve = ctx
                .next_charged(&mut curves, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            if curve.source_object.is_none() { continue; }
            insert_protected_carrier_root(
                curve.id.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    {
        let mut surfaces = ir.model.surfaces.iter();
        for _ in 0..surfaces.len() {
            let surface = ctx
                .next_charged(&mut surfaces, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            if surface.source_object.is_none() { continue; }
            insert_protected_carrier_root(
                surface.id.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    {
        let mut procedural_curves = ir.model.procedural_curves.iter();
        for _ in 0..procedural_curves.len() {
            let curve = ctx
                .next_charged(&mut procedural_curves, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            insert_protected_carrier_root(
                curve.id.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    {
        let mut procedural_surfaces = ir.model.procedural_surfaces.iter();
        for _ in 0..procedural_surfaces.len() {
            let surface = ctx
                .next_charged(&mut procedural_surfaces, "STEP retain unowned carriers chain traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected carrier source ended early"))?;
            insert_protected_carrier_root(
                surface.id.as_str(),
                &mut roots,
                &mut scratch_storage,
                ctx,
            )?;
        }
    }
    let mut protected_roots = BTreeSet::new();
    {
        let mut root_ids = (roots).into_iter();
        for _ in 0..root_ids.len() {
            let id = ctx
                .next_charged(&mut root_ids, "STEP protected root traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            if ctx.contains_btree_set(&unowned_pcurves, &id, "STEP unowned pcurve root lookup")? {
                continue;
            }
            scratch_storage.with_storage(|| {
                ctx.insert_btree_set(&mut protected_roots, id, "step_unowned_protected_root_copy")
            })?;
        }
    }
    let (protected_buffer, _protected_storage) = ctx
        .with_scoped_storage("STEP protected closure scratch", || {
            record_closure(&protected_roots, exchange, ctx)
        })?;
    let protected = protected_buffer;
    let (removed_closure_buffer, _removed_storage) = ctx
        .with_scoped_storage("STEP removed closure scratch", || {
            record_closure(&unowned_pcurves, exchange, ctx)
        })?;
    let removed_closure = removed_closure_buffer;
    let prior_pcurves = ir.model.pcurves.len();
    let prior_points = ir.model.points.len();
    let prior_curves = ir.model.curves.len();
    let prior_surfaces = ir.model.surfaces.len();
    let prior_procedural_curves = ir.model.procedural_curves.len();
    let prior_procedural_surfaces = ir.model.procedural_surfaces.len();
    ctx.retain_vec(
        &mut ir.model.pcurves,
        |pcurve| {
            ctx.contains_btree_set(
                &owned,
                pcurve.id.as_str(),
                "STEP owned pcurve identity lookup",
            )
        },
        "STEP unowned pcurves retention",
    )?;
    ctx.retain_vec(
        &mut ir.model.points,
        |point| retains_carrier(ctx, point.id.as_str(), &removed_closure, &protected),
        "STEP unowned points retention",
    )?;
    ctx.retain_vec(
        &mut ir.model.curves,
        |curve| retains_carrier(ctx, curve.id.as_str(), &removed_closure, &protected),
        "STEP unowned curves retention",
    )?;
    ctx.retain_vec(
        &mut ir.model.surfaces,
        |surface| retains_carrier(ctx, surface.id.as_str(), &removed_closure, &protected),
        "STEP unowned surfaces retention",
    )?;
    ctx.retain_vec(
        &mut ir.model.procedural_curves,
        |curve| retains_carrier(ctx, curve.id.as_str(), &removed_closure, &protected),
        "STEP unowned procedural_curves retention",
    )?;
    ctx.retain_vec(
        &mut ir.model.procedural_surfaces,
        |surface| retains_carrier(ctx, surface.id.as_str(), &removed_closure, &protected),
        "STEP unowned procedural_surfaces retention",
    )?;
    let deleted_pcurves = prior_pcurves - ir.model.pcurves.len();
    let deleted_points = prior_points - ir.model.points.len();
    let deleted_curves = prior_curves - ir.model.curves.len();
    let deleted_surfaces = prior_surfaces - ir.model.surfaces.len();
    let deleted_procedural_curves = prior_procedural_curves - ir.model.procedural_curves.len();
    let deleted_procedural_surfaces =
        prior_procedural_surfaces - ir.model.procedural_surfaces.len();
    {
        let mut unowned_ids = (unowned_pcurves).iter();
        for _ in 0..unowned_ids.len() {
            let id = ctx
                .next_charged(&mut unowned_ids, "STEP unowned pcurve claim release")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            ctx.remove_hash_set(typed_records, id, "STEP unowned pcurve claim release")?;
        }
    }
    {
        let mut removed_ids = (removed_closure).iter();
        for _ in 0..removed_ids.len() {
            let id = ctx
                .next_charged(&mut removed_ids, "STEP removed carrier claim release")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            if !ctx.contains_btree_set(&protected, id, "STEP removed carrier protection lookup")? {
                ctx.remove_hash_set(typed_records, id, "STEP removed carrier claim release")?;
            }
        }
    }
    let mut protected_pcurves = 0;
    {
        let mut unowned_ids = unowned_pcurves.iter();
        for _ in 0..unowned_ids.len() {
            let id = ctx
                .next_charged(&mut unowned_ids, "STEP protected pcurve traversal")?
                .ok_or_else(|| CodecError::malformed("STEP protected pcurve source ended early"))?;
            protected_pcurves += usize::from(ctx.contains_btree_set(
                &protected,
                id,
                "STEP mod protected contains",
            )?);
        }
    }
    let opaque_pcurves = unowned_pcurves.len() - protected_pcurves;
    ctx.push_scoped_vec(&mut slot_storage.borrow_mut(), losses, StepLossCode::DecodeWarning.note(format!(
        "unowned STEP carrier retention: opaque_pcurves={opaque_pcurves}, protected_pcurves={protected_pcurves}, deleted pcurves={deleted_pcurves}, points={deleted_points}, curves={deleted_curves}, surfaces={deleted_surfaces}, procedural_curves={deleted_procedural_curves}, procedural_surfaces={deleted_procedural_surfaces}"
    )), "step_decode_loss_notes")?;
    Ok(())
}

fn insert_unowned_direct_carrier(
    identity: &str,
    exchange: &Exchange,
    referenced: &BTreeSet<u64>,
    unowned: &mut BTreeSet<u64>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let Some(id) = step_instance_id(ctx, identity)? else {
        return Ok(());
    };
    if ctx.contains_key_btree_map(exchange.records(), &id, "STEP direct carrier record lookup")?
        && !ctx.contains_btree_set(referenced, &id, "STEP direct carrier reference lookup")?
    {
        storage.with_storage(|| {
            ctx.insert_btree_set(unowned, id, "step_unowned_direct_carriers")
        })?;
    }
    Ok(())
}

fn insert_protected_carrier_root(
    identity: &str,
    roots: &mut BTreeSet<u64>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if let Some(id) = step_instance_id(ctx, identity)? {
        storage.with_storage(|| {
            ctx.insert_btree_set(roots, id, "step_unowned_protected_roots")
        })?;
    }
    Ok(())
}

fn associate_unowned_direct_carriers(
    ir: &mut CadIr,
    ids: &BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    {
        let mut points = (ir.model.points).iter_mut();
        for _ in 0..points.len() {
            let point = ctx
                .next_charged(&mut points, "STEP direct carrier association traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            let Some(id) = step_instance_id(ctx, point.id.as_str())? else {
                continue;
            };
            if ctx.contains_btree_set(ids, &id, "STEP mod ids contains")?
                && point.source_object.is_none()
            {
                point.source_object = Some(step_source_association(ctx, id, None)?);
            }
        }
    }
    {
        let mut curves = (ir.model.curves).iter_mut();
        for _ in 0..curves.len() {
            let curve = ctx
                .next_charged(&mut curves, "STEP direct carrier association traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            let Some(id) = step_instance_id(ctx, curve.id.as_str())? else {
                continue;
            };
            if ctx.contains_btree_set(ids, &id, "STEP mod ids contains")?
                && curve.source_object.is_none()
            {
                curve.source_object = Some(step_source_association(ctx, id, None)?);
            }
        }
    }
    {
        let mut surfaces = (ir.model.surfaces).iter_mut();
        for _ in 0..surfaces.len() {
            let surface = ctx
                .next_charged(&mut surfaces, "STEP direct carrier association traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            let Some(id) = step_instance_id(ctx, surface.id.as_str())? else {
                continue;
            };
            if ctx.contains_btree_set(ids, &id, "STEP mod ids contains")?
                && surface.source_object.is_none()
            {
                surface.source_object = Some(step_source_association(ctx, id, None)?);
            }
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
    ctx: &DecodeContext<'_>,
    identity: &str,
    removed_closure: &BTreeSet<u64>,
    protected: &BTreeSet<u64>,
) -> Result<bool, CodecError> {
    let Some(id) = step_instance_id(ctx, identity)? else {
        return Ok(true);
    };
    Ok(
        !ctx.contains_btree_set(removed_closure, &id, "STEP removed carrier lookup")?
            || ctx.contains_btree_set(protected, &id, "STEP protected carrier lookup")?,
    )
}

/// Extract the numeric STEP instance id from a canonical IR identity.
fn step_instance_id(ctx: &DecodeContext<'_>, identity: &str) -> Result<Option<u64>, CodecError> {
    let Some((_, number)) =
        ctx.rsplit_once(identity, "#", "STEP instance identity reverse split")?
    else {
        return Ok(None);
    };
    Ok(ctx
        .parse_text::<u64>(number, "STEP instance identity number parse")?
        .ok())
}

fn record_closure(
    roots: &BTreeSet<u64>,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut closure = BTreeSet::new();
    let mut pending = ctx.collection_vec(roots.len(), "step_record_closure_pending")?;
    pending.extend(
        ctx.admit_iter(roots, "STEP closure root traversal")?
            .copied(),
    );
    while !pending.is_empty() {
        ctx.charge_work(1, "STEP mod worklist step")?;
        let id = pending.pop().ok_or_else(|| {
            CodecError::malformed("STEP closure pending source ended early")
        })?;
        if ctx.contains_btree_set(&closure, &id, "STEP mod closure contains")? {
            continue;
        }
        ctx.insert_btree_set(&mut closure, id, "step_record_closure_ids")?;
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP mod record get")?
        else {
            continue;
        };
        let mut reference_storage = ctx.reserve_scoped(0, "STEP closure edge scratch")?;
        let mut references = BTreeSet::new();
        {
            let mut partials = (record.partials[..]).iter();
            for _ in 0..partials.len() {
                let partial = ctx
                    .next_charged(&mut partials, "STEP record closure traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                {
                    let mut parameter_values = (partial.parameters.as_slice()).iter();
                    for _ in 0..parameter_values.len() {
                        let value = ctx
                            .next_charged(&mut parameter_values, "STEP record parameter traversal")?
                            .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                        reference_storage
                            .with_storage(|| collect_references(value, &mut references, ctx))?;
                    }
                }
            }
        }
        ctx.reserve_vec(
            &mut pending,
            references.len(),
            "step_record_closure_pending",
        )?;
        pending.extend(ctx.admit_iter(references, "STEP closure reference transfer")?);
    }
    Ok(closure)
}

fn referenced_record_ids(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut references = BTreeSet::new();
    {
        let mut referenced_records = (exchange.records()).iter();
        for _ in 0..referenced_records.len() {
            let (_, record) = ctx
                .next_charged(&mut referenced_records, "STEP referenced record ids map traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            {
                let mut reference_partials = ((record.partials)[..]).iter();
                for _ in 0..reference_partials.len() {
                    let partial = ctx
                        .next_charged(&mut reference_partials, "STEP referenced record ids traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                    {
                        let mut reference_parameters = (partial.parameters.as_slice()).iter();
                        for _ in 0..reference_parameters.len() {
                            let parameter = ctx
                                .next_charged(&mut reference_parameters, "STEP record parameter traversal")?
                                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                            collect_references(parameter, &mut references, ctx)?;
                        }
                    }
                }
            }
        }
    }
    Ok(references)
}

fn record_type_text(
    record: &parse::RawRecord,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    let (names_buffer, _storage) =
        ctx.temporary_vec(record.partials.len(), "STEP record type fragments")?;
    let mut names = names_buffer;
    names.extend(
        ctx.admit_iter(&record.partials[..], "STEP record type traversal")?
            .map(|partial| partial.name.as_str()),
    );
    ctx.join_retained(&names, "+", operation)
}

fn count_unknown_kind(
    counts: &mut BTreeMap<String, usize>,
    record: &parse::RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let kind = record_type_text(record, ctx, "step_opaque_kind_text")?;

    *ctx.entry_btree_map(counts, kind, "step_opaque_kind_counts")?
        .or_default() += 1;
    Ok(())
}

fn opaque_record_id(
    id: u64,
    record: &parse::RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<UnknownId, CodecError> {
    let operation = "step_opaque_kind_name";
    let len = ctx
        .admit_iter(&record.partials[..], "STEP opaque record id traversal")?
        .enumerate()
        .fold(0usize, |length, (index, partial)| {
            length + usize::from(index > 0) + partial.name.len()
        });
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut kind = storage.with_storage(|| ctx.retained_string(len, operation))?;

    {
        let mut partial_names = (record.partials[..]).iter().enumerate();
        for _ in 0..partial_names.len() {
            let (index, partial) = ctx
                .next_charged(&mut partial_names, "STEP opaque record id traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            if index > 0 {
                kind.push('_');
            }
            for byte in ctx.admit_iter(partial.name.as_bytes(), "STEP opaque record id traversal")? {
                // The admitted byte iteration writes one ASCII byte into reserved capacity.
                kind.push(char::from(byte.to_ascii_lowercase()));
            }
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
    include_record: impl Fn(u64) -> Result<bool, CodecError>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, BTreeSet<String>>, CodecError> {
    let mut targets = BTreeMap::<u64, BTreeSet<String>>::new();
    for identity in cadmpeg_ir::index::ModelIndex::build(ir, ctx)?.identities(ctx) {
        let identity = identity?;
        let Some(record_id) = source_record_id(ctx, identity)? else {
            continue;
        };
        if !include_record(record_id)? {
            continue;
        }
        let values = ctx
            .entry_btree_map(&mut targets, record_id, "step_opaque_target_records")?
            .or_default();
        if !ctx.contains_btree_set(values, identity, "STEP values membership")? {
            let copy =
                ctx.format_retained(format_args!("{identity}"), "step_opaque_target_identity")?;
            ctx.insert_btree_set(values, copy, "step_opaque_target_ids")?;
        }
    }
    Ok(targets)
}

fn source_record_id(ctx: &DecodeContext<'_>, identity: &str) -> Result<Option<u64>, CodecError> {
    let Some((_, suffix)) =
        ctx.rsplit_once(identity, "#", "STEP source record identity reverse split")?
    else {
        return Ok(None);
    };
    let number = ctx
        .split_once(suffix, "-", "STEP source identity suffix split")?
        .map_or(suffix, |(number, _)| number);
    Ok(ctx
        .parse_text::<u64>(number, "STEP source record identity number parse")?
        .ok())
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
    let (classes_buffer, _class_storage) = ctx.with_scoped_storage("step byte classes", || {
        ctx.alloc_filled(input.len(), ByteClass::Unclassified, "step byte classes")
    })?;
    let mut classes = classes_buffer;
    {
        let mut accounted_records = (exchange.records()).iter();
        for _ in 0..accounted_records.len() {
            let (&id, record) = ctx
                .next_charged(&mut accounted_records, "STEP byte accounting traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            let class =
                if ctx.contains_hash_set(typed_records, &id, "STEP mod typed_records contains")? {
                    ByteClass::Typed
                } else {
                    ByteClass::Opaque
                };
            claim_range(
                ctx,
                &mut classes,
                &record.span,
                class,
                format_args!("record #{id}"),
            )?;
        }
    }
    {
        let mut accounted_signatures = (exchange.signatures()).iter();
        for _ in 0..accounted_signatures.len() {
            let signature = ctx
                .next_charged(&mut accounted_signatures, "STEP byte accounting borrowed traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            claim_range(
                ctx,
                &mut classes,
                signature,
                ByteClass::Structural,
                format_args!("file signature at byte {}", signature.start),
            )?;
        }
    }
    let mut lexer = crate::lex::Lexer::new(input, ctx);
    lexer.set_transient_literals();
    let mut cursor = 0;
    loop {
        ctx.charge_work(1, "STEP byte accounting token step")?;
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
            ctx,
            &mut classes,
            &token.span,
            ByteClass::Structural,
            format_args!("token at byte {}", token.span.start),
        )?;
        claim_trivia(input, cursor..token.span.start, &mut classes, ctx)?;
        cursor = token.span.end;
    }
    claim_trivia(input, cursor..input.len(), &mut classes, ctx)?;

    Ok(ctx
        .admit_iter(classes, "STEP byte accounting count traversal")?
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
    ctx: &DecodeContext<'_>,
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
    for byte_class in ctx.admit_iter(claimed, "STEP byte range claim traversal")? {
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
    ctx: &DecodeContext<'_>,
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
        ctx.charge_work(1, "STEP byte trivia step")?;
        let Some(&byte) = input.get(at) else {
            return Ok(());
        };
        if classes.get(at) != Some(&ByteClass::Unclassified) {
            at += 1;
        } else if byte.is_ascii_control() || byte == b' ' {
            classes[at] = ByteClass::Structural;
            at += 1;
        } else if let Some(after_print_control) = crate::lex::print_control_end(ctx, input, at)? {
            if after_print_control > end {
                return Err(CodecError::malformed(format_args!(
                    "STEP print control directive at byte {at} ends at {after_print_control}, but its trivia run ends at {end}"
                )));
            }
            claim_range(
                ctx,
                classes,
                &(at..after_print_control),
                ByteClass::Structural,
                format_args!("print control directive at byte {at}"),
            )?;
            at = after_print_control;
        } else if input[at..end].starts_with(b"/*") {
            let Some(relative_end) = ctx.position_by(
                input[at + 2..end].windows(2),
                |window| Ok(window == b"*/"),
                "STEP byte trivia comment search",
            )?
            else {
                return Ok(());
            };
            claim_range(
                ctx,
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
    decoded_text_result(
        exchange.decode_string_with_context(bytes, ctx),
        (losses, None),
        record_id,
        (field, code),
        ctx,
    )
}

fn decode_output_text(
    exchange: &Exchange,
    value: &Value,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    record_id: u64,
    field: &str,
    code: StepLossCode,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let Value::String(bytes) = value else {
        return Ok(None);
    };
    decoded_text_result(
        exchange.decode_string_with_context(bytes, ctx),
        (losses, Some(slot_storage)),
        record_id,
        (field, code),
        ctx,
    )
}

fn decode_text_scoped(
    exchange: &Exchange,
    value: &Value,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    record_id: u64,
    (field, code): (&str, StepLossCode),
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Option<String>, CodecError> {
    let Value::String(bytes) = value else {
        return Ok(None);
    };
    decoded_text_result(
        storage.with_storage(|| exchange.decode_string_with_context(bytes, ctx)),
        (losses, Some(slot_storage)),
        record_id,
        (field, code),
        ctx,
    )
}

fn decoded_text_result(
    result: Result<String, crate::strings::StringDecodeFailure>,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        Option<&std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>>,
    ),
    record_id: u64,
    (field, code): (&str, StepLossCode),
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    match result {
        Ok(text) => Ok(Some(text)),
        Err(crate::strings::StringDecodeFailure::Invalid(error)) => {
            let message = ctx.format_retained(
                format_args!("STEP record #{record_id} has an invalid {field} string: {error}"),
                "step_invalid_string_loss_text",
            )?;
            if let Some(storage) = slot_storage {
                storage
                    .borrow_mut()
                    .with_storage(|| ctx.reserve_vec(losses, 1, "step_invalid_string_losses"))?;
            } else {
                ctx.reserve_vec(losses, 1, "step_invalid_string_losses")?;
            }
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
            {
                let mut list_values = (values.as_slice()).iter();
                for _ in 0..list_values.len() {
                    let value = ctx
                        .next_charged(&mut list_values, "STEP collect references value traversal")?
                        .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
                    collect_references(value, output, ctx)?;
                }
            }
        }
        Value::Typed(_, value) => {
            ctx.charge_work(1, "STEP typed reference descent")?;
            collect_references(value, output, ctx)?;
        }
        _ => {}
    }
    Ok(())
}

/// Record accessors shared by the reader submodules.
trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn parameters(&self) -> &[Value];
    fn parameter(&self, index: usize) -> Option<&Value>;
    /// Finds the partial record named `name`, charging one step per partial
    /// visited. `name` is a schema entity literal: comparing it reads at most
    /// its own length, so each comparison is fixed work.
    fn partial(
        &self,
        ctx: &DecodeContext<'_>,
        name: &'static str,
    ) -> Result<Option<&crate::parse::PartialRecord>, CodecError>;
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
    fn partial(
        &self,
        ctx: &DecodeContext<'_>,
        name: &'static str,
    ) -> Result<Option<&crate::parse::PartialRecord>, CodecError> {
        ctx.find_by(
            &self.partials[..],
            |partial| Ok(partial.name == name),
            "STEP partial record search",
        )
    }
}

/// Value accessors shared by the reader submodules.
trait ValueExt {
    fn number(&self) -> Option<f64>;
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn enumeration(&self) -> Option<&str>;
    fn integer(&self) -> Option<i64>;
    fn logical(&self) -> Option<bool>;
}

impl ValueExt for Value {
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

fn named_parameter<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    name: &str,
    index: usize,
) -> Result<Option<&'a Value>, CodecError> {
    Ok(ctx
        .find_map(
            &record.partials[..],
            |partial| -> Result<Option<_>, CodecError> {
                Ok(
                    (ctx.equal(partial.name.as_str(), name, "STEP named parameter equality")?)
                        .then_some(partial),
                )
            },
            "STEP named attribute partial traversal",
        )?
        .and_then(|partial| partial.parameters.get(index)))
}

fn find_record_value<T>(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
    mut predicate: impl FnMut(&Value) -> Result<Option<T>, CodecError>,
) -> Result<Option<T>, CodecError> {
    ctx.find_map(
        &record.partials[..],
        |partial| {
            ctx.find_map(
                partial.parameters.as_slice(),
                &mut predicate,
                "STEP record value parameter traversal",
            )
        },
        "STEP record value partial traversal",
    )
}

fn source_numeric_id(
    ctx: &DecodeContext<'_>,
    identity: &str,
    kind: &str,
) -> Result<Option<u64>, CodecError> {
    let Some(suffix) = identity
        .strip_prefix("step:data:")
        .map(|suffix| ctx.strip_prefix(suffix, kind, "STEP source numeric kind prefix"))
        .transpose()?
        .flatten()
        .and_then(|suffix| suffix.strip_prefix('#'))
    else {
        return Ok(None);
    };
    let suffix = suffix.strip_prefix("poly-point-").unwrap_or(suffix);
    let number = ctx
        .split_once(suffix, "-", "STEP source identity suffix split")?
        .map_or(suffix, |(number, _)| number);
    Ok(ctx
        .parse_text::<u64>(number, "STEP source numeric identity parse")?
        .ok())
}

fn inspect_opaque_offsets(
    exchange: &Exchange,
    typed_records: &HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<usize>, CodecError> {
    let mut offsets = BTreeSet::new();
    {
        let mut opaque_records = (exchange.records()).iter();
        for _ in 0..opaque_records.len() {
            let (id, record) = ctx
                .next_charged(&mut opaque_records, "STEP inspect opaque offsets traversal")?
                .ok_or_else(|| CodecError::malformed("STEP traversal source ended early"))?;
            if ctx.contains_hash_set(typed_records, id, "STEP mod typed_records contains")?
                || ctx.contains_btree_set(&offsets, &record.span.start, "STEP mod offsets contains")?
            {
                continue;
            }
            ctx.insert_btree_set(
                &mut offsets,
                record.span.start,
                "step_inspect_opaque_offsets",
            )?;
        }
    }
    Ok(offsets)
}

#[cfg(test)]
pub(crate) mod tests;
