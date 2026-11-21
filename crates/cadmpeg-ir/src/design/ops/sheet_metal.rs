// SPDX-License-Identifier: Apache-2.0
//! Sheet metal.
//!
//! Walls, flanges, bends, rips, conversions, corner breaks, stamps, tool forms,
//! and the unfold and refold of sheet-metal bodies, with the rules that hold
//! the defaults several features share.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{Joint, PairElement, Span};
use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::expr::Expression;
use crate::design::ids::SheetMetalRuleId;
use crate::design::options::{NativeOptions, ProfileRef, Thickness};
use crate::design::selection::{DefinitionRef, OrderedSelection, SetSelection};
use crate::features::FeatureId;
use crate::ids::ProductDefinitionId;
use crate::scalar::{
    Angle, FiniteReal, Length, NonNegativeLength, NonNegativeReal, PositiveLength, PositiveReal,
};

/// A sheet-metal feature. A field that is `None` takes the governing rule's
/// value, and the target default where no rule governs, except a `radius_side`,
/// which no writer defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SheetMetal {
    /// What the feature does.
    pub kind: SheetMetalKind,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
}

/// A record of the scope that holds sheet-metal defaults which several features
/// and bodies share. It is no member of a container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SheetMetalRule {
    /// The rule's identity.
    pub id: SheetMetalRuleId,
    /// The product definition whose scope holds the rule.
    pub scope: ProductDefinitionId,
    /// The rule's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<NonBlankString>,
    /// The sheet thickness.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thickness"
    )]
    pub thickness: Option<Dimension<PositiveLength>>,
    /// The default bend.
    pub bend: BendSpec,
    /// How the flat length of a governed bend is found.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_allowance"
    )]
    pub allowance: Option<BendAllowance>,
    /// Source options whose effect is not established.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub native_options: NativeOptions,
    /// Source options that select how the source computes the result.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub source_settings: NativeOptions,
    /// The source's own reference to the rule record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// How the flat length of a governed bend is found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BendAllowance {
    /// By one K-factor.
    KFactor(Dimension<FiniteReal>),
    /// By a table of K-factors.
    KFactorTable(KFactorTable),
    /// By a bend table.
    Table(BendTable),
    /// By expressions of the bend's own angle, radius, and thickness.
    Equations(BendEquations),
}

/// A bend table that states the K-factor by the ratio of a bend's inside radius
/// to the thickness. A bend whose ratio lies above every row takes the last
/// row's K-factor, and one below every row the first row's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct KFactorTable {
    /// The rows, in increasing ratio.
    pub rows: Vec<KFactorRow>,
    /// How a bend takes its K-factor from the rows.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lookup"
    )]
    pub lookup: Option<TableLookup>,
}

/// A bend's flat length by its thickness, inside radius, and angle: one block
/// per thickness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BendTable {
    /// The blocks, one per thickness.
    pub blocks: Vec<BendTableBlock>,
    /// What the angle column is. `None` is not read.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<BendTableAngle>,
    /// What a cell is.
    pub value: BendTableValue,
    /// How a bend is read that falls between or outside the table's entries.
    pub lookup: BendTableLookup,
}

/// The block of a bend table for one thickness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BendTableBlock {
    /// The thickness.
    pub thickness: PositiveLength,
    /// The column of inside radii.
    pub radii: Vec<PositiveLength>,
    /// The column of angles.
    pub angles: Vec<Angle>,
    /// A value per angle and radius: `values[i][j]` for `angles[i]` and
    /// `radii[j]`.
    pub values: Vec<Vec<FiniteReal>>,
}

/// What a bend angle is measured as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BendTableAngle {
    /// The rotation of the bent flange from the unbent wall.
    Deflection,
    /// 180° less the deflection.
    Opening,
}

/// What a bend table's cell is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BendTableValue {
    /// The length x, in the document's length unit, by which the flat length
    /// falls short of the two flange lengths: L = A + B − x.
    Deduction {
        /// Where the two flange lengths run to. `None` states neither, so the
        /// value is established only up to 90° of deflection.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_flanges"
        )]
        flanges: Option<FlangeMeasure>,
    },
    /// The length of the bend's neutral arc, in the document's length unit,
    /// which the flat pattern adds to the flanges measured to the bend's
    /// tangent lines.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Allowance,
    /// The K-factor the bend takes.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    KFactor,
}

/// Where the two flange lengths of a bend deduction or compensation run to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FlangeMeasure {
    /// The intersection of the two outer faces, at every deflection.
    OuterSharp,
    /// That intersection up to a deflection of 90°, and above it, for each
    /// flange, the plane tangent to the bend's outer face and perpendicular to
    /// that flange: a setback of the inside radius plus the thickness.
    OuterSharpThenTangent,
}

