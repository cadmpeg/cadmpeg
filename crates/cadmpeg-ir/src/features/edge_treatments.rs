// SPDX-License-Identifier: Apache-2.0
//! Fillet and chamfer operands, dimensions, and radius laws.

use super::{face_selections_overlap, EdgeSelection, FaceSelection};
use crate::scalar::{
    FiniteReal, Fraction, InteriorAngle, Length, NonNegativeLength, PositiveLength,
};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Identified radius law form for a radius that is not yet dimensioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RadiusForm {
    /// Same radius along the whole edge chain.
    Constant,
    /// Constant transverse chord length.
    Chordal,
    /// Distinct offsets along the two support faces.
    Asymmetric,
    /// Radius varying along the edge chain.
    Variable,
}

/// Radius assignment along filleted edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RadiusSpec {
    /// The law is not dimensioned; `form` names it when the source identified one.
    Unresolved {
        /// Identified law form, when the source established one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_form"
        )]
        form: Option<RadiusForm>,
    },
    /// Same radius along the whole edge chain.
    Constant {
        /// The fillet radius.
        radius: PositiveLength,
    },
    /// Constant transverse chord length across the fillet surface.
    Chordal {
        /// Distance between the fillet's two support boundaries.
        chord_length: PositiveLength,
    },
    /// Distinct offsets from the selected edge along its two support faces.
    Asymmetric {
        /// Offset on the first support-face side.
        offset_one: PositiveLength,
        /// Offset on the second support-face side.
        offset_two: PositiveLength,
    },
    /// Radius varying along the edge chain per explicit control points.
    Variable {
        /// Radius samples along the edge chain, in chain-parameter order.
        points: VariableRadii,
    },
}

impl RadiusSpec {
    /// Returns whether the radius law lacks its required dimensions.
    pub const fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved { .. })
    }
}

/// One independently dimensioned group of filleted edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FilletGroup {
    /// Edges sharing this radius law.
    pub edges: EdgeSelection,
    /// Radius assignment along the edges.
    pub radius: RadiusSpec,
    /// Dimensionless tangency weight, when specified.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tangency_weight"
    )]
    pub tangency_weight: Option<FiniteReal>,
}

/// One full-round fillet group with pairwise disjoint face selections.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct FullRoundFilletGroup {
    #[serde(rename = "center_faces")]
    center: FaceSelection,
    #[serde(rename = "side_one_faces")]
    side_one: FullRoundSideSelection,
    #[serde(rename = "side_two_faces")]
    side_two: FullRoundSideSelection,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FullRoundFilletGroupWire {
    #[serde(rename = "center_faces")]
    center: FaceSelection,
    #[serde(rename = "side_one_faces")]
    side_one: FullRoundSideSelection,
    #[serde(rename = "side_two_faces")]
    side_two: FullRoundSideSelection,
}

impl FullRoundFilletGroup {
    /// Admit center and side selections with pairwise disjoint membership.
    pub fn new(
        center_faces: FaceSelection,
        side_one_faces: FullRoundSideSelection,
        side_two_faces: FullRoundSideSelection,
    ) -> Result<Self, &'static str> {
        fn explicit(side: &FullRoundSideSelection) -> Option<&FaceSelection> {
            match side {
                FullRoundSideSelection::Explicit(faces) => Some(faces),
                FullRoundSideSelection::Automatic | FullRoundSideSelection::Unresolved => None,
            }
        }
        let first = explicit(&side_one_faces);
        let second = explicit(&side_two_faces);
        if first.is_some_and(|faces| face_selections_overlap(&center_faces, faces))
            || second.is_some_and(|faces| face_selections_overlap(&center_faces, faces))
            || first
                .zip(second)
                .is_some_and(|(first, second)| face_selections_overlap(first, second))
        {
            return Err(
                "center_faces, side_one_faces and side_two_faces must be pairwise disjoint",
            );
        }
        Ok(Self {
            center: center_faces,
            side_one: side_one_faces,
            side_two: side_two_faces,
        })
    }

    /// Return the center-face selection.
    pub fn center_faces(&self) -> &FaceSelection {
        &self.center
    }

    /// Return the first side-face selection.
    pub fn side_one_faces(&self) -> &FullRoundSideSelection {
        &self.side_one
    }

    /// Return the second side-face selection.
    pub fn side_two_faces(&self) -> &FullRoundSideSelection {
        &self.side_two
    }
}

impl<'de> Deserialize<'de> for FullRoundFilletGroup {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = FullRoundFilletGroupWire::deserialize(deserializer)?;
        Self::new(wire.center, wire.side_one, wire.side_two).map_err(serde::de::Error::custom)
    }
}

