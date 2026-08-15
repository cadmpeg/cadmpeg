// SPDX-License-Identifier: Apache-2.0
//! STEP drawing definitions, revisions, sheets, views, and their relations.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind, DrawingTarget};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::NativeRecord;

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::representation;
use super::{decode_text, opaque_record_id, record_targets, StageOutcome};

const DRAWING_ENTITIES: &[&str] = &[
    "DRAWING_DEFINITION",
    "DRAWING_REVISION",
    "DRAWING_SHEET_REVISION",
    "PRESENTATION_VIEW",
    "PRESENTATION_SIZE",
    "DRAUGHTING_MODEL",
    "DRAUGHTING_CALLOUT",
];
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

impl TargetContext<'_> {
    fn target(&self, id: u64) -> Option<DrawingTarget> {
        target_for(
            id,
            self.target_identities,
            self.known_typed,
            self.exchange,
            self.external_documents,
        )
    }

    fn ambiguous(&self, id: u64) -> Option<BTreeSet<String>> {
        if let Some(identities) = self
            .target_identities
            .get(&id)
            .filter(|identities| identities.len() > 1)
        {
            return Some(identities.clone());
        }
        wrapper_target_identities(id, self.target_identities, self.exchange)
            .filter(|identities| identities.len() > 1)
    }
}

/// Decode the drawing object graph without claiming unsupported graphics.
pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    known_typed: &HashSet<u64>,
) -> StageOutcome<()> {
    let mut losses = Vec::new();
    let mut candidates = exchange
        .records
        .values()
        .filter_map(|record| drawing_type(record).map(|name| (record.id, name)))
        .filter(|(id, name)| {
            let valid = required_parameter_count(name)
                .is_none_or(|count| source_parameters(&exchange.records[id], name).len() >= count);
            if !valid {
                losses.push(StepLossCode::DrawingRecordTooFewParameters.note(format!(
                        "STEP drawing record #{id} has too few {name} parameters and was retained opaque"
                    )));
            }
            valid
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(id, _)| exchange.records[id].span.start);

    if candidates.is_empty() {
        return StageOutcome {
            value: (),
            claims: HashSet::new(),
            warnings: Vec::new(),
            losses,
            notes: Vec::new(),
        };
    }

    let drawing_ids = candidates
        .iter()
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    let hidden_drawing_ids = exchange
        .records
        .values()
        .filter(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "INVISIBILITY")
        })
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

    let drawing_identities = candidates
        .iter()
        .map(|(id, name)| (*id, drawing_identity(*id, name)))
        .collect::<BTreeMap<_, _>>();
    let mut target_identities = record_targets(ir, |record_id| known_typed.contains(&record_id));
    for (&id, identity) in &drawing_identities {
        target_identities
            .entry(id)
            .or_default()
            .insert(identity.clone());
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
    for (order, &(id, name)) in candidates.iter().enumerate() {
        let record = &exchange.records[&id];
        let identity = drawing_identities
            .get(&id)
            .expect("drawing candidates have identities")
            .clone();
        let parameters = source_parameters(record, name);
        let mut stored_parameters = BTreeMap::new();
        stored_parameters.insert("source_id".into(), format!("#{id}"));
        stored_parameters.insert("source_type".into(), name.into());
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
                id: DrawingId(identity.clone()),
                object: identity.clone(),
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
                native_ref: identity,
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
        warnings: Vec::new(),
        losses,
        notes: Vec::new(),
    }
}

pub(super) fn is_supported_invisibility_target(record: &RawRecord) -> bool {
    let Some(name) = drawing_type(record) else {
        return false;
    };
    required_parameter_count(name)
        .is_none_or(|count| source_parameters(record, name).len() >= count)
}

