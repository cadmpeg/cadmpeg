// SPDX-License-Identifier: Apache-2.0
//! Typed metadata keeps neutral order independent of XML declaration layout.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

use super::xml::Object;
use crate::native::PropertyRecord;

const FEATURE: &str = "CadmpegFeatureOrdinal";
const PARAMETERS: &str = "CadmpegParameterOrdinals";
const OCCURRENCE: &str = "CadmpegOccurrenceOrdinal";
const HAS_TAG: &str = "CadmpegHasFeatureSourceTag";
const TAG: &str = "CadmpegFeatureSourceTag";

fn scalar(object: &mut Object, name: &str, value: impl ToString + Copy) {
    object.set_projection_text(name, &value.to_string());
}

/// Emit only order and source-tag fields; the native objects hold their content.
pub(super) fn preserve(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    if ir.scope.design {
        for object in objects.iter_mut() {
            object
                .properties
                .retain(|name, _| !matches!(name.as_str(), FEATURE | PARAMETERS | HAS_TAG | TAG));
        }
    }
    for feature in &ir.model.features {
        let Some(object) = names
            .get(feature.id.as_str())
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
        else {
            continue;
        };
        scalar(object, FEATURE, feature.ordinal);
        object.set_projection_boolean(HAS_TAG, feature.source_tag.is_some());
        object.properties.remove(TAG);
        if let Some(tag) = &feature.source_tag {
            scalar(object, TAG, tag);
        }
    }
    let native_properties = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    let mut rows = BTreeMap::<String, Vec<String>>::new();
    for parameter in &ir.model.parameters {
        let owner = parameter
            .owner
            .as_ref()
            .and_then(|owner| names.get(owner.as_str()))
            .or_else(|| {
                parameter
                    .native_ref
                    .as_ref()
                    .and_then(|id| native_properties.iter().find(|property| property.id == *id))
                    .and_then(|property| names.get(&property.owner))
            })
            .cloned()
            .or_else(|| {
                objects
                    .iter()
                    .find(|object| {
                        object.kind == "App::VarSet"
                            && object.properties.contains_key(&parameter.name)
                    })
                    .map(|object| object.name.clone())
            });
        if let Some(owner) = owner {
            rows.entry(owner)
                .or_default()
                .push(format!("{}={}", parameter.name, parameter.ordinal));
        }
    }
    for (owner, rows) in rows {
        let Some(object) = objects.iter_mut().find(|object| object.name == owner) else {
            continue;
        };
        object.set_projection_texts(PARAMETERS, &rows);
    }
    for occurrence in &ir.model.occurrences {
        let Some(object) = names
            .get(occurrence.id.as_str())
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
        else {
            continue;
        };
        scalar(object, OCCURRENCE, occurrence.ordinal);
    }
    Ok(())
}

fn properties<'a>(
    records: &'a [PropertyRecord],
    owner: &str,
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let mut matching = records.iter().filter(|property| {
        property.owner == owner
            && property.name == name
            && property
                .dynamic
                .as_ref()
                .is_some_and(|dynamic| dynamic.group == "CADIR")
    });
    let first = matching.next();
    if matching.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "duplicate CADIR metadata {owner}:{name}"
        )));
    }
    Ok(first)
}

fn value<'a>(property: &'a PropertyRecord, kind: &str, tag: &str) -> Result<&'a str, CodecError> {
    if property.type_name != kind || property.values().len() != 1 || property.values()[0].tag != tag
    {
        return Err(CodecError::malformed(format_args!(
            "CADIR metadata {} has an invalid scalar type",
            property.id
        )));
    }
    property.values()[0]
        .attributes
        .get("value")
        .map(String::as_str)
        .ok_or_else(|| CodecError::malformed("CADIR metadata scalar has no value"))
}

