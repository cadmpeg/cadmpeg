// SPDX-License-Identifier: Apache-2.0
//! STEP drawing definitions, revisions, sheets, views, and their relations.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind, DrawingTarget};
use cadmpeg_ir::report::LossNote;

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

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

struct TargetContext<'a> {
    target_identities: &'a BTreeMap<u64, BTreeSet<String>>,
    known_typed: &'a HashSet<u64>,
    exchange: &'a Exchange,
    external_documents: &'a BTreeMap<u64, &'a str>,
}

impl TargetContext<'_> {
    fn targets(&self, id: u64) -> Vec<DrawingTarget> {
        targets_for(
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
            parameters,
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
    add_draughting_model_associations(exchange, &mut drawings, &target_context, &mut losses);

    let typed_records = drawings.keys().copied().collect::<HashSet<_>>();
    ir.model.drawings.extend(drawings.into_values());
    StageOutcome {
        value: (),
        claims: typed_records,
        warnings: Vec::new(),
        losses,
        notes: Vec::new(),
    }
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

fn source_parameters<'a>(record: &'a RawRecord, name: &str) -> &'a [Value] {
    record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .map_or(&[], |partial| partial.parameters.as_slice())
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
            let targets = target_context.targets(target_id);
            if targets.is_empty() {
                losses.push(StepLossCode::DrawingRelationshipUntypedTarget.note(format!(
                        "STEP drawing #{source_id} {name} relationship {role} references source-typed record #{target_id} without a neutral identity; the raw source parameter is retained"
                    )));
                continue;
            }
            relationships
                .entry(role.into())
                .or_default()
                .extend(targets);
        }
    }
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
                parameters.get(2),
            ))
        })
        .collect::<Vec<_>>();
    for (usage_id, sheet_id, revision_id, sequence) in usages {
        let sheet_targets = target_context.targets(revision_id);
        let revision_targets = target_context.targets(sheet_id);
        if let Some(sheet) = drawings.get_mut(&sheet_id) {
            if sheet_targets.is_empty() {
                losses.push(StepLossCode::DrawingSheetRevisionUnresolved.note(format!(
                    "STEP drawing sheet #{sheet_id} usage #{usage_id} has no resolvable drawing revision #{revision_id}"
                )));
            } else {
                sheet
                    .relationships
                    .entry("drawing_revision".into())
                    .or_default()
                    .extend(sheet_targets);
            }
            if let Some(sequence) = sequence.and_then(|value| {
                value_text(
                    target_context.exchange,
                    value,
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
            if revision_targets.is_empty() {
                losses.push(StepLossCode::DrawingRevisionSheetUnresolved.note(format!(
                    "STEP drawing revision #{revision_id} usage #{usage_id} has no resolvable sheet revision #{sheet_id}"
                )));
            } else {
                revision
                    .relationships
                    .entry("sheet_revision".into())
                    .or_default()
                    .extend(revision_targets);
            }
        }
    }
}

fn add_draughting_model_associations(
    exchange: &Exchange,
    drawings: &mut BTreeMap<u64, Drawing>,
    target_context: &TargetContext<'_>,
    losses: &mut Vec<LossNote>,
) {
    for (association_id, record) in exchange.entities("DRAUGHTING_MODEL_ITEM_ASSOCIATION") {
        let parameters = source_parameters(record, "DRAUGHTING_MODEL_ITEM_ASSOCIATION");
        let Some(model_id) = parameters.get(3).and_then(value_reference) else {
            continue;
        };
        let Some(model) = drawings.get_mut(&model_id) else {
            continue;
        };
        if let Some(definition_id) = parameters.get(2).and_then(value_reference) {
            let definitions = target_context.targets(definition_id);
            if definitions.is_empty() {
                losses.push(StepLossCode::DraughtingSemanticDefinitionUntyped.note(format!(
                    "STEP draughting model #{model_id} association #{association_id} references a typed semantic definition without a neutral identity; the raw source parameter is retained"
                )));
            } else {
                model
                    .relationships
                    .entry("semantic_definition".into())
                    .or_default()
                    .extend(definitions);
            }
        }
        let Some(items) = parameters.get(4) else {
            continue;
        };
        let mut references = Vec::new();
        collect_references(items, &mut references);
        for item_id in references {
            let targets = target_context.targets(item_id);
            if targets.is_empty() {
                losses.push(StepLossCode::DraughtingAssociatedItemUntyped.note(format!(
                    "STEP draughting model #{model_id} association #{association_id} references source-typed item #{item_id} without a neutral identity; the raw source parameter is retained"
                )));
            } else {
                model
                    .relationships
                    .entry("associated_items".into())
                    .or_default()
                    .extend(targets);
            }
        }
    }
}

fn targets_for(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    known_typed: &HashSet<u64>,
    exchange: &Exchange,
    external_documents: &BTreeMap<u64, &str>,
) -> Vec<DrawingTarget> {
    if let Some(identities) = target_identities.get(&id) {
        return identities
            .iter()
            .map(|identity| DrawingTarget {
                target: Some(identity.clone()),
                external_document: None,
                external_object: None,
                is_null: false,
                subelements: Vec::new(),
            })
            .collect();
    }
    if let Some(uri) = external_documents.get(&id) {
        return vec![DrawingTarget {
            target: None,
            external_document: Some((*uri).into()),
            external_object: Some(format!("#{id}")),
            is_null: false,
            subelements: Vec::new(),
        }];
    }
    if known_typed.contains(&id) {
        return Vec::new();
    }
    exchange
        .records
        .get(&id)
        .map(|record| {
            vec![DrawingTarget {
                target: Some(opaque_record_id(record).0),
                external_document: None,
                external_object: None,
                is_null: false,
                subelements: Vec::new(),
            }]
        })
        .unwrap_or_default()
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
