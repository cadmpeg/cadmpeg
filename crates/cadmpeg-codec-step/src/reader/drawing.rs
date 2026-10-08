// SPDX-License-Identifier: Apache-2.0
//! STEP drawing definitions, revisions, sheets, views, and their relations.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind};
use cadmpeg_ir::ids::{Identity, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{NativeField, NativeRecord};
use cadmpeg_ir::{ReferenceSelection, ReferenceTarget};

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, ReferenceName, Value};

use super::representation;
use super::{decode_output_text, opaque_record_id, record_targets, StageOutcome};
use super::{RecordExt, ValueExt};

const DRAWING_ASSOCIATION_TYPES: &[&str] = &[
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER",
];

struct TargetContext<'a> {
    target_identities: &'a BTreeMap<u64, BTreeSet<String>>,
    known_typed: &'a HashSet<u64>,
    exchange: &'a Exchange,
    external_documents: &'a BTreeMap<u64, &'a str>,
    ctx: &'a DecodeContext<'a>,
}

struct DrawingCandidate<'a> {
    id: u64,
    name: &'static str,
    identity: Identity,
    offset: usize,
    parameters: DrawingParameters<'a>,
}

#[derive(Clone, Copy)]
struct DrawingParameters<'a> {
    inherited_name: Option<&'a Value>,
    direct: &'a [Value],
}

impl<'a> DrawingParameters<'a> {
    fn from_slice(direct: &'a [Value]) -> Self {
        Self {
            inherited_name: None,
            direct,
        }
    }

    fn len(self) -> usize {
        self.direct.len() + usize::from(self.inherited_name.is_some())
    }

    fn get(self, index: usize) -> Option<&'a Value> {
        match self.inherited_name {
            Some(name) if index == 0 => Some(name),
            Some(_) => index
                .checked_sub(1)
                .and_then(|index| self.direct.get(index)),
            None => self.direct.get(index),
        }
    }

    fn first(self) -> Option<&'a Value> {
        self.get(0)
    }

    fn values(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<impl Iterator<Item = &'a Value>, CodecError> {
        Ok(self
            .inherited_name
            .into_iter()
            .chain(ctx.admit_iter(self.direct, "STEP drawing inherited parameter traversal")?))
    }
}

enum TargetResolution<'ctx> {
    Resolved(ReferenceSelection),
    Ambiguous((BTreeSet<String>, ScopedReservation<'ctx>)),
    Unresolved,
}

fn ensure_drawing_relationship_group(
    relationships: &mut BTreeMap<NonBlankString, Vec<ReferenceSelection>>,
    role: NonBlankString,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if !ctx.contains_key_btree_map(
        relationships,
        &role,
        "STEP drawing relationships contains_key",
    )? {
        ctx.insert_btree_map(
            relationships,
            role,
            Vec::new(),
            "step_drawing_relationship_groups",
        )?;
    }
    Ok(())
}

fn clone_drawing_identities(
    source: &BTreeSet<String>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<String>, CodecError> {
    let mut copy = BTreeSet::new();
    for identity in ctx.admit_iter(source, "STEP clone drawing identities traversal")? {
        let text = ctx.copy_retained_text(identity, "step_drawing_ambiguous_identity_text")?;
        ctx.insert_btree_set(&mut copy, text, "step_drawing_ambiguous_identity_copy")?;
    }
    Ok(copy)
}

fn visit_drawing_references(
    value: &Value,
    ctx: &DecodeContext<'_>,
    visitor: &mut impl FnMut(u64) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_drawing_reference_walk")?;
    match value {
        Value::Reference(id) => visitor(*id)?,
        Value::List(values) => {
            for value in ctx.admit_iter(
                values.as_slice(),
                "STEP visit drawing references value traversal",
            )? {
                visit_drawing_references(value, ctx, visitor)?;
            }
        }
        Value::Typed(_, value) => {
            ctx.charge_work(1, "STEP typed drawing reference descent")?;
            visit_drawing_references(value, ctx, visitor)?;
        }
        _ => {}
    }
    Ok(())
}

impl TargetContext<'_> {
    fn resolve(&self, id: u64) -> Result<TargetResolution<'_>, CodecError> {
        target_resolution(
            id,
            self.target_identities,
            self.known_typed,
            self.exchange,
            self.external_documents,
            self.ctx,
        )
    }
}

