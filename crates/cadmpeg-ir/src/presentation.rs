// SPDX-License-Identifier: Apache-2.0
//! Neutral persisted document and view presentation state.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

crate::ids::id_type!(
    /// Stable presentation-document identity.
    PresentationId
);

/// Persisted camera pose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CameraState {
    /// Camera position in document coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_position")]
    pub position: Option<crate::units::FiniteVector<3>>,
    /// Persisted Inventor axis-angle orientation as X, Y, Z, angle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_orientation")]
    pub orientation: Option<crate::units::NonzeroVector<4>>,
    /// Other camera fields retained by exact source name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

/// Closed set of document GUI state families.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", content = "value")]
#[serde(deny_unknown_fields)]
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

/// Ordered non-provider GUI state such as clipping or section state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
#[serde(
    try_from = "PresentationDocumentWire",
    into = "PresentationDocumentWire"
)]
pub struct PresentationDocument {
    /// Globally unique presentation identity.
    pub id: PresentationId,
    /// Persisted GUI schema version.
    pub schema_version: Option<u32>,
    /// Active view name or identity.
    pub active_view: Option<String>,
    /// Ordered document-level GUI states.
    states: Vec<PresentationState>,
    /// Native GUI document record supplying this state.
    pub native_ref: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PresentationDocumentWire {
    id: PresentationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_view: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    states: Vec<PresentationState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_ref: Option<String>,
}

impl PresentationDocument {
    /// Construct document presentation with no persisted states.
    #[must_use]
    pub fn new(id: PresentationId) -> Self {
        Self {
            id,
            schema_version: None,
            active_view: None,
            states: Vec::new(),
            native_ref: None,
        }
    }

    /// Return the persisted states in source order.
    #[must_use]
    pub fn states(&self) -> &[PresentationState] {
        &self.states
    }

    /// Replace persisted states after checking that their orders are distinct.
    pub fn set_states(&mut self, states: Vec<PresentationState>) -> Result<(), String> {
        let mut orders = std::collections::HashSet::new();
        if states.iter().any(|state| !orders.insert(state.order)) {
            return Err("states must have distinct order values".into());
        }
        self.states = states;
        Ok(())
    }

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
        Self {
            id: document.id,
            schema_version: document.schema_version,
            active_view: document.active_view,
            states: document.states,
            native_ref: document.native_ref,
        }
    }
}

impl TryFrom<PresentationDocumentWire> for PresentationDocument {
    type Error = String;

    fn try_from(wire: PresentationDocumentWire) -> Result<Self, Self::Error> {
        let mut document = Self::new(wire.id);
        document.schema_version = wire.schema_version;
        document.active_view = wire.active_view;
        document.native_ref = wire.native_ref;
        document.set_states(wire.states)?;
        Ok(document)
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
#[serde(deny_unknown_fields)]
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
    #[serde(deserialize_with = "deserialize_line_width")]
    pub line_width: Option<crate::units::NonNegativeScalar>,
    /// Point size in persisted display units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_point_size")]
    pub point_size: Option<crate::units::NonNegativeScalar>,
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
#[serde(deny_unknown_fields)]
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
        #[serde(deserialize_with = "deserialize_source_id")]
        source_id: crate::products::NonEmptyString,
    },
}

/// One presentation layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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

crate::units::named_field!(
    deserialize_position,
    Option<crate::units::FiniteVector<3>>,
    "position"
);

crate::units::named_field!(
    deserialize_orientation,
    Option<crate::units::NonzeroVector<4>>,
    "orientation"
);

crate::units::named_field!(
    deserialize_line_width,
    Option<crate::units::NonNegativeScalar>,
    "line_width"
);

crate::units::named_field!(
    deserialize_point_size,
    Option<crate::units::NonNegativeScalar>,
    "point_size"
);

crate::units::named_field!(
    deserialize_source_id,
    crate::products::NonEmptyString,
    "source_id"
);

