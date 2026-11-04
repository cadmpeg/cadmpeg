// SPDX-License-Identifier: Apache-2.0
//! Native property selectors and canonical authored parameter names.

use super::super::xml::Object;
use crate::control_links::Measurement;
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, HashMap};

/// Allocate valid, distinct physical selectors without changing semantic names.
/// Native controls, diameter/radius conversions and arbitrary authored names
/// share the same current-state selector mechanism.
fn identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub(super) fn prepare(
    ir: &CadIr,
    objects: &[Object],
    names: &mut HashMap<String, String>,
) -> Result<(), CodecError> {
    let native_properties = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::PropertyRecord>("properties"))
        .transpose()?
        .unwrap_or_default();
    let mut controls = BTreeMap::new();
    for feature in &ir.model.features {
        controls.insert(feature.id.clone(), super::controls::bindings(feature)?);
    }
    let mut assigned = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for (index, parameter) in ir.model.parameters.iter().enumerate() {
        if ir.model.sketch_constraints.iter().any(|constraint| {
            constraint
                .definition
                .kind()
                .dimension_parameters()
                .is_ok_and(|parameters| parameters.contains(&parameter.id))
        }) {
            continue;
        }
        let valid = identifier(&parameter.name);
        // Neutral spreadsheet cells and generated sheet cells alike use
        // the cell address (or the parameter name, when it is a valid
        // alias) instead of an allocated selector.
        if let Some(address) = ir
            .model
            .spreadsheets
            .iter()
            .flat_map(cadmpeg_ir::Spreadsheet::cells)
            .find(|cell| cell.parameter == parameter.id)
            .map(|cell| cell.address.a1())
            .or_else(|| names.get(&format!("parameter-cell:{}", parameter.id)).cloned())
        {
            if !valid {
                names.insert(format!("parameter-path:{}", parameter.id), address);
            }
            continue;
        }
        let Some(owner) = parameter
            .owner
            .as_ref()
            .and_then(|owner| names.get(owner.as_str()))
            .or_else(|| names.get(&format!("parameter-owner:{}", parameter.id)))
            .cloned()
        else {
            continue;
        };
        let Some(object) = objects.iter().find(|object| object.name == owner) else {
            continue;
        };
        let control = parameter
            .owner
            .as_ref()
            .and_then(|owner| controls.get(owner))
            .and_then(|controls| controls.get(&parameter.name));
        let own_control = control.is_some_and(|bindings| {
            !bindings.is_empty()
                && bindings.iter().all(|(id, measurement)| {
                    id == parameter.id.as_str() && *measurement == Measurement::Direct
                })
        });
        let native_home = parameter.native_ref.as_ref().is_some_and(|id| {
            native_properties
                .iter()
                .any(|property| property.id == *id && property.name == parameter.name)
        });
        let occupied =
            object.properties.contains_key(&parameter.name) && !own_control && !native_home;
        let duplicate = !assigned
            .entry(owner.clone())
            .or_default()
            .insert(parameter.name.clone());
        if valid && !occupied && !duplicate && control.is_none_or(|_| own_control) {
            continue;
        }
        let mut name = format!("Parameter{index}");
        while object.properties.contains_key(&name)
            || ir
                .model
                .parameters
                .iter()
                .any(|parameter| parameter.name == name)
            || assigned
                .get(&owner)
                .is_some_and(|assigned| assigned.contains(&name))
        {
            name.push('_');
        }
        assigned.entry(owner).or_default().insert(name.clone());
        names.insert(format!("parameter-path:{}", parameter.id), name);
    }
    Ok(())
}