/// How a bend is read whose thickness, inside radius, or angle is not an entry
/// of the table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BendTableLookup {
    /// The law of the thickness axis. `None` is not stated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thickness"
    )]
    pub thickness: Option<AxisLookup>,
    /// The law of the radius axis. `None` is not stated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_radius"
    )]
    pub radius: Option<AxisLookup>,
    /// The law of the angle axis. `None` is not stated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<AxisLookup>,
    /// What a bend outside the grid takes. `None` does not state it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_outside"
    )]
    pub outside: Option<OutsideGrid>,
}

/// How one axis of a bend table reads a value that is not an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AxisLookup {
    /// The bend takes the entry equal to its own value, within the source's
    /// tolerance; a bend with no such entry lies outside the grid.
    Exact,
    /// The cell varies linearly between the two entries that bracket the bend's
    /// value.
    Linear,
}

/// What a bend outside a bend table's grid takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OutsideGrid {
    /// A backup K-factor.
    BackupKFactor {
        /// The K-factor. `None` where the source's carrier of it is not read.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_k_factor"
        )]
        k_factor: Option<Dimension<FiniteReal>>,
    },
}

/// A bend allowance stated as expressions over ranges of the bend's angle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BendEquations {
    /// What each range's value is.
    pub yields: BendQuantity,
    /// How the bend's angle is measured.
    pub angle: BendTableAngle,
    /// The ranges, in increasing order.
    pub ranges: Vec<AngleRange>,
}

/// What a bend equation's value is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BendQuantity {
    /// A length v that the flat length adds to the two flange lengths: L = A +
    /// B + v.
    Compensation {
        /// Where the two flange lengths run to. `None` states neither, so the
        /// value is established only up to 90° of deflection.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_flanges"
        )]
        flanges: Option<FlangeMeasure>,
    },
    /// As a bend table's deduction cell.
    Deduction {
        /// Where the two flange lengths run to. `None` states neither, so the
        /// value is established only up to 90° of deflection.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_flanges"
        )]
        flanges: Option<FlangeMeasure>,
    },
    /// As a bend table's allowance cell.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Allowance,
    /// As a bend table's K-factor cell.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    KFactor,
    /// A source quantity whose meaning is not established.
    Native(NonBlankString),
}

/// One range of a bend equation: a bend whose angle lies in it takes its
/// `value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AngleRange {
    /// The lower bound.
    pub from: Angle,
    /// Whether the lower bound is in the range.
    pub from_inclusive: bool,
    /// The upper bound.
    pub to: Angle,
    /// Whether the upper bound is in the range.
    pub to_inclusive: bool,
    /// An expression of the bend's own angle, inside radius, and thickness.
    pub value: Expression,
}

/// How a bend takes its K-factor from a K-factor table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TableLookup {
    /// The K-factor of the first row whose ratio is not below the bend's own.
    NextAtOrAbove,
    /// The linear interpolation between the two rows whose ratios bracket it.
    Linear,
}

/// A row of a K-factor table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct KFactorRow {
    /// The ratio of the inside radius to the thickness.
    pub ratio: NonNegativeReal,
    /// The K-factor.
    pub k_factor: Dimension<FiniteReal>,
}