#[cfg(test)]
mod tests {
    #[test]
    fn camera_kinds_and_states_round_trip_without_payload_or_tag_loss() {
        use super::*;
        let kinds = [
            PresentationStateKind::Camera(CameraState {
                position: Some(
                    crate::units::FiniteVector::new([1.0, 2.0, 3.0]).expect("finite position"),
                ),
                orientation: None,
                properties: BTreeMap::new(),
            }),
            PresentationStateKind::Camera(CameraState {
                position: Some(
                    crate::units::FiniteVector::new([4.0, 5.0, 6.0]).expect("finite position"),
                ),
                orientation: None,
                properties: BTreeMap::new(),
            }),
            PresentationStateKind::Native("Camera".to_owned()),
        ];
        let mut states = Vec::new();
        for (order, kind) in kinds.into_iter().enumerate() {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(
                serde_json::from_str::<PresentationStateKind>(&json).unwrap(),
                kind
            );
            let state = PresentationState {
                kind,
                order: order as u32,
                attributes: BTreeMap::new(),
                assets: Vec::new(),
            };
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(
                serde_json::from_str::<PresentationState>(&json).unwrap(),
                state
            );
            states.push(state);
        }
        let mut document = PresentationDocument::new(
            PresentationId::mint("synthetic:test:presentation#presentation").unwrap(),
        );
        document.set_states(states).expect("distinct orders");
        let json = serde_json::to_string(&document).unwrap();
        assert_eq!(
            serde_json::from_str::<PresentationDocument>(&json).unwrap(),
            document
        );
    }

    use super::*;
    use crate::document::CadIr;
    use crate::report::Check;
    use crate::validate::validate_neutral;

    #[test]
    fn source_layer_items_validate_without_fabricated_geometry() {
        let mut ir = CadIr::empty();
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId::mint("test:presentation:layer#construction").expect("valid identity"),
            name: "construction".into(),
            description: None,
            visible: None,
            items: vec![PresentationItem::Source {
                source_id: crate::products::NonEmptyString::prefixed('#', 42),
            }],
        });

        assert!(validate_neutral(&ir, Vec::new()).is_ok());
    }

    #[test]
    fn empty_layer_name_is_valid() {
        let mut ir = CadIr::empty();
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId::mint("test:presentation:layer#unnamed").expect("valid identity"),
            name: String::new(),
            description: None,
            visible: None,
            items: vec![PresentationItem::Source {
                source_id: crate::products::NonEmptyString::prefixed('#', 42),
            }],
        });

        assert!(validate_neutral(&ir, Vec::new()).is_ok());
    }

    #[test]
    fn missing_typed_layer_item_is_invalid() {
        let mut ir = CadIr::empty();
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId::mint("test:presentation:layer#missing").expect("valid identity"),
            name: "missing".into(),
            description: None,
            visible: None,
            items: vec![PresentationItem::Face {
                face: FaceId::mint("test:model:face#missing").expect("valid identity"),
            }],
        });

        assert!(validate_neutral(&ir, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.check == Check::Presentation));
    }

    #[test]
    fn source_identity_admission_preserves_nonempty_wire() {
        let wire = serde_json::json!({"kind": "source", "source_id": "#42"});
        let value: PresentationItem =
            serde_json::from_value(wire.clone()).expect("nonempty source_id");
        assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
        let error = serde_json::from_value::<PresentationItem>(
            serde_json::json!({"kind": "source", "source_id": ""}),
        )
        .expect_err("empty source_id");
        assert!(error.to_string().contains("source_id"));
        assert!(crate::products::NonEmptyString::new("").is_none());
    }

    #[test]
    fn document_state_orders_are_checked_without_reordering_or_partial_updates() {
        let state = |order| PresentationState {
            kind: PresentationStateKind::Native("View".into()),
            order,
            attributes: BTreeMap::new(),
            assets: Vec::new(),
        };
        let mut document = PresentationDocument::new(
            PresentationId::mint("synthetic:test:presentation#presentation")
                .expect("valid identity"),
        );
        assert!(document.states().is_empty());
        let states = vec![state(9), state(2)];
        document
            .set_states(states.clone())
            .expect("distinct orders");
        assert_eq!(document.states(), states);
        let wire = serde_json::to_value(&document).expect("serialize");
        assert_eq!(
            serde_json::from_value::<PresentationDocument>(wire.clone()).expect("valid wire"),
            document
        );
        assert!(document.set_states(vec![state(2), state(2)]).is_err());
        assert_eq!(document.states(), states);
        let mut invalid = wire;
        invalid["states"] =
            serde_json::to_value(vec![state(2), state(2)]).expect("serialize states");
        let error = serde_json::from_value::<PresentationDocument>(invalid)
            .expect_err("duplicate state order");
        assert!(error.to_string().contains("states"));
        document.set_states(Vec::new()).expect("empty states");
        assert!(document.states().is_empty());
    }
}
