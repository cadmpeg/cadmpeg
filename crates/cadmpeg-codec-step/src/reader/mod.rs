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

use self::claims::{ClaimSets, SourceClaims};
use self::closure_ids::ClosureIds;

mod claims;
mod closure_ids;
pub(crate) mod dependencies;
mod drawing;
pub(crate) mod geometry;
mod groups;
mod index;
mod ownership;
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

/// Merge the smaller claim set into the larger one.
fn merge_claims(target: &mut HashSet<u64>, mut source: HashSet<u64>) {
    if source.len() > target.len() {
        std::mem::swap(target, &mut source);
    }
    for claim in source {
        target.insert(claim);
    }
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
    typed_records: ClaimSets,
    admitted_ir_entities: u64,
    ctx: &'ctx DecodeContext<'arena>,
}

impl<'ctx, 'arena> StepDecodeSession<'ctx, 'arena> {
    fn new(
        exchange: &Exchange,
        diagnostics: &[ParseDiagnostic],
        ctx: &'ctx DecodeContext<'arena>,
        mode: DecodeMode,
    ) -> StepDecodeSession<'ctx, 'arena> {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            cadmpeg_core::nonblank_literal!("schema"),
            exchange.joined_schema_identifiers(),
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
        let primary = StepDialect::classify(exchange);
        let dialect_loss = crate::dialect::dialect_loss(&primary);
        let ir = CadIr::empty();

        let mut body = DecodeBody::new(if ctx.container_only() {
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {}
        } else {
            cadmpeg_ir::report::decode::DecodeTransfer::full(false)
        });
        for entry in exchange.references() {
            body.notes.push(format!(
                "external reference {} -> {}",
                entry.name, entry.uri
            ));
        }
        if let Some(loss) = dialect_loss {
            body.losses.push(loss);
        }
        for diagnostic in diagnostics {
            let (code, tag) = match diagnostic.kind {
                crate::parse::ParseDiagnosticKind::RecordOmitted => {
                    (StepLossCode::ParseRecordOmitted, "data_record")
                }
                crate::parse::ParseDiagnosticKind::PreambleOmitted => {
                    (StepLossCode::ParsePreambleOmitted, "preamble")
                }
                crate::parse::ParseDiagnosticKind::DraftExchangeGrammar => {
                    (StepLossCode::ParseDraftGrammar, "draft_grammar")
                }
                crate::parse::ParseDiagnosticKind::EnvelopeIncomplete => {
                    (StepLossCode::ParseEnvelopeIncomplete, "envelope")
                }
                crate::parse::ParseDiagnosticKind::HeaderMetadataNoncanonical => {
                    (StepLossCode::HeaderMetadataNoncanonical, "header_metadata")
                }
                crate::parse::ParseDiagnosticKind::PresentationMetadataUnusable => (
                    StepLossCode::ParseNoncanonicalSyntax,
                    "presentation_metadata",
                ),
                crate::parse::ParseDiagnosticKind::ComplexPartialsNotAlphabetical => {
                    (StepLossCode::ParseNoncanonicalSyntax, "complex_entity")
                }
                crate::parse::ParseDiagnosticKind::OmittedEntityName => {
                    (StepLossCode::ParseNoncanonicalSyntax, "entity_name")
                }
                crate::parse::ParseDiagnosticKind::EntityNameUnreadable => {
                    (StepLossCode::MetadataStringInvalid, "entity_name")
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
            let message = diagnostic.message.clone();
            let loss = code.note(message).with_provenance(
                cadmpeg_ir::SourceProvenance::root(
                    crate::dialect::FORMAT,
                    u64_from_index(diagnostic.offset),
                )
                .with_tag(tag),
            );
            body.losses.push(loss);
        }

        Self {
            ir,
            matched: primary,
            source_attributes: attributes,
            body,
            typed_records: ClaimSets::default(),
            admitted_ir_entities: 0,
            ctx,
        }
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

    fn absorb<T>(&mut self, outcome: &mut StageOutcome<T>) {
        self.typed_records
            .admit(std::mem::take(&mut outcome.claims));

        self.body.losses.append(&mut outcome.losses);

        self.body.notes.append(&mut outcome.notes);
    }

    fn into_result(
        mut self,
        source_fidelity: SourceFidelity,
        opaque_offsets: BTreeSet<usize>,
    ) -> AnalyzedExchange {
        self.ir.source = Some(SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(self.matched.clone()),
            self.source_attributes,
        ));
        AnalyzedExchange {
            decoded: Decoded {
                ir: self.ir,
                body: self.body,
                source_fidelity,
            },
            matched: self.matched,
            opaque_offsets,
        }
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
    let mut session = StepDecodeSession::new(exchange, diagnostics, ctx, mode);
    if ctx.container_only() {
        return Ok(session.into_result(SourceFidelity::default(), BTreeSet::new()));
    }

    let mut header_spans = Vec::new();
    if diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.kind,
            crate::parse::ParseDiagnosticKind::DraftExchangeGrammar
                | crate::parse::ParseDiagnosticKind::HeaderMetadataNoncanonical
                | crate::parse::ParseDiagnosticKind::ImplementationLevelUnverified
                | crate::parse::ParseDiagnosticKind::SchemaObjectIdentifierOutOfRange
        )
    }) {
        for record in exchange.header() {
            header_spans.push(record.offset..record.end);
        }
    }
    session.charge_pending_ir_entities("step_geometry_decode")?;
    let (mut geometry, carrier_index) = geometry::decode(exchange, &mut session.ir, session.ctx)?;
    session.charge_pending_ir_entities("step_dependency_decode")?;
    let mut dependencies = dependencies::decode(exchange)?;
    session.charge_pending_ir_entities("step_carrier_index")?;
    session.charge_pending_ir_entities("step_topology_decode")?;

