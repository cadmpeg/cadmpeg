// SPDX-License-Identifier: Apache-2.0
//! Geometric tolerance zones in canonical millimetres.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::PmiTarget;
use crate::references::InstanceTarget;
use crate::scalar::{Length, PositiveLength};

/// The form of one geometric tolerance zone and its projected extents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ToleranceZone {
    /// The zone's form.
    pub form: ToleranceZoneForm,
    /// Projected extents of this zone, in source order.
    pub projections: Vec<ToleranceProjection>,
}

/// The form of a tolerance zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ToleranceZoneForm {
    /// A zone within a cylinder or within a circle.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    CylindricalOrCircular,
    /// A zone within a sphere.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Spherical,
    /// The source's name of another form.
    Other(String),
}

impl ToleranceZoneForm {
    /// Whether an `Other` name is one of the typed forms, compared without
    /// regard to ASCII case (part 10).
    #[must_use]
    pub fn other_names_a_typed_form(&self) -> bool {
        match self {
            Self::Other(name) => ["cylindrical or circular", "spherical"]
                .iter()
                .any(|form| name.eq_ignore_ascii_case(form)),
            _ => false,
        }
    }
}

/// A projected tolerance zone and its terminating feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ToleranceProjection {
    /// Features that bound the projected zone.
    pub boundaries: Vec<InstanceTarget<PmiTarget>>,
    /// Feature at the projection end.
    pub end: InstanceTarget<PmiTarget>,
    /// Positive projection length in millimetres.
    pub length: PositiveLength,
    /// Signed projection offset in millimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_offset"
    )]
    pub offset: Option<Length>,
}

rewrite_record!(ToleranceZone, []; {form, projections});
rewrite_enum!(ToleranceZoneForm, []; {
    CylindricalOrCircular,
    Spherical,
    Other(field0),
});
rewrite_record!(ToleranceProjection, []; {boundaries, end, length, offset});

cadmpeg_core::named_optional_field!(deserialize_offset, Length, "offset");

impl ToleranceProjection {
    /// Features that bound or terminate this projection.
    pub fn targets(&self) -> impl Iterator<Item = &InstanceTarget<PmiTarget>> {
        self.boundaries.iter().chain(std::iter::once(&self.end))
    }
}

#[cfg(test)]
mod tests {
    use super::{ToleranceZone, ToleranceZoneForm};

    #[test]
    fn a_zone_form_states_its_kind_alone_or_its_source_name() {
        let zone = serde_json::json!({"form": {"kind": "spherical"}, "projections": []});
        let read = cadmpeg_core::serde_erase::json::from_value::<ToleranceZone>(zone.clone())
            .expect("a typed form");
        assert_eq!(read.form, ToleranceZoneForm::Spherical);
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(&read).expect("serialize"),
            zone
        );
        for value in [serde_json::Value::Null, serde_json::json!({})] {
            assert!(
                cadmpeg_core::serde_erase::json::from_value::<ToleranceZoneForm>(
                    serde_json::json!({"kind": "spherical", "value": value})
                )
                .is_err()
            );
        }
        let other = serde_json::json!({"kind": "other", "value": "square"});
        assert_eq!(
            cadmpeg_core::serde_erase::json::from_value::<ToleranceZoneForm>(other)
                .expect("another form"),
            ToleranceZoneForm::Other("square".into())
        );
        assert!(
            cadmpeg_core::serde_erase::json::from_value::<ToleranceZone>(
                serde_json::json!({"form": "cylindrical or circular", "projections": []})
            )
            .is_err()
        );
    }
}
