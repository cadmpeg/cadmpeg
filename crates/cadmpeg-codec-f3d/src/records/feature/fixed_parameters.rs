// SPDX-License-Identifier: Apache-2.0
//! Fixed extrude, fillet and chamfer parameter payloads.

use crate::records::serde_column::SliceColumn;
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal, PositiveReal};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_along_distance,
    DesignFixedExtrudeDistance,
    "along_distance"
);
cadmpeg_core::named_optional_field!(
    deserialize_tangency_weight,
    DesignFixedFilletScalar,
    "tangency_weight"
);
cadmpeg_core::named_optional_field!(
    deserialize_taper_angle,
    DesignFixedExtrudeScalar,
    "taper_angle"
);
cadmpeg_core::named_optional_field!(
    deserialize_against_distance,
    DesignFixedExtrudeScalar<NonZeroReal>,
    "against_distance"
);
cadmpeg_core::named_optional_field!(
    deserialize_side_two_taper_angle,
    DesignFixedExtrudeScalar,
    "side_two_taper_angle"
);
cadmpeg_core::named_optional_field!(
    deserialize_profile_offset,
    DesignFixedExtrudeScalar,
    "profile_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_side_one_offset,
    DesignFixedExtrudeScalar,
    "side_one_offset"
);
/// One exact scalar carrier used by an Extrude scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignFixedExtrudeScalar<T = FiniteReal> {
    /// Scalar value in source centimetres for a distance or radians for an angle.
    pub(crate) value: T,
    /// Referenced record carrying the scalar.
    pub(crate) record_index: u32,
    /// Byte offset of the scalar of a located scope.
    #[serde(skip)]
    pub(crate) value_offset: Option<u64>,
}

/// Exact carrier of an Extrude's one-sided distance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "carrier", content = "scalar", rename_all = "snake_case")]
pub(crate) enum DesignFixedExtrudeDistance {
    /// Signed distance in an owner-local scalar lane.
    FixedScalar(DesignFixedExtrudeScalar<NonZeroReal>),
    /// Positive magnitude in an owned distance-construction frame.
    DistanceConstruction(DesignFixedExtrudeScalar<PositiveReal>),
}

#[cfg(test)]
mod tests {
    use super::{
        DesignFixedExtrudeDistance, DesignFixedFilletGroup, DesignFixedFilletGroupWire,
        FIXED_FILLET_GROUP_CLONE_COUNT,
    };
    use serde::Serialize;

    #[test]
    fn fixed_extrude_distance_construction_json_refuses_nonpositive_value() {
        let mut wire = serde_json::json!({
            "carrier": "distance_construction",
            "scalar": { "value": 1.0, "record_index": 1 }
        });
        assert!(serde_json::from_value::<DesignFixedExtrudeDistance>(wire.clone()).is_ok());
        wire["scalar"]["value"] = serde_json::json!(0.0);
        assert!(serde_json::from_value::<DesignFixedExtrudeDistance>(wire.clone()).is_err());
        wire["scalar"]["value"] = serde_json::json!(-1.0);
        assert!(serde_json::from_value::<DesignFixedExtrudeDistance>(wire).is_err());
    }

    #[test]
    fn fixed_extrude_scalar_json_refuses_zero_distance() {
        let mut wire = serde_json::json!({
            "carrier": "fixed_scalar",
            "scalar": { "value": -1.0, "record_index": 1 }
        });
        assert!(serde_json::from_value::<DesignFixedExtrudeDistance>(wire.clone()).is_ok());
        wire["scalar"]["value"] = serde_json::json!(0.0);
        assert!(serde_json::from_value::<DesignFixedExtrudeDistance>(wire).is_err());
    }

    fn group(variable: bool, tangency: bool) -> DesignFixedFilletGroup {
        let mut wire = if variable {
            serde_json::json!({
                "radii": [1.0, 2.0, 3.0],
                "radius_record_indexes": [10, 11, 12],
                "intermediate_parameters": [0.25],
                "intermediate_parameter_record_indexes": [20]
            })
        } else {
            serde_json::json!({
                "radii": [1.0],
                "radius_record_indexes": [10]
            })
        };
        if tangency {
            wire["tangency_weight"] = serde_json::json!({"value": 1.0, "record_index": 5});
        }
        serde_json::from_value(wire).unwrap()
    }

    #[test]
    fn fixed_fillet_group_borrowed_wire_matches_owned_wire_bytes() {
        for variable in [false, true] {
            for tangency in [false, true] {
                let group = group(variable, tangency);
                let owned = DesignFixedFilletGroupWire::from(group.clone());
                assert_eq!(
                    serde_json::to_vec(&group).unwrap(),
                    serde_json::to_vec(&owned).unwrap()
                );
            }
        }
    }

