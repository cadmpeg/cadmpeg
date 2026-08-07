// SPDX-License-Identifier: Apache-2.0
//! External document and source dependency decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::report::{LossKind, LossNote};

use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text;

pub(super) struct DependencyResult {
    pub typed_records: BTreeSet<u64>,
    pub notes: Vec<String>,
    pub losses: Vec<LossNote>,
}

pub(super) fn decode(exchange: &Exchange) -> DependencyResult {
    let mut losses = Vec::new();
    let documents = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            let parameters = document_parameters(record)?;
            Some((
                id,
                (
                    parameters
                        .first()
                        .and_then(|value| {
                            decode_text(
                                value,
                                &mut losses,
                                id,
                                "document identifier",
                                LossKind::MetadataNotTransferred,
                            )
                        })
                        .unwrap_or_default(),
                    parameters
                        .get(1)
                        .and_then(|value| {
                            decode_text(
                                value,
                                &mut losses,
                                id,
                                "document name",
                                LossKind::MetadataNotTransferred,
                            )
                        })
                        .unwrap_or_default(),
                    parameters.get(3).and_then(ValueExt::reference),
                ),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let sources = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            let parameters = record.partial("EXTERNAL_SOURCE")?.parameters.as_slice();
            Some((
                id,
                parameters
                    .first()
                    .and_then(|value| source_text(value, &mut losses, id, "external source")),
            ))
        })
        .filter_map(|(id, source)| source.map(|source| (id, source)))
        .collect::<BTreeMap<_, _>>();
    let mut typed = BTreeSet::new();
    let mut notes = BTreeSet::new();

    for (&id, record) in &exchange.records {
        if let Some(parameters) = document_reference_parameters(record) {
            let Some(document_id) = parameters.first().and_then(ValueExt::reference) else {
                continue;
            };
            let Some((identifier, name, kind)) = documents.get(&document_id) else {
                continue;
            };
            let source = parameters
                .get(1)
                .and_then(|value| {
                    decode_text(
                        value,
                        &mut losses,
                        id,
                        "document reference source",
                        LossKind::MetadataNotTransferred,
                    )
                })
                .unwrap_or_default();
            notes.insert(document_note(identifier, name, &source));
            typed.extend([id, document_id]);
            typed.extend(kind);
        }
        if let Some(partial) = record.partial("EXTERNALLY_DEFINED_ITEM") {
            let Some(source_id) = partial.parameters.get(1).and_then(ValueExt::reference) else {
                continue;
            };
            let Some(source) = sources.get(&source_id) else {
                continue;
            };
            let item = partial
                .parameters
                .first()
                .and_then(|value| source_text(value, &mut losses, id, "external item"))
                .unwrap_or_default();
            notes.insert(format!("external source {source} item {item}"));
            typed.extend([id, source_id]);
        }
    }

    DependencyResult {
        typed_records: typed,
        notes: notes.into_iter().collect(),
        losses,
    }
}

fn document_parameters(record: &RawRecord) -> Option<&[Value]> {
    record
        .partial("DOCUMENT")
        .or_else(|| record.partial("DOCUMENT_FILE"))
        .map(|partial| partial.parameters.as_slice())
}

fn document_reference_parameters(record: &RawRecord) -> Option<&[Value]> {
    record
        .partial("DOCUMENT_REFERENCE")
        .or_else(|| record.partial("APPLIED_DOCUMENT_REFERENCE"))
        .map(|partial| partial.parameters.as_slice())
}

fn source_text(
    value: &Value,
    losses: &mut Vec<LossNote>,
    record_id: u64,
    field: &str,
) -> Option<String> {
    match value {
        Value::String(_) => decode_text(
            value,
            losses,
            record_id,
            field,
            LossKind::MetadataNotTransferred,
        ),
        Value::Typed(_, value) => source_text(value, losses, record_id, field),
        _ => None,
    }
}

fn document_note(identifier: &str, name: &str, source: &str) -> String {
    let identity = match (identifier.is_empty(), name.is_empty()) {
        (false, false) => format!("{identifier} ({name})"),
        (false, true) => identifier.to_owned(),
        (true, false) => name.to_owned(),
        (true, true) => "unnamed".to_owned(),
    };
    if source.is_empty() {
        format!("external document {identity}")
    } else {
        format!("external document {identity} from {source}")
    }
}

trait RecordExt {
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
}

impl RecordExt for RawRecord {
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials.iter().find(|partial| partial.name == name)
    }
}

trait ValueExt {
    fn reference(&self) -> Option<u64>;
}

impl ValueExt for Value {
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }
}