/// Decode the drawing object graph without claiming unsupported graphics.
pub(super) fn decode<'ctx>(
    exchange: &Exchange,
    ir: &mut CadIr,
    known_typed: &HashSet<u64>,
    product_definition_ids_by_shape: &BTreeMap<u64, ProductDefinitionId>,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<
    StageOutcome<(
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
    CodecError,
> {
    let slot_storage = std::cell::RefCell::new(ctx.reserve_scoped(0, "STEP stage report buffers")?);
    let mut claim_storage = ctx.reserve_scoped(0, "STEP stage claim storage")?;
    let mut scratch_storage = ctx.reserve_scoped(0, "STEP decode scratch")?;
    let mut losses = Vec::new();
    let mut candidates = Vec::new();
    for (&id, record) in ctx.admit_iter(exchange.records(), "STEP decode traversal")? {
        let Some((name, kind)) = drawing_type(ctx, record)? else {
            continue;
        };
        let parameters = source_parameters(ctx, record, name)?;
        if required_parameter_count(name).is_some_and(|count| parameters.len() < count) {
            slot_storage
                .borrow_mut()
                .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_drawing_losses"))?;
            losses.push(
                StepLossCode::DrawingRecordTooFewParameters.note(ctx.format_retained(
                    format_args!(
                "STEP drawing record #{id} has too few {name} parameters and was retained opaque"
            ),
                    "STEP decode text",
                )?),
            );
            continue;
        }
        scratch_storage
            .with_storage(|| ctx.reserve_vec(&mut candidates, 1, "step_drawing_candidates"))?;
        candidates.push(DrawingCandidate {
            id,
            name,
            identity: ids::drawing(kind, id),
            offset: record.span.start,
            parameters,
        });
    }
    ctx.stable_sort_by(
        &mut candidates,
        |value| &value.offset,
        Ord::cmp,
        "step_drawing_candidates_sort",
    )?;

    if candidates.is_empty() {
        return Ok(StageOutcome {
            value: (claim_storage, slot_storage.into_inner()),
            claims: BTreeSet::new(),
            losses,
            notes: Vec::new(),
        });
    }

    let mut drawing_ids = BTreeSet::new();
    for candidate in ctx.admit_iter(&candidates[..], "STEP decode traversal")? {
        scratch_storage.with_storage(|| {
            ctx.insert_btree_set(&mut drawing_ids, candidate.id, "step_drawing_ids")
        })?;
    }
    let mut hidden_drawing_ids = BTreeSet::new();
    for record in ctx
        .admit_iter(exchange.records(), "STEP decode map traversal")?
        .map(|(_, value)| value)
    {
        let Some(items) = record
            .partial(ctx, "INVISIBILITY")?
            .and_then(|partial| partial.parameters.first())
        else {
            continue;
        };
        visit_drawing_references(items, ctx, &mut |id| {
            if ctx.contains_btree_set(&drawing_ids, &id, "STEP drawing drawing_ids contains")? {
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut hidden_drawing_ids, id, "step_hidden_drawing_ids")
                })?;
            }
            Ok(())
        })?;
    }

    let (mut target_identities, mut target_storage) =
        ctx.with_scoped_storage("STEP drawing target index scratch", || {
            record_targets(
                ir,
                |record_id| {
                    ctx.contains_hash_set(
                        known_typed,
                        &record_id,
                        "STEP drawing known_typed contains",
                    )
                },
                ctx,
            )
        })?;
    for candidate in ctx.admit_iter(&candidates[..], "STEP decode traversal")? {
        let targets = target_storage
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut target_identities,
                    candidate.id,
                    "step_drawing_target_groups",
                )
            })?
            .or_default();
        target_storage.with_storage(|| {
            ctx.insert_btree_set(
                targets,
                ctx.copy_retained_text(
                    candidate.identity.as_str(),
                    "step_drawing_target_member_text",
                )?,
                "step_drawing_target_members",
            )
        })?;
    }
    // DR-01: a drawing association scoped by PRODUCT_DEFINITION_SHAPE targets
    // that shape's one owning product-definition view, not a product-wide
    // identity set.
    for (&shape_id, product_definition_id) in
        ctx.admit_iter(product_definition_ids_by_shape, "STEP decode traversal")?
    {
        let targets = target_storage
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut target_identities,
                    shape_id,
                    "step_drawing_target_groups",
                )
            })?
            .or_default();
        target_storage.with_storage(|| {
            ctx.insert_btree_set(
                targets,
                ctx.copy_retained_text(
                    product_definition_id.as_str(),
                    "step_drawing_target_member_text",
                )?,
                "step_drawing_target_members",
            )
        })?;
    }
    let (drawing_target_ids, _target_id_storage) = ctx
        .with_scoped_storage("STEP drawing referenced target scratch", || {
            referenced_target_ids(exchange, &candidates, ctx)
        })?;
    add_source_typed_targets(
        ir,
        exchange,
        known_typed,
        &drawing_target_ids,
        &mut target_identities,
        &mut target_storage,
        ctx,
    )?;
    let mut external_documents = BTreeMap::new();
    for entry in ctx.admit_iter(exchange.references(), "STEP decode borrowed traversal")? {
        if let ReferenceName::Entity(id) = entry.name {
            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut external_documents,
                    id,
                    entry.uri.as_str(),
                    "step_drawing_external_documents",
                )
            })?;
        }
    }
    let target_context = TargetContext {
        target_identities: &target_identities,
        known_typed,
        exchange,
        external_documents: &external_documents,
        ctx,
    };

    let mut drawings = BTreeMap::<u64, Drawing>::new();
    for (order, candidate) in ctx
        .admit_iter(candidates, "STEP drawing candidate traversal")?
        .enumerate()
    {
        let DrawingCandidate {
            id,
            name,
            identity,
            parameters,
            ..
        } = candidate;
        let mut stored_parameters = BTreeMap::new();
        ctx.insert_btree_map(
            &mut stored_parameters,
            cadmpeg_core::nonblank_literal!("source_id"),
            format!("#{id}"),
            "step_drawing_stored_parameters",
        )?;
        ctx.insert_btree_map(
            &mut stored_parameters,
            cadmpeg_core::nonblank_literal!("source_type"),
            ctx.copy_retained_text(name, "STEP drawing source type")?,
            "step_drawing_stored_parameters",
        )?;
        for (index, value) in parameters.values(ctx)?.enumerate() {
            if let Some(value) = value_text(
                exchange,
                value,
                (&mut losses, &slot_storage),
                id,
                &format!("drawing parameter {index}"),
                ctx,
            )? {
                let key = parameter_key(ctx, name, index)?;

                ctx.insert_btree_map(
                    &mut stored_parameters,
                    key,
                    value,
                    "step_drawing_stored_parameters",
                )?;
            }
        }

        let Some(order) = cadmpeg_core::decode::id_from_index(order) else {
            slot_storage
                .borrow_mut()
                .with_storage(|| ctx.reserve_vec(&mut losses, 1, "step_drawing_losses"))?;
            losses.push(StepLossCode::DrawingOrderUnstatable.note(format!(
                "drawing #{id} position in the stored order exceeds the stated order width"
            )));
            continue;
        };

        let mut relationships = BTreeMap::new();
        add_reference_fields(
            &mut relationships,
            name,
            parameters,
            id,
            &target_context,
            (&mut losses, &slot_storage),
        )?;

        let drawing = Drawing {
            id: DrawingId::from(identity.try_clone_for_decode(ctx, "step_drawing_identity_copy")?),
            object: ctx.copy_retained_text(identity.as_str(), "step_drawing_object_copy")?,
            kind: drawing_kind(name),
            runtime_type: ctx.copy_retained_text(name, "STEP drawing runtime type")?,
            order,
            visible: ctx
                .contains_btree_set(
                    &hidden_drawing_ids,
                    &id,
                    "STEP drawing hidden_drawing_ids contains",
                )?
                .then_some(false),
            relationships,
            template: None,
            position: None,
            scale: None,
            direction: None,
            rotation_degrees: None,
            parameters: stored_parameters,
            assets: Vec::new(),
            native_ref: identity.into_string(),
        };
        scratch_storage.with_storage(|| {
            ctx.insert_btree_map(&mut drawings, id, drawing, "step_drawing_entries")
        })?;
    }

    add_sheet_revision_usages(
        exchange,
        &mut drawings,
        &target_context,
        (&mut losses, &slot_storage),
        ctx,
    )?;
    let mut association_ids = BTreeSet::new();
    let mut association_storage =
        ctx.reserve_scoped(0, "STEP drawing association claim candidates")?;
    add_draughting_model_associations(
        exchange,
        &mut drawings,
        &target_context,
        (&mut losses, &slot_storage),
        (&mut association_ids, &mut association_storage),
    )?;

    let mut typed_records = BTreeSet::new();
    for &id in ctx
        .admit_iter(&(drawings), "STEP decode map traversal")?
        .map(|(key, _)| key)
    {
        claim_storage.with_storage(|| {
            ctx.insert_btree_set(&mut typed_records, id, "step_drawing_typed_claims")
        })?;
    }
    for id in ctx.admit_iter(association_ids, "step_drawing_typed_claims")? {
        claim_storage.with_storage(|| {
            ctx.insert_btree_set(&mut typed_records, id, "step_drawing_typed_claims")
        })?;
    }
    drop(association_storage);
    ctx.reserve_vec(
        &mut ir.model.drawings,
        drawings.len(),
        "step_drawing_ir_items",
    )?;
    ir.model.drawings.extend(
        ctx.admit_iter(drawings, "STEP drawing arena transfer")?
            .map(|(_, drawing)| drawing),
    );
    Ok(StageOutcome {
        value: (claim_storage, slot_storage.into_inner()),
        claims: typed_records,
        losses,
        notes: Vec::new(),
    })
}

