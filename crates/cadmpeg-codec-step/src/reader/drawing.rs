// SPDX-License-Identifier: Apache-2.0
//! STEP drawing definitions, revisions, sheets, views, and their relations.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
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

use super::claims::SourceClaims;
use super::representation;
use super::ValueExt;
use super::{decode_text_charged, opaque_record_id, record_targets, StageOutcome};

const DRAWING_ASSOCIATION_TYPES: &[&str] = &[
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER",
];

struct TargetContext<'a> {
    target_identities: &'a BTreeMap<u64, BTreeSet<String>>,
    known_typed: &'a dyn SourceClaims,
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

    fn iter(self) -> impl Iterator<Item = &'a Value> {
        self.inherited_name.into_iter().chain(self.direct.iter())
    }
}

enum TargetResolution {
    Resolved(ReferenceSelection),
    Ambiguous(BTreeSet<String>),
    Unresolved,
}

fn clone_drawing_identities(
    source: &BTreeSet<String>,
) -> std::collections::BTreeSet<std::string::String> {
    let mut copy = BTreeSet::new();
    for identity in source {
        let text = identity.to_owned();
        copy.insert(text);
    }
    copy
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
            for value in values {
                visit_drawing_references(value, ctx, visitor)?;
            }
        }
        Value::Typed(_, value) => visit_drawing_references(value, ctx, visitor)?,
        _ => {}
    }
    Ok(())
}

