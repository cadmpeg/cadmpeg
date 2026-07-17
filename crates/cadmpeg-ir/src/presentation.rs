// SPDX-License-Identifier: Apache-2.0
//! Named presentation layers and their model-item membership.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{
    BodyId, CurveId, EdgeId, FaceId, LayerId, OccurrenceId, PmiId, PointId, ProductId, SurfaceId,
    VertexId,
};

/// A model or presentation object assigned to a layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PresentationItem {
    /// Shape body.
    Body {
        /// Assigned body.
        body: BodyId,
    },
    /// Topological face.
    Face {
        /// Assigned face.
        face: FaceId,
    },
    /// Topological edge.
    Edge {
        /// Assigned edge.
        edge: EdgeId,
    },
    /// Topological vertex.
    Vertex {
        /// Assigned vertex.
        vertex: VertexId,
    },
    /// Point carrier.
    Point {
        /// Assigned point.
        point: PointId,
    },
    /// Curve carrier.
    Curve {
        /// Assigned curve.
        curve: CurveId,
    },
    /// Surface carrier.
    Surface {
        /// Assigned surface.
        surface: SurfaceId,
    },
    /// Product prototype.
    Product {
        /// Assigned product.
        product: ProductId,
    },
    /// Product occurrence.
    Occurrence {
        /// Assigned occurrence.
        occurrence: OccurrenceId,
    },
    /// PMI annotation.
    Pmi {
        /// Assigned PMI annotation.
        annotation: PmiId,
    },
    /// Tessellation identity.
    Tessellation {
        /// Assigned tessellation identity.
        tessellation: String,
    },
    /// Source item whose neutral target type is not modeled.
    Source {
        /// Stable source item identity.
        source_id: String,
    },
}

/// One named presentation layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationLayer {
    /// Stable layer identity.
    pub id: LayerId,
    /// Layer name.
    pub name: String,
    /// Optional layer description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Assigned items in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<PresentationItem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CadIr;
    use crate::report::Check;
    use crate::units::Units;
    use crate::validate;

    #[test]
    fn source_layer_items_validate_without_fabricated_geometry() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId("test:presentation:layer#construction".into()),
            name: "construction".into(),
            description: None,
            items: vec![PresentationItem::Source {
                source_id: "#42".into(),
            }],
        });

        assert!(validate(&ir, Vec::new()).is_ok());
    }

    #[test]
    fn missing_typed_layer_item_is_invalid() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId("test:presentation:layer#missing".into()),
            name: "missing".into(),
            description: None,
            items: vec![PresentationItem::Face {
                face: FaceId("test:model:face#missing".into()),
            }],
        });

        assert!(validate(&ir, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.check == Check::Presentation));
    }
}