pub(super) fn is_supported_invisibility_target(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<bool, CodecError> {
    let Some((name, _)) = drawing_type(ctx, record)? else {
        return Ok(false);
    };
    let Some(count) = required_parameter_count(name) else {
        return Ok(true);
    };
    Ok(source_parameters(ctx, record, name)?.len() >= count)
}

fn referenced_target_ids(
    exchange: &Exchange,
    candidates: &[DrawingCandidate<'_>],
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut ids = BTreeSet::new();
    for candidate in ctx.admit_iter(candidates, "STEP referenced target ids traversal")? {
        for &index in relationship_indices(candidate.name) {
            if let Some(value) = candidate.parameters.get(index) {
                collect_reference_ids(value, &mut ids, ctx)?;
            }
        }
    }
    for (_, record) in exchange.entities(ctx, "DRAWING_SHEET_REVISION_USAGE")? {
        let parameters = source_parameters(ctx, record, "DRAWING_SHEET_REVISION_USAGE")?;
        for value in [parameters.get(0), parameters.get(1)].into_iter().flatten() {
            collect_reference_ids(value, &mut ids, ctx)?;
        }
    }
    for entity in
        exchange.matching_entity_ids(ctx, |name| DRAWING_ASSOCIATION_TYPES.contains(&name))?
    {
        let association_id = entity?;
        let Some(record) = ctx.get_btree_map(
            exchange.records(),
            &association_id,
            "STEP drawing record get",
        )?
        else {
            continue;
        };
        let Some(parameters) = association_parameters(ctx, record)? else {
            continue;
        };
        for index in [2, 4] {
            if let Some(value) = parameters.get(index) {
                collect_reference_ids(value, &mut ids, ctx)?;
            }
        }
        if record
            .partial(ctx, "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER")?
            .is_some()
        {
            if let Some(placeholder_id) =
                association_placeholder_reference(ctx, record, parameters)?
            {
                ctx.insert_btree_set(&mut ids, placeholder_id, "step_drawing_referenced_targets")?;
            }
        }
    }
    Ok(ids)
}

fn collect_reference_ids(
    value: &Value,
    output: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    visit_drawing_references(value, ctx, &mut |id| {
        ctx.insert_btree_set(output, id, "step_drawing_referenced_targets")
            .map(|_| ())
    })
}

fn add_source_typed_targets(
    ir: &mut CadIr,
    exchange: &Exchange,
    known_typed: &HashSet<u64>,
    referenced_ids: &BTreeSet<u64>,
    target_identities: &mut BTreeMap<u64, BTreeSet<String>>,
    storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let (mut native_targets, mut native_storage) =
        ctx.temporary_vec(0, "step_drawing_native_target_items")?;
    for &id in ctx.admit_iter(referenced_ids, "STEP add source typed targets traversal")? {
        if !ctx.contains_hash_set(known_typed, &id, "STEP drawing known_typed contains")?
            || ctx.contains_key_btree_map(
                target_identities,
                &id,
                "STEP drawing target_identities contains_key",
            )?
        {
            continue;
        }
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP drawing record get")?
        else {
            continue;
        };
        let (wrapper, _wrapper_storage) = ctx
            .with_scoped_storage("STEP drawing wrapper probe scratch", || {
                wrapper_target_resolution(id, target_identities, exchange, ctx)
            })?;
        if wrapper.is_some() {
            continue;
        }
        let identity = opaque_record_id(id, record, ctx)?;
        let copied_identity = storage.with_storage(|| {
            ctx.copy_retained_text(
                identity.as_str(),
                "step_drawing_native_target_identity_copy",
            )
        })?;
        let source_type = super::record_type_text(record, ctx, "step_drawing_source_type_text")?;
        ctx.reserve_scoped_vec(
            &mut native_storage,
            &mut native_targets,
            1,
            "step_drawing_native_target_items",
        )?;
        native_targets.push(NativeRecord::from_identity(
            identity,
            [
                ("source_id".to_owned(), NativeField::Text(format!("#{id}"))),
                ("source_type".to_owned(), NativeField::Text(source_type)),
            ],
        ));

        let mut members = BTreeSet::new();
        storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut members,
                copied_identity,
                "step_drawing_native_target_members",
            )
        })?;
        storage.with_storage(|| {
            ctx.insert_btree_map(
                target_identities,
                id,
                members,
                "step_drawing_native_target_groups",
            )
        })?;
    }
    if native_targets.is_empty() {
        return Ok(());
    }
    let namespace = ir.native.namespace_mut("step");
    let arenas = namespace.arenas_mut();
    let arena_key = String::from("drawing_targets");

    let target_arena = ctx
        .entry_btree_map(arenas, arena_key, "step_drawing_native_arena")?
        .or_default();
    ctx.reserve_vec(
        target_arena,
        native_targets.len(),
        "step_drawing_native_arena_items",
    )?;
    target_arena.extend(ctx.admit_iter(native_targets, "STEP drawing native arena transfer")?);
    Ok(())
}

