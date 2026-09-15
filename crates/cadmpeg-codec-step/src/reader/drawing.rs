// SPDX-License-Identifier: Apache-2.0
//! STEP drawing definitions, revisions, sheets, views, and their relations.

use crate::ids::kind;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind};
use cadmpeg_ir::ids::{Identity, ProductDefinitionId};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::NativeRecord;
use cadmpeg_ir::{ReferenceSelection, ReferenceTarget};

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::representation;
use super::{decode_text, opaque_record_id, record_targets, StageOutcome};

const DRAWING_ASSOCIATION_TYPES: &[&str] = &[
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER",
];

struct TargetContext<'a> {
    target_identities: &'a BTreeMap<u64, BTreeSet<String>>,
    known_typed: &'a HashSet<u64>,
    exchange: &'a Exchange,
    external_documents: &'a BTreeMap<u64, &'a str>,
}

struct DrawingCandidate<'a> {
    id: u64,
    name: &'static str,
    identity: Identity,
    offset: usize,
    parameters: Cow<'a, [Value]>,
}

enum TargetResolution {
    Resolved(ReferenceSelection),
    Ambiguous(BTreeSet<String>),
    Unresolved,
}

impl TargetContext<'_> {
    fn resolve(&self, id: u64) -> TargetResolution {
        target_resolution(
            id,
            self.target_identities,
            self.known_typed,
            self.exchange,
            self.external_documents,
        )
    }
}