    #[test]
    fn fixed_fillet_group_native_retained_limit_refuses_before_clone() {
        #[derive(Serialize)]
        struct NestedRecord<'a> {
            id: &'static str,
            value: &'a DesignFixedFilletGroup,
        }
        let group = group(true, true);
        let record = NestedRecord {
            id: "f3d:native:fixed-fillet-group#0",
            value: &group,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || FIXED_FILLET_GROUP_CLONE_COUNT.with(|count| count.set(0)),
            || FIXED_FILLET_GROUP_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }
}

/// Exact fixed scalar lanes carried by an Extrude scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignFixedExtrudeParameters {
    /// First-side distance carrier in source centimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_along_distance"
    )]
    pub(crate) along_distance: Option<DesignFixedExtrudeDistance>,
    /// Taper-angle lane in radians.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_taper_angle"
    )]
    pub(crate) taper_angle: Option<DesignFixedExtrudeScalar>,
    /// Signed second-side distance lane of a two-sided Extrude in source
    /// centimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_against_distance"
    )]
    pub(crate) against_distance: Option<DesignFixedExtrudeScalar<NonZeroReal>>,
    /// Second-side taper-angle lane of a two-sided Extrude in radians.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_side_two_taper_angle"
    )]
    pub(crate) side_two_taper_angle: Option<DesignFixedExtrudeScalar>,
    /// Start-offset lane of an offset-start or face-start Extrude in source
    /// centimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_profile_offset"
    )]
    pub(crate) profile_offset: Option<DesignFixedExtrudeScalar>,
    /// First-side termination-offset lane of a to-face Extrude in source
    /// centimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_side_one_offset"
    )]
    pub(crate) side_one_offset: Option<DesignFixedExtrudeScalar>,
}

/// Exact fixed scalar lanes carried by a Fillet scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignFixedFilletParameters {
    /// Radius laws in scalar-lane order.
    pub(crate) groups: Vec<DesignFixedFilletGroup>,
}

/// One fillet radius law and its optional tangency weight.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignFixedFilletGroupWire")]
pub(crate) struct DesignFixedFilletGroup {
    tangency_weight: Option<DesignFixedFilletScalar>,
    law: DesignFixedFilletLaw,
}

#[cfg(test)]
thread_local! {
    static FIXED_FILLET_GROUP_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignFixedFilletGroup {
    fn clone(&self) -> Self {
        #[cfg(test)]
        FIXED_FILLET_GROUP_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            tangency_weight: self.tangency_weight.clone(),
            law: self.law.clone(),
        }
    }
}

impl DesignFixedFilletGroup {
    /// Admit a fillet radius law and optional tangency weight.
    pub(crate) fn try_new(
        tangency_weight: Option<DesignFixedFilletScalar>,
        law: DesignFixedFilletLaw,
    ) -> Result<Self, String> {
        if tangency_weight
            .as_ref()
            .is_some_and(|weight| PositiveReal::new(weight.value.get()).is_none())
        {
            return Err("tangency_weight must be positive and finite".into());
        }
        if !law.radii().all(|radius| radius.value.get() >= 0.0) {
            return Err("radii must be finite and non-negative".into());
        }
        if !law.radii().any(|radius| radius.value.get() > 0.0) {
            return Err("radii must contain a positive radius".into());
        }
        if !law
            .intermediate()
            .iter()
            .all(|row| (0.0..1.0).contains(&row.parameter.value.get()))
        {
            return Err("intermediate_parameters must be finite and in [0, 1)".into());
        }
        if !law
            .intermediate()
            .windows(2)
            .all(|rows| rows[0].parameter.value < rows[1].parameter.value)
        {
            return Err("intermediate_parameters must be strictly increasing".into());
        }
        Ok(Self {
            tangency_weight,
            law,
        })
    }

    /// The admitted radius law.
    pub(crate) fn law(&self) -> &DesignFixedFilletLaw {
        &self.law
    }

