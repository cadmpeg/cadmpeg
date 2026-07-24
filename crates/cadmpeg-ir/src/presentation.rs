// SPDX-License-Identifier: Apache-2.0
//! Neutral persisted document and view presentation state.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable presentation-document identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct PresentationId(pub String);

/// Persisted camera pose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraState {
    /// Camera position in document coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 3]>,
    /// Persisted orientation quaternion in source component order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<[f64; 4]>,
    /// Other camera fields retained by exact source name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// Ordered non-provider GUI state such as clipping or section state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationState {
    /// Persisted state element name.
    pub kind: String,
    /// Source order among document GUI state elements.
    pub order: u32,
    /// Exact root attributes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    /// Referenced display assets as global native entry ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<String>,
}

/// Document-wide persisted GUI state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PresentationDocument {
    /// Globally unique presentation identity.
    pub id: PresentationId,
    /// Persisted GUI schema version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    /// Active view name or identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_view: Option<String>,
    /// Persisted active camera.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraState>,
    /// Ordered document-level GUI states.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<PresentationState>,
    /// Native GUI document record supplying this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Presentation state owned by one persisted view provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewPresentation {
    /// Globally unique view-provider identity.
    pub id: PresentationId,
    /// Owning application object identity, if resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    /// Source order in the provider table.
    pub order: u32,
    /// Persisted tree expansion state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    /// Persisted object visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Display mode name or numeric code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_mode: Option<String>,
    /// Selection rendering mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_style: Option<String>,
    /// Line width in persisted display units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_width: Option<f64>,
    /// Point size in persisted display units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_size: Option<f64>,
    /// Remaining view properties by exact source property name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    /// Native view-provider record supplying this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

// Named presentation layers and their model-item membership.

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
    use crate::units::Units;
    use crate::validate::validate;
    use crate::validate::Check;

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
