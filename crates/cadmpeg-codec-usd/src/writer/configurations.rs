// SPDX-License-Identifier: Apache-2.0
//! Writes design configurations as `CadConfiguration` prims (spec §18.8).

use cadmpeg_ir::features::DesignConfiguration;

use super::text::{quoted, Names};
use super::Writer;

/// Name of the scope that holds the configurations.
pub(super) const CONFIGURATIONS: &str = "Configurations";

/// Fields of the wire form of a configuration that typed attributes state.
const TYPED_FIELDS: &[&str] = &[
    "id",
    "ordinal",
    "active",
    "source_index",
    "name",
    "material",
    "properties",
    "native_ref",
];

impl Writer<'_> {
    /// Writes the `Configurations` scope.
    pub(super) fn configurations(&mut self, names: &mut Names) {
        if self.ir.model.configurations.is_empty() {
            return;
        }
        names.reserve(CONFIGURATIONS);
        self.prim_head("def Scope", CONFIGURATIONS, &[]);
        self.out.open();
        let mut configuration_names = Names::default();
        for configuration in &self.ir.model.configurations {
            let name = configuration_names
                .unique(configuration.name.as_deref().unwrap_or("configuration"));
            self.configuration(configuration, &name);
        }
        self.out.close();
    }

    fn configuration(&mut self, configuration: &DesignConfiguration, name: &str) {
        let Ok(serde_json::Value::Object(mut payload)) = serde_json::to_value(configuration) else {
            self.unwritten.configurations += 1;
            return;
        };
        for field in TYPED_FIELDS {
            payload.remove(*field);
        }
        self.prim_head("def CadConfiguration", name, &[]);
        self.out.open();
        let mut lines = vec![
            format!(
                "uniform string cad:id = {}",
                quoted(configuration.id.as_str())
            ),
            format!(
                "uniform uint cad:configuration:ordinal = {}",
                configuration.ordinal
            ),
        ];
        if configuration.active {
            lines.push("uniform bool cad:configuration:active = 1".to_owned());
        }
        if let Some(index) = configuration.source_index {
            lines.push(format!(
                "uniform uint cad:configuration:sourceIndex = {index}"
            ));
        }
        if let Some(name) = &configuration.name {
            lines.push(format!(
                "uniform string cad:configuration:name = {}",
                quoted(name)
            ));
        }
        if let Some(material) = &configuration.material {
            lines.push(format!(
                "uniform string cad:configuration:material = {}",
                quoted(material)
            ));
        }
        for line in lines {
            self.out.line(&line);
        }
        if !configuration.properties.is_empty() {
            let keys: Vec<String> = configuration
                .properties
                .keys()
                .map(|key| quoted(key.as_str()))
                .collect();
            let values: Vec<String> = configuration
                .properties
                .values()
                .map(|value| quoted(value))
                .collect();
            self.out.array(
                "uniform string[] cad:configuration:propertyNames",
                &keys,
                "",
            );
            self.out.array(
                "uniform string[] cad:configuration:propertyValues",
                &values,
                "",
            );
        }
        if !payload.is_empty() {
            self.out.line(&format!(
                "uniform string cad:configuration:payload = {}",
                quoted(&serde_json::Value::Object(payload).to_string())
            ));
        }
        self.out.close();
    }
}
