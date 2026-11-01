// SPDX-License-Identifier: Apache-2.0
//! Design parameters from `CadParameter` prims (spec §18.6).

use std::collections::BTreeMap;

use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, DistinctMembers, ParameterId, ParameterValue,
};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};

use crate::schema::{boolean, integer, token, vector};
use crate::stage::Prim;

/// The type of a `CadParameter` prim.
pub(super) const PARAMETER_TYPE: &str = "CadParameter";

impl super::Builder<'_, '_, '_, '_> {
    /// Assigns the IR identity of each `CadParameter` prim of the stage: its
    /// `cad:parameter:id` when that is valid and unused, and otherwise the
    /// identity of its path.
    pub(super) fn parameter_ids(&mut self) {
        let stage = self.stage;
        for prim in stage
            .prims
            .iter()
            .filter(|prim| prim.type_name() == Some(PARAMETER_TYPE))
        {
            let id = token(prim, "cad:parameter:id", &mut self.issues)
                .filter(|text| !self.used_ids.contains(text))
                .and_then(|text| ParameterId::mint(text).ok())
                .unwrap_or_else(|| crate::ids::sketch::parameter(&prim.path));
            self.used_ids.insert(id.to_string());
            self.parameter_ids.insert(prim.path.clone(), id);
        }
    }

    /// Adds the design parameter of each `CadParameter` prim.
    pub(super) fn parameters(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let stage = self.stage;
        for prim in stage
            .prims
            .iter()
            .filter(|prim| prim.type_name() == Some(PARAMETER_TYPE))
        {
            match self.parameter(prim) {
                Ok(parameter) => {
                    self.ir.model.parameters.push(parameter);
                    self.admit("usd_parameter")?;
                }
                Err(reason) => self
                    .tallies
                    .rejected_parameters
                    .push(format!("{}: {reason}", prim.path)),
            }
        }
        Ok(())
    }

    fn parameter(&mut self, prim: &Prim<'_>) -> Result<DesignParameter, String> {
        let id = self
            .parameter_ids
            .get(&prim.path)
            .cloned()
            .ok_or("the prim has no identity")?;
        let text = |builder: &mut Self, name: &str| token(prim, name, &mut builder.issues);
        let real = |builder: &mut Self, name: &str| {
            vector(prim, name, 1, &mut builder.issues).and_then(|value| value.first().copied())
        };
        let mut values = Vec::new();
        if let Some(value) = real(self, "cad:parameter:length") {
            values.push(
                Length::new(value * self.scale)
                    .map(ParameterValue::Length)
                    .ok_or("cad:parameter:length is not finite")?,
            );
        }
        if let Some(value) = real(self, "cad:parameter:angle") {
            values.push(
                Angle::new(value.to_radians())
                    .map(ParameterValue::Angle)
                    .ok_or("cad:parameter:angle is not finite")?,
            );
        }
        if let Some(value) = real(self, "cad:parameter:real") {
            values.push(
                FiniteReal::new(value)
                    .map(ParameterValue::Real)
                    .ok_or("cad:parameter:real is not finite")?,
            );
        }
        if let Some(value) = integer(prim, "cad:parameter:integer", &mut self.issues) {
            values.push(ParameterValue::Integer(
                i64::try_from(value).map_err(|_| "cad:parameter:integer exceeds 64 bits")?,
            ));
        }
        if let Some(value) = boolean(prim, "cad:parameter:boolean", &mut self.issues) {
            values.push(ParameterValue::Boolean(value));
        }
        if let Some(value) = text(self, "cad:parameter:string") {
            values.push(ParameterValue::String(value));
        }
        if values.len() > 1 {
            return Err("more than one value attribute is authored".to_owned());
        }
        let display = match text(self, "cad:parameter:display").as_deref() {
            None => None,
            Some("diameter") => Some(DimensionDisplay::Diameter),
            Some("radius") => Some(DimensionDisplay::Radius),
            Some(other) => return Err(format!("unknown cad:parameter:display `{other}`")),
        };
        let dependencies = prim
            .relationship_targets("cad:parameter:dependencies")
            .iter()
            .map(|target| {
                self.parameter_ids
                    .get(target)
                    .cloned()
                    .ok_or_else(|| format!("dependency <{target}> is not a CadParameter prim"))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let dependencies = DistinctMembers::try_from(dependencies)
            .map_err(|_| "a dependency is named twice".to_owned())?;
        let product_definition = match prim
            .relationship_targets("cad:parameter:definition")
            .as_slice()
        {
            [] => None,
            [target] => {
                let stage = self.stage;
                let holder = stage
                    .find(target)
                    .ok_or_else(|| format!("definition <{target}> is not a prim"))?;
                let definition = crate::ids::definition(&self.definition_key(holder));
                if !self
                    .ir
                    .model
                    .product_definitions
                    .iter()
                    .any(|candidate| candidate.id == definition)
                {
                    return Err(format!("<{target}> defines no product definition"));
                }
                Some(definition)
            }
            _ => return Err("cad:parameter:definition has more than one target".to_owned()),
        };
        let names = text_array(self, prim, "cad:parameter:propertyNames");
        let property_values = text_array(self, prim, "cad:parameter:propertyValues");
        if names.len() != property_values.len() {
            return Err("the property names and values differ in length".to_owned());
        }
        let mut properties = BTreeMap::new();
        for (name, value) in names.into_iter().zip(property_values) {
            let name =
                cadmpeg_core::text::NonBlankString::new(name).ok_or("a property name is blank")?;
            properties.insert(name, value);
        }
        Ok(DesignParameter {
            product_definition,
            id,
            owner: None,
            ordinal: integer(prim, "cad:parameter:ordinal", &mut self.issues)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0),
            name: text(self, "cad:parameter:name").unwrap_or_else(|| prim.name.clone()),
            expression: text(self, "cad:parameter:expression").unwrap_or_default(),
            display,
            value: values.pop(),
            dependencies,
            properties,
            pmi: None,
            native_ref: None,
        })
    }
}

fn text_array(
    builder: &mut super::Builder<'_, '_, '_, '_>,
    prim: &Prim<'_>,
    name: &str,
) -> Vec<String> {
    crate::schema::tokens(prim, name, &mut builder.issues).unwrap_or_default()
}