fn referenced_target_ids(exchange: &Exchange, candidates: &[(u64, &str)]) -> BTreeSet<u64> {
    let mut ids = BTreeSet::new();
    for &(source_id, name) in candidates {
        let parameters = source_parameters(&exchange.records[&source_id], name);
        for &(index, _) in relationship_fields(name) {
            if let Some(value) = parameters.get(index) {
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
        let Some(record) = exchange.records.get(&association_id) else {
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
        let Some(record) = exchange.records.get(&id) else {
            continue;
        };
        if is_representation_context(record)
            || (is_wrapper_record(record, exchange)
                && !is_cyclic_wrapper(id, target_identities, exchange))
        {
            continue;
        }
        let identity = opaque_record_id(record).0;
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
        native_targets.push(NativeRecord::new(identity.clone(), fields));
        target_identities.insert(id, BTreeSet::from([identity]));
    }
    if native_targets.is_empty() {
        return;
    }
    let namespace = ir.native.namespace_mut("step");
    if namespace.version == 0 {
        namespace.version = 1;
    }
    namespace
        .arenas
        .entry("drawing_targets".into())
        .or_default()
        .extend(native_targets);
}

fn is_wrapper_record(record: &RawRecord, exchange: &Exchange) -> bool {
    record
        .partials
        .iter()
        .any(|partial| partial.name == "ANNOTATION_PLANE")
        || mapped_representation(record, exchange).is_some()
}

fn is_representation_context(record: &RawRecord) -> bool {
    record
        .partials
        .iter()
        .any(|partial| partial.name == "REPRESENTATION_CONTEXT")
}

fn is_cyclic_wrapper(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    exchange: &Exchange,
) -> bool {
    if target_identities.contains_key(&id) {
        return false;
    }
    let mut identities = BTreeSet::new();
    let mut active = BTreeSet::new();
    collect_wrapper_targets(
        id,
        target_identities,
        exchange,
        &mut active,
        &mut identities,
    )
}

fn drawing_type(record: &RawRecord) -> Option<&'static str> {
    DRAWING_ENTITIES
        .iter()
        .copied()
        .find(|name| record.partials.iter().any(|partial| partial.name == *name))
}

fn drawing_kind(name: &str) -> DrawingKind {
    match name {
        "DRAWING_SHEET_REVISION" => DrawingKind::Page,
        "PRESENTATION_VIEW" => DrawingKind::View,
        "DRAUGHTING_CALLOUT" => DrawingKind::Annotation,
        _ => DrawingKind::Other,
    }
}

fn drawing_identity(id: u64, name: &str) -> String {
    StepIdentity::drawing(&name.to_ascii_lowercase(), id)
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

fn parameter_key(name: &str, index: usize) -> String {
    let field = match (name, index) {
        ("DRAWING_DEFINITION", 0) => "name",
        ("DRAWING_DEFINITION", 1) => "description",
        ("DRAWING_REVISION", 0) => "name",
        ("DRAWING_REVISION", 1) => "drawing",
        ("DRAWING_REVISION", 2) => "description",
        ("DRAWING_SHEET_REVISION", 0) => "name",
        ("DRAWING_SHEET_REVISION", 1) => "items",
        ("DRAWING_SHEET_REVISION", 2) => "presentation_context",
        ("DRAWING_SHEET_REVISION", 3) => "revision",
        ("PRESENTATION_VIEW", 0) => "name",
        ("PRESENTATION_VIEW", 1) => "items",
        ("PRESENTATION_VIEW", 2) => "presentation_context",
        ("PRESENTATION_SIZE", 0) => "drawing_sheet_revision",
        ("PRESENTATION_SIZE", 1) => "size",
        ("DRAUGHTING_MODEL", 0) => "name",
        ("DRAUGHTING_MODEL", 1) => "items",
        ("DRAUGHTING_MODEL", 2) => "presentation_context",
        ("DRAUGHTING_CALLOUT", 0) => "name",
        ("DRAUGHTING_CALLOUT", 1) => "contents",
        _ => return format!("parameter_{index}"),
    };
    field.into()
}

fn relationship_fields(name: &str) -> &'static [(usize, &'static str)] {
    match name {
        "DRAWING_REVISION" => &[(1, "drawing")],
        "DRAWING_SHEET_REVISION" => &[(1, "items"), (2, "presentation_context"), (3, "revision")],
        "PRESENTATION_VIEW" => &[(1, "items"), (2, "presentation_context")],
        "PRESENTATION_SIZE" => &[(0, "drawing_sheet_revision"), (1, "size")],
        "DRAUGHTING_MODEL" => &[(1, "items"), (2, "presentation_context")],
        "DRAUGHTING_CALLOUT" => &[(1, "contents")],
        _ => &[],
    }
}

fn add_reference_fields(
    relationships: &mut BTreeMap<String, Vec<DrawingTarget>>,
    name: &str,
    parameters: &[Value],
    source_id: u64,
    target_context: &TargetContext<'_>,
    losses: &mut Vec<LossNote>,
) {
    for &(index, role) in relationship_fields(name) {
        let Some(value) = parameters.get(index) else {
            continue;
        };
        let mut references = Vec::new();
        collect_references(value, &mut references);
        for target_id in references {
            match target_context.target(target_id) {
                Some(target) => relationships.entry(role.into()).or_default().push(target),
                None => {
                    if let Some(identities) = target_context.ambiguous(target_id) {
                        note_ambiguous_target(
                            losses,
                            &format!("drawing #{source_id} {name}"),
                            role,
                            target_id,
                            &identities,
                        );
                    } else {
                        losses.push(StepLossCode::DrawingRelationshipUntypedTarget.note(format!(
                                "STEP drawing #{source_id} {name} relationship {role} references source-typed record #{target_id} without a neutral identity; the raw source parameter is retained"
                            )));
                    }
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
        let sheet_target = target_context.target(revision_id);
        let revision_target = target_context.target(sheet_id);
        if let Some(sheet) = drawings.get_mut(&sheet_id) {
            if let Some(target) = sheet_target {
                sheet
                    .relationships
                    .entry("drawing_revision".into())
                    .or_default()
                    .push(target);
            } else if let Some(identities) = target_context.ambiguous(revision_id) {
                note_ambiguous_target(
                    losses,
                    &format!("drawing sheet #{sheet_id} usage #{usage_id}"),
                    "drawing_revision",
                    revision_id,
                    &identities,
                );
            } else {
                losses.push(StepLossCode::DrawingSheetRevisionUnresolved.note(format!(
                        "STEP drawing sheet #{sheet_id} usage #{usage_id} has no resolvable drawing revision #{revision_id}"
                    )));
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
                sheet
                    .parameters
                    .insert(format!("usage_{usage_id}_sequence"), sequence);
            }
        }
        if let Some(revision) = drawings.get_mut(&revision_id) {
            if let Some(target) = revision_target {
                revision
                    .relationships
                    .entry("sheet_revision".into())
                    .or_default()
                    .push(target);
            } else if let Some(identities) = target_context.ambiguous(sheet_id) {
                note_ambiguous_target(
                    losses,
                    &format!("drawing revision #{revision_id} usage #{usage_id}"),
                    "sheet_revision",
                    sheet_id,
                    &identities,
                );
            } else {
                losses.push(StepLossCode::DrawingRevisionSheetUnresolved.note(format!(
                        "STEP drawing revision #{revision_id} usage #{usage_id} has no resolvable sheet revision #{sheet_id}"
                    )));
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
        let Some(record) = exchange.records.get(&association_id) else {
            continue;
        };
        let Some(parameters) = association_parameters(record) else {
            continue;
        };
        let Some(model_id) = parameters.get(3).and_then(value_reference) else {
            continue;
        };
        if !drawings.contains_key(&model_id) {
            continue;
        }

        let mut complete = true;
        let definition_id = parameters.get(2).and_then(value_reference);
        let definition_target = definition_id.and_then(|definition_id| {
            match target_context.target(definition_id) {
                Some(definition) => Some(definition),
                None if target_context.ambiguous(definition_id).is_some() => {
                    note_ambiguous_target(
                        losses,
                        &format!("draughting model #{model_id} association #{association_id}"),
                        "semantic_definition",
                        definition_id,
                        &target_context
                            .ambiguous(definition_id)
                            .expect("ambiguity checked above"),
                    );
                    complete = false;
                    None
                }
                None => {
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
            .filter_map(|item_id| match target_context.target(item_id) {
                Some(item) => Some(item),
                None if target_context.ambiguous(item_id).is_some() => {
                    note_ambiguous_target(
                        losses,
                        &format!("draughting model #{model_id} association #{association_id}"),
                        "associated_items",
                        item_id,
                        &target_context
                            .ambiguous(item_id)
                            .expect("ambiguity checked above"),
                    );
                    complete = false;
                    None
                }
                None => {
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
                Some(placeholder_id) => match target_context.target(placeholder_id) {
                    Some(placeholder) => Some(placeholder),
                    None if target_context.ambiguous(placeholder_id).is_some() => {
                        note_ambiguous_target(
                            losses,
                            &format!("draughting model #{model_id} association #{association_id}"),
                            "annotation_placeholder",
                            placeholder_id,
                            &target_context
                                .ambiguous(placeholder_id)
                                .expect("ambiguity checked above"),
                        );
                        complete = false;
                        None
                    }
                    None => {
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

        let Some(model) = drawings.get_mut(&model_id) else {
            continue;
        };
        if let Some(definition) = definition_target {
            model
                .relationships
                .entry("semantic_definition".into())
                .or_default()
                .push(definition);
        }
        model
            .relationships
            .entry("associated_items".into())
            .or_default()
            .extend(item_targets);
        if let Some(placeholder) = placeholder_target {
            model
                .relationships
                .entry("annotation_placeholder".into())
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
            .filter(|partial| {
                partial.name == "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER"
                    || partial.name == "ANNOTATION_PLACEHOLDER_OCCURRENCE"
            })
            .flat_map(|partial| partial.parameters.iter())
            .flat_map(|value| {
                let mut references = Vec::new();
                collect_references(value, &mut references);
                references
            })
            .next()
    })
}

fn target_for(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    known_typed: &HashSet<u64>,
    exchange: &Exchange,
    external_documents: &BTreeMap<u64, &str>,
) -> Option<DrawingTarget> {
    if let Some(identity) = target_identities
        .get(&id)
        .filter(|identities| identities.len() == 1)
        .and_then(|identities| identities.iter().next())
    {
        return Some(DrawingTarget {
            target: Some(identity.clone()),
            external_document: None,
            external_object: None,
            is_null: false,
            subelements: Vec::new(),
        });
    }
    if let Some(uri) = external_documents.get(&id) {
        return Some(DrawingTarget {
            target: None,
            external_document: Some((*uri).into()),
            external_object: Some(format!("#{id}")),
            is_null: false,
            subelements: Vec::new(),
        });
    }
    if let Some(identities) = wrapper_target_identities(id, target_identities, exchange) {
        if identities.len() == 1 {
            return Some(DrawingTarget {
                target: Some(
                    identities
                        .into_iter()
                        .next()
                        .expect("one wrapper target identity"),
                ),
                external_document: None,
                external_object: None,
                is_null: false,
                subelements: Vec::new(),
            });
        }
    }
    if known_typed.contains(&id) {
        return None;
    }
    exchange.records.get(&id).map(|record| DrawingTarget {
        target: Some(opaque_record_id(record).0),
        external_document: None,
        external_object: None,
        is_null: false,
        subelements: Vec::new(),
    })
}

fn wrapper_target_identities(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    exchange: &Exchange,
) -> Option<BTreeSet<String>> {
    if target_identities.contains_key(&id) {
        return None;
    }
    let mut identities = BTreeSet::new();
    let mut active = BTreeSet::new();
    let cyclic = collect_wrapper_targets(
        id,
        target_identities,
        exchange,
        &mut active,
        &mut identities,
    );
    (!cyclic && !identities.is_empty()).then_some(identities)
}

fn collect_wrapper_targets(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    identities: &mut BTreeSet<String>,
) -> bool {
    if !active.insert(id) {
        return true;
    }
    if let Some(targets) = target_identities.get(&id) {
        identities.extend(targets.iter().cloned());
        active.remove(&id);
        return false;
    }
    let Some(record) = exchange.records.get(&id) else {
        active.remove(&id);
        return false;
    };
    let cyclic = if let Some(plane) = record
        .partials
        .iter()
        .find(|partial| partial.name == "ANNOTATION_PLANE")
        .and_then(|partial| partial.parameters.get(2))
        .and_then(value_reference)
    {
        collect_wrapper_targets(plane, target_identities, exchange, active, identities)
    } else if let Some(representation) = mapped_representation(record, exchange) {
        if let Some(record) = exchange.records.get(&representation) {
            if let Some(items) = representation::items(record) {
                let mut cyclic = false;
                for item in items {
                    cyclic |= collect_wrapper_targets(
                        item,
                        target_identities,
                        exchange,
                        active,
                        identities,
                    );
                }
                cyclic
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };
    active.remove(&id);
    cyclic
}

fn mapped_representation(record: &RawRecord, exchange: &Exchange) -> Option<u64> {
    let map_id = record
        .partials
        .iter()
        .find(|partial| partial.name == "MAPPED_ITEM")
        .and_then(|partial| partial.parameters.get(1))
        .and_then(value_reference)?;
    exchange
        .records
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
            value.bit_len,
            value.data.iter().fold(String::new(), |mut output, byte| {
                write!(&mut output, "{byte:02X}").expect("writing binary value to String");
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