fn ordinal(records: &[PropertyRecord], owner: &str, name: &str) -> Result<Option<u64>, CodecError> {
    properties(records, owner, name)?
        .map(|property| {
            value(property, "App::PropertyString", "String")?
                .parse()
                .map_err(CodecError::malformed)
        })
        .transpose()
}

/// Restore explicit neutral order after projection, before any content digest.
pub(crate) fn apply(records: &[PropertyRecord], ir: &mut CadIr) -> Result<(), CodecError> {
    let mut feature_owners = HashMap::new();
    for feature in &mut ir.model.features {
        let Some(owner) = &feature.native_ref else {
            continue;
        };
        feature_owners.insert(feature.id.clone(), owner.clone());
        if let Some(ordinal) = ordinal(records, owner, FEATURE)? {
            feature.ordinal = ordinal;
        }
        if let Some(property) = properties(records, owner, HAS_TAG)? {
            feature.source_tag = match value(property, "App::PropertyBool", "Bool")? {
                "0" | "false" => None,
                "1" | "true" => Some(
                    value(
                        properties(records, owner, TAG)?.ok_or_else(|| {
                            CodecError::malformed("CADIR feature source tag is absent")
                        })?,
                        "App::PropertyString",
                        "String",
                    )?
                    .to_owned(),
                ),
                _ => {
                    return Err(CodecError::malformed(
                        "CADIR feature source-tag presence is invalid",
                    ))
                }
            };
        }
    }
    let mut parameter_ordinals = HashMap::new();
    for property in records.iter().filter(|property| {
        property.name == PARAMETERS
            && property
                .dynamic
                .as_ref()
                .is_some_and(|dynamic| dynamic.group == "CADIR")
    }) {
        if property.type_name != "App::PropertyStringList" {
            return Err(CodecError::malformed(
                "CADIR parameter ordinals require a string list",
            ));
        }
        let mut rows = property.values().iter();
        let root = rows
            .next()
            .filter(|value| value.tag == "StringList")
            .ok_or_else(|| {
                CodecError::malformed("CADIR parameter ordinals lack a string-list root")
            })?;
        let count = root
            .attributes
            .get("count")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| CodecError::malformed("CADIR parameter ordinal count is invalid"))?;
        if property.values().len().checked_sub(1) != Some(count) {
            return Err(CodecError::malformed(
                "CADIR parameter ordinal count disagrees",
            ));
        }
        for row in rows {
            if row.tag != "String" || row.parent != Some(0) {
                return Err(CodecError::malformed(
                    "CADIR parameter ordinal entry is invalid",
                ));
            }
            let (name, ordinal) = row
                .attributes
                .get("value")
                .and_then(|value| value.rsplit_once('='))
                .ok_or_else(|| CodecError::malformed("CADIR parameter ordinal lacks its name"))?;
            let ordinal = ordinal.parse::<u32>().map_err(CodecError::malformed)?;
            if parameter_ordinals
                .insert((property.owner.as_str(), name), ordinal)
                .is_some()
            {
                return Err(CodecError::malformed("duplicate CADIR parameter ordinal"));
            }
        }
    }
    for parameter in &mut ir.model.parameters {
        let owner = parameter
            .native_ref
            .as_ref()
            .and_then(|id| records.iter().find(|record| record.id == *id))
            .map(|record| record.owner.as_str())
            .or_else(|| {
                parameter
                    .owner
                    .as_ref()
                    .and_then(|owner| feature_owners.get(owner))
                    .map(String::as_str)
            });
        if let Some(ordinal) =
            owner.and_then(|owner| parameter_ordinals.get(&(owner, parameter.name.as_str())))
        {
            parameter.ordinal = *ordinal;
        }
    }
    for occurrence in &mut ir.model.occurrences {
        if let Some(owner) = &occurrence.native_ref {
            if let Some(ordinal) = ordinal(records, owner, OCCURRENCE)? {
                occurrence.ordinal = u32::try_from(ordinal).map_err(CodecError::malformed)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
