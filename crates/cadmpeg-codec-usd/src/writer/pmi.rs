// SPDX-License-Identifier: Apache-2.0
//! Writes product-manufacturing information as `CadPmi` prims (spec §18.9).

use cadmpeg_ir::pmi::PmiAnnotation;

use super::text::{quoted, Names};
use super::Writer;

/// Name of the scope that holds the annotations.
pub(super) const PMI: &str = "Pmi";

impl Writer<'_> {
    /// Writes the `Pmi` scope.
    pub(super) fn pmi(&mut self, names: &mut Names) {
        if self.ir.model.pmi.is_empty() {
            return;
        }
        names.reserve(PMI);
        self.prim_head("def Scope", PMI, &[]);
        self.out.open();
        let mut pmi_names = Names::default();
        for annotation in &self.ir.model.pmi {
            let name = pmi_names.unique(annotation.name.as_deref().unwrap_or("annotation"));
            self.annotation(annotation, &name);
        }
        self.out.close();
    }

    fn annotation(&mut self, annotation: &PmiAnnotation, name: &str) {
        let Ok(serde_json::Value::Object(mut payload)) = serde_json::to_value(annotation) else {
            self.unwritten.pmi += 1;
            return;
        };
        for field in ["id", "name", "visible"] {
            payload.remove(field);
        }
        let kind = payload
            .get("definition")
            .and_then(|definition| definition.get("kind"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        self.prim_head("def CadPmi", name, &[]);
        self.out.open();
        self.out.line(&format!(
            "uniform string cad:id = {}",
            quoted(annotation.id.as_str())
        ));
        if let Some(name) = &annotation.name {
            self.out
                .line(&format!("uniform string cad:pmi:name = {}", quoted(name)));
        }
        if let Some(visible) = annotation.visible {
            self.out.line(&format!(
                "uniform bool cad:pmi:visible = {}",
                u8::from(visible)
            ));
        }
        self.out
            .line(&format!("uniform token cad:pmi:kind = {}", quoted(&kind)));
        self.out.line(&format!(
            "uniform string cad:pmi:payload = {}",
            quoted(&serde_json::Value::Object(payload).to_string())
        ));
        self.out.close();
    }
}
