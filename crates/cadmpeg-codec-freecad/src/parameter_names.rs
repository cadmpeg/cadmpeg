// SPDX-License-Identifier: Apache-2.0
//! Semantic parameter names for native selectors that have fixed syntax.

use crate::native::{PropertyBody, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub(crate) const PROPERTY: &str = "CadmpegParameterNames";

cadmpeg_core::named_optional_field!(deserialize_driving, bool, "driving");

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParameterName {
    pub(crate) selector: String,
    pub(crate) name: String,
    /// Generic scalar/cell controls have no native solver-driving flag.
    /// Sketch constraints use their existing isDriving lane instead.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_driving"
    )]
    pub(crate) driving: Option<bool>,
}

pub(crate) fn apply(ir: &mut CadIr, properties: &[PropertyRecord]) -> Result<(), CodecError> {
    for property in properties.iter().filter(|property| property.name == PROPERTY
        && matches!(&property.body, PropertyBody::Persisted { dynamic: Some(dynamic), .. } if dynamic.group == "CADIR")) {
        let mut selectors = BTreeSet::new();
        for value in property.values().iter().filter(|value| value.tag == "String") {
            let row: ParameterName = serde_json::from_str(value.attributes.get("value")
                .ok_or_else(|| CodecError::malformed("parameter-name row lacks its typed value"))?)
                .map_err(CodecError::malformed)?;
            if row.selector.is_empty() || !selectors.insert(row.selector.clone()) {
                return Err(CodecError::malformed("invalid or duplicate parameter-name selector"));
            }
            let constraint = row.selector.strip_prefix("Constraints[").and_then(|value| value.strip_suffix(']'))
                .and_then(|value| value.parse::<u32>().ok());
            let constraint_parameter = constraint.and_then(|index| {
                let source = properties.iter().find(|source| source.owner == property.owner && source.name == "Constraints")?;
                let constraint = ir.model.sketch_constraints.iter().filter(|constraint| constraint.native_ref.as_ref() == Some(&source.id)).nth(usize::try_from(index).ok()?)?;
                let ids = constraint.definition.kind().dimension_parameters().ok()?;
                (ids.len() == 1).then(|| ids[0].clone())
            });
            let cell_parameter = row.selector.strip_prefix("cells:").and_then(|address| {
                let source = properties.iter().find(|source| source.owner == property.owner && source.name == "cells")?;
                ir.model.spreadsheets.iter().flat_map(cadmpeg_ir::Spreadsheet::cells).find(|cell| {
                    cell.address.a1() == address && ir.model.parameters.iter().any(|parameter| parameter.id == cell.parameter && parameter.native_ref.as_ref() == Some(&source.id))
                }).map(|cell| cell.parameter.clone())
            });
            let selected = ir.model.parameters.iter_mut().find(|parameter| {
                if row.selector.starts_with("cells:") {
                    return cell_parameter.as_ref() == Some(&parameter.id);
                }
                if constraint.is_some() {
                    return constraint_parameter.as_ref() == Some(&parameter.id);
                }
                parameter.native_ref.as_ref().is_some_and(|native| properties.iter().any(|source| source.id == *native && source.owner == property.owner && source.name == row.selector))
            });
            if let Some(parameter) = selected {
                parameter.name = row.name;
                if let Some(driving) = row.driving {
                    if constraint.is_some() && parameter.driving != driving {
                        return Err(CodecError::malformed("parameter-driving metadata disagrees with the native sketch constraint"));
                    }
                    parameter.driving = driving;
                }
            }
        }
    }
    Ok(())
}