    /// The optional positive tangency weight.
    pub(crate) fn tangency_weight(&self) -> Option<&DesignFixedFilletScalar> {
        self.tangency_weight.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DesignFixedFilletLaw {
    Constant(DesignFixedFilletScalar),
    Variable {
        start: DesignFixedFilletScalar,
        end: DesignFixedFilletScalar,
        intermediate: Vec<DesignFixedFilletIntermediate>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignFixedFilletIntermediate {
    pub(crate) radius: DesignFixedFilletScalar,
    pub(crate) parameter: DesignFixedFilletScalar,
}

impl DesignFixedFilletLaw {
    pub(crate) fn radii(&self) -> impl Iterator<Item = &DesignFixedFilletScalar> {
        let (first, second, intermediate) = match self {
            Self::Constant(radius) => (radius, None, &[][..]),
            Self::Variable {
                start,
                end,
                intermediate,
            } => (start, Some(end), intermediate.as_slice()),
        };
        std::iter::once(first)
            .chain(second)
            .chain(intermediate.iter().map(|row| &row.radius))
    }

    pub(crate) fn intermediate(&self) -> &[DesignFixedFilletIntermediate] {
        match self {
            Self::Constant(_) => &[],
            Self::Variable { intermediate, .. } => intermediate,
        }
    }
}

/// One Fillet radius law carried by fixed scalar lanes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignFixedFilletGroupWire {
    /// Optional explicit dimensionless tangency-weight lane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangency_weight"
    )]
    tangency_weight: Option<DesignFixedFilletScalar>,
    /// One constant radius, or endpoint radii followed by intermediate radii,
    /// in source centimetres.
    radii: Vec<FiniteReal>,
    /// Referenced radius scalar records in semantic radius order.
    radius_record_indexes: Vec<u32>,
    /// Normalized edge-chain positions paired with the intermediate radii.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    intermediate_parameters: Vec<FiniteReal>,
    /// Referenced intermediate-position scalar records in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    intermediate_parameter_record_indexes: Vec<u32>,
}

#[derive(Clone, Copy)]
enum FilletRadiusField {
    Value,
    RecordIndex,
}

struct FilletRadiusColumn<'a> {
    law: &'a DesignFixedFilletLaw,
    field: FilletRadiusField,
}

impl Serialize for FilletRadiusColumn<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.field {
            FilletRadiusField::Value => {
                serializer.collect_seq(self.law.radii().map(|scalar| scalar.value))
            }
            FilletRadiusField::RecordIndex => {
                serializer.collect_seq(self.law.radii().map(|scalar| scalar.record_index))
            }
        }
    }
}

#[derive(Serialize)]
struct DesignFixedFilletGroupWireRef<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    tangency_weight: Option<&'a DesignFixedFilletScalar>,
    radii: FilletRadiusColumn<'a>,
    radius_record_indexes: FilletRadiusColumn<'a>,
    #[serde(skip_serializing_if = "SliceColumn::is_empty")]
    intermediate_parameters: SliceColumn<'a, DesignFixedFilletIntermediate, FiniteReal>,
    #[serde(skip_serializing_if = "SliceColumn::is_empty")]
    intermediate_parameter_record_indexes: SliceColumn<'a, DesignFixedFilletIntermediate, u32>,
}

impl Serialize for DesignFixedFilletGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let radius = |field| FilletRadiusColumn {
            law: &self.law,
            field,
        };
        let intermediate = self.law.intermediate();
        DesignFixedFilletGroupWireRef {
            tangency_weight: self.tangency_weight.as_ref(),
            radii: radius(FilletRadiusField::Value),
            radius_record_indexes: radius(FilletRadiusField::RecordIndex),
            intermediate_parameters: SliceColumn::new(intermediate, |row| row.parameter.value),
            intermediate_parameter_record_indexes: SliceColumn::new(intermediate, |row| {
                row.parameter.record_index
            }),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignFixedFilletGroupWire> for DesignFixedFilletGroup {
    type Error = String;

    fn try_from(wire: DesignFixedFilletGroupWire) -> Result<Self, Self::Error> {
        if wire.radii.len() != wire.radius_record_indexes.len() {
            return Err("radii and radius_record_indexes must have equal lengths".into());
        }
        if wire.intermediate_parameters.len() != wire.intermediate_parameter_record_indexes.len() {
            return Err("intermediate_parameters and intermediate_parameter_record_indexes must have equal lengths".into());
        }
        let mut radii =
            wire.radii
                .into_iter()
                .zip(wire.radius_record_indexes)
                .map(|(value, record_index)| DesignFixedFilletScalar {
                    value,
                    record_index,
                    value_offset: None,
                });
        let start = radii
            .next()
            .ok_or("radii requires a constant radius or two endpoints")?;
        let law = match radii.next() {
            None if wire.intermediate_parameters.is_empty() => {
                DesignFixedFilletLaw::Constant(start)
            }
            None => return Err("intermediate_parameters requires two endpoint radii".into()),
            Some(end) => {
                if radii.len() != wire.intermediate_parameters.len() {
                    return Err("radii must contain two endpoints and one radius per intermediate_parameters entry".into());
                }
                let parameters = wire
                    .intermediate_parameters
                    .into_iter()
                    .zip(wire.intermediate_parameter_record_indexes)
                    .map(|(value, record_index)| DesignFixedFilletScalar {
                        value,
                        record_index,
                        value_offset: None,
                    });
                DesignFixedFilletLaw::Variable {
                    start,
                    end,
                    intermediate: radii
                        .zip(parameters)
                        .map(|(radius, parameter)| DesignFixedFilletIntermediate {
                            radius,
                            parameter,
                        })
                        .collect(),
                }
            }
        };
        Self::try_new(wire.tangency_weight, law)
    }
}

#[cfg(test)]
impl From<DesignFixedFilletGroup> for DesignFixedFilletGroupWire {
    fn from(group: DesignFixedFilletGroup) -> Self {
        Self {
            tangency_weight: group.tangency_weight,
            radii: group.law.radii().map(|scalar| scalar.value).collect(),
            radius_record_indexes: group
                .law
                .radii()
                .map(|scalar| scalar.record_index)
                .collect(),
            intermediate_parameters: group
                .law
                .intermediate()
                .iter()
                .map(|row| row.parameter.value)
                .collect(),
            intermediate_parameter_record_indexes: group
                .law
                .intermediate()
                .iter()
                .map(|row| row.parameter.record_index)
                .collect(),
        }
    }
}

/// One fixed fillet scalar and its source record and value location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignFixedFilletScalar {
    /// Radius, normalized position, or tangency weight.
    pub(crate) value: FiniteReal,
    /// Referenced scalar record.
    pub(crate) record_index: u32,
    /// Byte offset of the scalar of a located scope.
    #[serde(skip)]
    pub(crate) value_offset: Option<u64>,
}