/// What a sheet-metal feature does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SheetMetalKind {
    /// A flat plate whose separate bend joins it to existing sheet edges.
    AttachedPlate {
        /// The closed profile in the plate's plane.
        profile: ProfileRef,
        /// The sheet thickness.
        thickness: Dimension<PositiveLength>,
        /// Where the thickness lies against the profile normal.
        side: NormalSide,
        /// The edges of the existing sheet.
        attachment: OrderedSelection,
    },
    /// The bend that joins a preceding attached plate to existing sheet edges.
    AttachedBend {
        /// The preceding attached plate.
        plate: FeatureId,
        /// The edges in this bend's input state.
        attachment: OrderedSelection,
        /// The inside radius.
        inside_radius: Dimension<PositiveLength>,
        /// The bend relief.
        relief: AttachedRelief,
        /// The transition form.
        transition_form: TransitionForm,
        /// The transition radius.
        transition_radius: Dimension<NonNegativeLength>,
    },
    /// A corner child that preserves its preceding bend's geometry and topology.
    UnchangedCorner {
        /// The preceding bend.
        bend: FeatureId,
    },
    /// A rule made to govern bodies from its position on.
    ApplyRule {
        /// The rule.
        rule: SheetMetalRuleId,
        /// The bodies; `None` is the state's sheet-metal bodies.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_bodies"
        )]
        bodies: Option<SetSelection>,
    },
    /// A flat sheet from a closed profile.
    Wall {
        /// The profile.
        profile: ProfileRef,
        /// The thickness.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thickness"
        )]
        thickness: Option<Dimension<PositiveLength>>,
        /// Where the thickness lies against the profile's oriented normal.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_side"
        )]
        side: Option<NormalSide>,
        /// The rule that governs the body the wall makes, where the source
        /// names one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rule"
        )]
        rule: Option<SheetMetalRuleId>,
    },
    /// A sheet from an open planar profile: the profile, each vertex replaced
    /// by a bend, extruded normal to its plane and thickened about it.
    ContourFlange {
        /// The open profile.
        profile: ProfileRef,
        /// The edges of existing sheet metal the flange attaches to; the
        /// profile starts at each edge, in a plane normal to it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_edges"
        )]
        edges: Option<OrderedSelection>,
        /// The extent along an attached edge.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_width"
        )]
        width: Option<FlangeWidth>,
        /// The extent from the profile's plane along its normal, with the
        /// extrude's conventions; `None` where `edges` is stated.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extent"
        )]
        extent: Option<Span<Length>>,
        /// The thickness about the profile.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_thickness"
        )]
        thickness: Option<Thickness>,
        /// The bend at each vertex.
        bend: BendSpec,
        /// The vertices whose bend differs from `bend`.
        vertex_bends: Vec<VertexBend>,
        /// The sheet-metal rule the feature names, where the source names one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rule"
        )]
        rule: Option<SheetMetalRuleId>,
    },
    /// A wall grown from sheet edges.
    Flange(Flange),
    /// The sheet folded back over itself at edges.
    Hem(Hem),
    /// Existing sheet metal bent along a line.
    Bend {
        /// The bend line, a sketch line or an edge.
        line: SetSelection,
        /// The deflection: the rotation of the bent flange from the unbent
        /// wall.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<Angle>>,
        /// The bend radius.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius"
        )]
        radius: Option<Dimension<PositiveLength>>,
        /// Which radius `radius` is.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius_side"
        )]
        radius_side: Option<RadiusSide>,
        /// The length of flat sheet a rolled bend rolls at `radius`, in place
        /// of `angle`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_flat_length"
        )]
        flat_length: Option<Dimension<PositiveLength>>,
        /// Where the bend's developed zone lies against `line`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_zone"
        )]
        zone: Option<BendZone>,
        /// Which face of the bent sheet the line lies on.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_line_face"
        )]
        line_face: Option<BendLineFace>,
        /// Which side of the line stays put.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fixed_side"
        )]
        fixed_side: Option<LineSide>,
    },
    /// Sheet faces joined with a bend.
    Connect {
        /// The faces it joins.
        between: ConnectBetween,
        /// The bend radius; `None` takes the governing rule's.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius"
        )]
        radius: Option<Dimension<PositiveLength>>,
        /// How the faces meet the bend.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extension"
        )]
        extension: Option<BendExtension>,
        /// The form of a connect between two parallel faces that are not
        /// coplanar.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_double"
        )]
        double: Option<DoubleBend>,
    },
    /// A cut of sheet metal that does not bend it.
    Rip(Rip),
    /// A solid turned into sheet metal.
    Convert(Convert),
    /// Corners of the sheet broken through its thickness.
    BreakCorner {
        /// The corners, each an edge of the sheet's thickness at a corner.
        corners: SetSelection,
        /// The radius or the chamfer leg.
        size: Dimension<PositiveLength>,
        /// The shape of the break.
        shape: CornerBreak,
    },
    /// An existing wall carried on from an edge, in the wall's own surface,
    /// until it meets a plane.
    ExtendWall {
        /// The edge.
        edge: SetSelection,
        /// The plane.
        to: Construction<kinds::Plane>,
    },
    /// The sheet pierced along `bore`, the material around the hole formed into
    /// a flange normal to the sheet.
    ExtrudedHole {
        /// The bore; the flange's bore wall lies on it and its outer wall one
        /// thickness further out.
        bore: StampOutline,
        /// The face the punch enters; the flange rises beyond the sheet's other
        /// face.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_entry"
        )]
        entry: Option<SetSelection>,
        /// The distance of the flange's tip from the entry face, the thickness
        /// included.
        height: Dimension<PositiveLength>,
        /// The radius of the flange's root on the die side, that plus the
        /// thickness on the punch side; `None` leaves the root sharp.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_root_fillet"
        )]
        root_fillet: Option<Dimension<PositiveLength>>,
        /// The sheet-metal rule the feature names, where the source names one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rule"
        )]
        rule: Option<SheetMetalRuleId>,
    },
    /// The sheet formed by a punch along the normal of `entry`, the result
    /// rising beyond the sheet's other face.
    Stamp {
        /// The face the punch enters.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_entry"
        )]
        entry: Option<SetSelection>,
        /// The form.
        form: StampForm,
        /// The sheet-metal rule the feature names, where the source names one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rule"
        )]
        rule: Option<SheetMetalRuleId>,
    },
    /// The sheet formed over a tool body. The tool's surfaces, carried by the
    /// placement, become the sheet's driving side, and its other side lies one
    /// thickness away along the tool face's outward normal.
    ToolForm {
        /// The tool's definition.
        tool: DefinitionRef,
        /// The tool's placement in the part.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placement"
        )]
        placement: Option<Construction<kinds::Frame>>,
        /// The joints that define the placement, each between a face of the
        /// part and a face of the tool.
        constraints: Vec<Joint>,
        /// The sheet-metal rule the feature names, where the source names one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rule"
        )]
        rule: Option<SheetMetalRuleId>,
    },
    /// Bends flattened about the fixed face.
    Unfold {
        /// The fixed face.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fixed"
        )]
        fixed: Option<SetSelection>,
        /// The bends.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_bends"
        )]
        bends: Option<BendSet>,
    },
    /// Bends folded back.
    Refold {
        /// The bends.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_bends"
        )]
        bends: Option<BendSet>,
    },
    /// The flat state derived.
    FlatPattern {
        /// The fixed face.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fixed"
        )]
        fixed: Option<SetSelection>,
    },
    /// A source sheet-metal kind whose meaning is not established.
    Native(NonBlankString),
}

