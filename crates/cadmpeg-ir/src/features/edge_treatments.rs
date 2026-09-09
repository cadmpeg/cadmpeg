// SPDX-License-Identifier: Apache-2.0
use super::{face_selections_overlap, EdgeSelection, FaceSelection};
use crate::scalar::{FiniteReal, InteriorAngle, Length, PositiveLength};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Radius assignment along filleted edges.
#[derive(Debug, Clone, PartialEq)]
pub enum RadiusSpec {
    /// Radius law form is not identified.
    Unresolved,
    /// A constant-radius law is identified without its radius.
    UnresolvedConstant,
    /// A chordal law is identified without its chord length.
    UnresolvedChordal,
    /// An asymmetric law is identified without its offsets.
    UnresolvedAsymmetric,
    /// A variable-radius law is identified without its control points.
    UnresolvedVariable,
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
    pub fn is_unresolved(&self) -> bool {
        matches!(
            self,
            Self::Unresolved
                | Self::UnresolvedConstant
                | Self::UnresolvedChordal
                | Self::UnresolvedAsymmetric
                | Self::UnresolvedVariable
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RadiusSpecWire {
    Unresolved {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<RadiusFormWire>,
    },
    Constant {
        radius: PositiveLength,
    },
    Chordal {
        chord_length: PositiveLength,
    },
    Asymmetric {
        offset_one: PositiveLength,
        offset_two: PositiveLength,
    },
    Variable {
        points: VariableRadii,
    },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum RadiusFormWire {
    Constant,
    Chordal,
    Asymmetric,
    Variable,
}

impl From<RadiusSpec> for RadiusSpecWire {
    fn from(value: RadiusSpec) -> Self {
        match value {
            RadiusSpec::Unresolved => Self::Unresolved { form: None },
            RadiusSpec::UnresolvedConstant => Self::Unresolved {
                form: Some(RadiusFormWire::Constant),
            },
            RadiusSpec::UnresolvedChordal => Self::Unresolved {
                form: Some(RadiusFormWire::Chordal),
            },
            RadiusSpec::UnresolvedAsymmetric => Self::Unresolved {
                form: Some(RadiusFormWire::Asymmetric),
            },
            RadiusSpec::UnresolvedVariable => Self::Unresolved {
                form: Some(RadiusFormWire::Variable),
            },
            RadiusSpec::Constant { radius } => Self::Constant { radius },
            RadiusSpec::Chordal { chord_length } => Self::Chordal { chord_length },
            RadiusSpec::Asymmetric {
                offset_one,
                offset_two,
            } => Self::Asymmetric {
                offset_one,
                offset_two,
            },
            RadiusSpec::Variable { points } => Self::Variable { points },
        }
    }
}

impl From<RadiusSpecWire> for RadiusSpec {
    fn from(value: RadiusSpecWire) -> Self {
        match value {
            RadiusSpecWire::Unresolved { form: None } => Self::Unresolved,
            RadiusSpecWire::Unresolved {
                form: Some(RadiusFormWire::Constant),
            } => Self::UnresolvedConstant,
            RadiusSpecWire::Unresolved {
                form: Some(RadiusFormWire::Chordal),
            } => Self::UnresolvedChordal,
            RadiusSpecWire::Unresolved {
                form: Some(RadiusFormWire::Asymmetric),
            } => Self::UnresolvedAsymmetric,
            RadiusSpecWire::Unresolved {
                form: Some(RadiusFormWire::Variable),
            } => Self::UnresolvedVariable,
            RadiusSpecWire::Constant { radius } => Self::Constant { radius },
            RadiusSpecWire::Chordal { chord_length } => Self::Chordal { chord_length },
            RadiusSpecWire::Asymmetric {
                offset_one,
                offset_two,
            } => Self::Asymmetric {
                offset_one,
                offset_two,
            },
            RadiusSpecWire::Variable { points } => Self::Variable { points },
        }
    }
}

impl Serialize for RadiusSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RadiusSpecWire::from(self.clone()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RadiusSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(RadiusSpecWire::deserialize(deserializer)?.into())
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for RadiusSpec {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RadiusSpec".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        RadiusSpecWire::json_schema(generator)
    }
}

/// One independently dimensioned group of filleted edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct FilletGroup {
    /// Edges sharing this radius law.
    pub edges: EdgeSelection,
    /// Radius assignment along the edges.
    pub radius: RadiusSpec,
    /// Dimensionless tangency weight, when specified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

    /// Admit all edited selections before replacing the group.
    pub fn try_edit(
        &mut self,
        edit: impl FnOnce(&mut FaceSelection, &mut FullRoundSideSelection, &mut FullRoundSideSelection),
    ) -> Result<(), &'static str> {
        let mut center = self.center.clone();
        let mut first = self.side_one.clone();
        let mut second = self.side_two.clone();
        edit(&mut center, &mut first, &mut second);
        *self = Self::new(center, first, second)?;
        Ok(())
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
pub struct ChamferGroup {
    /// Edges sharing this dimensional specification.
    pub edges: EdgeSelection,
    /// Dimensional definition applied to the edges.
    pub spec: ChamferSpec,
}

/// An ordered variable-radius law with at least two samples and a positive radius.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "Vec<VariableRadius>", into = "Vec<VariableRadius>")]
pub struct VariableRadii(Vec<VariableRadius>);

impl VariableRadii {
    /// Admits finite ordered parameters in [0, 1] and nonnegative radii with one positive radius.
    pub fn new(points: Vec<VariableRadius>) -> Result<Self, &'static str> {
        if points.len() < 2
            || !points.iter().all(|point| {
                point.parameter.is_finite()
                    && (0.0..=1.0).contains(&point.parameter)
                    && point.radius.get() >= 0.0
            })
            || !points.iter().any(|point| point.radius.get() > 0.0)
            || !points
                .windows(2)
                .all(|pair| pair[0].parameter < pair[1].parameter)
        {
            return Err("variable radius points require at least two ordered parameters in [0, 1] and nonnegative radii with one positive radius");
        }
        Ok(Self(points))
    }

    /// Returns the admitted radius samples in parameter order.
    pub fn as_slice(&self) -> &[VariableRadius] {
        &self.0
    }
}

impl TryFrom<Vec<VariableRadius>> for VariableRadii {
    type Error = &'static str;

    fn try_from(points: Vec<VariableRadius>) -> Result<Self, Self::Error> {
        Self::new(points)
    }
}

impl From<VariableRadii> for Vec<VariableRadius> {
    fn from(points: VariableRadii) -> Self {
        points.0
    }
}

/// Radius at a normalized position along a filleted edge chain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct VariableRadius {
    /// Position in `[0, 1]` along the edge chain.
    pub parameter: f64,
    /// Fillet radius at this position.
    pub radius: Length,
}

/// Dimensional definition of an edge chamfer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChamferSpec {
    /// Dimensional specification form is not identified.
    Unresolved,
    /// An equal-distance form is identified without its distance.
    UnresolvedDistance,
    /// A two-distance form is identified without its distances.
    UnresolvedTwoDistances,
    /// A distance-angle form is identified without its dimensions.
    UnresolvedDistanceAngle,
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
    pub fn is_unresolved(self) -> bool {
        matches!(
            self,
            Self::Unresolved
                | Self::UnresolvedDistance
                | Self::UnresolvedTwoDistances
                | Self::UnresolvedDistanceAngle
        )
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ChamferSpecWire {
    Unresolved {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<ChamferFormWire>,
    },
    Distance {
        distance: PositiveLength,
    },
    TwoDistances {
        first: PositiveLength,
        second: PositiveLength,
    },
    DistanceAngle {
        distance: PositiveLength,
        angle: InteriorAngle,
    },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum ChamferFormWire {
    Distance,
    TwoDistances,
    DistanceAngle,
}

impl From<ChamferSpec> for ChamferSpecWire {
    fn from(value: ChamferSpec) -> Self {
        match value {
            ChamferSpec::Unresolved => Self::Unresolved { form: None },
            ChamferSpec::UnresolvedDistance => Self::Unresolved {
                form: Some(ChamferFormWire::Distance),
            },
            ChamferSpec::UnresolvedTwoDistances => Self::Unresolved {
                form: Some(ChamferFormWire::TwoDistances),
            },
            ChamferSpec::UnresolvedDistanceAngle => Self::Unresolved {
                form: Some(ChamferFormWire::DistanceAngle),
            },
            ChamferSpec::Distance { distance } => Self::Distance { distance },
            ChamferSpec::TwoDistances { first, second } => Self::TwoDistances { first, second },
            ChamferSpec::DistanceAngle { distance, angle } => {
                Self::DistanceAngle { distance, angle }
            }
        }
    }
}

impl From<ChamferSpecWire> for ChamferSpec {
    fn from(value: ChamferSpecWire) -> Self {
        match value {
            ChamferSpecWire::Unresolved { form: None } => Self::Unresolved,
            ChamferSpecWire::Unresolved {
                form: Some(ChamferFormWire::Distance),
            } => Self::UnresolvedDistance,
            ChamferSpecWire::Unresolved {
                form: Some(ChamferFormWire::TwoDistances),
            } => Self::UnresolvedTwoDistances,
            ChamferSpecWire::Unresolved {
                form: Some(ChamferFormWire::DistanceAngle),
            } => Self::UnresolvedDistanceAngle,
            ChamferSpecWire::Distance { distance } => Self::Distance { distance },
            ChamferSpecWire::TwoDistances { first, second } => Self::TwoDistances { first, second },
            ChamferSpecWire::DistanceAngle { distance, angle } => {
                Self::DistanceAngle { distance, angle }
            }
        }
    }
}

impl Serialize for ChamferSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ChamferSpecWire::from(*self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ChamferSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(ChamferSpecWire::deserialize(deserializer)?.into())
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for ChamferSpec {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ChamferSpec".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ChamferSpecWire::json_schema(generator)
    }
}
