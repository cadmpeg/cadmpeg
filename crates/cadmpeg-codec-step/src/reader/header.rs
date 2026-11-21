// SPDX-License-Identifier: Apache-2.0
//! HEADER section strings as source attributes.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;

use crate::parse::{Exchange, HeaderRecord, Value};

/// One header attribute: its record, its parameter position, and its key.
struct Field {
    record: &'static str,
    position: usize,
    key: &'static str,
}

const FIELDS: [Field; 10] = [
    Field {
        record: "FILE_DESCRIPTION",
        position: 0,
        key: "file_description.description",
    },
    Field {
        record: "FILE_DESCRIPTION",
        position: 1,
        key: "file_description.implementation_level",
    },
    Field {
        record: "FILE_NAME",
        position: 0,
        key: "file_name.name",
    },
    Field {
        record: "FILE_NAME",
        position: 1,
        key: "file_name.time_stamp",
    },
    Field {
        record: "FILE_NAME",
        position: 2,
        key: "file_name.author",
    },
    Field {
        record: "FILE_NAME",
        position: 3,
        key: "file_name.organization",
    },
    Field {
        record: "FILE_NAME",
        position: 4,
        key: "file_name.preprocessor_version",
    },
    Field {
        record: "FILE_NAME",
        position: 5,
        key: "file_name.originating_system",
    },
    Field {
        record: "FILE_NAME",
        position: 6,
        key: "file_name.authorization",
    },
    Field {
        record: "FILE_SCHEMA",
        position: 0,
        key: "file_schema.schema_identifiers",
    },
];

/// Insert the first `FILE_DESCRIPTION`, `FILE_NAME`, and `FILE_SCHEMA` strings.
///
/// A string parameter goes under the field key. A list member goes under the
/// field key, a dot, and its zero-based list position. Control characters
/// removed from a decoded string are reported with
/// [`StepLossCode::HeaderMetadataNoncanonical`].
pub(super) fn add_source_attributes(
    exchange: &Exchange,
    attributes: &mut BTreeMap<NonBlankString, String>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    for field in &FIELDS {
        let Some(record) = first_record(exchange.header(), field.record) else {
            continue;
        };
        match record.parameters.get(field.position) {
            Some(Value::String(bytes)) => {
                let (value, removed) = text(exchange, bytes);
                if removed {
                    losses.push(
                        crate::loss::StepLossCode::HeaderMetadataNoncanonical.note(format!(
                            "STEP header attribute {} contained control characters that were omitted.",
                            field.key
                        )),
                    );
                }
                if let Some(value) = value {
                    insert(attributes, field.key.to_owned(), value);
                }
            }
            Some(Value::List(members)) => {
                for (position, member) in members.iter().enumerate() {
                    let Value::String(bytes) = member else {
                        continue;
                    };
                    let key = format!("{}.{position}", field.key);
                    let (value, removed) = text(exchange, bytes);
                    if removed {
                        losses.push(
                            crate::loss::StepLossCode::HeaderMetadataNoncanonical.note(format!(
                                "STEP header attribute {key} contained control characters that were omitted."
                            )),
                        );
                    }
                    if let Some(value) = value {
                        insert(attributes, key, value);
                    }
                }
            }
            _ => {}
        }
    }
    losses
}

fn first_record<'a>(header: &'a [HeaderRecord], name: &str) -> Option<&'a HeaderRecord> {
    header.iter().find(|record| record.name == name)
}

fn text(exchange: &Exchange, bytes: &[u8]) -> (Option<String>, bool) {
    let Ok(decoded) = exchange.decode_string(bytes) else {
        return (None, false);
    };
    cadmpeg_core::text::attribute_text_checked(&decoded)
}

fn insert(attributes: &mut BTreeMap<NonBlankString, String>, key: String, value: String) {
    if let Some(key) = NonBlankString::new(key) {
        attributes.insert(key, value);
    }
}
