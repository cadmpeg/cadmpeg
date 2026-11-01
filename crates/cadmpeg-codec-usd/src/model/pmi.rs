// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information from `CadPmi` prims (spec §18.9).

use std::collections::BTreeSet;

use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{PmiAnnotation, PmiTarget};

use crate::schema::{boolean, token};
use crate::stage::Prim;

/// The type of a `CadPmi` prim.
pub(super) const PMI_TYPE: &str = "CadPmi";

impl super::Builder<'_, '_, '_, '_> {
    /// Adds the annotation of each `CadPmi` prim, in stage order.
    pub(super) fn pmi(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let stage = self.stage;
        for prim in stage
            .prims
            .iter()
            .filter(|prim| prim.type_name() == Some(PMI_TYPE))
        {
            match self.annotation(prim) {
                Ok(annotation) => {
                    self.ir.model.pmi.push(annotation);
                    self.admit("usd_pmi")?;
                }
                Err(reason) => self
                    .tallies
                    .rejected_pmi
                    .push(format!("{}: {reason}", prim.path)),
            }
        }
        Ok(())
    }

    fn annotation(&mut self, prim: &Prim<'_>) -> Result<PmiAnnotation, String> {
        let id = self
            .cad_id(&prim.path, |text| PmiId::mint(text).ok())
            .unwrap_or_else(|| crate::ids::pmi(&prim.path));
        let payload = token(prim, "cad:pmi:payload", &mut self.issues)
            .ok_or("cad:pmi:payload is not authored")?;
        let serde_json::Value::Object(mut wire) =
            serde_json::from_str(&payload).map_err(|error| format!("cad:pmi:payload: {error}"))?
        else {
            return Err("cad:pmi:payload is not a JSON object".to_owned());
        };
        wire.insert("id".to_owned(), id.to_string().into());
        if let Some(name) = token(prim, "cad:pmi:name", &mut self.issues) {
            wire.insert("name".to_owned(), name.into());
        }
        if let Some(visible) = boolean(prim, "cad:pmi:visible", &mut self.issues) {
            wire.insert("visible".to_owned(), visible.into());
        }
        let mut annotation: PmiAnnotation = serde_json::from_value(serde_json::Value::Object(wire))
            .map_err(|error| format!("the annotation does not deserialize: {error}"))?;
        let model = &self.ir.model;
        let held: BTreeSet<&str> = model
            .bodies
            .iter()
            .map(|record| record.id.as_str())
            .chain(model.faces.iter().map(|record| record.id.as_str()))
            .chain(model.edges.iter().map(|record| record.id.as_str()))
            .chain(model.vertices.iter().map(|record| record.id.as_str()))
            .chain(model.points.iter().map(|record| record.id.as_str()))
            .chain(model.curves.iter().map(|record| record.id.as_str()))
            .chain(
                model
                    .product_definitions
                    .iter()
                    .map(|record| record.id.as_str()),
            )
            .chain(model.occurrences.iter().map(|record| record.id.as_str()))
            .collect();
        let listed = annotation.targets.len();
        annotation.targets.retain(|target| match target {
            PmiTarget::Body { body } => held.contains(body.as_str()),
            PmiTarget::Face { face } => held.contains(face.as_str()),
            PmiTarget::Edge { edge } => held.contains(edge.as_str()),
            PmiTarget::Vertex { vertex } => held.contains(vertex.as_str()),
            PmiTarget::Point { point } => held.contains(point.as_str()),
            PmiTarget::Curve { curve } => held.contains(curve.as_str()),
            PmiTarget::Product { product } => held.contains(product.as_str()),
            PmiTarget::Occurrence { occurrence } => held.contains(occurrence.as_str()),
            PmiTarget::ShapeAspect { .. } => true,
        });
        if annotation.targets.len() < listed {
            self.tallies.pmi_properties.push(format!(
                "{}: {} targets",
                prim.path,
                listed - annotation.targets.len()
            ));
        }
        Ok(annotation)
    }
}
