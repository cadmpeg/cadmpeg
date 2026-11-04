// SPDX-License-Identifier: Apache-2.0
//! Typed projection controls read back from retained documents.
//!
//! The writer emits product facts in FreeCAD's own state (object type, name,
//! Label, Placement, Group, Visibility) and no longer writes these
//! properties; the reader below restores facts from documents that still
//! carry them.

use crate::native::PropertyRecord;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{products::Occurrence, products::ProductDefinition};

const DEFINITION: &str = "CadmpegHasProductDefinition";
const OCCURRENCE: &str = "CadmpegHasProductOccurrence";
const SOURCE: &str = "CadmpegProductSourceName";
const LABEL: &str = "CadmpegProductLabel";
const NAME: &str = "CadmpegOccurrenceName";
const VISIBLE: &str = "CadmpegOccurrenceVisible";
const LOCAL: &str = "CadmpegOccurrenceLocalPlacement";
const HAS_LINKED: &str = "CadmpegHasLinkedPrototypePlacement";
const LINKED: &str = "CadmpegLinkedPrototypePlacement";

fn property<'a>(
    records: &'a [PropertyRecord],
    owner: &str,
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let mut records = records.iter().filter(|record| {
        record.owner == owner
            && record.name == name
            && record
                .dynamic
                .as_ref()
                .is_some_and(|dynamic| dynamic.group == "CADIR")
    });
    let first = records.next();
    if records.next().is_some() {
        return Err(CodecError::malformed("duplicate CADIR product metadata"));
    }
    Ok(first)
}

fn value<'a>(record: &'a PropertyRecord, kind: &str, tag: &str) -> Result<&'a str, CodecError> {
    if record.type_name != kind || record.values().len() != 1 || record.values()[0].tag != tag {
        return Err(CodecError::malformed(
            "CADIR product metadata has an invalid scalar type",
        ));
    }
    record.values()[0]
        .attributes
        .get("value")
        .map(String::as_str)
        .ok_or_else(|| CodecError::malformed("CADIR product metadata has no scalar value"))
}

pub(crate) fn included(
    records: &[PropertyRecord],
    owner: &str,
    definition: bool,
) -> Result<Option<bool>, CodecError> {
    property(
        records,
        owner,
        if definition { DEFINITION } else { OCCURRENCE },
    )?
    .map(|record| match value(record, "App::PropertyBool", "Bool")? {
        "0" | "false" => Ok(false),
        "1" | "true" => Ok(true),
        _ => Err(CodecError::malformed(
            "CADIR product projection control is not boolean",
        )),
    })
    .transpose()
}

enum Field<T> {
    Missing,
    Present(Option<T>),
}
fn optional<T: serde::de::DeserializeOwned>(
    records: &[PropertyRecord],
    owner: &str,
    name: &str,
) -> Result<Field<T>, CodecError> {
    let Some(record) = property(records, owner, name)? else {
        return Ok(Field::Missing);
    };
    serde_json::from_str(value(record, "App::PropertyString", "String")?)
        .map(Field::Present)
        .map_err(CodecError::malformed)
}
fn read_matrix(
    records: &[PropertyRecord],
    owner: &str,
    name: &str,
) -> Result<Option<cadmpeg_ir::transform::Transform>, CodecError> {
    let Some(property) = property(records, owner, name)? else {
        return Ok(None);
    };
    if property.type_name != "App::PropertyMatrix"
        || property.values().len() != 1
        || property.values()[0].tag != "PropertyMatrix"
    {
        return Err(CodecError::malformed(
            "CADIR occurrence placement requires a matrix",
        ));
    }
    let mut rows = [[0.0; 4]; 4];
    for (row, values) in rows.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = property.values()[0]
                .attributes
                .get(&format!("a{}{}", row + 1, column + 1))
                .ok_or_else(|| {
                    CodecError::malformed("CADIR occurrence placement matrix is incomplete")
                })?
                .parse()
                .map_err(CodecError::malformed)?;
        }
    }
    if rows[3] != [0.0, 0.0, 0.0, 1.0] {
        return Err(CodecError::malformed(
            "CADIR occurrence placement is not affine",
        ));
    }
    Ok(Some(
        cadmpeg_ir::transform::Transform::affine([rows[0], rows[1], rows[2]])
            .ok_or_else(|| CodecError::malformed("CADIR occurrence placement is not finite"))?,
    ))
}

pub(crate) fn apply(
    records: &[PropertyRecord],
    definitions: &mut [ProductDefinition],
    occurrences: &mut [Occurrence],
) -> Result<(), CodecError> {
    for definition in definitions {
        let Some(owner) = definition.native_ref.as_deref() else {
            continue;
        };
        if let Field::Present(source) = optional(records, owner, SOURCE)? {
            definition.source_name = source;
        }
        if let Field::Present(label) = optional(records, owner, LABEL)? {
            definition.label = label;
        }
    }
    for occurrence in occurrences {
        let Some(owner) = occurrence.native_ref.as_deref() else {
            continue;
        };
        if let Some(transform) = read_matrix(records, owner, LOCAL)? {
            occurrence.transform = Some(transform);
        }
        if let Some(property) = property(records, owner, HAS_LINKED)? {
            occurrence.linked_prototype = match value(property, "App::PropertyBool", "Bool")? {
                "0" | "false" => None,
                "1" | "true" => Some(read_matrix(records, owner, LINKED)?.ok_or_else(|| {
                    CodecError::malformed("CADIR linked prototype placement is absent")
                })?),
                _ => {
                    return Err(CodecError::malformed(
                        "CADIR linked prototype presence is invalid",
                    ))
                }
            };
        }
        if let Field::Present(name) = optional(records, owner, NAME)? {
            occurrence.name = name;
        }
        if let Field::Present(visible) = optional(records, owner, VISIBLE)? {
            occurrence.visible = visible;
        }
    }
    Ok(())
}
