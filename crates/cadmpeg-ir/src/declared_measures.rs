// SPDX-License-Identifier: Apache-2.0
//! Geometric quantities that a source states for its own bodies, so that a
//! reader can compare them with the quantities it measures on the decoded
//! geometry.
//!
//! Volumes are cubic millimetres, areas square millimetres, and points
//! millimetres, in the coordinates of the target.

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::refs::StateRef;
use crate::features::geometry::FinitePoint3;
use crate::ids::{BodyId, DeclaredMeasureId, ProductDefinitionId};
use crate::scalar::{FiniteReal, NonNegativeReal};

/// The bodies that a declared measure describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeclaredTarget {
    /// The bodies that a definition lists, in the definition's coordinates.
    /// A solid quantity covers its solid bodies and a sheet quantity its
    /// sheet bodies; a body whose `visible` is `false` is not covered.
    Definition {
        /// The definition.
        definition: ProductDefinitionId,
    },
    /// The listed bodies together, each in its own coordinates.
    Bodies {
        /// The bodies.
        bodies: Vec<BodyId>,
    },
    /// The bodies of a state of the design, whether or not that state's
    /// geometry is stored.
    State(StateRef),
    /// The source names a target that does not resolve to bodies of the
    /// model.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Unresolved,
}

/// One stated quantity.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeclaredQuantity {
    /// The enclosed volume of the solids, in cubic millimetres.
    Volume {
        /// The volume.
        value: FiniteReal,
    },
    /// The area of every face of the solids, void shells included, in
    /// square millimetres.
    SolidArea {
        /// The area.
        value: FiniteReal,
    },
    /// The area of the faces of the outer shells of the solids, in square
    /// millimetres.
    WettedArea {
        /// The area.
        value: FiniteReal,
    },
    /// The centroid of the volume of the solids, in millimetres.
    SolidCentroid {
        /// The centroid.
        value: FinitePoint3,
    },
    /// The area of the faces of the sheets, in square millimetres.
    SheetArea {
        /// The area.
        value: FiniteReal,
    },
    /// The centroid of the area of the sheets, in millimetres.
    SheetCentroid {
        /// The centroid.
        value: FinitePoint3,
    },
}

/// Whether a stated value can predate the bodies it describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum MeasureBasis {
    /// A value written with the bodies for a reader to check a transfer
    /// against.
    Validation,
    /// The source's own last computation, which an edit after it leaves
    /// stale.
    Cache,
}

/// A quantity that the source states for a target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeclaredMeasure {
    /// Globally unique record identity.
    pub id: DeclaredMeasureId,
    /// The described bodies.
    pub target: DeclaredTarget,
    /// The stated quantity.
    pub quantity: DeclaredQuantity,
    /// Description text stored with the quantity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_label"
    )]
    pub label: Option<String>,
    /// Whether the value can predate the bodies; `None` states neither.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_basis"
    )]
    pub basis: Option<MeasureBasis>,
    /// The bound within which the stored value holds the value the source
    /// computed, in the quantity's unit and for each coordinate of a
    /// centroid.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_precision"
    )]
    pub precision: Option<NonNegativeReal>,
    /// Format-native record supplying the quantity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

impl DeclaredMeasure {
    /// A stated quantity for one or more bodies.
    #[must_use]
    pub fn new(id: DeclaredMeasureId, target: DeclaredTarget, quantity: DeclaredQuantity) -> Self {
        Self {
            id,
            target,
            quantity,
            label: None,
            basis: None,
            precision: None,
            native_ref: None,
        }
    }

    /// Whether the value can predate the bodies.
    #[must_use]
    pub fn with_basis(mut self, basis: MeasureBasis) -> Self {
        self.basis = Some(basis);
        self
    }

    /// The bound within which the stored value holds the computed value.
    #[must_use]
    pub fn with_precision(mut self, precision: NonNegativeReal) -> Self {
        self.precision = Some(precision);
        self
    }

    /// The source description of this quantity.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The native record that supplies this quantity.
    #[must_use]
    pub fn with_native_ref(mut self, native_ref: impl Into<String>) -> Self {
        self.native_ref = Some(native_ref.into());
        self
    }
}

mod identity_rewrite;

