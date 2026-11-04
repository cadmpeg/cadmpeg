// SPDX-License-Identifier: Apache-2.0
//! `FILE_NAME` header projection into document attributes.
//!
//! The Part 21 header is the AP242 home for document metadata: the file
//! name, the write timestamp, the author and organization lists, and the
//! originating system. The decoder projects each held field into a
//! document-targeted neutral attribute, so a re-decode of a file the writer
//! produced holds the same attributes the writer folded into the header.
//! The attribute names are the names the IGES and FreeCAD decoders use for
//! the same facts, so their document attributes pair across formats.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::AttributeId;

use crate::ids::{self, key_word, kind};
use crate::parse::{Exchange, Value};

/// One `FILE_NAME` string parameter as decoded text, or empty when the
/// parameter is not a decodable string.
fn header_string(exchange: &Exchange, value: &Value) -> String {
    match value {
        Value::String(bytes) => exchange.decode_string(bytes).unwrap_or_default(),
        _ => String::new(),
    }
}

/// One `FILE_NAME` list parameter as its decoded entries.
fn header_list(exchange: &Exchange, value: &Value) -> Vec<String> {
    match value {
        Value::List(items) => items
            .iter()
            .map(|item| header_string(exchange, item))
            .collect(),
        _ => Vec::new(),
    }
}

/// Project the `FILE_NAME` header record into document attributes.
///
/// Every mapped name is emitted, even for an empty field, so an empty
/// foreign document property pairs with the re-decode. The first record
/// wins when the header repeats the name. A record with the wrong arity
/// projects nothing; an undecodable value projects as empty.
pub(super) fn project_file_name_attributes(
    exchange: &Exchange,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let Some(record) = exchange
        .header()
        .iter()
        .find(|record| record.name == "FILE_NAME")
    else {
        return Ok(());
    };
    let [name, timestamp, authors, organizations, _preprocessor, system, _authorization] =
        record.parameters.as_slice()
    else {
        return Ok(());
    };
    let name = header_string(exchange, name);
    let timestamp = header_string(exchange, timestamp);
    let authors = header_list(exchange, authors);
    let organizations = header_list(exchange, organizations);
    let system = match system {
        Value::Omitted => String::new(),
        value => header_string(exchange, value),
    };
    let author_first = authors.first().cloned().unwrap_or_default();
    // A lone author is both creator and modifier.
    let author_second = authors
        .get(1)
        .cloned()
        .unwrap_or_else(|| author_first.clone());
    let organization_first = organizations.first().cloned().unwrap_or_default();
    let entries: [(&str, &str, cadmpeg_ir::ids::IdentityKey); 11] = [
        ("Label", name.as_str(), key_word!("label")),
        ("author", author_first.as_str(), key_word!("author")),
        ("CreatedBy", author_first.as_str(), key_word!("created-by")),
        (
            "LastModifiedBy",
            author_second.as_str(),
            key_word!("last-modified-by"),
        ),
        (
            "organization",
            organization_first.as_str(),
            key_word!("organization"),
        ),
        ("Company", organization_first.as_str(), key_word!("company")),
        (
            "generation_date",
            timestamp.as_str(),
            key_word!("generation-date"),
        ),
        ("model_date", timestamp.as_str(), key_word!("model-date")),
        (
            "CreationDate",
            timestamp.as_str(),
            key_word!("creation-date"),
        ),
        (
            "LastModifiedDate",
            timestamp.as_str(),
            key_word!("last-modified-date"),
        ),
        ("system_id", system.as_str(), key_word!("system-id")),
    ];
    for (attribute_name, value, key) in entries {
        ir.model.attributes.push(SourceAttribute::new(
            ctx,
            AttributeId::from(ids::data(kind!("file-name"), key)),
            None,
            AttributeTarget::Document,
            attribute_name.to_owned(),
            vec![AttributeValue::String(value.to_owned())],
        )?);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
