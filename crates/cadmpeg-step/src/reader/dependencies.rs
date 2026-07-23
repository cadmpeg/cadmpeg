// SPDX-License-Identifier: Apache-2.0
//! External document and source dependency decoding.

use std::collections::{BTreeMap, BTreeSet};

use crate::parse::{Exchange, RawRecord, Value};
use crate::vocab::{
    APPLIED_DOCUMENT_REFERENCE, DOCUMENT, DOCUMENT_FILE, DOCUMENT_REFERENCE,
    EXTERNALLY_DEFINED_ITEM, EXTERNAL_SOURCE,
};

pub(super) struct DependencyResult {
    pub typed_records: BTreeSet<u64>,
    pub notes: Vec<String>,
}

pub(super) fn decode(exchange: &Exchange) -> DependencyResult {
    let documents = exchange
        .records
        .iter()
        .filter(|(_, record)| matches!(record.simple_name(), Some(DOCUMENT | DOCUMENT_FILE)))
        .map(|(&id, record)| {
            (
                id,
                (
                    record
                        .parameter(0)
                        .and_then(ValueExt::text)
                        .unwrap_or_default(),
                    record
                        .parameter(1)
                        .and_then(ValueExt::text)
                        .unwrap_or_default(),
                    record.parameter(3).and_then(ValueExt::reference),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let sources = exchange
        .records
        .iter()
        .filter(|(_, record)| record.simple_name() == Some(EXTERNAL_SOURCE))
        .map(|(&id, record)| (id, record.parameter(0).and_then(ValueExt::source_text)))
        .filter_map(|(id, source)| source.map(|source| (id, source)))
        .collect::<BTreeMap<_, _>>();
    let mut typed = BTreeSet::new();
    let mut notes = BTreeSet::new();

    for (&id, record) in &exchange.records {
        if matches!(
            record.simple_name(),
            Some(APPLIED_DOCUMENT_REFERENCE | DOCUMENT_REFERENCE)
        ) {
            let Some(document_id) = record.parameter(0).and_then(ValueExt::reference) else {
                continue;
            };
            let Some((identifier, name, kind)) = documents.get(&document_id) else {
                continue;
            };
            let source = record
                .parameter(1)
                .and_then(ValueExt::text)
                .unwrap_or_default();
            notes.insert(document_note(identifier, name, &source));
            typed.extend([id, document_id]);
            typed.extend(kind);
        }
        if record.simple_name() == Some(EXTERNALLY_DEFINED_ITEM) {
            let Some(source_id) = record.parameter(1).and_then(ValueExt::reference) else {
                continue;
            };
            let Some(source) = sources.get(&source_id) else {
                continue;
            };
            let item = record
                .parameter(0)
                .and_then(ValueExt::source_text)
                .unwrap_or_default();
            notes.insert(format!("external source {source} item {item}"));
            typed.extend([id, source_id]);
        }
    }

    DependencyResult {
        typed_records: typed,
        notes: notes.into_iter().collect(),
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
    fn simple_name(&self) -> Option<&str>;
    fn parameter(&self, index: usize) -> Option<&Value>;
}

impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }

    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials.first()?.parameters.get(index)
    }
}

trait ValueExt {
    fn reference(&self) -> Option<u64>;
    fn text(&self) -> Option<String>;
    fn source_text(&self) -> Option<String>;
}

impl ValueExt for Value {
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }

    fn text(&self) -> Option<String> {
        if let Value::String(bytes) = self {
            crate::strings::decode(bytes).ok()
        } else {
            None
        }
    }

    fn source_text(&self) -> Option<String> {
        match self {
            Value::String(_) => self.text(),
            Value::Typed(_, value) => value.source_text(),
            _ => None,
        }
    }
}
