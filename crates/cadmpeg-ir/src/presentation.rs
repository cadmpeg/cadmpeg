// SPDX-License-Identifier: Apache-2.0
//! Neutral persisted document and view presentation state.

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

crate::ids::id_type!(
    /// Stable presentation-document identity.
    PresentationId, compose
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
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub properties: BTreeMap<NonBlankString, String>,
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
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub attributes: BTreeMap<NonBlankString, String>,
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
    /// Globally unique presentation identity.
    id: PresentationId,
    /// Persisted GUI schema version.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_schema_version"
    )]
    schema_version: Option<u32>,
    /// Active view name or identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_active_view"
    )]
    active_view: Option<String>,
    /// Ordered document-level GUI states.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    states: Vec<PresentationState>,
    /// Native GUI document record supplying this state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
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
        if states.windows(2).all(Self::orders_increase) {
            self.states = states;
            return Ok(());
        }
        let mut orders = std::collections::BTreeSet::new();
        if states.iter().any(|state| !orders.insert(state.order)) {
            return Err("states must have distinct order values".into());
        }
        self.states = states;
        Ok(())
    }

    fn orders_increase(pair: &[PresentationState]) -> bool {
        pair[0].order < pair[1].order
    }

    /// Check decoded state orders with scoped uniqueness storage before assignment.
    pub fn set_states_for_decode(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        states: Vec<PresentationState>,
    ) -> Result<Result<(), String>, cadmpeg_core::CodecError> {
        if !ctx.all_by(states.windows(2), |pair| Ok(Self::orders_increase(pair)), "IR presentation state order")? {
            let mut storage = ctx.reserve_scoped(0, "IR presentation order slots")?;
            let mut orders = std::collections::BTreeSet::new();
            if ctx.any_by(&states, |state| {
                Ok(!ctx.insert_scoped_btree_set(&mut storage, &mut orders, state.order,
                    "IR presentation order lookup", "IR presentation order slots")?)
            }, "IR presentation order uniqueness")? {
                return Ok(Err(ctx.copy_retained_text("states must have distinct order values",
                    "IR presentation order refusal")?));
            }
        }
        self.states = states;
        Ok(Ok(()))
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
        let mut document = Self {
            schema_version: wire.schema_version,
            active_view: wire.active_view,
            native_ref: wire.native_ref,
            ..Self::new(wire.id)
        };
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_object"
    )]
    pub object: Option<String>,
    /// Source order in the provider table.
    pub order: u32,
    /// Persisted tree expansion state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_expanded"
    )]
    pub expanded: Option<bool>,
    /// Persisted object visibility.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// Display mode name or numeric code.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_display_mode"
    )]
    pub display_mode: Option<String>,
    /// Selection rendering mode.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_selection_style"
    )]
    pub selection_style: Option<String>,
    /// Line width in persisted display units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_line_width")]
    pub line_width: Option<crate::scalar::NonNegativeReal>,
    /// Point size in persisted display units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_point_size")]
    pub point_size: Option<crate::scalar::NonNegativeReal>,
    /// Remaining view properties by exact source property name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub properties: BTreeMap<NonBlankString, String>,
    /// Native view-provider record supplying this state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
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
        source_id: cadmpeg_core::text::NonBlankString,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_description"
    )]
    pub description: Option<String>,
    /// Explicit layer visibility; `false` means the layer is hidden.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// Assigned items in deterministic projection order; order has no semantic meaning.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<PresentationItem>,
}

cadmpeg_core::named_optional_field!(
    deserialize_position,
    crate::units::FiniteVector<3>,
    "position"
);

cadmpeg_core::named_optional_field!(
    deserialize_orientation,
    crate::units::NonzeroVector<4>,
    "orientation"
);

cadmpeg_core::named_optional_field!(
    deserialize_line_width,
    crate::scalar::NonNegativeReal,
    "line_width"
);

cadmpeg_core::named_optional_field!(
    deserialize_point_size,
    crate::scalar::NonNegativeReal,
    "point_size"
);

crate::units::named_field!(
    deserialize_source_id,
    cadmpeg_core::text::NonBlankString,
    "source_id"
);

