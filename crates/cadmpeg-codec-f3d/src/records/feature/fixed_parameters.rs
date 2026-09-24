// SPDX-License-Identifier: Apache-2.0
//! Fixed extrude, fillet and chamfer parameter payloads.

use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
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
/// One exact scalar carrier used by an Extrude scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignFixedExtrudeScalar {
    /// Scalar value in source centimetres for a distance or radians for an angle.
    pub(crate) value: FiniteReal,
    /// Referenced record carrying the scalar.
    pub(crate) record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) value_offset: u64,
}

/// Exact carrier of an Extrude's one-sided distance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "carrier", content = "scalar", rename_all = "snake_case")]
pub(crate) enum DesignFixedExtrudeDistance {
    /// Signed distance in an owner-local scalar lane.
    FixedScalar(DesignFixedExtrudeScalar),
    /// Positive magnitude in an owned distance-construction frame.
    DistanceConstruction(DesignFixedExtrudeScalar),
}

/// Exact fixed scalar lanes carried by an Extrude scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignFixedExtrudeParameters {
    /// One-sided distance carrier in source centimetres.
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
}

/// Exact fixed scalar lanes carried by a Fillet scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignFixedFilletParameters {
    /// Radius laws in scalar-lane order.
    pub(crate) groups: Vec<DesignFixedFilletGroup>,
}

/// One fillet radius law and its optional tangency weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignFixedFilletGroupWire",
    into = "DesignFixedFilletGroupWire"
)]
pub(crate) struct DesignFixedFilletGroup {
    tangency_weight: Option<DesignFixedFilletScalar>,
    law: DesignFixedFilletLaw,
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
    /// Byte offsets of the radius scalars in semantic radius order.
    radius_offsets: Vec<u64>,
    /// Normalized edge-chain positions paired with the intermediate radii.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    intermediate_parameters: Vec<FiniteReal>,
    /// Referenced intermediate-position scalar records in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    intermediate_parameter_record_indexes: Vec<u32>,
    /// Byte offsets of intermediate-position scalars in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    intermediate_parameter_offsets: Vec<u64>,
}

impl TryFrom<DesignFixedFilletGroupWire> for DesignFixedFilletGroup {
    type Error = String;

    fn try_from(wire: DesignFixedFilletGroupWire) -> Result<Self, Self::Error> {
        if wire.radii.len() != wire.radius_record_indexes.len()
            || wire.radii.len() != wire.radius_offsets.len()
        {
            return Err(
                "radii, radius_record_indexes, and radius_offsets must have equal lengths".into(),
            );
        }
        if wire.intermediate_parameters.len() != wire.intermediate_parameter_record_indexes.len()
            || wire.intermediate_parameters.len() != wire.intermediate_parameter_offsets.len()
        {
            return Err("intermediate_parameters, intermediate_parameter_record_indexes, and intermediate_parameter_offsets must have equal lengths".into());
        }
        let mut radii = wire
            .radii
            .into_iter()
            .zip(wire.radius_record_indexes)
            .zip(wire.radius_offsets)
            .map(
                |((value, record_index), value_offset)| DesignFixedFilletScalar {
                    value,
                    record_index,
                    value_offset,
                },
            );
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
                    .zip(wire.intermediate_parameter_offsets)
                    .map(
                        |((value, record_index), value_offset)| DesignFixedFilletScalar {
                            value,
                            record_index,
                            value_offset,
                        },
                    );
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
            radius_offsets: group
                .law
                .radii()
                .map(|scalar| scalar.value_offset)
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
            intermediate_parameter_offsets: group
                .law
                .intermediate()
                .iter()
                .map(|row| row.parameter.value_offset)
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
    /// Byte offset of the scalar.
    pub(crate) value_offset: u64,
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
    /// Byte offset of the scalar.
    pub(crate) value_offset: u64,
}