/// Each drawing entity name and the identity kind that name spells.
///
/// The pairing is the type that makes the drawing mint path total: a drawing
/// record reaches [`ids::drawing`] with a kind that came from this table, so
/// no identity kind is ever derived from record text at run time.
fn drawing_entities() -> [(&'static str, &'static crate::ids::IdentityKind); 7] {
    [
        ("DRAWING_DEFINITION", kind!("drawing_definition")),
        ("DRAWING_REVISION", kind!("drawing_revision")),
        ("DRAWING_SHEET_REVISION", kind!("drawing_sheet_revision")),
        ("PRESENTATION_VIEW", kind!("presentation_view")),
        ("PRESENTATION_SIZE", kind!("presentation_size")),
        ("DRAUGHTING_MODEL", kind!("draughting_model")),
        ("DRAUGHTING_CALLOUT", kind!("draughting_callout")),
    ]
}

fn drawing_type(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<(&'static str, &'static crate::ids::IdentityKind)>, CodecError> {
    for (name, kind) in drawing_entities() {
        if record.partial(ctx, name)?.is_some() {
            return Ok(Some((name, kind)));
        }
    }
    Ok(None)
}

fn drawing_kind(name: &str) -> DrawingKind {
    match name {
        "DRAWING_SHEET_REVISION" => DrawingKind::Page,
        "PRESENTATION_VIEW" => DrawingKind::View,
        "DRAUGHTING_CALLOUT" => DrawingKind::Annotation,
        _ => DrawingKind::Other,
    }
}

fn required_parameter_count(name: &str) -> Option<usize> {
    match name {
        "DRAWING_DEFINITION" => Some(2),
        "DRAWING_REVISION" => Some(3),
        "DRAWING_SHEET_REVISION" => Some(4),
        "PRESENTATION_VIEW" => Some(3),
        "PRESENTATION_SIZE" => Some(2),
        "DRAUGHTING_MODEL" => Some(3),
        "DRAUGHTING_CALLOUT" => Some(2),
        _ => None,
    }
}

fn source_parameters<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
    name: &'static str,
) -> Result<DrawingParameters<'a>, CodecError> {
    let direct = record
        .partial(ctx, name)?
        .map(|partial| partial.parameters.as_slice());
    if name == "DRAUGHTING_CALLOUT" {
        if let Some(parameters) = direct.filter(|parameters| parameters.len() >= 2) {
            return Ok(DrawingParameters::from_slice(parameters));
        }
        let inherited_name = record
            .partial(ctx, "REPRESENTATION_ITEM")?
            .and_then(|partial| partial.parameters.first());
        return Ok(DrawingParameters {
            inherited_name,
            direct: direct.unwrap_or_default(),
        });
    }
    if let Some(parameters) = direct.filter(|parameters| !parameters.is_empty()) {
        return Ok(DrawingParameters::from_slice(parameters));
    }
    if matches!(
        name,
        "DRAUGHTING_MODEL" | "PRESENTATION_VIEW" | "DRAWING_SHEET_REVISION"
    ) {
        if let Some(parameters) = representation::parameters(ctx, record)? {
            return Ok(DrawingParameters::from_slice(parameters));
        }
    }
    Ok(DrawingParameters::from_slice(direct.unwrap_or_default()))
}