/// The sheet faces a connect joins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ConnectBetween {
    /// Two sheet faces that do not touch, by one model edge on each face.
    Apart([SetSelection; 2]),
    /// One edge that two sheet faces of one body already share: the corner the
    /// faces make there is replaced by a bend tangent to both, its axis
    /// parallel to the edge in the plane that bisects the corner.
    SharedEdge(SetSelection),
}

/// A cut of sheet metal that does not bend it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rip {
    /// The wall under each picked face removed; the walls that meet it stay
    /// whole up to the face's surface.
    Faces {
        /// The faces.
        faces: SetSelection,
    },
    /// A cut through the sheet's thickness along a path, its two walls normal
    /// to the sheet where they stand.
    Along {
        /// The path.
        path: RipPath,
        /// What the cut removes.
        width: RipWidth,
    },
}

/// The path of a rip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RipPath {
    /// From `at`, a point on the boundary of `face`, along the straight line in
    /// `face` to the point at the same fraction of the opposite boundary edge.
    Ruling {
        /// The face.
        face: SetSelection,
        /// The start point.
        at: Box<Construction<kinds::Point>>,
    },
    /// Straight in `face` between two points on its boundary.
    Line {
        /// The face.
        face: SetSelection,
        /// The first point.
        from: Box<Construction<kinds::Point>>,
        /// The second point.
        to: Box<Construction<kinds::Point>>,
    },
    /// Along each picked edge where two walls meet, cutting both walls back
    /// from it. It occurs only in a conversion's rip.
    Edges(SetSelection),
    /// Along each curve of a sketched chain.
    Sketched {
        /// The chain.
        profile: ProfileRef,
        /// Which wall the chain cuts.
        through: SketchedRipDepth,
    },
}

/// Which wall a sketched rip cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SketchedRipDepth {
    /// The one wall the sketch lies on, along the chain.
    SketchWall,
    /// Where the chain, carried along its sketch's normal, meets the sheet,
    /// through its thickness, normal to the sheet, with zero width.
    Projected,
}

/// What a rip removes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RipWidth {
    /// Nothing: the two lips coincide.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    Tear,
    /// The lips `gap` apart.
    Gap {
        /// The gap.
        gap: Dimension<PositiveLength>,
        /// Where the gap lies against the path. `None` is not stated.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_side"
        )]
        side: Option<KerfSide>,
    },
}

/// Where a rip's gap lies against its path, d being the path's direction and m
/// the outward normal of the face it lies in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum KerfSide {
    /// The path midway between the lips.
    Centered,
    /// One lip through the path and the gap on the side of d × m.
    Left,
    /// One lip through the path and the gap on the side of m × d.
    Right,
}

/// A solid turned into sheet metal `thickness` thick, with a cut at each rip
/// and the face `fixed` staying in place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Convert {
    /// The solid.
    pub body: SetSelection,
    /// The face that stays in place.
    pub fixed: SetSelection,
    /// The sheet thickness.
    pub thickness: Dimension<PositiveLength>,
    /// How the sheet is built.
    pub from: ConvertFrom,
    /// The rips.
    pub rips: Vec<ConvertRip>,
    /// The default bend.
    pub bend: BendSpec,
    /// Whether the solid stays as a body beside the sheet.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_keep_solid"
    )]
    pub keep_solid: Option<bool>,
    /// The sheet-metal rule the feature names, where the source names one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rule"
    )]
    pub rule: Option<SheetMetalRuleId>,
}

/// How a conversion builds its sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConvertFrom {
    /// Walls built on the solid's faces with a bend at each of `bends`.
    Faces {
        /// Where the sheet lies against the solid's faces.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_side"
        )]
        side: Option<ConvertSide>,
        /// The bends.
        bends: Vec<ConvertBend>,
    },
    /// A solid that is already walls `thickness` thick, its walls recognised as
    /// the sheet and its cylindrical walls as the bends.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    ThinSolid,
}

/// A rip of a conversion, centred on its path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ConvertRip {
    /// The path, `Edges` or `Sketched`.
    pub path: RipPath,
    /// The gap.
    pub gap: Dimension<PositiveLength>,
}