#[cfg(test)]
mod tests {
    #[test]
    fn a_named_optional_key_states_a_value_or_is_left_out() {
        use super::ViewPresentation;

        let refusal = serde_json::from_value::<ViewPresentation>(serde_json::json!({
            "id": "freecad:presentation:view#1",
            "order": 0,
            "line_width": null
        }))
        .expect_err("null is not a spelling of an absent line width");
        let text = refusal.to_string();
        assert!(
            text.contains("line_width")
                && text.contains("this key states a value or is left out; it does not state null"),
            "{text}"
        );
        let absent = serde_json::from_value::<ViewPresentation>(serde_json::json!({
            "id": "freecad:presentation:view#1",
            "order": 0
        }))
        .expect("an absent key reaches the field default");
        assert!(absent.line_width.is_none());
    }

    #[test]
    fn camera_kinds_and_states_round_trip_without_payload_or_tag_loss() {
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
                order: u32::try_from(order).expect("test order fits u32"),
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

    use std::collections::BTreeMap;

    use super::{
        CameraState, PresentationDocument, PresentationId, PresentationItem, PresentationLayer,
        PresentationState, PresentationStateKind,
    };
    use crate::document::CadIr;
    use crate::ids::{FaceId, LayerId};
    use crate::report::check::Check;
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
                source_id: cadmpeg_core::nonblank_literal!(
                    &cadmpeg_test_support::service_decode_context(),
                    "#{}",
                    42
                )
                .unwrap(),
            }],
        });

        assert!(validate_neutral(&ir, Vec::new())
            .expect("resource allocation did not fail")
            .is_ok());
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
                source_id: cadmpeg_core::nonblank_literal!(
                    &cadmpeg_test_support::service_decode_context(),
                    "#{}",
                    42
                )
                .unwrap(),
            }],
        });

        assert!(validate_neutral(&ir, Vec::new())
            .expect("resource allocation did not fail")
            .is_ok());
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
            .expect("resource allocation did not fail")
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
        assert!(cadmpeg_core::text::NonBlankString::try_from("").is_err());
    }

    #[test]
    fn decoded_state_orders_preserve_order_and_refuse_before_assignment() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let state = |order| PresentationState { kind: PresentationStateKind::Native("View".into()),
            order, attributes: BTreeMap::new(), assets: Vec::new() };
        for orders in [vec![], vec![1], vec![1, 2, 3], vec![9, 2], vec![2, 2]] {
            let mut standard = PresentationDocument::new(PresentationId::mint("test:model:presentation#states").unwrap());
            let mut decoded = standard.clone();
            standard.set_states(vec![state(99)]).unwrap();
            decoded.set_states(vec![state(99)]).unwrap();
            let states: Vec<_> = orders.into_iter().map(state).collect();
            let expected = standard.set_states(states.clone());
            let ctx = cadmpeg_test_support::service_decode_context();
            assert_eq!(decoded.set_states_for_decode(&ctx, states).unwrap(), expected);
            assert_eq!(decoded, standard);
        }
        for (dimension, operation, orders) in [
            (ResourceDimension::WorkUnits, "IR presentation state order", vec![1, 2, 3]),
            (ResourceDimension::CollectionItems, "IR presentation order slots", vec![9, 2]),
            (ResourceDimension::MaterializedBytes, "IR presentation order slots", vec![9, 2]),
            (ResourceDimension::RetainedBytes, "IR presentation order refusal", vec![2, 2]),
        ] {
            cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                    ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                    _ => unreachable!(),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut document = PresentationDocument::new(PresentationId::mint("test:model:presentation#states").unwrap());
                document.set_states(vec![state(99)]).unwrap();
                let before = document.clone();
                let result = document.set_states_for_decode(&ctx, orders.iter().copied().map(state).collect());
                if let Err(CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(document, before);
                    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
                }
                result
            });
        }
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

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_schema_version, u32, "schema_version");
cadmpeg_core::named_optional_field!(deserialize_active_view, String, "active_view");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_object, String, "object");
cadmpeg_core::named_optional_field!(deserialize_expanded, bool, "expanded");
cadmpeg_core::named_optional_field!(deserialize_visible, bool, "visible");
cadmpeg_core::named_optional_field!(deserialize_display_mode, String, "display_mode");
cadmpeg_core::named_optional_field!(deserialize_selection_style, String, "selection_style");
cadmpeg_core::named_optional_field!(deserialize_description, String, "description");

mod identity_rewrite;