fn parameter_key(
    ctx: &DecodeContext<'_>,
    name: &str,
    index: usize,
) -> Result<NonBlankString, CodecError> {
    Ok(match (name, index) {
        ("DRAWING_DEFINITION", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAWING_DEFINITION", 1) => cadmpeg_core::nonblank_literal!("description"),
        ("DRAWING_REVISION", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAWING_REVISION", 1) => cadmpeg_core::nonblank_literal!("drawing"),
        ("DRAWING_REVISION", 2) => cadmpeg_core::nonblank_literal!("description"),
        ("DRAWING_SHEET_REVISION", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAWING_SHEET_REVISION", 1) => cadmpeg_core::nonblank_literal!("items"),
        ("DRAWING_SHEET_REVISION", 2) => cadmpeg_core::nonblank_literal!("presentation_context"),
        ("DRAWING_SHEET_REVISION", 3) => cadmpeg_core::nonblank_literal!("revision"),
        ("PRESENTATION_VIEW", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("PRESENTATION_VIEW", 1) => cadmpeg_core::nonblank_literal!("items"),
        ("PRESENTATION_VIEW", 2) => cadmpeg_core::nonblank_literal!("presentation_context"),
        ("PRESENTATION_SIZE", 0) => cadmpeg_core::nonblank_literal!("drawing_sheet_revision"),
        ("PRESENTATION_SIZE", 1) => cadmpeg_core::nonblank_literal!("size"),
        ("DRAUGHTING_MODEL", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAUGHTING_MODEL", 1) => cadmpeg_core::nonblank_literal!("items"),
        ("DRAUGHTING_MODEL", 2) => cadmpeg_core::nonblank_literal!("presentation_context"),
        ("DRAUGHTING_CALLOUT", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAUGHTING_CALLOUT", 1) => cadmpeg_core::nonblank_literal!("contents"),
        _ => cadmpeg_core::nonblank_literal!(ctx, "parameter_{index}")?,
    })
}

fn relationship_indices(name: &str) -> &'static [usize] {
    match name {
        "DRAWING_REVISION" | "DRAUGHTING_CALLOUT" => &[1],
        "DRAWING_SHEET_REVISION" => &[1, 2, 3],
        "PRESENTATION_VIEW" | "DRAUGHTING_MODEL" => &[1, 2],
        "PRESENTATION_SIZE" => &[0, 1],
        _ => &[],
    }
}

fn add_reference_fields(
    relationships: &mut BTreeMap<NonBlankString, Vec<ReferenceSelection>>,
    name: &str,
    parameters: DrawingParameters<'_>,
    source_id: u64,
    target_context: &TargetContext<'_>,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
) -> Result<(), CodecError> {
    for &index in relationship_indices(name) {
        let Some(value) = parameters.get(index) else {
            continue;
        };
        let role = parameter_key(target_context.ctx, name, index)?;
        visit_drawing_references(value, target_context.ctx, &mut |target_id| {
            match target_context.resolve(target_id)? {
                TargetResolution::Resolved(target) => {
                    (target_context.ctx).push_btree_group(
                        relationships,
                        role.clone(),
                        target,
                        "step_drawing_relationship_groups",
                        "step_drawing_relationship_members",
                    )?;
                }
                TargetResolution::Ambiguous((identities, _storage)) => {
                    let (source, _source_storage) = target_context.ctx.format_scoped(
                        format_args!("drawing #{source_id} {name}"),
                        "STEP drawing source label",
                    )?;
                    note_ambiguous_target(
                        (losses, slot_storage),
                        &source,
                        role.as_str(),
                        target_id,
                        &identities,
                        target_context.ctx,
                    )?;
                }
                TargetResolution::Unresolved => {
                    slot_storage.borrow_mut().with_storage(|| {
                        target_context
                            .ctx
                            .reserve_vec(losses, 1, "step_drawing_losses")
                    })?;
                    losses.push(StepLossCode::DrawingRelationshipUntypedTarget.note(target_context.ctx.format_retained(format_args!(
                        "STEP drawing #{source_id} {name} relationship {role} references source-typed record #{target_id} without a neutral identity; the raw source parameter is retained"
                    ), "STEP unresolved drawing relationship")?));
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn note_ambiguous_target(
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    source: &str,
    role: &str,
    target_id: u64,
    identities: &BTreeSet<String>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let (mut parts, _parts_storage) =
        ctx.temporary_vec(identities.len(), "STEP drawing ambiguity fragments")?;
    parts.extend(
        ctx.admit_iter(identities, "STEP drawing ambiguity fragment traversal")?
            .map(String::as_str),
    );
    let (identities, _identity_storage) = ctx
        .with_scoped_storage("STEP drawing ambiguity detail scratch", || {
            ctx.join_retained(&parts, ", ", "step_drawing_ambiguous_identities_text")
        })?;
    slot_storage
        .borrow_mut()
        .with_storage(|| ctx.reserve_vec(losses, 1, "step_drawing_losses"))?;
    let message = ctx.format_retained(format_args!("STEP {source} relationship {role} references source record #{target_id} with multiple neutral identities ({identities}); no target was selected and the raw source parameter is retained"), "step_drawing_ambiguous_loss_text")?;
    losses.push(StepLossCode::DrawingRelationshipTargetAmbiguous.note(message));
    Ok(())
}

fn add_sheet_revision_usages(
    exchange: &Exchange,
    drawings: &mut BTreeMap<u64, Drawing>,
    target_context: &TargetContext<'_>,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (usage_id, record) in exchange.entities(ctx, "DRAWING_SHEET_REVISION_USAGE")? {
        let parameters = source_parameters(ctx, record, "DRAWING_SHEET_REVISION_USAGE")?;
        let Some(sheet_id) = parameters.first().and_then(ValueExt::reference) else {
            continue;
        };
        let Some(revision_id) = parameters.get(1).and_then(ValueExt::reference) else {
            continue;
        };
        let sheet_target = target_context.resolve(revision_id)?;
        let revision_target = target_context.resolve(sheet_id)?;
        if let Some(sheet) =
            ctx.get_mut_btree_map(drawings, &sheet_id, "STEP drawing drawings get_mut")?
        {
            match sheet_target {
                TargetResolution::Resolved(target) => (target_context.ctx).push_btree_group(
                    &mut sheet.relationships,
                    cadmpeg_core::nonblank_literal!("drawing_revision"),
                    target,
                    "step_drawing_relationship_groups",
                    "step_drawing_relationship_members",
                )?,
                TargetResolution::Ambiguous((identities, _storage)) => note_ambiguous_target(
                    (losses, slot_storage),
                    &format!("drawing sheet #{sheet_id} usage #{usage_id}"),
                    "drawing_revision",
                    revision_id,
                    &identities,
                    target_context.ctx,
                )?,
                TargetResolution::Unresolved => {
                    slot_storage.borrow_mut().with_storage(|| {
                        target_context
                            .ctx
                            .reserve_vec(losses, 1, "step_drawing_losses")
                    })?;
                    losses.push(StepLossCode::DrawingSheetRevisionUnresolved.note(format!(
                        "STEP drawing sheet #{sheet_id} usage #{usage_id} has no resolvable drawing revision #{revision_id}"
                    )));
                }
            }
            if let Some(sequence) = parameters
                .get(2)
                .map(|value| {
                    value_text(
                        target_context.exchange,
                        value,
                        (losses, slot_storage),
                        usage_id,
                        "drawing sheet revision usage sequence",
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
            {
                let key = cadmpeg_core::nonblank_literal!(ctx, "usage_{usage_id}_sequence")?;
                target_context.ctx.insert_btree_map(
                    &mut sheet.parameters,
                    key,
                    sequence,
                    "step_drawing_usage_sequences",
                )?;
            }
        }
        if let Some(revision) =
            ctx.get_mut_btree_map(drawings, &revision_id, "STEP drawing drawings get_mut")?
        {
            match revision_target {
                TargetResolution::Resolved(target) => (target_context.ctx).push_btree_group(
                    &mut revision.relationships,
                    cadmpeg_core::nonblank_literal!("sheet_revision"),
                    target,
                    "step_drawing_relationship_groups",
                    "step_drawing_relationship_members",
                )?,
                TargetResolution::Ambiguous((identities, _storage)) => note_ambiguous_target(
                    (losses, slot_storage),
                    &format!("drawing revision #{revision_id} usage #{usage_id}"),
                    "sheet_revision",
                    sheet_id,
                    &identities,
                    target_context.ctx,
                )?,
                TargetResolution::Unresolved => {
                    slot_storage.borrow_mut().with_storage(|| {
                        target_context
                            .ctx
                            .reserve_vec(losses, 1, "step_drawing_losses")
                    })?;
                    losses.push(StepLossCode::DrawingRevisionSheetUnresolved.note(format!(
                        "STEP drawing revision #{revision_id} usage #{usage_id} has no resolvable sheet revision #{sheet_id}"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn add_draughting_model_associations(
    exchange: &Exchange,
    drawings: &mut BTreeMap<u64, Drawing>,
    target_context: &TargetContext<'_>,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    (typed, claim_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
) -> Result<(), CodecError> {
    let ctx = target_context.ctx;
    for entity in
        exchange.matching_entity_ids(ctx, |name| DRAWING_ASSOCIATION_TYPES.contains(&name))?
    {
        let association_id = entity?;
        let Some(record) = ctx.get_btree_map(
            exchange.records(),
            &association_id,
            "STEP drawing record get",
        )?
        else {
            continue;
        };
        let Some(parameters) = association_parameters(ctx, record)? else {
            continue;
        };
        let Some(model_id) = parameters.get(3).and_then(ValueExt::reference) else {
            continue;
        };
        let Some(model) =
            ctx.get_mut_btree_map(drawings, &model_id, "STEP drawing drawings get_mut")?
        else {
            continue;
        };

        let mut complete = true;
        let definition_id = parameters.get(2).and_then(ValueExt::reference);
        let definition_target = if let Some(definition_id) = definition_id {
            match target_context.resolve(definition_id)? {
                TargetResolution::Resolved(definition) => Some(definition),
                TargetResolution::Ambiguous((identities, _storage)) => {
                    note_ambiguous_target(
                        (losses, slot_storage),
                        &format!("draughting model #{model_id} association #{association_id}"),
                        "semantic_definition",
                        definition_id,
                        &identities,
                        target_context.ctx,
                    )?;
                    complete = false;
                    None
                }
                TargetResolution::Unresolved => {
                    slot_storage.borrow_mut().with_storage(|| {
                        target_context
                            .ctx
                            .reserve_vec(losses, 1, "step_drawing_losses")
                    })?;
                    losses.push(StepLossCode::DraughtingSemanticDefinitionUntyped.note(
                        format!(
                            "STEP draughting model #{model_id} association #{association_id} references a typed semantic definition without a neutral identity; the raw source parameter is retained"
                        ),
                    ));
                    complete = false;
                    None
                }
            }
        } else {
            None
        };
        if definition_id.is_none() {
            complete = false;
        }

        ensure_drawing_relationship_group(
            &mut model.relationships,
            cadmpeg_core::nonblank_literal!("associated_items"),
            target_context.ctx,
        )?;
        let mut has_items = false;
        if let Some(items) = parameters.get(4) {
            visit_drawing_references(items, target_context.ctx, &mut |item_id| {
                has_items = true;
                match target_context.resolve(item_id)? {
                    TargetResolution::Resolved(item) => (target_context.ctx).push_btree_group(
                        &mut model.relationships,
                        cadmpeg_core::nonblank_literal!("associated_items"),
                        item,
                        "step_drawing_relationship_groups",
                        "step_drawing_relationship_members",
                    )?,
                    TargetResolution::Ambiguous((identities, _storage)) => {
                        note_ambiguous_target(
                            (losses, slot_storage),
                            &format!("draughting model #{model_id} association #{association_id}"),
                            "associated_items",
                            item_id,
                            &identities,
                            target_context.ctx,
                        )?;
                        complete = false;
                    }
                    TargetResolution::Unresolved => {
                        slot_storage.borrow_mut().with_storage(|| {
                            target_context
                                .ctx
                                .reserve_vec(losses, 1, "step_drawing_losses")
                        })?;
                        losses.push(StepLossCode::DraughtingAssociatedItemUntyped.note(
                            format!(
                                "STEP draughting model #{model_id} association #{association_id} references source-typed item #{item_id} without a neutral identity; the raw source parameter is retained"
                            ),
                        ));
                        complete = false;
                    }
                }
                Ok(())
            })?;
        }
        if !has_items {
            complete = false;
        }

        let placeholder_target = if record
            .partial(ctx, "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER")?
            .is_some()
        {
            match association_placeholder_reference(ctx, record, parameters)? {
                Some(placeholder_id) => match target_context.resolve(placeholder_id)? {
                    TargetResolution::Resolved(placeholder) => Some(placeholder),
                    TargetResolution::Ambiguous((identities, _storage)) => {
                        note_ambiguous_target(
                            (losses, slot_storage),
                            &format!("draughting model #{model_id} association #{association_id}"),
                            "annotation_placeholder",
                            placeholder_id,
                            &identities,
                            target_context.ctx,
                        )?;
                        complete = false;
                        None
                    }
                    TargetResolution::Unresolved => {
                        slot_storage.borrow_mut().with_storage(|| {
                            target_context
                                .ctx
                                .reserve_vec(losses, 1, "step_drawing_losses")
                        })?;
                        losses.push(StepLossCode::DrawingRelationshipUntypedTarget.note(format!(
                            "STEP draughting model #{model_id} association #{association_id} relationship annotation_placeholder references source-typed record #{placeholder_id} without a neutral identity"
                        )));
                        complete = false;
                        None
                    }
                },
                None => {
                    complete = false;
                    None
                }
            }
        } else {
            None
        };

        if let Some(definition) = definition_target {
            (target_context.ctx).push_btree_group(
                &mut model.relationships,
                cadmpeg_core::nonblank_literal!("semantic_definition"),
                definition,
                "step_drawing_relationship_groups",
                "step_drawing_relationship_members",
            )?;
        }
        if let Some(placeholder) = placeholder_target {
            (target_context.ctx).push_btree_group(
                &mut model.relationships,
                cadmpeg_core::nonblank_literal!("annotation_placeholder"),
                placeholder,
                "step_drawing_relationship_groups",
                "step_drawing_relationship_members",
            )?;
        }
        if complete {
            claim_storage.with_storage(|| {
                target_context.ctx.insert_btree_set(
                    typed,
                    association_id,
                    "step_drawing_typed_claims",
                )
            })?;
        }
    }
    Ok(())
}

fn association_parameters<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<&'a [Value]>, CodecError> {
    for name in [
        "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER",
        "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
        "ITEM_IDENTIFIED_REPRESENTATION_USAGE",
    ] {
        if let Some(partial) = ctx.find_map(
            &record.partials[..],
            |partial| -> Result<Option<_>, CodecError> {
                Ok((partial.name == name && partial.parameters.len() >= 5).then_some(partial))
            },
            "STEP drawing association parameter traversal",
        )? {
            return Ok(Some(partial.parameters.as_slice()));
        }
    }
    Ok(None)
}

fn association_placeholder_reference(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    parameters: &[Value],
) -> Result<Option<u64>, CodecError> {
    if let Some(reference) = parameters.get(5).and_then(ValueExt::reference) {
        return Ok(Some(reference));
    }
    Ok(record
        .partial(ctx, "ANNOTATION_PLACEHOLDER_OCCURRENCE")?
        .and_then(|partial| partial.parameters.first())
        .and_then(ValueExt::reference))
}

fn target_resolution<'ctx>(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    known_typed: &HashSet<u64>,
    exchange: &Exchange,
    external_documents: &BTreeMap<u64, &str>,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<TargetResolution<'ctx>, CodecError> {
    if let Some(identity) = ctx
        .get_btree_map(target_identities, &id, "STEP drawing target_identities get")?
        .filter(|identities| identities.len() == 1)
        .and_then(|identities| identities.first())
    {
        return Ok(TargetResolution::Resolved(ReferenceSelection::new(
            ReferenceTarget::Local(
                ctx.copy_retained_text(identity, "step_drawing_local_target_text")?,
            ),
            Vec::new(),
        )));
    }
    if let Some(uri) = ctx.get_btree_map(
        external_documents,
        &id,
        "STEP drawing external_documents get",
    )? {
        return Ok(TargetResolution::Resolved(ReferenceSelection::new(
            ReferenceTarget::External {
                document: ctx.copy_retained_text(uri, "step_drawing_external_target_text")?,
                object: format!("#{id}"),
            },
            Vec::new(),
        )));
    }
    let wrapper_ambiguity = match wrapper_target_resolution(id, target_identities, exchange, ctx)? {
        Some(WrapperTargetResolution::Singleton(identity)) => {
            return Ok(TargetResolution::Resolved(ReferenceSelection::new(
                ReferenceTarget::Local(identity),
                Vec::new(),
            )));
        }
        Some(WrapperTargetResolution::Ambiguous(identities)) => Some(identities),
        None => None,
    };
    if !ctx.contains_hash_set(known_typed, &id, "STEP drawing known_typed contains")? {
        if let Some(record) =
            ctx.get_btree_map(exchange.records(), &id, "STEP drawing record get")?
        {
            return Ok(TargetResolution::Resolved(ReferenceSelection::new(
                ReferenceTarget::Local(opaque_record_id(id, record, ctx)?.into_string()),
                Vec::new(),
            )));
        }
    }
    let ambiguity = ctx
        .get_btree_map(target_identities, &id, "STEP drawing target_identities get")?
        .filter(|identities| identities.len() > 1)
        .map(|identities| {
            ctx.with_scoped_storage("STEP drawing ambiguity scratch", || {
                clone_drawing_identities(identities, ctx)
            })
        })
        .transpose()?
        .or(wrapper_ambiguity);
    Ok(ambiguity.map_or(TargetResolution::Unresolved, TargetResolution::Ambiguous))
}

enum WrapperTargetResolution<'ctx> {
    Singleton(String),
    Ambiguous((BTreeSet<String>, ScopedReservation<'ctx>)),
}

fn wrapper_target_resolution<'ctx>(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    exchange: &Exchange,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<Option<WrapperTargetResolution<'ctx>>, CodecError> {
    if ctx.contains_key_btree_map(
        target_identities,
        &id,
        "STEP drawing target_identities contains_key",
    )? {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "STEP drawing wrapper scratch")?;
    let mut identities = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    let mut pending = storage
        .with_storage(|| ctx.alloc_filled(1, (id, false), "step_drawing_wrapper_pending"))?;
    while let Some((id, leaving)) = pending.pop() {
        ctx.charge_work(1, "STEP drawing worklist step")?;
        if leaving {
            ctx.remove_btree_set(&mut active, &id, "STEP drawing active remove")?;
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut complete, id, "step_drawing_wrapper_complete")
            })?;
            continue;
        }
        if ctx.contains_btree_set(&complete, &id, "STEP drawing complete contains")? {
            continue;
        }
        if ctx.contains_btree_set(&active, &id, "STEP drawing active contains")? {
            return Ok(None);
        }
        storage.with_storage(|| {
            ctx.insert_btree_set(&mut active, id, "step_drawing_wrapper_active")
        })?;
        storage
            .with_storage(|| ctx.reserve_vec(&mut pending, 1, "step_drawing_wrapper_pending"))?;
        pending.push((id, true));
        if let Some(targets) =
            ctx.get_btree_map(target_identities, &id, "STEP drawing target_identities get")?
        {
            for target in ctx.admit_iter(targets, "STEP drawing targets traversal")? {
                if !ctx.contains_btree_set(&identities, target, "STEP identities membership")? {
                    let copy = storage.with_storage(|| {
                        ctx.copy_retained_text(target, "step_drawing_wrapper_identity_text")
                    })?;
                    storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut identities,
                            copy,
                            "step_drawing_wrapper_identities",
                        )
                    })?;
                }
            }
            continue;
        }
        let Some(record) = ctx.get_btree_map(exchange.records(), &id, "STEP drawing record get")?
        else {
            continue;
        };
        if let Some(plane) = record
            .partial(ctx, "ANNOTATION_PLANE")?
            .and_then(|partial| partial.parameters.get(2))
            .and_then(ValueExt::reference)
        {
            storage.with_storage(|| {
                ctx.reserve_vec(&mut pending, 1, "step_drawing_wrapper_pending")
            })?;
            pending.push((plane, false));
        } else if let Some(representation) = mapped_representation(ctx, record, exchange)?
            .map(|representation| {
                ctx.get_btree_map(
                    exchange.records(),
                    &representation,
                    "STEP drawing record get",
                )
            })
            .transpose()?
            .flatten()
        {
            if let Some(items) = representation::items(ctx, representation)? {
                for item in items.rev() {
                    storage.with_storage(|| {
                        ctx.reserve_vec(&mut pending, 1, "step_drawing_wrapper_pending")
                    })?;
                    pending.push((item, false));
                }
            }
        }
    }
    if identities.len() == 1 {
        let Some(identity) = ctx
            .admit_iter(identities, "STEP drawing singleton target traversal")?
            .next()
        else {
            return Ok(None);
        };
        return Ok(Some(WrapperTargetResolution::Singleton(
            ctx.copy_retained_text(&identity, "step_drawing_wrapper_identity_text")?,
        )));
    }
    if identities.is_empty() {
        Ok(None)
    } else {
        Ok(Some(WrapperTargetResolution::Ambiguous((
            identities, storage,
        ))))
    }
}

fn mapped_representation(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
    exchange: &Exchange,
) -> Result<Option<u64>, CodecError> {
    let Some(map_id) = record
        .partial(ctx, "MAPPED_ITEM")?
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference)
    else {
        return Ok(None);
    };
    let Some(map) = ctx.get_btree_map(exchange.records(), &map_id, "STEP drawing record get")?
    else {
        return Ok(None);
    };
    Ok(map
        .partial(ctx, "REPRESENTATION_MAP")?
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference))
}

fn value_text(
    exchange: &Exchange,
    value: &Value,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<ScopedReservation<'_>>,
    ),
    record_id: u64,
    field: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    if matches!(value, Value::String(_)) {
        let _depth_guard = ctx.enter_nested("step_drawing_value_text_depth")?;
        return decode_output_text(
            exchange,
            value,
            (losses, slot_storage),
            record_id,
            field,
            StepLossCode::MetadataStringInvalid,
            ctx,
        );
    }
    let (mut parts, mut storage) = ctx.temporary_vec(0, "STEP drawing text fragments")?;
    if !collect_value_text(
        value,
        exchange,
        (losses, slot_storage),
        (record_id, field),
        (&mut parts, &mut storage),
        ctx,
    )? {
        return Ok(None);
    }
    let (mut text_parts, _text_part_storage) =
        ctx.temporary_vec(parts.len(), "STEP drawing text join fragments")?;
    text_parts.extend(
        ctx.admit_iter(&parts, "STEP drawing text join fragment traversal")?
            .map(std::convert::AsRef::as_ref),
    );
    Ok(Some(ctx.join_retained(
        &text_parts,
        "",
        "step_drawing_value_text",
    )?))
}

fn collect_value_text<'a>(
    value: &'a Value,
    exchange: &Exchange,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<ScopedReservation<'_>>,
    ),
    (record_id, field): (u64, &str),
    (parts, storage): (
        &mut Vec<std::borrow::Cow<'a, str>>,
        &mut ScopedReservation<'_>,
    ),
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    use std::borrow::Cow;
    let _depth_guard = ctx.enter_nested("step_drawing_value_text_depth")?;
    let text: Cow<'a, str> = match value {
        Value::Reference(id) => Cow::Owned(storage.with_storage(|| {
            ctx.format_retained(format_args!("#{id}"), "step_drawing_value_text")
        })?),
        Value::ExternalReference(id) => Cow::Owned(storage.with_storage(|| {
            ctx.format_retained(format_args!("@{id}"), "step_drawing_value_text")
        })?),
        Value::ConstantEntity(name) | Value::ExpressValueConstant(name) => {
            let prefix = if matches!(value, Value::ConstantEntity(_)) {
                "#"
            } else {
                "@"
            };
            ctx.push_scoped_vec(
                storage,
                parts,
                Cow::Borrowed(prefix),
                "STEP drawing text fragments",
            )?;
            Cow::Borrowed(name)
        }
        Value::Integer(value) => Cow::Owned(storage.with_storage(|| {
            ctx.format_retained(format_args!("{value}"), "step_drawing_value_text")
        })?),
        Value::Real(value) => Cow::Owned(storage.with_storage(|| {
            ctx.format_retained(format_args!("{}", value.get()), "step_drawing_value_text")
        })?),
        Value::Enumeration(value) => {
            ctx.push_scoped_vec(
                storage,
                parts,
                Cow::Borrowed("."),
                "STEP drawing text fragments",
            )?;
            ctx.push_scoped_vec(
                storage,
                parts,
                Cow::Borrowed(value),
                "STEP drawing text fragments",
            )?;
            Cow::Borrowed(".")
        }
        Value::String(_) => {
            let Some(text) = super::decode_text_scoped(
                exchange,
                value,
                (losses, slot_storage),
                record_id,
                (field, StepLossCode::MetadataStringInvalid),
                ctx,
                storage,
            )?
            else {
                return Ok(false);
            };
            Cow::Owned(text)
        }
        Value::Binary(value) => {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            let mut text = storage.with_storage(|| {
                ctx.format_retained(
                    format_args!("binary:{}:", value.bit_len()),
                    "step_drawing_value_text",
                )
            })?;
            // A byte Vec has at most isize::MAX entries; twice its length fits usize.
            storage.with_storage(|| {
                ctx.try_reserve_retained_text(
                    &mut text,
                    value.data().len() * 2,
                    "step_drawing_value_text",
                )
            })?;
            for byte in ctx.admit_iter(value.data(), "STEP value text borrowed traversal")? {
                text.push(char::from(HEX[usize::from(byte >> 4)]));
                text.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
            Cow::Owned(text)
        }
        Value::Resource(value) => {
            ctx.push_scoped_vec(
                storage,
                parts,
                Cow::Borrowed("<"),
                "STEP drawing text fragments",
            )?;
            ctx.push_scoped_vec(
                storage,
                parts,
                Cow::Borrowed(value),
                "STEP drawing text fragments",
            )?;
            Cow::Borrowed(">")
        }
        Value::Omitted => Cow::Borrowed("$"),
        Value::Derived => Cow::Borrowed("*"),
        Value::List(values) => {
            ctx.push_scoped_vec(
                storage,
                parts,
                Cow::Borrowed("("),
                "STEP drawing text fragments",
            )?;
            for (index, value) in ctx
                .admit_iter(values, "STEP value text traversal")?
                .enumerate()
            {
                if index != 0 {
                    ctx.push_scoped_vec(
                        storage,
                        parts,
                        Cow::Borrowed(","),
                        "STEP drawing text fragments",
                    )?;
                }
                if !collect_value_text(
                    value,
                    exchange,
                    (losses, slot_storage),
                    (record_id, field),
                    (parts, storage),
                    ctx,
                )? {
                    return Ok(false);
                }
            }
            Cow::Borrowed(")")
        }
        Value::Typed(name, value) => {
            ctx.push_scoped_vec(
                storage,
                parts,
                Cow::Borrowed(name),
                "STEP drawing text fragments",
            )?;
            ctx.push_scoped_vec(
                storage,
                parts,
                Cow::Borrowed("("),
                "STEP drawing text fragments",
            )?;
            if !collect_value_text(
                value,
                exchange,
                (losses, slot_storage),
                (record_id, field),
                (parts, storage),
                ctx,
            )? {
                return Ok(false);
            }
            Cow::Borrowed(")")
        }
    };
    ctx.push_scoped_vec(storage, parts, text, "STEP drawing text fragments")?;
    Ok(true)
}

#[cfg(test)]
mod tests;