cadmpeg_core::named_optional_field!(deserialize_basis, MeasureBasis, "basis");
cadmpeg_core::named_optional_field!(deserialize_precision, NonNegativeReal, "precision");
cadmpeg_core::named_optional_field!(deserialize_label, String, "label");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

#[cfg(test)]
mod tests {
    use super::{DeclaredMeasure, DeclaredQuantity, DeclaredTarget, MeasureBasis};
    use crate::ids::{BodyId, DeclaredMeasureId};
    use crate::scalar::{FiniteReal, NonNegativeReal};

    fn measure() -> DeclaredMeasure {
        DeclaredMeasure::new(
            DeclaredMeasureId::mint("test:model:declared_measure#volume").expect("id"),
            DeclaredTarget::Bodies {
                bodies: vec![BodyId::mint("test:model:body#0").expect("body id")],
            },
            DeclaredQuantity::Volume {
                value: FiniteReal::new(10.0).expect("finite value"),
            },
        )
        .with_precision(NonNegativeReal::new(0.5).expect("nonnegative precision"))
    }

    #[test]
    fn precision_round_trips_and_omits_when_absent() {
        let measure = measure();
        let json = serde_json::to_value(&measure).expect("serialize measure");
        assert_eq!(json["precision"], 0.5);
        assert_eq!(
            serde_json::from_value::<DeclaredMeasure>(json).unwrap(),
            measure
        );

        let exact = DeclaredMeasure::new(
            measure.id.clone(),
            measure.target.clone(),
            measure.quantity.clone(),
        );
        let json = serde_json::to_value(&exact).expect("serialize exact measure");
        assert!(json.get("precision").is_none());
        assert_eq!(
            serde_json::from_value::<DeclaredMeasure>(json).unwrap(),
            exact
        );
    }

    #[test]
    fn a_target_is_a_state_or_a_bare_unresolved_tag() {
        let state = serde_json::json!({
            "kind": "state",
            "of": {"kind": "tool", "value": "test:design:feature#3"}
        });
        let target = serde_json::from_value::<DeclaredTarget>(state.clone()).unwrap();
        assert!(matches!(target, DeclaredTarget::State(_)));
        assert_eq!(serde_json::to_value(&target).unwrap(), state);
        let unresolved = serde_json::json!({"kind": "unresolved"});
        assert_eq!(
            serde_json::from_value::<DeclaredTarget>(unresolved.clone()).unwrap(),
            DeclaredTarget::Unresolved
        );
        assert_eq!(
            serde_json::to_value(DeclaredTarget::Unresolved).unwrap(),
            unresolved
        );
        for wire in [
            serde_json::json!({"kind": "unresolved", "value": null}),
            serde_json::json!({"kind": "unresolved", "bodies": []}),
            serde_json::json!({"kind": "state", "value": {"of": {"kind": "tool", "value": "test:design:feature#3"}}}),
            serde_json::json!({"kind": "state", "configuration": null, "of": {"kind": "tool", "value": "test:design:feature#3"}}),
        ] {
            assert!(
                serde_json::from_value::<DeclaredTarget>(wire.clone()).is_err(),
                "{wire}"
            );
        }
    }

    #[test]
    fn precision_rejects_negative_values() {
        let json = serde_json::to_value(measure()).expect("serialize measure");
        let mut invalid = json;
        invalid["precision"] = serde_json::json!(-0.5);
        assert!(serde_json::from_value::<DeclaredMeasure>(invalid).is_err());
    }

    #[test]
    fn basis_round_trips_in_snake_case_and_omits_when_absent() {
        let unstated = serde_json::to_value(measure()).expect("serialize measure");
        assert!(unstated.get("basis").is_none());
        for (basis, wire) in [
            (MeasureBasis::Validation, "validation"),
            (MeasureBasis::Cache, "cache"),
        ] {
            let stated = measure().with_basis(basis);
            let json = serde_json::to_value(&stated).expect("serialize measure");
            assert_eq!(json["basis"], wire);
            assert_eq!(
                serde_json::from_value::<DeclaredMeasure>(json).unwrap(),
                stated
            );
        }
        let mut unknown = unstated;
        unknown["basis"] = serde_json::json!("saved_cache");
        assert!(serde_json::from_value::<DeclaredMeasure>(unknown).is_err());
    }
}