/// Where a converted sheet lies against the solid's faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ConvertSide {
    /// The solid's faces are the sheet's outer faces.
    Inside,
    /// The solid's faces are its inner faces.
    Outside,
}

/// The shape of a corner break.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CornerBreak {
    /// Each corner rounded, tangent to both edges that meet there, its axis
    /// normal to the sheet.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Round,
    /// Each corner cut at 45 degrees, both legs equal, the cut face normal to
    /// the sheet.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Chamfer,
    /// A source shape whose meaning is not established.
    Native(NonBlankString),
}

/// A bend of a conversion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConvertBend {
    /// An edge of the solid where two walls meet.
    Edge {
        /// The edge.
        edge: SetSelection,
        /// The bend's inner radius; `None` takes `bend.radius`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius"
        )]
        radius: Option<Dimension<PositiveLength>>,
    },
    /// A cylindrical face of the solid, whose radius r gives the bend's inner
    /// radius: r where the sheet lies `Inside` against a concave face or
    /// `Outside` against a convex one, and r − `thickness` otherwise.
    Face {
        /// The face.
        face: SetSelection,
    },
}

/// Where a wall's thickness lies against its profile's oriented normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum NormalSide {
    /// Along the normal.
    Along,
    /// Against the normal.
    Against,
    /// Equally on both sides.
    Symmetric,
}

/// The bend at one vertex of a contour flange's profile, where it differs from
/// the flange's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VertexBend {
    /// The vertex.
    pub vertex: SetSelection,
    /// The radius.
    pub radius: Dimension<PositiveLength>,
    /// Which radius `radius` is.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_radius_side"
    )]
    pub radius_side: Option<RadiusSide>,
}

/// The outline of a stamp or a bore.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StampOutline {
    /// A profile.
    Profile(ProfileRef),
    /// A circle.
    Circle {
        /// The centre.
        center: Construction<kinds::Point>,
        /// The diameter.
        diameter: Dimension<PositiveLength>,
    },
}

/// The form a stamp presses. Its outline is the sharp contour of the punch-side
/// wall face on the entry face; the bends are tangent fillets of that sharp
/// shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StampForm {
    /// A floor offset from the entry face inside walls.
    Raised {
        /// The outline.
        outline: StampOutline,
        /// The distance from the entry face to the floor's face on the same
        /// side.
        height: Dimension<PositiveLength>,
        /// The walls' angle to the sheet plane.
        wall_angle: Dimension<Angle>,
        /// The rim bend's radius on the die-side face, that plus the thickness
        /// on the punch side.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rim_radius"
        )]
        rim_radius: Option<Dimension<PositiveLength>>,
        /// The floor bend's radius on the punch side, that plus the thickness
        /// on the die side.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_floor_radius"
        )]
        floor_radius: Option<Dimension<PositiveLength>>,
    },
    /// A strip raised on two legs, its sides cut free of the sheet by slots as
    /// wide as the sheet is thick.
    Bridge {
        /// The strip's centre on the entry face, its length along the frame's
        /// x.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placement"
        )]
        placement: Option<Construction<kinds::Frame>>,
        /// The plateau's height above the sheet.
        height: Dimension<PositiveLength>,
        /// The legs' angle.
        wall_angle: Dimension<Angle>,
        /// The strip's base length on the entry face between the legs' sharp
        /// contours.
        length: Dimension<PositiveLength>,
        /// The strip's width.
        width: Dimension<PositiveLength>,
        /// The rim bend's radius.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rim_radius"
        )]
        rim_radius: Option<Dimension<PositiveLength>>,
        /// The floor bend's radius.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_floor_radius"
        )]
        floor_radius: Option<Dimension<PositiveLength>>,
        /// How far each slot runs straight past the lower bend's tangent line
        /// before it ends in a half-round.
        relief_length: [Option<Dimension<PositiveLength>>; 2],
    },
    /// The flap a profile bounds, cut along an edge of it and pressed below the
    /// entry face between side walls.
    Louver {
        /// The profile.
        profile: ProfileRef,
        /// The line of the profile the sheet is cut along.
        open_edge: SetSelection,
        /// The flap's depth below the entry face at the open edge.
        depth: Dimension<PositiveLength>,
        /// The flap's tilt to the sheet.
        flap_angle: Dimension<Angle>,
        /// The side walls' angle.
        wall_angle: Dimension<Angle>,
        /// The rim bend's radius.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_rim_radius"
        )]
        rim_radius: Option<Dimension<PositiveLength>>,
    },
    /// A crown raised along an open sketch curve, its section one arc and its
    /// ends spheres.
    Bead {
        /// The curve.
        path: ProfileRef,
        /// The rise of the entry-side arc above the entry face.
        height: Dimension<PositiveLength>,
        /// The entry-side arc's radius; the other face's arc has that plus the
        /// thickness on the same centre.
        section_radius: Dimension<PositiveLength>,
        /// The radius of the end spheres; the other face's has that plus the
        /// thickness.
        end_radius: Dimension<PositiveLength>,
    },
    /// A groove pressed across one bend from its outer side, symmetric about
    /// the plane through `at` normal to the bend's axis, running square to the
    /// bisector of the two flanges.
    StiffeningRib {
        /// The bend's face.
        bend: SetSelection,
        /// A point whose position along the bend's axis places the rib.
        at: Construction<kinds::Point>,
        /// The chord, in the mid-plane, between the two flanges' outer faces
        /// along the groove's bottom.
        length: Dimension<PositiveLength>,
        /// The radius of the groove's bottom, a cylinder whose axis runs across
        /// the bend.
        bottom_radius: Dimension<PositiveLength>,
        /// The side walls' angle against the sheet.
        wall_angle: Dimension<Angle>,
        /// The rims' fillet radius on the inner-face side, that plus the
        /// thickness on the outer.
        rim_radius: Dimension<PositiveLength>,
    },
}

