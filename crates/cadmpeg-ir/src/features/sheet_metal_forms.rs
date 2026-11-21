// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal relief, transition, hem, flange and thickness settings.

use super::selections::NonEmptyMembers;
use super::sheet_metal;
use super::{deserialize_flange_first, deserialize_flange_second, FeatureId};
use crate::scalar::{Angle, Length, NonNegativeLength, PositiveLength};
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Relief form selected by the document's sheet-metal rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SheetMetalReliefForm {
    /// Use the active document rule's bend-relief form.
    StyleDefault,
}

/// Bend-relief operands with lengths in millimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SheetMetalRelief {
    /// Construction form.
    pub form: SheetMetalReliefForm,
    /// Evaluated relief width.
    pub width: PositiveLength,
    /// Evaluated relief depth.
    pub depth: PositiveLength,
    /// Minimum material remnant.
    pub minimum_remnant: crate::scalar::NonNegativeLength,
    /// Gap between adjacent sheet edges.
    pub seam_gap: crate::scalar::NonNegativeLength,
}

/// Transition form selected by the document's sheet-metal rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SheetMetalTransitionForm {
    /// Use the active document rule's transition form.
    StyleDefault,
}

/// Sheet-metal transition operands with lengths in millimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SheetMetalTransition {
    /// Construction form.
    pub form: SheetMetalTransitionForm,
    /// Evaluated transition radius.
    pub radius: crate::scalar::NonNegativeLength,
}

/// Face pair a sheet-metal flange height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalHeightDatum {
    /// The height is measured from the inner faces of the sheet.
    InnerFaces,
    /// The height is measured from the outer faces of the sheet.
    OuterFaces,
}

/// Geometric effect of a sheet-metal corner evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalCornerEffect {
    /// The output body equals the input body, including its topology.
    Unchanged,
}

/// Placement of a sheet-metal bend region against its selected edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalBendPosition {
    /// The bend lies outside the selected edge.
    Outside,
    /// The bend lies inside the selected edge.
    Inside,
    /// The bend starts at the selected edge.
    Adjacent,
    /// The bend is tangent to the side reference plane.
    TangentToSide,
}

/// Dimensional owner layout carried by a sheet-metal hem.
///
/// The source uses one owner layout for flat and open hems. A resolved
/// transition projects that layout to [`Self::Flat`] or [`Self::Open`];
/// [`Self::GapLength`] retains the dimensional owners when the transition does
/// not prove either semantic form.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalHemForm {
    /// Flat hem with its developed length.
    Flat {
        /// Developed length of the hem.
        length: PositiveLength,
    },
    /// Open hem with a gap and a developed length.
    Open {
        /// Gap between the folded wall and its source.
        gap: NonNegativeLength,
        /// Developed length of the hem.
        length: PositiveLength,
    },
    /// Unresolved flat-or-open form with a gap and a length owner.
    GapLength {
        /// Gap between the folded wall and its source.
        gap: NonNegativeLength,
        /// Developed length of the hem.
        length: PositiveLength,
    },
    /// Rolled source form with radius and included-angle owners.
    Rolled {
        /// Rolled section radius.
        radius: PositiveLength,
        /// Included angle of the rolled section.
        angle: Angle,
    },
    /// Teardrop source form with gap, length, and radius owners.
    Teardrop {
        /// Gap between the folded wall and its source.
        gap: NonNegativeLength,
        /// Developed length of the hem.
        length: PositiveLength,
        /// Teardrop section radius.
        radius: PositiveLength,
    },
}

/// Placement of a sheet-metal hem bend against its selected edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalHemBendPosition {
    /// The bend starts at the selected edge.
    Adjacent,
    /// The bend is tangent to the source sheet face.
    Tangent,
}

/// Direction of a sheet-metal hem fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalHemDirection {
    /// Fold in the source operation's forward direction.
    Forward,
    /// Fold opposite the source operation's forward direction.
    Reverse,
    /// Source direction carrier is not resolved.
    Unresolved,
}

/// Construction feature used as the reference for a sheet-metal flange height.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalFlangeHeightTarget {
    /// A neutral construction feature in the same design history.
    Feature(FeatureId),
    /// A source selection whose neutral construction identity is not resolved.
    Native(String),
}