/// Decode the drawing object graph without claiming unsupported graphics.
pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    known_typed: &HashSet<u64>,
    product_definition_ids_by_shape: &BTreeMap<u64, ProductDefinitionId>,
) -> StageOutcome<()> {
    let mut losses = Vec::new();
    let mut candidates = exchange
        .records()
        .iter()
        .filter_map(|(&id, record)| {
            let (name, kind) = drawing_type(record)?;
            let parameters = source_parameters(record, name);
            if required_parameter_count(name).is_some_and(|count| parameters.len() < count) {
                losses.push(StepLossCode::DrawingRecordTooFewParameters.note(format!(
                        "STEP drawing record #{id} has too few {name} parameters and was retained opaque"
                    )));
                return None;
            }
            Some(DrawingCandidate {
                id,
                name,
                identity: ids::drawing(kind, id),
                offset: record.span.start,
                parameters,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.offset);

    if candidates.is_empty() {
        return StageOutcome {
            value: (),
            claims: HashSet::new(),
            losses,
            notes: Vec::new(),
        };
    }

    let drawing_ids = candidates
        .iter()
        .map(|candidate| candidate.id)
        .collect::<BTreeSet<_>>();
    let hidden_drawing_ids = exchange
        .records()
        .values()
        .filter_map(|record| {
            record
                .partials
                .iter()
                .find(|partial| partial.name == "INVISIBILITY")
                .and_then(|partial| partial.parameters.first())
        })
        .flat_map(|items| {
            let mut targets = Vec::new();
            collect_references(items, &mut targets);
            targets
        })
        .filter(|id| drawing_ids.contains(id))
        .collect::<BTreeSet<_>>();

    let mut target_identities = record_targets(ir, |record_id| known_typed.contains(&record_id));
    for candidate in &candidates {
        target_identities
            .entry(candidate.id)
            .or_default()
            .insert(candidate.identity.as_str().to_owned());
    }
    // DR-01: a drawing association scoped by PRODUCT_DEFINITION_SHAPE targets
    // that shape's one owning product-definition view, not a product-wide
    // identity set.
    for (&shape_id, product_definition_id) in product_definition_ids_by_shape {
        target_identities
            .entry(shape_id)
            .or_default()
            .insert(product_definition_id.as_str().to_owned());
    }
    let drawing_target_ids = referenced_target_ids(exchange, &candidates);
    add_source_typed_targets(
        ir,
        exchange,
        known_typed,
        &drawing_target_ids,
        &mut target_identities,
    );
    let external_documents = exchange
        .references
        .iter()
        .filter_map(|entry| {
            let id = entry.name.strip_prefix('#')?.parse().ok()?;
            Some((id, entry.uri.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let target_context = TargetContext {
        target_identities: &target_identities,
        known_typed,
        exchange,
        external_documents: &external_documents,
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
            ) {
                stored_parameters.insert(parameter_key(name, index), value);
            }
        }

        let mut relationships = BTreeMap::new();
        add_reference_fields(
            &mut relationships,
            name,
            parameters.as_ref(),
            id,
            &target_context,
            &mut losses,
        );
        drawings.insert(
            id,
            Drawing {
                id: DrawingId::from(identity.clone()),
                object: identity.as_str().to_owned(),
                kind: drawing_kind(name),
                runtime_type: name.into(),
                order: u32::try_from(order).unwrap_or(u32::MAX),
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

    add_sheet_revision_usages(exchange, &mut drawings, &target_context, &mut losses);
    let mut association_ids = HashSet::new();
    add_draughting_model_associations(
        exchange,
        &mut drawings,
        &target_context,
        &mut losses,
        &mut association_ids,
    );

    let mut typed_records = drawings.keys().copied().collect::<HashSet<_>>();
    typed_records.extend(association_ids);
    ir.model.drawings.extend(drawings.into_values());
    StageOutcome {
        value: (),
        claims: typed_records,
        losses,
        notes: Vec::new(),
    }
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
) -> BTreeSet<u64> {
    let mut ids = BTreeSet::new();
    for candidate in candidates {
        for &index in relationship_indices(candidate.name) {
            if let Some(value) = candidate.parameters.get(index) {
                collect_reference_ids(value, &mut ids);
            }
        }
    }
    for (_, record) in exchange.entities("DRAWING_SHEET_REVISION_USAGE") {
        let parameters = source_parameters(record, "DRAWING_SHEET_REVISION_USAGE");
        for value in parameters.iter().take(2) {
            collect_reference_ids(value, &mut ids);
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
                collect_reference_ids(value, &mut ids);
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
    ids
}

fn collect_reference_ids(value: &Value, output: &mut BTreeSet<u64>) {
    let mut references = Vec::new();
    collect_references(value, &mut references);
    output.extend(references);
}

fn add_source_typed_targets(
    ir: &mut CadIr,
    exchange: &Exchange,
    known_typed: &HashSet<u64>,
    referenced_ids: &BTreeSet<u64>,
    target_identities: &mut BTreeMap<u64, BTreeSet<String>>,
) {
    let mut native_targets = Vec::new();
    for &id in referenced_ids {
        if !known_typed.contains(&id) || target_identities.contains_key(&id) {
            continue;
        }
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        if wrapper_target_resolution(id, target_identities, exchange).is_some() {
            continue;
        }
        let identity = opaque_record_id(id, record);
        let source_type = record
            .partials
            .iter()
            .map(|partial| partial.name.as_str())
            .collect::<Vec<_>>()
            .join("+");
        let mut fields = serde_json::Map::new();
        fields.insert(
            "source_id".into(),
            serde_json::Value::String(format!("#{id}")),
        );
        fields.insert("source_type".into(), serde_json::Value::String(source_type));
        native_targets.push(NativeRecord::from_identity(identity.clone(), fields));
        target_identities.insert(id, BTreeSet::from([identity.into_string()]));
    }
    if native_targets.is_empty() {
        return;
    }
    let namespace = ir.native.namespace_mut("step");
    namespace
        .arenas_mut()
        .entry("drawing_targets".into())
        .or_default()
        .extend(native_targets);
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

fn source_parameters<'a>(record: &'a RawRecord, name: &str) -> Cow<'a, [Value]> {
    let direct = record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .map(|partial| partial.parameters.as_slice());
    if name == "DRAUGHTING_CALLOUT" {
        if let Some(parameters) = direct.filter(|parameters| parameters.len() >= 2) {
            return Cow::Borrowed(parameters);
        }
        let mut parameters = Vec::new();
        if let Some(value) = record
            .partials
            .iter()
            .find(|partial| partial.name == "REPRESENTATION_ITEM")
            .and_then(|partial| partial.parameters.first())
        {
            parameters.push(value.clone());
        }
        parameters.extend(direct.unwrap_or_default().iter().cloned());
        return Cow::Owned(parameters);
    }
    if let Some(parameters) = direct.filter(|parameters| !parameters.is_empty()) {
        return Cow::Borrowed(parameters);
    }
    if matches!(
        name,
        "DRAUGHTING_MODEL" | "PRESENTATION_VIEW" | "DRAWING_SHEET_REVISION"
    ) {
        if let Some(parameters) = representation::parameters(record) {
            return Cow::Borrowed(parameters);
        }
    }
    Cow::Borrowed(direct.unwrap_or_default())
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
    parameters: &[Value],
    source_id: u64,
    target_context: &TargetContext<'_>,
    losses: &mut Vec<LossNote>,
) {
    for &index in relationship_indices(name) {
        let Some(value) = parameters.get(index) else {
            continue;
        };
        let role = parameter_key(name, index);
        let mut references = Vec::new();
        collect_references(value, &mut references);
        for target_id in references {
            match target_context.resolve(target_id) {
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
        }
    }
}

fn note_ambiguous_target(
    losses: &mut Vec<LossNote>,
    source: &str,
    role: &str,
    target_id: u64,
    identities: &BTreeSet<String>,
) {
    let identities = identities.iter().cloned().collect::<Vec<_>>().join(", ");
    losses.push(StepLossCode::DrawingRelationshipTargetAmbiguous.note(format!(
        "STEP {source} relationship {role} references source record #{target_id} with multiple neutral identities ({identities}); no target was selected and the raw source parameter is retained"
    )));
}

fn add_sheet_revision_usages(
    exchange: &Exchange,
    drawings: &mut BTreeMap<u64, Drawing>,
    target_context: &TargetContext<'_>,
    losses: &mut Vec<LossNote>,
) {
    let usages = exchange
        .entities("DRAWING_SHEET_REVISION_USAGE")
        .filter_map(|(id, record)| {
            let parameters = source_parameters(record, "DRAWING_SHEET_REVISION_USAGE");
            Some((
                id,
                value_reference(parameters.first()?)?,
                value_reference(parameters.get(1)?)?,
                parameters.get(2).cloned(),
            ))
        })
        .collect::<Vec<_>>();
    for (usage_id, sheet_id, revision_id, sequence) in usages {
        let sheet_target = target_context.resolve(revision_id);
        let revision_target = target_context.resolve(sheet_id);
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
            if let Some(sequence) = sequence.and_then(|value| {
                value_text(
                    target_context.exchange,
                    &value,
                    losses,
                    usage_id,
                    "drawing sheet revision usage sequence",
                )
            }) {
                sheet.parameters.insert(
                    cadmpeg_core::nonblank_literal!("usage_{usage_id}_sequence"),
                    sequence,
                );
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
}

fn add_draughting_model_associations(
    exchange: &Exchange,
    drawings: &mut BTreeMap<u64, Drawing>,
    target_context: &TargetContext<'_>,
    losses: &mut Vec<LossNote>,
    typed: &mut HashSet<u64>,
) {
    for association_id in
        exchange.matching_entity_ids(|name| DRAWING_ASSOCIATION_TYPES.contains(&name))
    {
        let Some(record) = exchange.records().get(&association_id) else {
            continue;
        };
        let Some(parameters) = association_parameters(record) else {
            continue;
        };
        let Some(model_id) = parameters.get(3).and_then(value_reference) else {
            continue;
        };
        let Some(model) = drawings.get_mut(&model_id) else {
            continue;
        };

        let mut complete = true;
        let definition_id = parameters.get(2).and_then(value_reference);
        let definition_target = definition_id.and_then(|definition_id| {
            match target_context.resolve(definition_id) {
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
        });
        if definition_id.is_none() {
            complete = false;
        }

        let item_ids = parameters
            .get(4)
            .into_iter()
            .flat_map(|items| {
                let mut references = Vec::new();
                collect_references(items, &mut references);
                references
            })
            .collect::<Vec<_>>();
        if item_ids.is_empty() {
            complete = false;
        }
        let item_targets = item_ids
            .into_iter()
            .filter_map(|item_id| match target_context.resolve(item_id) {
                TargetResolution::Resolved(item) => Some(item),
                TargetResolution::Ambiguous(identities) => {
                    note_ambiguous_target(
                        losses,
                        &format!("draughting model #{model_id} association #{association_id}"),
                        "associated_items",
                        item_id,
                        &identities,
                    );
                    complete = false;
                    None
                }
                TargetResolution::Unresolved => {
                    losses.push(StepLossCode::DraughtingAssociatedItemUntyped.note(
                        format!(
                            "STEP draughting model #{model_id} association #{association_id} references source-typed item #{item_id} without a neutral identity; the raw source parameter is retained"
                        ),
                    ));
                    complete = false;
                    None
                }
            })
            .collect::<Vec<_>>();

        let placeholder_target = if record
            .partials
            .iter()
            .any(|partial| partial.name == "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER")
        {
            match association_placeholder_reference(record, parameters) {
                Some(placeholder_id) => match target_context.resolve(placeholder_id) {
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
        model
            .relationships
            .entry(cadmpeg_core::nonblank_literal!("associated_items"))
            .or_default()
            .extend(item_targets);
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
    parameters.get(5).and_then(value_reference).or_else(|| {
        record
            .partials
            .iter()
            .find(|partial| partial.name == "ANNOTATION_PLACEHOLDER_OCCURRENCE")
            .and_then(|partial| partial.parameters.first())
            .and_then(value_reference)
    })
}

fn target_resolution(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    known_typed: &HashSet<u64>,
    exchange: &Exchange,
    external_documents: &BTreeMap<u64, &str>,
) -> TargetResolution {
    if let Some(identity) = target_identities
        .get(&id)
        .filter(|identities| identities.len() == 1)
        .and_then(|identities| identities.first())
    {
        return TargetResolution::Resolved(ReferenceSelection::new(
            ReferenceTarget::Local(identity.clone()),
            Vec::new(),
        ));
    }
    if let Some(uri) = external_documents.get(&id) {
        return TargetResolution::Resolved(ReferenceSelection::new(
            ReferenceTarget::External {
                document: (*uri).into(),
                object: format!("#{id}"),
            },
            Vec::new(),
        ));
    }
    let wrapper_ambiguity = match wrapper_target_resolution(id, target_identities, exchange) {
        Some(WrapperTargetResolution::Singleton(identity)) => {
            return TargetResolution::Resolved(ReferenceSelection::new(
                ReferenceTarget::Local(identity),
                Vec::new(),
            ));
        }
        Some(WrapperTargetResolution::Ambiguous(identities)) => Some(identities),
        None => None,
    };
    if !known_typed.contains(&id) {
        if let Some(record) = exchange.records().get(&id) {
            return TargetResolution::Resolved(ReferenceSelection::new(
                ReferenceTarget::Local(opaque_record_id(id, record).into_string()),
                Vec::new(),
            ));
        }
    }
    target_identities
        .get(&id)
        .filter(|identities| identities.len() > 1)
        .cloned()
        .or(wrapper_ambiguity)
        .map_or(TargetResolution::Unresolved, TargetResolution::Ambiguous)
}

enum WrapperTargetResolution {
    Singleton(String),
    Ambiguous(BTreeSet<String>),
}

fn wrapper_target_resolution(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    exchange: &Exchange,
) -> Option<WrapperTargetResolution> {
    if target_identities.contains_key(&id) {
        return None;
    }
    let mut identities = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    let mut pending = vec![(id, false)];
    while let Some((id, leaving)) = pending.pop() {
        if leaving {
            active.remove(&id);
            complete.insert(id);
            continue;
        }
        if complete.contains(&id) {
            continue;
        }
        if !active.insert(id) {
            return None;
        }
        pending.push((id, true));
        if let Some(targets) = target_identities.get(&id) {
            identities.extend(targets.iter().cloned());
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
            .and_then(value_reference)
        {
            pending.push((plane, false));
        } else if let Some(items) = mapped_representation(record, exchange)
            .and_then(|representation| exchange.records().get(&representation))
            .and_then(representation::items)
        {
            pending.extend(items.into_iter().rev().map(|item| (item, false)));
        }
    }
    let identity = identities.pop_first()?;
    if identities.is_empty() {
        return Some(WrapperTargetResolution::Singleton(identity));
    }
    identities.insert(identity);
    Some(WrapperTargetResolution::Ambiguous(identities))
}

fn mapped_representation(record: &RawRecord, exchange: &Exchange) -> Option<u64> {
    let map_id = record
        .partials
        .iter()
        .find(|partial| partial.name == "MAPPED_ITEM")
        .and_then(|partial| partial.parameters.get(1))
        .and_then(value_reference)?;
    exchange
        .records()
        .get(&map_id)
        .and_then(|map| {
            map.partials
                .iter()
                .find(|partial| partial.name == "REPRESENTATION_MAP")
        })
        .and_then(|partial| partial.parameters.get(1))
        .and_then(value_reference)
}

fn collect_references(value: &Value, output: &mut Vec<u64>) {
    match value {
        Value::Reference(id) => output.push(*id),
        Value::List(values) => values
            .iter()
            .for_each(|value| collect_references(value, output)),
        Value::Typed(_, value) => collect_references(value, output),
        _ => {}
    }
}

fn value_reference(value: &Value) -> Option<u64> {
    match value {
        Value::Reference(id) => Some(*id),
        _ => None,
    }
}

fn value_text(
    exchange: &Exchange,
    value: &Value,
    losses: &mut Vec<LossNote>,
    record_id: u64,
    field: &str,
) -> Option<String> {
    match value {
        Value::Reference(id) => Some(format!("#{id}")),
        Value::ValueReference(id) => Some(format!("@{id}")),
        Value::ConstantEntity(name) => Some(format!("#{name}")),
        Value::ConstantValue(name) => Some(format!("@{name}")),
        Value::Integer(value) => Some(value.to_string()),
        Value::Real(value) => Some(value.to_string()),
        Value::Enumeration(value) => Some(format!(".{value}.")),
        Value::String(_) => decode_text(
            exchange,
            value,
            losses,
            record_id,
            field,
            StepLossCode::MetadataStringInvalid,
        ),
        Value::Binary(value) => Some(format!(
            "binary:{}:{}",
            value.bit_len(),
            value.data().iter().fold(String::new(), |mut output, byte| {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                output.push(char::from(HEX[(byte >> 4) as usize]));
                output.push(char::from(HEX[(byte & 0x0F) as usize]));
                output
            })
        )),
        Value::Resource(value) => Some(format!("<{value}>")),
        Value::Omitted => Some("$".into()),
        Value::Derived => Some("*".into()),
        Value::List(values) => values
            .iter()
            .map(|value| value_text(exchange, value, losses, record_id, field))
            .collect::<Option<Vec<_>>>()
            .map(|values| format!("({})", values.join(","))),
        Value::Typed(name, value) => value_text(exchange, value, losses, record_id, field)
            .map(|value| format!("{name}({value})")),
    }
}

#[cfg(test)]
mod tests;