/// The bends an unfold or refold acts on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BendSet {
    /// Every bend.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    All,
    /// The selected bends.
    Selected(SetSelection),
}

/// A bend's radius and relief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BendSpec {
    /// The radius.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_radius"
    )]
    pub radius: Option<Dimension<PositiveLength>>,
    /// Which radius `radius` is. `None` is not stated, and no writer defaults
    /// it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_radius_side"
    )]
    pub radius_side: Option<RadiusSide>,
    /// The bend relief.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_relief"
    )]
    pub relief: Option<Relief>,
}

/// Which radius of a bend a radius states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RadiusSide {
    /// The inner bend radius.
    Inside,
    /// The inner radius plus the thickness.
    Outside,
}

/// A bend relief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Relief {
    /// The shape.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_shape"
    )]
    pub shape: Option<ReliefShape>,
    /// The width.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_width"
    )]
    pub width: Option<Dimension<PositiveLength>>,
    /// The width as a multiple of the thickness.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_width_ratio"
    )]
    pub width_ratio: Option<Dimension<PositiveReal>>,
    /// The depth.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_depth"
    )]
    pub depth: Option<Dimension<PositiveLength>>,
}

/// The shape of a bend relief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ReliefShape {
    /// The governing rule's relief, chosen explicitly.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Rule,
    /// A slot with half-round ends.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Obround,
    /// A source shape whose meaning is not established.
    Native(NonBlankString),
}

/// The relief operands of an attached bend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AttachedRelief {
    /// The source relief form.
    pub form: AttachedReliefForm,
    /// The width.
    pub width: Dimension<PositiveLength>,
    /// The depth.
    pub depth: Dimension<PositiveLength>,
    /// The minimum material remnant.
    pub minimum_remnant: Dimension<NonNegativeLength>,
    /// The gap between adjacent sheet edges.
    pub seam_gap: Dimension<NonNegativeLength>,
}

/// The construction form of an attached bend's relief.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AttachedReliefForm {
    /// The source document's sheet-metal style relief.
    StyleDefault,
}

/// The transition at the end of an attached bend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TransitionForm {
    /// The source document's sheet-metal style transition.
    StyleDefault,
}

/// A wall grown from sheet edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Flange {
    /// The edges, in order.
    pub edges: OrderedSelection,
    /// The wall's height.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_height"
    )]
    pub height: Option<FlangeHeight>,
    /// The bend's sweep: the angle the wall turns through from the plane of the
    /// face the edge bounds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    pub angle: Option<Dimension<Angle>>,
    /// The wall's extent along each edge.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_width"
    )]
    pub width: Option<FlangeWidth>,
    /// The bend.
    pub bend: BendSpec,
    /// The sheet-metal rule the feature names, where the source names one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rule"
    )]
    pub rule: Option<SheetMetalRuleId>,
    /// The line a `Distance` height is measured from. It decides how the height
    /// is read.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_height_datum"
    )]
    pub height_datum: Option<HeightDatum>,
    /// Where the bend lies against the selected edge.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_position"
    )]
    pub position: Option<BendPosition>,
    /// The wall's outline, an open chain sketched in the wall's own plane, on
    /// the face `height_datum` names, its two free ends on that sharp. It gives
    /// the wall's height, width, and sweep, so a flange with an outline states
    /// none of the three, on its one edge.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_outline"
    )]
    pub outline: Option<ProfileRef>,
}

/// The height of a flange.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FlangeHeight {
    /// A distance from the height datum.
    Distance(Dimension<PositiveLength>),
    /// Up to a target.
    UpTo {
        /// The target.
        target: SetSelection,
        /// The offset from the target.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_offset"
        )]
        offset: Option<Dimension<Length>>,
    },
}