/// One side of a full-round fillet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum FullRoundSideSelection {
    /// The kernel infers this side from the center-face selection.
    Automatic,
    /// Explicit side-face selection.
    Explicit(FaceSelection),
    /// A side-face selection exists but cannot be resolved.
    Unresolved,
}

/// One independently dimensioned group of chamfered edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ChamferGroup {
    /// Edges sharing this dimensional specification.
    pub edges: EdgeSelection,
    /// Dimensional definition applied to the edges.
    pub spec: ChamferSpec,
}

const INVALID_VARIABLE_RADII: &str = "variable radius points require at least two ordered parameters in [0, 1] and nonnegative radii with one positive radius";

/// An ordered variable-radius law with at least two samples and a positive radius.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(into = "Vec<VariableRadius>"))]
#[serde(try_from = "Vec<VariableRadius>")]
pub struct VariableRadii(Vec<VariableRadius<Fraction, NonNegativeLength>>);

impl VariableRadii {
    /// Admits finite ordered parameters in [0, 1] and nonnegative radii with one positive radius.
    pub fn new(points: Vec<VariableRadius>) -> Result<Self, &'static str> {
        let points = points
            .into_iter()
            .map(|point| {
                Some(VariableRadius {
                    parameter: Fraction::new(point.parameter)?,
                    radius: NonNegativeLength::try_from(point.radius).ok()?,
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(INVALID_VARIABLE_RADII)?;
        Self::from_parts(points)
    }

    /// Builds the law from admitted samples. The sample types state the
    /// parameter range and the radius sign, so only the sample count, one
    /// positive radius and the parameter order are tested.
    pub fn from_parts(
        points: Vec<VariableRadius<Fraction, NonNegativeLength>>,
    ) -> Result<Self, &'static str> {
        if points.len() < 2
            || !points.iter().any(|point| point.radius.get() > 0.0)
            || !points
                .windows(2)
                .all(|pair| pair[0].parameter < pair[1].parameter)
        {
            return Err(INVALID_VARIABLE_RADII);
        }
        Ok(Self(points))
    }

    /// Returns the admitted radius samples in parameter order.
    pub fn as_slice(&self) -> &[VariableRadius<Fraction, NonNegativeLength>] {
        &self.0
    }
}

impl TryFrom<Vec<VariableRadius>> for VariableRadii {
    type Error = &'static str;

    fn try_from(points: Vec<VariableRadius>) -> Result<Self, Self::Error> {
        Self::new(points)
    }
}

impl Serialize for VariableRadii {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Radius at a normalized position along a filleted edge chain.
// A source states a raw parameter and a signed length; `VariableRadii`
// holds the admitted sample, whose parameter is a `Fraction` and whose
// radius is a `NonNegativeLength`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VariableRadius<P = f64, L = Length> {
    /// Position in `[0, 1]` along the edge chain.
    pub parameter: P,
    /// Fillet radius at this position.
    pub radius: L,
}

impl VariableRadius<Fraction, NonNegativeLength> {
    /// The sample with a raw parameter and a signed length radius.
    #[must_use]
    pub fn to_raw(&self) -> VariableRadius {
        VariableRadius {
            parameter: self.parameter.get(),
            radius: Length::from(self.radius),
        }
    }
}

/// Identified chamfer form for a chamfer that is not yet dimensioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ChamferForm {
    /// Equal setback distance on both faces.
    Distance,
    /// Independent setback distances on each face.
    TwoDistances,
    /// A setback distance plus an angle from its reference face.
    DistanceAngle,
}

/// Dimensional definition of an edge chamfer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChamferSpec {
    /// The chamfer is not dimensioned; `form` names it when the source identified one.
    Unresolved {
        /// Identified chamfer form, when the source established one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_chamfer_spec_form"
        )]
        form: Option<ChamferForm>,
    },
    /// Equal setback distance on both faces meeting the edge.
    Distance {
        /// Setback distance from the edge.
        distance: PositiveLength,
    },
    /// Independent setback distances on each face meeting the edge.
    TwoDistances {
        /// Setback distance on the first face.
        first: PositiveLength,
        /// Setback distance on the second face.
        second: PositiveLength,
    },
    /// A setback distance on one face plus an angle from it to the other.
    DistanceAngle {
        /// Setback distance on the reference face.
        distance: PositiveLength,
        /// Chamfer angle measured from the reference face.
        angle: InteriorAngle,
    },
}

impl ChamferSpec {
    /// Returns whether the chamfer form lacks its required dimensions.
    pub const fn is_unresolved(self) -> bool {
        matches!(self, Self::Unresolved { .. })
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_form, RadiusForm, "form");
cadmpeg_core::named_optional_field!(deserialize_tangency_weight, FiniteReal, "tangency_weight");
cadmpeg_core::named_optional_field!(deserialize_chamfer_spec_form, ChamferForm, "form");

#[cfg(test)]
mod tests;
