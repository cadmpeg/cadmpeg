// SPDX-License-Identifier: Apache-2.0
//! Neutral persisted document and view presentation state.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable presentation-document identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct PresentationId(
    #[serde(serialize_with = "crate::schema::serialize_reference_id")] pub String,
);

/// Persisted camera pose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CameraState {
    /// Camera position in document coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 3]>,
    /// Persisted Inventor axis-angle orientation as X, Y, Z, angle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<[f64; 4]>,
    /// Other camera fields retained by exact source name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// Closed set of document GUI state families.
#[derive(Debug, Clone, PartialEq)]
pub enum PresentationStateKind {
    /// Persisted camera pose.
    Camera(CameraState),
    /// Any other persisted GUI state element.
    Native(String),
}

impl PresentationStateKind {
    /// Wire spelling of this family.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Camera(_) => "Camera",
            Self::Native(kind) => kind,
        }
    }
}

impl Serialize for PresentationStateKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PresentationStateKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(if value == "Camera" {
            Self::Camera(CameraState {
                position: None,
                orientation: None,
                properties: BTreeMap::new(),
            })
        } else {
            Self::Native(value)
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for PresentationStateKind {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PresentationStateKind".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }
}

/// Ordered non-provider GUI state such as clipping or section state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PresentationState {
    /// Persisted state element family.
    pub kind: PresentationStateKind,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "PresentationDocumentWire", into = "PresentationDocumentWire")]
pub struct PresentationDocument {
    /// Globally unique presentation identity.
    pub id: PresentationId,
    /// Persisted GUI schema version.
    pub schema_version: Option<u32>,
    /// Active view name or identity.
    pub active_view: Option<String>,
    /// Ordered document-level GUI states.
    pub states: Vec<PresentationState>,
    /// Native GUI document record supplying this state.
    pub native_ref: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PresentationDocumentWire {
    id: PresentationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    camera: Option<CameraState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    states: Vec<PresentationState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_ref: Option<String>,
}

impl PresentationDocument {
    /// Persisted active camera, when a Camera state is present.
    #[must_use]
    pub fn camera(&self) -> Option<&CameraState> {
        self.states.iter().find_map(|state| match &state.kind {
            PresentationStateKind::Camera(camera) => Some(camera),
            PresentationStateKind::Native(_) => None,
        })
    }

    /// Mutable persisted active camera, when a Camera state is present.
    pub fn camera_mut(&mut self) -> Option<&mut CameraState> {
        self.states
            .iter_mut()
            .find_map(|state| match &mut state.kind {
                PresentationStateKind::Camera(camera) => Some(camera),
                PresentationStateKind::Native(_) => None,
            })
    }
}

impl From<PresentationDocument> for PresentationDocumentWire {
    fn from(document: PresentationDocument) -> Self {
        let camera = document.camera().cloned();
        Self {
            id: document.id,
            schema_version: document.schema_version,
            active_view: document.active_view,
            camera,
            states: document.states,
            native_ref: document.native_ref,
        }
    }
}

impl From<PresentationDocumentWire> for PresentationDocument {
    fn from(wire: PresentationDocumentWire) -> Self {
        let mut states = wire.states;
        if let Some(camera) = wire.camera {
            if let Some(state) = states
                .iter_mut()
                .find(|state| matches!(state.kind, PresentationStateKind::Camera(_)))
            {
                state.kind = PresentationStateKind::Camera(camera);
            }
        }
        Self {
            id: wire.id,
            schema_version: wire.schema_version,
            active_view: wire.active_view,
            states,
            native_ref: wire.native_ref,
        }
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for PresentationDocument {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PresentationDocument".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::PresentationDocument").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        PresentationDocumentWire::json_schema(generator)
    }
}

/// Presentation state owned by one persisted view provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    BodyId, CurveId, EdgeId, FaceId, LayerId, OccurrenceId, PmiId, PointId, ProductDefinitionId,
    SurfaceId, VertexId,
};

/// A model or presentation object assigned to a layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
        product: ProductDefinitionId,
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

/// One presentation layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PresentationLayer {
    /// Stable layer identity.
    pub id: LayerId,
    /// Layer name.
    pub name: String,
    /// Optional layer description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Explicit layer visibility; `false` means the layer is hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Assigned items in deterministic projection order; order has no semantic meaning.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<PresentationItem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CadIr;
    use crate::report::Check;
    use crate::validate::validate_neutral;

    #[test]
    fn source_layer_items_validate_without_fabricated_geometry() {
        let mut ir = CadIr::empty();
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId("test:presentation:layer#construction".into()),
            name: "construction".into(),
            description: None,
            visible: None,
            items: vec![PresentationItem::Source {
                source_id: "#42".into(),
            }],
        });

        assert!(validate_neutral(&ir, Vec::new()).is_ok());
    }

    #[test]
    fn empty_layer_name_is_valid() {
        let mut ir = CadIr::empty();
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId("test:presentation:layer#unnamed".into()),
            name: String::new(),
            description: None,
            visible: None,
            items: vec![PresentationItem::Source {
                source_id: "#42".into(),
            }],
        });

        assert!(validate_neutral(&ir, Vec::new()).is_ok());
    }

    #[test]
    fn missing_typed_layer_item_is_invalid() {
        let mut ir = CadIr::empty();
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId("test:presentation:layer#missing".into()),
            name: "missing".into(),
            description: None,
            visible: None,
            items: vec![PresentationItem::Face {
                face: FaceId("test:model:face#missing".into()),
            }],
        });

        assert!(validate_neutral(&ir, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.check == Check::Presentation));
    }
}