impl TargetContext<'_> {
    fn resolve(&self, id: u64) -> Result<TargetResolution, CodecError> {
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
pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    known_typed: &dyn SourceClaims,
    product_definition_ids_by_shape: &BTreeMap<u64, ProductDefinitionId>,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut losses = Vec::new();
    let mut candidates = Vec::new();
    for (&id, record) in exchange.records() {
        let Some((name, kind)) = drawing_type(record) else {
            continue;
        };
        let parameters = source_parameters(record, name);
        if required_parameter_count(name).is_some_and(|count| parameters.len() < count) {
            losses.push(StepLossCode::DrawingRecordTooFewParameters.note(format!(
                "STEP drawing record #{id} has too few {name} parameters and was retained opaque"
            )));
            continue;
        }

        candidates.push(DrawingCandidate {
            id,
            name,
            identity: ids::drawing(kind, id),
            offset: record.span.start,
            parameters,
        });
    }
    candidates.sort_by_key(|left| left.offset);

    if candidates.is_empty() {
        return Ok(StageOutcome {
            value: (),
            claims: HashSet::new(),
            losses,
            notes: Vec::new(),
        });
    }

    let mut drawing_ids = BTreeSet::new();
    for candidate in &candidates {
        drawing_ids.insert(candidate.id);
    }
    let mut hidden_drawing_ids = BTreeSet::new();
    for record in exchange.records().values() {
        let Some(items) = record
            .partials
            .iter()
            .find(|partial| partial.name == "INVISIBILITY")
            .and_then(|partial| partial.parameters.first())
        else {
            continue;
        };
        visit_drawing_references(items, ctx, &mut |id| {
            if drawing_ids.contains(&id) {
                hidden_drawing_ids.insert(id);
            }
            Ok(())
        })?;
    }

    let mut target_identities =
        record_targets(ir, |record_id| known_typed.contains(record_id), ctx)?;
    for candidate in &candidates {
        let targets = target_identities.entry(candidate.id).or_default();
        targets.insert(candidate.identity.as_str().to_owned());
    }
    // DR-01: a drawing association scoped by PRODUCT_DEFINITION_SHAPE targets
    // that shape's one owning product-definition view, not a product-wide
    // identity set.
    for (&shape_id, product_definition_id) in product_definition_ids_by_shape {
        let targets = target_identities.entry(shape_id).or_default();
        targets.insert(product_definition_id.as_str().to_owned());
    }
    let drawing_target_ids = referenced_target_ids(exchange, &candidates, ctx)?;
    add_source_typed_targets(
        ir,
        exchange,
        known_typed,
        &drawing_target_ids,
        &mut target_identities,
        ctx,
    )?;
    let mut external_documents = BTreeMap::new();
    for entry in exchange.references() {
        if let ReferenceName::Entity(id) = entry.name {
            external_documents.insert(id, entry.uri.as_str());
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
    for (order, candidate) in candidates.into_iter().enumerate() {
        let DrawingCandidate {
            id,
            name,
            identity,
            parameters,
            ..
        } = candidate;
        let mut stored_parameters = BTreeMap::new();
        stored_parameters.insert(
            cadmpeg_core::nonblank_literal!("source_id"),
            format!("#{id}"),
        );
        stored_parameters.insert(cadmpeg_core::nonblank_literal!("source_type"), name.into());
        for (index, value) in parameters.iter().enumerate() {
            if let Some(value) = value_text(
                exchange,
                value,
                &mut losses,
                id,
                &format!("drawing parameter {index}"),
                ctx,
            )? {
                let key = parameter_key(name, index);

                stored_parameters.insert(key, value);
            }
        }

        let Some(order) = cadmpeg_core::decode::id_from_index(order) else {
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
            &mut losses,
        )?;

        drawings.insert(
            id,
            Drawing {
                id: DrawingId::from(identity.clone()),
                object: identity.as_str().to_owned(),
                kind: drawing_kind(name),
                runtime_type: name.into(),
                order,
                visible: hidden_drawing_ids.contains(&id).then_some(false),
                relationships,
                template: None,
                position: None,
                scale: None,
                direction: None,
                rotation_degrees: None,
                parameters: stored_parameters,
                assets: Vec::new(),
                native_ref: identity.into_string(),
            },
        );
    }

    add_sheet_revision_usages(exchange, &mut drawings, &target_context, &mut losses, ctx)?;
    let mut association_ids = HashSet::new();
    add_draughting_model_associations(
        exchange,
        &mut drawings,
        &target_context,
        &mut losses,
        &mut association_ids,
    )?;

    let mut typed_records = HashSet::new();
    for &id in drawings.keys() {
        typed_records.insert(id);
    }
    for id in association_ids {
        typed_records.insert(id);
    }

    ir.model.drawings.extend(drawings.into_values());
    Ok(StageOutcome {
        value: (),
        claims: typed_records,
        losses,
        notes: Vec::new(),
    })
}

pub(super) fn is_supported_invisibility_target(record: &RawRecord) -> bool {
    let Some((name, _)) = drawing_type(record) else {
        return false;
    };
    required_parameter_count(name)
        .is_none_or(|count| source_parameters(record, name).len() >= count)
}

fn referenced_target_ids(
    exchange: &Exchange,
    candidates: &[DrawingCandidate<'_>],
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut ids = BTreeSet::new();
    for candidate in candidates {
        for &index in relationship_indices(candidate.name) {
            if let Some(value) = candidate.parameters.get(index) {
                collect_reference_ids(value, &mut ids, ctx)?;
            }
        }
    }
    for (_, record) in exchange.entities("DRAWING_SHEET_REVISION_USAGE") {
        let parameters = source_parameters(record, "DRAWING_SHEET_REVISION_USAGE");
        for value in parameters.iter().take(2) {
            collect_reference_ids(value, &mut ids, ctx)?;
        }
    }
    for association_id in
        exchange.matching_entity_ids(|name| DRAWING_ASSOCIATION_TYPES.contains(&name))
    {
        let Some(record) = exchange.records().get(&association_id) else {
            continue;
        };
        let Some(parameters) = association_parameters(record) else {
            continue;
        };
        for index in [2, 4] {
            if let Some(value) = parameters.get(index) {
                collect_reference_ids(value, &mut ids, ctx)?;
            }
        }
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER")
        {
            if let Some(placeholder_id) = association_placeholder_reference(record, parameters) {
                ids.insert(placeholder_id);
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
        output.insert(id);
        Ok(())
    })
}

fn add_source_typed_targets(
    ir: &mut CadIr,
    exchange: &Exchange,
    known_typed: &dyn SourceClaims,
    referenced_ids: &BTreeSet<u64>,
    target_identities: &mut BTreeMap<u64, BTreeSet<String>>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut native_targets = Vec::new();
    for &id in referenced_ids {
        if !known_typed.contains(id) || target_identities.contains_key(&id) {
            continue;
        }
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        if wrapper_target_resolution(id, target_identities, exchange, ctx)?.is_some() {
            continue;
        }
        let identity = opaque_record_id(id, record)?;
        let copied_identity = identity.as_str().to_owned();
        let source_type = record
            .partials
            .iter()
            .map(|partial| partial.name.as_str())
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join("+");

        native_targets.push(NativeRecord::from_identity(
            identity,
            [
                ("source_id".to_owned(), NativeField::Text(format!("#{id}"))),
                ("source_type".to_owned(), NativeField::Text(source_type)),
            ],
        ));

        target_identities.insert(id, BTreeSet::from([copied_identity]));
    }
    if native_targets.is_empty() {
        return Ok(());
    }
    let namespace = ir.native.namespace_mut("step");
    let arenas = namespace.arenas_mut();
    let arena_key = String::from("drawing_targets");

    let target_arena = arenas.entry(arena_key).or_default();

    target_arena.extend(native_targets);
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

fn drawing_type(record: &RawRecord) -> Option<(&'static str, &'static crate::ids::IdentityKind)> {
    drawing_entities()
        .into_iter()
        .find(|(name, _)| record.partials.iter().any(|partial| partial.name == *name))
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

fn source_parameters<'a>(record: &'a RawRecord, name: &str) -> DrawingParameters<'a> {
    let direct = record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .map(|partial| partial.parameters.as_slice());
    if name == "DRAUGHTING_CALLOUT" {
        if let Some(parameters) = direct.filter(|parameters| parameters.len() >= 2) {
            return DrawingParameters::from_slice(parameters);
        }
        let inherited_name = record
            .partials
            .iter()
            .find(|partial| partial.name == "REPRESENTATION_ITEM")
            .and_then(|partial| partial.parameters.first());
        return DrawingParameters {
            inherited_name,
            direct: direct.unwrap_or_default(),
        };
    }
    if let Some(parameters) = direct.filter(|parameters| !parameters.is_empty()) {
        return DrawingParameters::from_slice(parameters);
    }
    if matches!(
        name,
        "DRAUGHTING_MODEL" | "PRESENTATION_VIEW" | "DRAWING_SHEET_REVISION"
    ) {
        if let Some(parameters) = representation::parameters(record) {
            return DrawingParameters::from_slice(parameters);
        }
    }
    DrawingParameters::from_slice(direct.unwrap_or_default())
}

fn parameter_key(name: &str, index: usize) -> NonBlankString {
    match (name, index) {
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
        _ => cadmpeg_core::nonblank_literal!("parameter_{index}"),
    }
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
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for &index in relationship_indices(name) {
        let Some(value) = parameters.get(index) else {
            continue;
        };
        let role = parameter_key(name, index);
        visit_drawing_references(value, target_context.ctx, &mut |target_id| {
            match target_context.resolve(target_id)? {
                TargetResolution::Resolved(target) => {
                    relationships.entry(role.clone()).or_default().push(target);
                }
                TargetResolution::Ambiguous(identities) => note_ambiguous_target(
                    losses,
                    &format!("drawing #{source_id} {name}"),
                    role.as_str(),
                    target_id,
                    &identities,
                ),
                TargetResolution::Unresolved => {
                    losses.push(StepLossCode::DrawingRelationshipUntypedTarget.note(format!(
                        "STEP drawing #{source_id} {name} relationship {role} references source-typed record #{target_id} without a neutral identity; the raw source parameter is retained"
                    )));
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn note_ambiguous_target(
    losses: &mut Vec<LossNote>,
    source: &str,
    role: &str,
    target_id: u64,
    identities: &BTreeSet<String>,
) {
    let identities = identities
        .iter()
        .map(String::as_str)
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    let message = format!("STEP {source} relationship {role} references source record #{target_id} with multiple neutral identities ({identities}); no target was selected and the raw source parameter is retained");
    losses.push(StepLossCode::DrawingRelationshipTargetAmbiguous.note(message));
}

fn add_sheet_revision_usages(
    exchange: &Exchange,
    drawings: &mut BTreeMap<u64, Drawing>,
    target_context: &TargetContext<'_>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (usage_id, record) in exchange.entities("DRAWING_SHEET_REVISION_USAGE") {
        let parameters = source_parameters(record, "DRAWING_SHEET_REVISION_USAGE");
        let Some(sheet_id) = parameters.first().and_then(ValueExt::reference) else {
            continue;
        };
        let Some(revision_id) = parameters.get(1).and_then(ValueExt::reference) else {
            continue;
        };
        let sheet_target = target_context.resolve(revision_id)?;
        let revision_target = target_context.resolve(sheet_id)?;
        if let Some(sheet) = drawings.get_mut(&sheet_id) {
            match sheet_target {
                TargetResolution::Resolved(target) => sheet
                    .relationships
                    .entry(cadmpeg_core::nonblank_literal!("drawing_revision"))
                    .or_default()
                    .push(target),
                TargetResolution::Ambiguous(identities) => note_ambiguous_target(
                    losses,
                    &format!("drawing sheet #{sheet_id} usage #{usage_id}"),
                    "drawing_revision",
                    revision_id,
                    &identities,
                ),
                TargetResolution::Unresolved => {
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
                        losses,
                        usage_id,
                        "drawing sheet revision usage sequence",
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
            {
                let key = cadmpeg_core::nonblank_literal!("usage_{usage_id}_sequence");

                sheet.parameters.insert(key, sequence);
            }
        }
        if let Some(revision) = drawings.get_mut(&revision_id) {
            match revision_target {
                TargetResolution::Resolved(target) => revision
                    .relationships
                    .entry(cadmpeg_core::nonblank_literal!("sheet_revision"))
                    .or_default()
                    .push(target),
                TargetResolution::Ambiguous(identities) => note_ambiguous_target(
                    losses,
                    &format!("drawing revision #{revision_id} usage #{usage_id}"),
                    "sheet_revision",
                    sheet_id,
                    &identities,
                ),
                TargetResolution::Unresolved => {
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
    losses: &mut Vec<LossNote>,
    typed: &mut HashSet<u64>,
) -> Result<(), CodecError> {
    for association_id in
        exchange.matching_entity_ids(|name| DRAWING_ASSOCIATION_TYPES.contains(&name))
    {
        let Some(record) = exchange.records().get(&association_id) else {
            continue;
        };
        let Some(parameters) = association_parameters(record) else {
            continue;
        };
        let Some(model_id) = parameters.get(3).and_then(ValueExt::reference) else {
            continue;
        };
        let Some(model) = drawings.get_mut(&model_id) else {
            continue;
        };

        let mut complete = true;
        let definition_id = parameters.get(2).and_then(ValueExt::reference);
        let definition_target = if let Some(definition_id) = definition_id {
            match target_context.resolve(definition_id)? {
                TargetResolution::Resolved(definition) => Some(definition),
                TargetResolution::Ambiguous(identities) => {
                    note_ambiguous_target(
                        losses,
                        &format!("draughting model #{model_id} association #{association_id}"),
                        "semantic_definition",
                        definition_id,
                        &identities,
                    );
                    complete = false;
                    None
                }
                TargetResolution::Unresolved => {
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

        model
            .relationships
            .entry(cadmpeg_core::nonblank_literal!("associated_items"))
            .or_default();
        let mut has_items = false;
        if let Some(items) = parameters.get(4) {
            visit_drawing_references(items, target_context.ctx, &mut |item_id| {
                has_items = true;
                match target_context.resolve(item_id)? {
                    TargetResolution::Resolved(item) => model
                        .relationships
                        .entry(cadmpeg_core::nonblank_literal!("associated_items"))
                        .or_default()
                        .push(item),
                    TargetResolution::Ambiguous(identities) => {
                        note_ambiguous_target(
                            losses,
                            &format!("draughting model #{model_id} association #{association_id}"),
                            "associated_items",
                            item_id,
                            &identities,
                        );
                        complete = false;
                    }
                    TargetResolution::Unresolved => {
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
            .partials
            .iter()
            .any(|partial| partial.name == "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER")
        {
            match association_placeholder_reference(record, parameters) {
                Some(placeholder_id) => match target_context.resolve(placeholder_id)? {
                    TargetResolution::Resolved(placeholder) => Some(placeholder),
                    TargetResolution::Ambiguous(identities) => {
                        note_ambiguous_target(
                            losses,
                            &format!("draughting model #{model_id} association #{association_id}"),
                            "annotation_placeholder",
                            placeholder_id,
                            &identities,
                        );
                        complete = false;
                        None
                    }
                    TargetResolution::Unresolved => {
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
            model
                .relationships
                .entry(cadmpeg_core::nonblank_literal!("semantic_definition"))
                .or_default()
                .push(definition);
        }
        if let Some(placeholder) = placeholder_target {
            model
                .relationships
                .entry(cadmpeg_core::nonblank_literal!("annotation_placeholder"))
                .or_default()
                .push(placeholder);
        }
        if complete {
            typed.insert(association_id);
        }
    }
    Ok(())
}

fn association_parameters(record: &RawRecord) -> Option<&[Value]> {
    [
        "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER",
        "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
        "ITEM_IDENTIFIED_REPRESENTATION_USAGE",
    ]
    .into_iter()
    .find_map(|name| {
        record
            .partials
            .iter()
            .find(|partial| partial.name == name && partial.parameters.len() >= 5)
            .map(|partial| partial.parameters.as_slice())
    })
}

fn association_placeholder_reference(record: &RawRecord, parameters: &[Value]) -> Option<u64> {
    parameters.get(5).and_then(ValueExt::reference).or_else(|| {
        record
            .partials
            .iter()
            .find(|partial| partial.name == "ANNOTATION_PLACEHOLDER_OCCURRENCE")
            .and_then(|partial| partial.parameters.first())
            .and_then(ValueExt::reference)
    })
}

fn target_resolution(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    known_typed: &dyn SourceClaims,
    exchange: &Exchange,
    external_documents: &BTreeMap<u64, &str>,
    ctx: &DecodeContext<'_>,
) -> Result<TargetResolution, CodecError> {
    if let Some(identity) = target_identities
        .get(&id)
        .filter(|identities| identities.len() == 1)
        .and_then(|identities| identities.first())
    {
        return Ok(TargetResolution::Resolved(ReferenceSelection::new(
            ReferenceTarget::Local(identity.to_owned()),
            Vec::new(),
        )));
    }
    if let Some(uri) = external_documents.get(&id) {
        return Ok(TargetResolution::Resolved(ReferenceSelection::new(
            ReferenceTarget::External {
                document: (*uri).to_owned(),
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
    if !known_typed.contains(id) {
        if let Some(record) = exchange.records().get(&id) {
            return Ok(TargetResolution::Resolved(ReferenceSelection::new(
                ReferenceTarget::Local(opaque_record_id(id, record)?.into_string()),
                Vec::new(),
            )));
        }
    }
    let ambiguity = target_identities
        .get(&id)
        .filter(|identities| identities.len() > 1)
        .map(clone_drawing_identities)
        .or(wrapper_ambiguity);
    Ok(ambiguity.map_or(TargetResolution::Unresolved, TargetResolution::Ambiguous))
}

enum WrapperTargetResolution {
    Singleton(String),
    Ambiguous(BTreeSet<String>),
}

fn wrapper_target_resolution(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Option<WrapperTargetResolution>, CodecError> {
    if target_identities.contains_key(&id) {
        return Ok(None);
    }
    let mut identities = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    let mut pending = ctx.alloc_filled(1, (id, false), "step_drawing_wrapper_pending")?;
    while let Some((id, leaving)) = pending.pop() {
        if leaving {
            active.remove(&id);
            complete.insert(id);
            continue;
        }
        if complete.contains(&id) {
            continue;
        }
        if active.contains(&id) {
            return Ok(None);
        }
        active.insert(id);

        pending.push((id, true));
        if let Some(targets) = target_identities.get(&id) {
            for target in targets {
                if !identities.contains(target) {
                    let copy = target.to_owned();
                    identities.insert(copy);
                }
            }
            continue;
        }
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        if let Some(plane) = record
            .partials
            .iter()
            .find(|partial| partial.name == "ANNOTATION_PLANE")
            .and_then(|partial| partial.parameters.get(2))
            .and_then(ValueExt::reference)
        {
            pending.push((plane, false));
        } else if let Some(items) = mapped_representation(record, exchange)
            .and_then(|representation| exchange.records().get(&representation))
            .and_then(representation::items)
        {
            for item in items.rev() {
                pending.push((item, false));
            }
        }
    }
    if identities.len() == 1 {
        let Some(identity) = identities.pop_first() else {
            return Ok(None);
        };
        return Ok(Some(WrapperTargetResolution::Singleton(identity)));
    }
    if identities.is_empty() {
        Ok(None)
    } else {
        Ok(Some(WrapperTargetResolution::Ambiguous(identities)))
    }
}

fn mapped_representation(record: &RawRecord, exchange: &Exchange) -> Option<u64> {
    let map_id = record
        .partials
        .iter()
        .find(|partial| partial.name == "MAPPED_ITEM")
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference)?;
    exchange
        .records()
        .get(&map_id)
        .and_then(|map| {
            map.partials
                .iter()
                .find(|partial| partial.name == "REPRESENTATION_MAP")
        })
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference)
}

fn value_text(
    exchange: &Exchange,
    value: &Value,
    losses: &mut Vec<LossNote>,
    record_id: u64,
    field: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let _depth_guard = ctx.enter_nested("step_drawing_value_text_depth")?;
    let text = match value {
        Value::UninterpretedLiteral => return Ok(None),
        Value::Reference(id) => {
            format!("#{id}")
        }
        Value::ExternalReference(id) => {
            format!("@{id}")
        }
        Value::ConstantEntity(name) => {
            format!("#{name}")
        }
        Value::ExpressValueConstant(name) => {
            format!("@{name}")
        }
        Value::Integer(value) => {
            format!("{value}")
        }
        Value::Real(value) => {
            format!("{}", value.get())
        }
        Value::Enumeration(value) => {
            format!(".{value}.")
        }
        Value::String(_) => {
            return Ok(decode_text_charged(
                exchange,
                value,
                losses,
                record_id,
                field,
                StepLossCode::MetadataStringInvalid,
            ))
        }
        Value::Binary(value) => {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            let mut text = format!("binary:{}:", value.bit_len());
            for byte in value.data() {
                text.push(char::from(HEX[usize::from(byte >> 4)]));
                text.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
            text
        }
        Value::Resource(value) => {
            format!("<{value}>")
        }
        Value::Omitted => "$".to_string(),
        Value::Derived => "*".to_string(),
        Value::List(values) => {
            let mut text = "(".to_string();
            for (index, value) in values.iter().enumerate() {
                let Some(part) = value_text(exchange, value, losses, record_id, field, ctx)? else {
                    return Ok(None);
                };
                if index != 0 {
                    text.push(',');
                }
                text.push_str(&part);
            }
            text.push(')');
            text
        }
        Value::Typed(name, value) => {
            let Some(value) = value_text(exchange, value, losses, record_id, field, ctx)? else {
                return Ok(None);
            };
            format!("{name}({value})")
        }
    };
    Ok(Some(text))
}

#[cfg(test)]
mod tests;