/// Height law for a sheet-metal edge flange.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalFlangeHeight {
    /// Fixed distance from the operation's height datum.
    Distance(PositiveLength),
    /// Signed offset from a selected construction entity.
    ToObject(sheet_metal::SheetMetalFlangeTargetConstruction),
}

impl SheetMetalFlangeHeight {
    #[doc = "Construct a to-object height with a known target and signed offset."]
    pub fn to_object(target: SheetMetalFlangeHeightTarget, offset: Length) -> Self {
        Self::ToObject(sheet_metal::SheetMetalFlangeTargetConstruction {
            target: Some(target),
            offset: Some(offset),
        })
    }
}

/// Extent of a sheet-metal flange along its selected edge.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalFlangeWidth {
    #[doc = "Independently decoded symmetric widths in source-edge order."]
    SymmetricPerEdge {
        #[doc = "One width input for each source edge."]
        widths: NonEmptyMembers<sheet_metal::SheetMetalFlangeSymmetricWidth>,
    },
    #[doc = "Independent signed offsets and boundary programs in selected-edge order."]
    TwoOffsetsPerEdge {
        #[doc = "One independently decoded offset pair for each selected source edge."]
        offsets: NonEmptyMembers<sheet_metal::SheetMetalFlangeOffset>,
    },
    /// The flange spans the complete selected edge.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    FullEdge,
    /// The flange is centred on the edge with one width.
    Symmetric(sheet_metal::SheetMetalFlangeSymmetricWidth),
    /// The flange is measured inward from each end of the edge.
    TwoSides(SheetMetalFlangeTwoSidedWidth),
    /// Independent two-sided extents for each selected edge.
    ///
    /// The entries are in the same order as the operation's selected-edge
    /// groups. Each pair is local to its edge and is not an operation-wide
    /// pair shared by all edges.
    TwoSidesPerEdge {
        /// One first-end/second-end pair for each selected edge.
        widths: SheetMetalFlangeEdgeWidths,
    },
}

impl SheetMetalFlangeWidth {
    #[doc = "Construct a known total symmetric width."]
    pub fn symmetric(width: PositiveLength) -> Self {
        Self::Symmetric(sheet_metal::SheetMetalFlangeSymmetricWidth { width: Some(width) })
    }
    #[doc = "Construct known first-end and second-end distances."]
    pub fn two_sides(first: PositiveLength, second: PositiveLength) -> Self {
        Self::TwoSides(SheetMetalFlangeTwoSidedWidth::complete(first, second))
    }
}

/// Nonempty source-ordered per-edge flange widths.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SheetMetalFlangeEdgeWidths(pub(super) Vec<SheetMetalFlangeTwoSidedWidth>);

impl SheetMetalFlangeEdgeWidths {
    /// Construct widths for at least one selected source edge group.
    pub fn new(widths: Vec<SheetMetalFlangeTwoSidedWidth>) -> Result<Self, &'static str> {
        if widths.is_empty() {
            Err("sheet-metal flange widths must contain at least one pair")
        } else {
            Ok(Self(widths))
        }
    }

    /// Mutable width values. The number of selected groups cannot change.
    pub fn as_mut_slice(&mut self) -> &mut [SheetMetalFlangeTwoSidedWidth] {
        &mut self.0
    }

    #[doc = "Source-ordered width inputs."]
    pub fn as_slice(&self) -> &[SheetMetalFlangeTwoSidedWidth] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SheetMetalFlangeEdgeWidths {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Two-sided extent assigned to one selected sheet-metal flange edge.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SheetMetalFlangeTwoSidedWidth {
    /// Distance measured from the edge's first end.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flange_first"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "PositiveLength"))]
    pub first: Option<PositiveLength>,
    /// Distance measured from the edge's second end.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flange_second"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "PositiveLength"))]
    pub second: Option<PositiveLength>,
}

impl SheetMetalFlangeTwoSidedWidth {
    #[doc = "Construct a width pair with both distances present."]
    pub fn complete(first: PositiveLength, second: PositiveLength) -> Self {
        Self {
            first: Some(first),
            second: Some(second),
        }
    }
}

/// Distribution of sheet thickness relative to its construction plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SheetMetalThicknessSide {
    /// Thickness lies along the profile plane's positive normal.
    Forward,
    /// Thickness lies along the profile plane's negative normal.
    Reverse,
    /// Thickness is split equally across both sides of the profile plane.
    Symmetric,
}