/// The extent of a flange along its edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FlangeWidth {
    /// The edge's length.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Full,
    /// A width centred on the edge's midpoint.
    Centered {
        /// The width.
        width: Dimension<PositiveLength>,
    },
    /// The part of the edge left `first` in from its start and `second` in from
    /// its end.
    FromEnds {
        /// The setback from the start.
        first: Dimension<NonNegativeLength>,
        /// The setback from the end.
        second: Dimension<NonNegativeLength>,
    },
    /// `width` of the edge's line, starting `offset` from the point `from` and
    /// running in the edge's direction, or ending there and running against it
    /// where `reversed`; it may run past the edge's end.
    FromReference {
        /// The reference point.
        from: Box<Construction<kinds::Point>>,
        /// The offset from the point.
        offset: Dimension<Length>,
        /// The width.
        width: Dimension<PositiveLength>,
        /// Whether the width ends at the offset point and runs against the
        /// edge's direction.
        reversed: bool,
    },
    /// One pair of setbacks per selected edge, in edge order.
    PerEdge(Vec<[Dimension<PositiveLength>; 2]>),
    /// One width per selected edge, in edge order, each centred on that edge's
    /// midpoint.
    CenteredPerEdge(Vec<Dimension<PositiveLength>>),
}

/// The line a flange's distance height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HeightDatum {
    /// The intersection of the base's and the flange's inner faces.
    InnerSharp,
    /// The intersection of their outer faces.
    OuterSharp,
    /// The line tangent to the bend and parallel to the flange.
    TangentToBend,
}

/// Where a flange's or hem's bend lies against the selected edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BendPosition {
    /// The reference plane through the base's other edge with the flange's
    /// material inside it, so the bend starts inside the outer edges.
    MaterialInside,
    /// The reference plane through the selected edge with the material outside.
    MaterialOutside,
    /// The side face is the reference plane, with the bend starting at the
    /// selected edge.
    AtEdge,
    /// The side face is the reference plane, with the bend tangent to it where
    /// the angle is 90° or more.
    TangentToSide,
}

/// How a connect's faces meet its bend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BendExtension {
    /// The faces are extended to the bend.
    AlignedToSideFaces,
    /// The faces are met by a section perpendicular to them.
    PerpendicularToSideFaces,
}

/// Where a bend's developed zone lies against its line. The zone's length is
/// the bend's developed length under the governing allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BendZone {
    /// Wholly on the side that moves, starting at the line, so the fixed flat
    /// ends at the line.
    MovingSide,
    /// Centred on the line, so the fixed flat ends half that length short of
    /// it.
    Centered,
    /// Wholly on the fixed side, ending at the line, so the fixed flat ends
    /// that length short of it.
    FixedSide,
}

/// Which face of a bent sheet its bend line lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BendLineFace {
    /// The inner face: the moving flange turns toward the side the line's face
    /// faces.
    Inner,
    /// The outer face.
    Outer,
}

/// A side of a line on a sheet face: d being the line's direction and m the
/// outward normal of the face that carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LineSide {
    /// The side of d × m.
    Left,
    /// The other side.
    Right,
}

/// The form of a connect between two parallel faces that are not coplanar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DoubleBend {
    /// The two edges joined where they lie by two equal bends, tangent to each
    /// other or with a flat between, trimming or extending neither face.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::read")]
    EqualBends,
    /// The edge `fixed` names kept, the other face trimmed or extended, and the
    /// two joined by two bends of 45°, with a flat between where they are not
    /// tangent.
    Deg45 {
        /// The member kept. `None` does not state which.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fixed"
        )]
        fixed: Option<PairElement>,
    },
    /// The edge `fixed` names kept, the other face trimmed or extended, and the
    /// two joined by two bends of 90°, with a flat between where they are not
    /// tangent.
    Deg90 {
        /// The member kept. `None` does not state which.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fixed"
        )]
        fixed: Option<PairElement>,
    },
    /// The edge `fixed` names kept, the other face trimmed or extended, and the
    /// two joined by one bend of 180°, each of whose radii is half the distance
    /// between the two faces it is tangent to.
    FullRadius {
        /// The member kept. `None` does not state which.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fixed"
        )]
        fixed: Option<PairElement>,
    },
}

/// The sheet folded back over itself at each of `edges`, about an axis on the
/// edge, with the thickness of its rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Hem {
    /// The edges, each one of the two long edges of a wall's end face.
    pub edges: OrderedSelection,
    /// The hem's form.
    pub form: HemForm,
    /// Which face of the sheet the hem folds over. `None` does not state it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_side"
    )]
    pub side: Option<HemSide>,
    /// Where the fold's axis lies against the edge: `AtEdge` or
    /// `TangentToSide`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_position"
    )]
    pub position: Option<BendPosition>,
    /// The sheet-metal rule the feature names, where the source names one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rule"
    )]
    pub rule: Option<SheetMetalRuleId>,
}

/// Which face of the sheet a hem folds over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HemSide {
    /// About the picked edge, back over the sheet face that holds it.
    SelectedEdgeFace,
    /// About the end face's other long edge, over the other sheet face.
    OppositeFace,
}

