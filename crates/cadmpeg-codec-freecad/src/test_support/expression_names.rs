// SPDX-License-Identifier: Apache-2.0
//! Strict depth comparison through the decoder's actual source symbol table.

use crate::native::{ObjectRecord, PropertyRecord};
use cadmpeg_ir::attributes::SourceAttribute;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::contract::expressions::ExpressionSourceResolver;
use std::collections::BTreeMap;
use std::ops::Range;

pub(crate) struct Resolver {
    symbols: crate::design::ExpressionBindings,
    names: BTreeMap<ParameterId, String>,
    owners: BTreeMap<String, Option<FeatureId>>,
}

pub(crate) fn resolver(ir: &CadIr) -> Result<Option<Box<dyn ExpressionSourceResolver>>, String> {
    let Some(native) = ir.native.namespace("fcstd") else {
        return Ok(None);
    };
    let objects = native
        .arena_as::<ObjectRecord>("objects")
        .map_err(|e| e.to_string())?;
    let properties = native
        .arena_as::<PropertyRecord>("properties")
        .map_err(|e| e.to_string())?;
    let mut parameters = Vec::new();
    for parameter in &ir.model.parameters {
        let Some(property) = parameter
            .native_ref
            .as_ref()
            .and_then(|id| properties.iter().find(|p| &p.id == id))
        else {
            continue;
        };
        if let Some(owner) = &parameter.owner {
            if !ir.model.features.iter().any(|feature| {
                feature.id == *owner
                    && feature.native_ref.as_deref() == Some(property.owner.as_str())
            }) {
                return Err(format!(
                    "parameter {} has the wrong source owner",
                    parameter.id
                ));
            }
        }
        let mut current = parameter.clone();
        if property.type_name == "Sketcher::PropertyConstraintList" {
            let rows = property
                .values()
                .iter()
                .filter(|v| v.tag == "Constrain")
                .collect::<Vec<_>>();
            let mut matches = Vec::new();
            for (index, _) in rows.iter().enumerate() {
                let default = format!("Constraint{}", index + 1);
                let selector = format!("Constraints[{index}]");
                let mut authored_name = default.clone();
                for metadata in properties.iter().filter(|p| {
                    p.owner == property.owner
                        && p.name == crate::parameter_names::PROPERTY
                        && p.dynamic.as_ref().is_some_and(|d| d.group == "CADIR")
                }) {
                    for value in metadata.values().iter().filter(|v| v.tag == "String") {
                        let row: crate::parameter_names::ParameterName = serde_json::from_str(
                            value
                                .attributes
                                .get("value")
                                .ok_or("parameter name lacks value")?,
                        )
                        .map_err(|e| e.to_string())?;
                        if row.selector == selector {
                            authored_name = row.name;
                        }
                    }
                }
                if authored_name == parameter.name {
                    matches.push((index, default));
                }
            }
            if matches.len() != 1 {
                continue;
            }
            let (index, name) = matches.remove(0);
            current.ordinal = u32::try_from(index).map_err(|e| e.to_string())?;
            current.name = name;
        } else if property.type_name == "Spreadsheet::PropertySheet" {
            let cells = ir
                .model
                .spreadsheets
                .iter()
                .flat_map(|s| s.cells())
                .filter(|cell| cell.parameter == parameter.id)
                .collect::<Vec<_>>();
            if cells.len() != 1 {
                continue;
            }
            let address = cells[0].address.a1();
            let rows = property
                .values()
                .iter()
                .filter(|v| {
                    v.tag == "Cell" && v.attributes.get("address").is_some_and(|a| a == &address)
                })
                .collect::<Vec<_>>();
            if rows.len() != 1 {
                continue;
            }
            current.name = rows[0]
                .attributes
                .get("alias")
                .filter(|v| !v.is_empty())
                .cloned()
                .unwrap_or(address);
        } else {
            current.name.clone_from(&property.name);
        }
        parameters.push(current);
    }
    let symbols = crate::design::expression_symbols(
        &cadmpeg_test_support::service_decode_context(),
        &parameters,
        &objects,
        &ir.model.features,
        &ir.model.sketch_constraints,
        &properties,
        &[],
    )
    .map_err(|e| e.to_string())?;
    let mut names = BTreeMap::new();
    // Sorted actual qualified symbols make the choice deterministic without reading IDs.
    let mut qualified = symbols.source_names().iter().collect::<Vec<_>>();
    qualified.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (name, id) in qualified {
        names.entry(id.clone()).or_insert_with(|| name.clone());
    }
    let owners = properties
        .iter()
        .map(|property| {
            let owner = ir
                .model
                .features
                .iter()
                .find(|feature| feature.native_ref.as_deref() == Some(property.owner.as_str()))
                .map(|f| f.id.clone());
            (property.id.clone(), owner)
        })
        .collect();
    Ok(Some(Box::new(Resolver {
        symbols,
        names,
        owners,
    })))
}

impl ExpressionSourceResolver for Resolver {
    fn source_name(&self, parameter: &ParameterId) -> Result<String, String> {
        self.names
            .get(parameter)
            .cloned()
            .ok_or_else(|| format!("no unique source name for {parameter}"))
    }
    fn reference_spans(
        &self,
        attribute: &SourceAttribute,
        text: &str,
    ) -> Result<Vec<(Range<usize>, ParameterId)>, String> {
        let property = attribute
            .native_ref
            .as_ref()
            .ok_or("expression attribute has no native property")?;
        let owner = self
            .owners
            .get(property)
            .ok_or("expression native property is absent")?;
        self.symbols.source_spans(owner.as_ref(), text)
    }
}
