// SPDX-License-Identifier: Apache-2.0
//! HEADER section strings as source attributes.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;

use crate::parse::{Exchange, HeaderRecord, Value};

/// Effective length limit of a `FILE_DESCRIPTION` or `FILE_NAME` string.
const MAX_METADATA_CHARS: usize = 256;
/// Effective length limit of a `FILE_SCHEMA` identifier.
const MAX_SCHEMA_IDENTIFIER_CHARS: usize = 1024;
/// Largest number of list members kept from one header list.
const MAX_LIST_MEMBERS: usize = 64;

/// One header attribute: its record, its parameter position, and its key.
struct Field {
    record: &'static str,
    position: usize,
    key: &'static str,
    max_chars: usize,
}

const FIELDS: [Field; 10] = [
    Field {
        record: "FILE_DESCRIPTION",
        position: 0,
        key: "file_description.description",
        max_chars: MAX_METADATA_CHARS,
    },
    Field {
        record: "FILE_DESCRIPTION",
        position: 1,
        key: "file_description.implementation_level",
        max_chars: MAX_METADATA_CHARS,
    },
    Field {
        record: "FILE_NAME",
        position: 0,
        key: "file_name.name",
        max_chars: MAX_METADATA_CHARS,
    },
    Field {
        record: "FILE_NAME",
        position: 1,
        key: "file_name.time_stamp",
        max_chars: MAX_METADATA_CHARS,
    },
    Field {
        record: "FILE_NAME",
        position: 2,
        key: "file_name.author",
        max_chars: MAX_METADATA_CHARS,
    },
    Field {
        record: "FILE_NAME",
        position: 3,
        key: "file_name.organization",
        max_chars: MAX_METADATA_CHARS,
    },
    Field {
        record: "FILE_NAME",
        position: 4,
        key: "file_name.preprocessor_version",
        max_chars: MAX_METADATA_CHARS,
    },
    Field {
        record: "FILE_NAME",
        position: 5,
        key: "file_name.originating_system",
        max_chars: MAX_METADATA_CHARS,
    },
    Field {
        record: "FILE_NAME",
        position: 6,
        key: "file_name.authorization",
        max_chars: MAX_METADATA_CHARS,
    },
    Field {
        record: "FILE_SCHEMA",
        position: 0,
        key: "file_schema.schema_identifiers",
        max_chars: MAX_SCHEMA_IDENTIFIER_CHARS,
    },
];

/// Insert the first `FILE_DESCRIPTION`, `FILE_NAME`, and `FILE_SCHEMA` strings.
///
/// A string parameter goes under the field key. A list member goes under the
/// field key, a dot, and its zero-based list position.
pub(super) fn add_source_attributes(
    exchange: &Exchange,
    attributes: &mut BTreeMap<NonBlankString, String>,
) {
    for field in &FIELDS {
        let Some(record) = first_record(exchange.header(), field.record) else {
            continue;
        };
        match record.parameters.get(field.position) {
            Some(Value::String(bytes)) => {
                if let Some(value) = text(exchange, bytes, field.max_chars) {
                    insert(attributes, field.key.to_owned(), value);
                }
            }
            Some(Value::List(members)) => {
                for (position, member) in members.iter().enumerate().take(MAX_LIST_MEMBERS) {
                    let Value::String(bytes) = member else {
                        continue;
                    };
                    if let Some(value) = text(exchange, bytes, field.max_chars) {
                        insert(attributes, format!("{}.{position}", field.key), value);
                    }
                }
            }
            _ => {}
        }
    }
}

fn first_record<'a>(header: &'a [HeaderRecord], name: &str) -> Option<&'a HeaderRecord> {
    header.iter().find(|record| record.name == name)
}

fn text(exchange: &Exchange, bytes: &[u8], max_chars: usize) -> Option<String> {
    let decoded = exchange.decode_string(bytes).ok()?;
    cadmpeg_core::text::attribute_text(&decoded, max_chars)
}

fn insert(attributes: &mut BTreeMap<NonBlankString, String>, key: String, value: String) {
    if let Some(key) = NonBlankString::new(key) {
        attributes.insert(key, value);
    }
}
