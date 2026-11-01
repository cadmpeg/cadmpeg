// SPDX-License-Identifier: Apache-2.0
//! Writes design parameters as `CadParameter` prims (spec §18.6).

use cadmpeg_ir::features::{DesignParameter, DimensionDisplay, ParameterId, ParameterValue};
use cadmpeg_ir::products::PrototypeReference;

use super::text::{quoted, real, Names};
use super::{Writer, MILLIMETRES_PER_UNIT};

/// Name of the scope that holds the parameters.
pub(super) const PARAMETERS: &str = "Parameters";

impl Writer<'_> {
    /// Writes the `Parameters` scope, after naming the prim of each
    /// parameter so that a dependency names a later one.
    pub(super) fn parameters(&mut self, names: &mut Names) {
        if self.ir.model.parameters.is_empty() {
            return;
        }
        names.reserve(PARAMETERS);
        self.prim_head("def Scope", PARAMETERS, &[]);
        self.out.open();
        let scope = self.out.path();
        let mut parameter_names = Names::default();
        for parameter in &self.ir.model.parameters {
            let name = parameter_names.unique(&parameter.name);
            self.parameter_paths
                .insert(parameter.id.clone(), format!("{scope}/{name}"));
        }
        for parameter in &self.ir.model.parameters {
            self.parameter(parameter);
        }
        self.out.close();
    }

    fn parameter(&mut self, parameter: &DesignParameter) {
        let Some(path) = self.parameter_paths.get(&parameter.id) else {
            return;
        };
        let name = path.rsplit('/').next().unwrap_or(PARAMETERS).to_owned();
        self.prim_head("def CadParameter", &name, &[]);
        self.out.open();
        let line = |text: String, out: &mut super::Emitter| out.line(&text);
        line(
            format!(
                "uniform string cad:parameter:id = {}",
                quoted(parameter.id.as_str())
            ),
            &mut self.out,
        );
        line(
            format!(
                "uniform string cad:parameter:name = {}",
                quoted(&parameter.name)
            ),
            &mut self.out,
        );
        line(
            format!(
                "uniform string cad:parameter:expression = {}",
                quoted(&parameter.expression)
            ),
            &mut self.out,
        );
        line(
            format!("uniform uint cad:parameter:ordinal = {}", parameter.ordinal),
            &mut self.out,
        );
        if let Some(value) = &parameter.value {
            line(value_line(value), &mut self.out);
        }
        if let Some(display) = parameter.display {
            let token = match display {
                DimensionDisplay::Diameter => "diameter",
                DimensionDisplay::Radius => "radius",
            };
            line(
                format!("uniform token cad:parameter:display = {}", quoted(token)),
                &mut self.out,
            );
        }
        let targets: Vec<String> = parameter
            .dependencies
            .iter()
            .filter_map(|dependency| self.parameter_paths.get(dependency))
            .map(|path| format!("<{path}>"))
            .collect();
        if !targets.is_empty() {
            line(
                format!("rel cad:parameter:dependencies = [{}]", targets.join(", ")),
                &mut self.out,
            );
        }
        if let Some(path) = parameter
            .product_definition
            .as_ref()
            .and_then(|definition| self.definition_path(definition))
        {
            line(
                format!("rel cad:parameter:definition = <{path}>"),
                &mut self.out,
            );
        }
        if !parameter.properties.is_empty() {
            let keys: Vec<String> = parameter
                .properties
                .keys()
                .map(|key| quoted(key.as_str()))
                .collect();
            let values: Vec<String> = parameter
                .properties
                .values()
                .map(|value| quoted(value))
                .collect();
            self.out
                .array("uniform string[] cad:parameter:propertyNames", &keys, "");
            self.out
                .array("uniform string[] cad:parameter:propertyValues", &values, "");
        }
        self.out.close();
    }

    /// The prim path of the first written occurrence of `definition`.
    fn definition_path(&self, definition: &cadmpeg_ir::ids::ProductDefinitionId) -> Option<&str> {
        self.ir
            .model
            .occurrences
            .iter()
            .filter(|occurrence| {
                matches!(&occurrence.prototype, PrototypeReference::Local { definition: local } if local == definition)
            })
            .find_map(|occurrence| self.paths.get(&occurrence.id))
            .map(String::as_str)
    }

    /// The `CadParameter` prim path of `parameter`.
    pub(super) fn parameter_path(&self, parameter: &ParameterId) -> Option<&str> {
        self.parameter_paths.get(parameter).map(String::as_str)
    }
}

/// The value attribute of a parameter: a length in stage units, an angle in
/// degrees, or a scalar of its own type.
fn value_line(value: &ParameterValue) -> String {
    match value {
        ParameterValue::Length(length) => format!(
            "uniform double cad:parameter:length = {}",
            real(length.get() / MILLIMETRES_PER_UNIT)
        ),
        ParameterValue::Angle(angle) => format!(
            "uniform double cad:parameter:angle = {}",
            real(angle.get().to_degrees())
        ),
        ParameterValue::Real(value) => {
            format!("uniform double cad:parameter:real = {}", real(value.get()))
        }
        ParameterValue::Integer(value) => format!("uniform int64 cad:parameter:integer = {value}"),
        ParameterValue::Boolean(value) => {
            format!("uniform bool cad:parameter:boolean = {}", u8::from(*value))
        }
        ParameterValue::String(value) => {
            format!("uniform string cad:parameter:string = {}", quoted(value))
        }
    }
}