    let mut topology = topology::decode(exchange, &mut session.ir, &carrier_index, session.ctx)?;
    geometry::infer_edge_parameter_ranges(&mut session.ir, session.ctx)?;
    let owned_carriers = geometry::topology_owned_carriers(&session.ir, &carrier_index);
    session.charge_pending_ir_entities("step_topology_association")?;
    geometry::associate_topology_carriers(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
    );
    session.charge_pending_ir_entities("step_replica_association")?;
    geometry::associate_replica_bases(exchange, &mut session.ir, &carrier_index);
    session.charge_pending_ir_entities("step_pcurve_association")?;
    geometry::associate_pcurve_supports(exchange, &mut session.ir, &carrier_index);
    session.charge_pending_ir_entities("step_geometric_set_association")?;
    geometry::associate_free_geometric_set_members(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
    session.charge_pending_ir_entities("step_representation_association")?;
    geometry::associate_free_representation_members(
        exchange,
        &mut session.ir,
        &carrier_index,
        &owned_carriers,
        &mut geometry.losses,
    );
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
    session.charge_pending_ir_entities("step_source_ownership")?;
    ownership::mark_supports(exchange, &mut session.ir, session.ctx)?;
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
        &product.value.product_definition_ids_by_source,
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
    session.absorb(&mut dependencies);
    session.absorb(&mut presentation);
    session.absorb(&mut product);
    session.absorb(&mut tessellation);
    session.absorb(&mut topology);
    session.absorb(&mut geometry);
    session.absorb(&mut pmi);
    session.absorb(&mut validation);

    session.charge_pending_ir_entities("step_drawing_decode")?;
    let mut drawing = drawing::decode(
        exchange,
        &mut session.ir,
        &session.typed_records,
        &product.value.product_definition_ids_by_shape,
        session.ctx,
    )?;
    session.absorb(&mut drawing);
    let mut post_decode_losses = Vec::new();
    session.charge_pending_ir_entities("step_carrier_retention")?;
    retain_unowned_carriers(
        exchange,
        &mut session.ir,
        &carrier_index,
        &mut session.typed_records,
        &mut post_decode_losses,
        session.ctx,
    )?;

    session.body.losses.append(&mut post_decode_losses);

    session.charge_pending_ir_entities("step_opaque_record_retention")?;
    let recovered_boundary_roles = session
        .body
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::FaceMultipleOuterBounds.kind());
    if recovered_boundary_roles {
        topology::retain_noncanonical_face_sources(
            exchange,
            &topology.value,
            &mut session.typed_records,
            session.ctx,
        )?;
    }
    let opaque_offsets = match mode {
        DecodeMode::Decode(_) => BTreeSet::new(),
        DecodeMode::Inspect => inspect_opaque_offsets(exchange, &session.typed_records),
    };
    let mut counts = BTreeMap::<String, usize>::new();
    let mut opaque_ids = BTreeMap::new();
    let mut source_targets = BTreeMap::new();
    let mut opaque_sources = Vec::new();
    let mut source_fidelity = SourceFidelity::default();
    if matches!(mode, DecodeMode::Decode(_)) {
        let metadata_offsets = diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.kind == crate::parse::ParseDiagnosticKind::PresentationMetadataUnusable
            })
            .map(|diagnostic| diagnostic.offset)
            .collect::<std::collections::BTreeSet<_>>();
        for (&id, record) in exchange.records() {
            if metadata_offsets.contains(&record.span.start) {
                session.typed_records.remove(id);
            }
        }
        for (&id, record) in exchange.records() {
            if session.typed_records.contains(id) {
                continue;
            }
            let mut unknown_id = opaque_record_id(id, record)?;
            if let Some(faces) = topology.value.faces_by_source.get(&id) {
                if faces
                    .iter()
                    .any(|face| face.as_str() == unknown_id.as_str())
                {
                    // A source FACE can share its normal identity with the
                    // recovered neutral Face. Give its exact source image a
                    // separate identity; reference links still resolve both.
                    unknown_id = UnknownId::mint(format!("step:file:retained-data#{id}"))
                        .map_err(CodecError::malformed)?;
                }
            }
            opaque_ids.insert(id, unknown_id);
        }

        for (&id, record) in exchange.records() {
            if session.typed_records.contains(id) {
                continue;
            }
            count_unknown_kind(&mut counts, record);
            let mut links = BTreeSet::new();
            for partial in &record.partials {
                for value in &partial.parameters {
                    collect_references(value, &mut links, session.ctx)?;
                }
            }
            let unknown_id = &opaque_ids[&id];

            opaque_sources.push(OpaqueSourceRecord {
                unknown_id: unknown_id.clone(),
                span: record.span.clone(),
                links,
            });
        }
        let mut target_ids = BTreeSet::new();
        for id in opaque_sources
            .iter()
            .flat_map(|source| source.links.iter().copied())
        {
            target_ids.insert(id);
        }
        source_targets = record_targets(&session.ir, |record_id| target_ids.contains(&record_id));
    } else {
        for (&id, record) in exchange.records() {
            if session.typed_records.contains(id) {
                continue;
            }
            count_unknown_kind(&mut counts, record);
        }
    }
    let accounting = {
        byte_accounting(
            input,
            exchange,
            &session.typed_records,
            &header_spans,
            session.ctx,
        )?
    };
    if matches!(mode, DecodeMode::Decode(_)) {
        let omitted_spans = exchange.omitted_spans().to_vec();
        let signature_spans = exchange.release_source_graph();
        let mut opaque = Vec::new();
        for (span, omitted) in header_spans
            .iter()
            .map(|span| (span, false))
            .chain(omitted_spans.iter().map(|span| (span, true)))
        {
            let bytes = retained_source_copy(
                session.ctx,
                &input[span.clone()],
                if omitted {
                    "step_omitted_record"
                } else {
                    "step_header_record"
                },
            )?;
            let id = if omitted {
                ids::omitted_record(span.start)
            } else {
                ids::header(span.start)
            };

            opaque.push(UnknownRecord::retained(
                id,
                u64_from_index(span.start),
                bytes,
                Vec::new(),
            ));
        }
        for source in opaque_sources {
            let bytes = retained_source_copy(
                session.ctx,
                &input[source.span.clone()],
                "step_opaque_record",
            )?;
            let mut links = Vec::new();
            for id in source.links {
                if let Some(unknown_id) = opaque_ids.get(&id) {
                    links.push(unknown_id.as_str().to_string());
                }
                if let Some(targets) = source_targets.get(&id) {
                    for target in targets {
                        links.push(target.clone());
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
            let bytes = retained_source_copy(
                session.ctx,
                &input[signature.clone()],
                "step_signature_record",
            )?;
            let signature_kind = String::from("SIGNATURE");

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
        session
            .body
            .losses
            .push(StepLossCode::ByteAccountingUnclassified.note(format!(
                "STEP byte accounting left {} byte(s) unclassified",
                accounting.unclassified
            )));
    }
    let accounting_note = format!(
        "byte accounting: {} structural, {} typed, {} named opaque, {} unclassified",
        accounting.structural, accounting.typed, accounting.opaque, accounting.unclassified
    );

    session.body.notes.push(accounting_note);
    for (name, count) in counts {
        let message = format!("preserved {count} {name} instance(s) as named opaque STEP records");
        session
            .body
            .losses
            .push(StepLossCode::OpaqueRecordPreserved.note(message));
    }
    session.charge_pending_ir_entities("step_admit_ir_entities")?;
    Ok(session.into_result(source_fidelity, opaque_offsets))
}

/// Copy source bytes that the decoded document keeps as an opaque record.
fn retained_source_copy(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<Vec<u8>, CodecError> {
    ctx.retain_bytes(u64_from_index(bytes.len()), operation)?;
    Ok(bytes.to_vec())
}

fn insert_retained_identity(identities: &mut BTreeSet<String>, identity: &str) {
    if !identities.contains(identity) {
        let copy = identity.to_string();
        identities.insert(copy);
    }
}

fn retain_unowned_carriers(
    exchange: &Exchange,
    ir: &mut CadIr,
    carrier_index: &index::CarrierIndex,
    typed_records: &mut dyn SourceClaims,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut owned = BTreeSet::new();
    for coedge in &ir.model.coedges {
        for use_ in &coedge.pcurves {
            insert_retained_identity(&mut owned, use_.pcurve.as_str());
        }
    }
    for loop_ in &ir.model.loops {
        for pcurve in loop_.vertex_pcurves() {
            insert_retained_identity(&mut owned, pcurve.pcurve.as_str());
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
            insert_retained_identity(&mut owned, pcurve.as_str());
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
            unowned_pcurves.insert(id);
        }
    }
    let referenced = referenced_record_ids(exchange, ir, typed_records, carrier_index, ctx)?;
    // An opaque reference does not transfer its coordinate source. A temporary
    // coordinate lookup does not transfer the record either.
    for (id, _) in exchange.entities("CARTESIAN_POINT") {
        let transferred = carrier_index
            .points
            .get(&id)
            .and_then(|carrier| ir.model.points.get(carrier.index.0))
            .is_some_and(|point| point.source_object.is_some());
        if !transferred && !referenced.get(&id).copied().unwrap_or_default() {
            typed_records.remove(id);
        }
    }
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
        .filter(|id| exchange.records().contains_key(id) && !referenced.contains_key(id));
    let mut unowned_direct_carriers = BTreeSet::new();
    for id in direct_carriers {
        unowned_direct_carriers.insert(id);
    }
    associate_unowned_direct_carriers(ir, &unowned_direct_carriers);
    if unowned_pcurves.is_empty() {
        return Ok(());
    }
    let protected_roots = ir
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
                .filter(|point| {
                    point.source_object.as_ref().is_some_and(|source| {
                        source.geometry_role != Some(cadmpeg_ir::SourceGeometryRole::Support)
                    })
                })
                .map(|point| point.id.as_str()),
        )
        .chain(
            ir.model
                .curves
                .iter()
                .filter(|curve| {
                    curve.source_object.as_ref().is_some_and(|source| {
                        source.geometry_role != Some(cadmpeg_ir::SourceGeometryRole::Support)
                    })
                })
                .map(|curve| curve.id.as_str()),
        )
        .chain(
            ir.model
                .surfaces
                .iter()
                .filter(|surface| {
                    surface.source_object.as_ref().is_some_and(|source| {
                        source.geometry_role != Some(cadmpeg_ir::SourceGeometryRole::Support)
                    })
                })
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
        .filter(|id| !unowned_pcurves.contains(id));
    let protected = record_closure(protected_roots, exchange, ctx)?;
    let removed_closure = record_closure(unowned_pcurves.iter().copied(), exchange, ctx)?;
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
    typed_records.retain(&mut |id| {
        !unowned_pcurves.contains(id) && (!removed_closure.contains(*id) || protected.contains(*id))
    });
    let protected_pcurves = unowned_pcurves
        .iter()
        .filter(|id| protected.contains(**id))
        .count();
    let opaque_pcurves = unowned_pcurves.len() - protected_pcurves;
    losses.push(StepLossCode::DecodeWarning.note(format!(
        "unowned STEP carrier retention: opaque_pcurves={opaque_pcurves}, protected_pcurves={protected_pcurves}, deleted pcurves={deleted_pcurves}, points={deleted_points}, curves={deleted_curves}, surfaces={deleted_surfaces}, procedural_curves={deleted_procedural_curves}, procedural_surfaces={deleted_procedural_surfaces}"
    )));
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
        geometry_role: None,
        format: cadmpeg_ir::codec_format!(crate::dialect::FORMAT),
        object_id: step_source_id(id),
        name,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    }
}

fn retains_carrier(identity: &str, removed_closure: &ClosureIds, protected: &ClosureIds) -> bool {
    step_instance_id(identity)
        .is_none_or(|id| !removed_closure.contains(id) || protected.contains(id))
}

/// Extract the numeric STEP instance id from a canonical IR identity.
fn step_instance_id(identity: &str) -> Option<u64> {
    identity.rsplit_once('#')?.1.parse().ok()
}

fn record_closure(
    roots: impl IntoIterator<Item = u64>,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<ClosureIds, CodecError> {
    let mut closure = ClosureIds::default();
    let mut pending = Vec::new();
    for root in roots {
        if closure.contains(root) {
            continue;
        }
        pending.push(root);
        closure.insert(root);
        while let Some(id) = pending.pop() {
            let Some(record) = exchange.records().get(&id) else {
                continue;
            };
            for value in record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
            {
                queue_closure_references(value, &mut closure, &mut pending, ctx)?;
            }
        }
    }
    Ok(closure)
}

fn queue_closure_references(
    value: &Value,
    closure: &mut ClosureIds,
    pending: &mut Vec<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_reference_walk")?;

    match value {
        Value::Reference(id) => {
            if closure.insert(*id) {
                pending.push(*id);
            }
        }
        Value::List(values) => {
            for value in values {
                queue_closure_references(value, closure, pending, ctx)?;
            }
        }
        Value::Typed(_, value) => {
            queue_closure_references(value, closure, pending, ctx)?;
        }
        _ => {}
    }
    Ok(())
}

fn referenced_record_ids(
    exchange: &Exchange,
    ir: &CadIr,
    typed_records: &dyn SourceClaims,
    carrier_index: &index::CarrierIndex,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, bool>, CodecError> {
    let mut references = BTreeMap::new();
    let include = |id: u64| {
        let point = carrier_index
            .points
            .get(&id)
            .and_then(|carrier| ir.model.points.get(carrier.index.0));
        point.is_some_and(|point| point.source_object.is_none())
            || carrier_index
                .curves
                .get(&id)
                .and_then(|index| ir.model.curves.get(index.0))
                .is_some_and(|curve| curve.source_object.is_none())
            || carrier_index
                .surfaces
                .get(&id)
                .and_then(|index| ir.model.surfaces.get(index.0))
                .is_some_and(|surface| surface.source_object.is_none())
            || (point.is_none_or(|point| point.source_object.is_none())
                && exchange
                    .records()
                    .get(&id)
                    .is_some_and(|record| record.partial("CARTESIAN_POINT").is_some()))
    };
    for (&id, record) in exchange.records() {
        let typed_source = typed_records.contains(id);
        for parameter in record
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            collect_source_references(
                parameter,
                exchange,
                &include,
                typed_source,
                &mut references,
                ctx,
            )?;
        }
    }
    Ok(references)
}

/// Only unowned carriers and untransferred coordinates need reference storage.
/// Reuse carrier positions to read existing model ownership without another index.
fn collect_source_references(
    value: &Value,
    exchange: &Exchange,
    include: &impl Fn(u64) -> bool,
    typed_source: bool,
    output: &mut BTreeMap<u64, bool>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_reference_walk")?;

    match value {
        Value::Reference(id) => {
            if !include(*id) {
                return Ok(());
            }
            let transferred = typed_source
                && exchange
                    .records()
                    .get(id)
                    .is_some_and(|record| record.partial("CARTESIAN_POINT").is_some());

            output
                .entry(*id)
                .and_modify(|previous| *previous |= transferred)
                .or_insert(transferred);
        }
        Value::List(values) => {
            for value in values {
                collect_source_references(value, exchange, include, typed_source, output, ctx)?;
            }
        }
        Value::Typed(_, value) => {
            collect_source_references(value, exchange, include, typed_source, output, ctx)?;
        }
        _ => {}
    }
    Ok(())
}

fn count_unknown_kind(counts: &mut BTreeMap<String, usize>, record: &parse::RawRecord) {
    let kind = record
        .partials
        .iter()
        .map(|partial| partial.name.as_str())
        .collect::<Vec<_>>()
        .join("+");
    *counts.entry(kind).or_default() += 1;
}

fn opaque_record_id(id: u64, record: &RawRecord) -> Result<UnknownId, CodecError> {
    let mut kind = String::new();

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
    let text = format!("step:data:{kind}#{id}");
    let identity = Identity::new(text)
        .map_err(|_| CodecError::WrongFormat("invalid STEP opaque identity".into()))?;
    Ok(UnknownId::from(identity))
}

fn record_targets(
    ir: &CadIr,
    include_record: impl Fn(u64) -> bool,
) -> BTreeMap<u64, BTreeSet<String>> {
    let mut targets = BTreeMap::<u64, BTreeSet<String>>::new();
    for identity in
        cadmpeg_ir::index::ModelIndex::build(ir).identities(cadmpeg_ir::index::StandardIndex)
    {
        let Some(record_id) = source_record_id(identity) else {
            continue;
        };
        if !include_record(record_id) {
            continue;
        }
        let values = targets.entry(record_id).or_default();
        if !values.contains(identity) {
            values.insert(identity.to_owned());
        }
    }
    targets
}

fn source_record_id(identity: &str) -> Option<u64> {
    identity.rsplit_once('#')?.1.split('-').next()?.parse().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
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
    typed_records: &dyn SourceClaims,
    header_spans: &[std::ops::Range<usize>],
    ctx: &DecodeContext<'_>,
) -> Result<ByteAccounting, CodecError> {
    let mut classes =
        ctx.alloc_filled(input.len(), ByteClass::Unclassified, "step byte classes")?;
    for span in header_spans.iter().chain(exchange.omitted_spans()) {
        claim_range(
            &mut classes,
            span,
            ByteClass::Opaque,
            format_args!("retained source record at byte {}", span.start),
        )?;
    }
    for (&id, record) in exchange.records() {
        let class = if typed_records.contains(id) {
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
    let mut lexer = crate::lex::Lexer::new(input, ctx);
    // The exchange already owns typed admission. This pass accounts the exact
    // spans of bounded source literals, including unreadable metadata.
    lexer.set_literal_admission(crate::lex::LiteralAdmission::Metadata);
    let draft = lexer.is_draft();
    let mut omitted = (0..exchange.omitted_spans().len())
        .map(|index| Ok(exchange.omitted_spans()[index].clone()))
        .collect::<Result<Vec<_>, CodecError>>()?;
    omitted.sort_unstable_by_key(|left| left.start);
    let mut cursor = 0;
    loop {
        let token = match lexer.next_token() {
            Ok(Some(token)) => token,
            Ok(None) => break,
            Err(error) => {
                let offset = error.offset();
                if let Some(resource) = error.into_resource_error() {
                    return Err(resource);
                }
                let end = omitted.partition_point(|span| span.start <= offset);
                if let Some(span) = end
                    .checked_sub(1)
                    .and_then(|index| omitted.get(index))
                    .filter(|span| offset < span.end)
                {
                    claim_trivia(input, cursor..span.end, &mut classes, draft)?;
                    lexer.seek(span.end);
                    cursor = span.end;
                    continue;
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
        claim_trivia(input, cursor..token.span.start, &mut classes, draft)?;
        cursor = token.span.end;
    }
    claim_trivia(input, cursor..input.len(), &mut classes, draft)?;

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
    draft: bool,
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
        } else if input[at..end].starts_with(b"/*") || (draft && input[at..end].starts_with(b"!*"))
        {
            let close = if input[at] == b'/' { b"*/" } else { b"*!" };
            let Some(relative_end) = input[at + 2..end]
                .windows(2)
                .position(|window| window == close)
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
) -> Option<String> {
    let Value::String(bytes) = value else {
        return None;
    };
    match exchange.decode_string(bytes) {
        Ok(text) => Some(text),
        Err(error) => {
            let message =
                format!("STEP record #{record_id} has an invalid {field} string: {error}");

            losses.push(code.note(message));
            None
        }
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
            output.insert(*id);
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
    fn parameters(&self) -> &[Value];
    fn parameter(&self, index: usize) -> Option<&Value>;
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
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
    typed_records: &dyn SourceClaims,
) -> std::collections::BTreeSet<usize> {
    let mut offsets = BTreeSet::new();
    for (id, record) in exchange.records() {
        if typed_records.contains(*id) || offsets.contains(&record.span.start) {
            continue;
        }
        offsets.insert(record.span.start);
    }
    offsets
}

#[cfg(test)]
pub(crate) mod tests;