/// Exact fixed scalar lanes carried by a Chamfer scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignFixedChamferParameters {
    /// One equal setback distance applies to both incident faces.
    EqualDistance {
        /// Equal setback distance.
        distance: DesignFixedChamferDistance,
    },
    /// The two incident faces have independently oriented setback distances.
    TwoDistances {
        /// Setback on the first incident face.
        first: DesignFixedChamferDistance,
        /// Setback on the second incident face.
        second: DesignFixedChamferDistance,
    },
}

/// One fixed Chamfer distance lane and its source provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignFixedChamferDistance {
    /// Distance in source centimetres.
    pub(crate) value: PositiveReal,
    /// Referenced scalar record.
    pub(crate) record_index: u32,
    /// Byte offset of the scalar of a located scope.
    #[serde(skip)]
    pub(crate) value_offset: Option<u64>,
}

impl<T> crate::records::source_offsets::SourceOffsets for DesignFixedExtrudeScalar<T> {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("value_offset", &mut self.value_offset);
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignFixedExtrudeDistance {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        match self {
            Self::FixedScalar(scalar) => offsets.record("scalar", scalar),
            Self::DistanceConstruction(scalar) => offsets.record("scalar", scalar),
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignFixedExtrudeParameters {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.optional("along_distance", &mut self.along_distance);
        offsets.optional("taper_angle", &mut self.taper_angle);
        offsets.optional("against_distance", &mut self.against_distance);
        offsets.optional("side_two_taper_angle", &mut self.side_two_taper_angle);
        offsets.optional("profile_offset", &mut self.profile_offset);
        offsets.optional("side_one_offset", &mut self.side_one_offset);
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignFixedFilletParameters {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        for (index, group) in self.groups.iter_mut().enumerate() {
            offsets.nested(&format!("groups.{index}"), |offsets| {
                if let Some(weight) = &mut group.tangency_weight {
                    offsets.offset("tangency_weight.value_offset", &mut weight.value_offset);
                }
                let mut radius = 0_usize;
                let mut visit_radius =
                    |offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
                     scalar: &mut DesignFixedFilletScalar| {
                        offsets.offset(
                            &format!("radius_offsets.{radius}"),
                            &mut scalar.value_offset,
                        );
                        radius += 1;
                    };
                match &mut group.law {
                    DesignFixedFilletLaw::Constant(scalar) => visit_radius(offsets, scalar),
                    DesignFixedFilletLaw::Variable {
                        start,
                        end,
                        intermediate,
                    } => {
                        visit_radius(offsets, start);
                        visit_radius(offsets, end);
                        for row in intermediate.iter_mut() {
                            visit_radius(offsets, &mut row.radius);
                        }
                        for (index, row) in intermediate.iter_mut().enumerate() {
                            offsets.offset(
                                &format!("intermediate_parameter_offsets.{index}"),
                                &mut row.parameter.value_offset,
                            );
                        }
                    }
                }
            });
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignFixedChamferParameters {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        match self {
            Self::EqualDistance { distance } => offsets.record("distance", distance),
            Self::TwoDistances { first, second } => {
                offsets.record("first", first);
                offsets.record("second", second);
            }
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignFixedChamferDistance {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("value_offset", &mut self.value_offset);
    }
}

mod identity_rewrite;
