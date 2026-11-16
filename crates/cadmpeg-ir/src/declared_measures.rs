// SPDX-License-Identifier: Apache-2.0
//! Geometric quantities that a source states for its own bodies, so that a
//! reader can compare them with the quantities it measures on the decoded
//! geometry.
//!
//! Volumes are cubic millimetres, areas square millimetres, and points
//! millimetres, in the coordinates of the target.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::features::FinitePoint3;
use crate::ids::{BodyId, DeclaredMeasureId, ProductDefinitionId};
use crate::scalar::{FiniteReal, NonNegativeReal};

/// The bodies that a declared measure describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The source names a target that does not resolve to bodies of the
    /// model.
    Unresolved {},
}

/// One stated quantity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// The evidence class of a source-stated quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum DeclaredMeasureProvenance {
    /// A source-stated quantity without a more specific evidence class.
    #[default]
    SourceDeclaration,
    /// A source validation property that states an expected geometric value.
    ValidationProperty,
    /// A computed quantity retained by the source for later reuse.
    SavedCache,
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
    /// The evidence class of the stated quantity.
    pub provenance: DeclaredMeasureProvenance,
    /// Absolute bound on source-value representation error, in the quantity's
    /// units. For centroid quantities, this is a Euclidean-distance bound.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source_error"
    )]
    pub source_error: Option<NonNegativeReal>,
    /// Description text stored with the quantity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_label"
    )]
    pub label: Option<String>,
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
            provenance: DeclaredMeasureProvenance::SourceDeclaration,
            source_error: None,
            label: None,
            native_ref: None,
        }
    }

    /// The evidence class of this quantity.
    #[must_use]
    pub fn with_provenance(mut self, provenance: DeclaredMeasureProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    /// The bound on source-value representation error.
    #[must_use]
    pub fn with_source_error(mut self, source_error: NonNegativeReal) -> Self {
        self.source_error = Some(source_error);
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

cadmpeg_core::named_optional_field!(deserialize_source_error, NonNegativeReal, "source_error");
cadmpeg_core::named_optional_field!(deserialize_label, String, "label");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

#[cfg(test)]
mod tests {
    use super::{DeclaredMeasure, DeclaredMeasureProvenance, DeclaredQuantity, DeclaredTarget};
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
        .with_source_error(NonNegativeReal::new(0.5).expect("nonnegative error"))
    }

    #[test]
    fn source_error_round_trips_and_omits_when_absent() {
        let measure = measure();
        let json = serde_json::to_value(&measure).expect("serialize measure");
        assert_eq!(json["source_error"], 0.5);
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
        assert!(json.get("source_error").is_none());
        assert_eq!(
            serde_json::from_value::<DeclaredMeasure>(json).unwrap(),
            exact
        );
    }

    #[test]
    fn source_error_rejects_negative_values() {
        let json = serde_json::to_value(measure()).expect("serialize measure");
        let mut invalid = json;
        invalid["source_error"] = serde_json::json!(-0.5);
        assert!(serde_json::from_value::<DeclaredMeasure>(invalid).is_err());
    }

    #[test]
    fn provenance_round_trips_and_is_required_in_serialized_measures() {
        let default = measure();
        assert_eq!(
            default.provenance,
            DeclaredMeasureProvenance::SourceDeclaration
        );

        let cache = measure().with_provenance(DeclaredMeasureProvenance::SavedCache);
        let mut json = serde_json::to_value(&cache).expect("serialize cache measure");
        assert_eq!(json["provenance"], "saved_cache");
        assert_eq!(
            serde_json::from_value::<DeclaredMeasure>(json.clone()).unwrap(),
            cache
        );

        json.as_object_mut()
            .expect("measure object")
            .remove("provenance");
        assert!(serde_json::from_value::<DeclaredMeasure>(json).is_err());
    }
}