/// The form of a hem. `length` runs parallel to the face from the fold's outer
/// apex to the hem's free end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HemForm {
    /// A fold through 180° to an inner radius of half the gap, returning
    /// parallel to the face at the gap.
    Flat {
        /// The length.
        length: Dimension<PositiveLength>,
        /// The tight gap the source stores, where it stores one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_gap"
        )]
        gap: Option<Dimension<PositiveLength>>,
    },
    /// A fold through 180° to an inner radius of half the gap, returning
    /// parallel to the face at the gap.
    Open {
        /// The length.
        length: Dimension<PositiveLength>,
        /// The gap.
        gap: Dimension<PositiveLength>,
    },
    /// A fold to an inner radius of `radius` beyond 180°, with a straight
    /// return tangent to the fold whose inner face ends `gap` from the face.
    Teardrop {
        /// The inner radius.
        radius: Dimension<PositiveLength>,
        /// The length.
        length: Dimension<PositiveLength>,
        /// The gap.
        gap: Dimension<PositiveLength>,
    },
    /// An arc of inner radius `radius` through `angle`, tangent to the face at
    /// the edge, with no straight part.
    Rolled {
        /// The inner radius.
        radius: Dimension<PositiveLength>,
        /// The angle.
        angle: Dimension<Angle>,
    },
    /// A source hem form whose meaning is not established.
    Native(NonBlankString),
}

cadmpeg_core::named_optional_field!(deserialize_allowance<T>, T, "allowance");
cadmpeg_core::named_optional_field!(deserialize_angle<T>, T, "angle");
cadmpeg_core::named_optional_field!(deserialize_bends<T>, T, "bends");
cadmpeg_core::named_optional_field!(deserialize_bodies<T>, T, "bodies");
cadmpeg_core::named_optional_field!(deserialize_depth<T>, T, "depth");
cadmpeg_core::named_optional_field!(deserialize_double<T>, T, "double");
cadmpeg_core::named_optional_field!(deserialize_edges<T>, T, "edges");
cadmpeg_core::named_optional_field!(deserialize_entry<T>, T, "entry");
cadmpeg_core::named_optional_field!(deserialize_extension<T>, T, "extension");
cadmpeg_core::named_optional_field!(deserialize_extent<T>, T, "extent");
cadmpeg_core::named_optional_field!(deserialize_fixed<T>, T, "fixed");
cadmpeg_core::named_optional_field!(deserialize_fixed_side<T>, T, "fixed_side");
cadmpeg_core::named_optional_field!(deserialize_flanges<T>, T, "flanges");
cadmpeg_core::named_optional_field!(deserialize_flat_length<T>, T, "flat_length");
cadmpeg_core::named_optional_field!(deserialize_floor_radius<T>, T, "floor_radius");
cadmpeg_core::named_optional_field!(deserialize_gap<T>, T, "gap");
cadmpeg_core::named_optional_field!(deserialize_height<T>, T, "height");
cadmpeg_core::named_optional_field!(deserialize_height_datum<T>, T, "height_datum");
cadmpeg_core::named_optional_field!(deserialize_k_factor<T>, T, "k_factor");
cadmpeg_core::named_optional_field!(deserialize_keep_solid<T>, T, "keep_solid");
cadmpeg_core::named_optional_field!(deserialize_line_face<T>, T, "line_face");
cadmpeg_core::named_optional_field!(deserialize_lookup<T>, T, "lookup");
cadmpeg_core::named_optional_field!(deserialize_name<T>, T, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_offset<T>, T, "offset");
cadmpeg_core::named_optional_field!(deserialize_outline<T>, T, "outline");
cadmpeg_core::named_optional_field!(deserialize_outside<T>, T, "outside");
cadmpeg_core::named_optional_field!(deserialize_placement<T>, T, "placement");
cadmpeg_core::named_optional_field!(deserialize_position<T>, T, "position");
cadmpeg_core::named_optional_field!(deserialize_radius<T>, T, "radius");
cadmpeg_core::named_optional_field!(deserialize_radius_side<T>, T, "radius_side");
cadmpeg_core::named_optional_field!(deserialize_relief<T>, T, "relief");
cadmpeg_core::named_optional_field!(deserialize_rim_radius<T>, T, "rim_radius");
cadmpeg_core::named_optional_field!(deserialize_root_fillet<T>, T, "root_fillet");
cadmpeg_core::named_optional_field!(deserialize_rule<T>, T, "rule");
cadmpeg_core::named_optional_field!(deserialize_shape<T>, T, "shape");
cadmpeg_core::named_optional_field!(deserialize_side<T>, T, "side");
cadmpeg_core::named_optional_field!(deserialize_thickness<T>, T, "thickness");
cadmpeg_core::named_optional_field!(deserialize_width<T>, T, "width");
cadmpeg_core::named_optional_field!(deserialize_width_ratio<T>, T, "width_ratio");
cadmpeg_core::named_optional_field!(deserialize_zone<T>, T, "zone");

#[cfg(test)]
mod tests;
