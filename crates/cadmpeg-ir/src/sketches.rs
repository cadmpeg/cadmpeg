// SPDX-License-Identifier: Apache-2.0
//! Neutral planar sketches, solved entities, and geometric constraints.

use crate::math::{Point2, Point3, Vector3};
use crate::products::NonEmptyString;
use crate::transform::Transform;
use crate::{
    features::ParameterId,
    scalar::{Angle, Length},
};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

crate::ids::id_type!(
    /// Identifies a neutral planar sketch.
    SketchId
);
crate::ids::id_type!(
    /// Identifies solved geometry in a sketch.
    SketchEntityId
);
crate::ids::id_type!(
    /// Identifies a neutral spatial sketch.
    SpatialSketchId
);
crate::ids::id_type!(
    /// Identifies solved geometry in a spatial sketch.
    SpatialSketchEntityId
);
crate::ids::id_type!(
    /// Identifies a geometric sketch constraint.
    SketchConstraintId
);

/// Font weight admitted by neutral sketch text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i32", into = "i32")]
#[repr(i32)]
pub enum SketchFontWeight {
    /// Regular text weight.
    Regular = 400,
    /// Medium text weight.
    Medium = 500,
    /// Bold text weight.
    Bold = 750,
}

impl TryFrom<i32> for SketchFontWeight {
    type Error = &'static str;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            400 => Ok(Self::Regular),
            500 => Ok(Self::Medium),
            750 => Ok(Self::Bold),
            _ => Err("sketch text font_weight must be 400, 500, or 750"),
        }
    }
}

impl From<SketchFontWeight> for i32 {
    fn from(value: SketchFontWeight) -> Self {
        value as Self
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for SketchFontWeight {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SketchFontWeight".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "integer", "enum": [400, 500, 750]})
    }
}

/// Horizontal placement of sketch text about its text anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "native_value", rename_all = "snake_case")]
pub enum SketchTextHorizontalAlignment {
    /// Align the text's left edge with its anchor.
    Left,
    /// Center the text about its anchor.
    Center,
    /// Align the text's right edge with its anchor.
    Right,
    /// Source alignment ordinal without an assigned neutral meaning.
    Native(u32),
}

/// Vertical placement of sketch text about its text anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "native_value", rename_all = "snake_case")]
pub enum SketchTextVerticalAlignment {
    /// Align the text's top with its anchor.
    Top,
    /// Center the text about its anchor.
    Middle,
    /// Align the text's bottom with its anchor.
    Bottom,
    /// Source alignment ordinal without an assigned neutral meaning.
    Native(u32),
}

/// Canonical reference axis in neutral sketch coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SketchAxis {
    /// Positive sketch-u direction.
    Horizontal,
    /// Positive sketch-v direction.
    Vertical,
}

/// A planar sketch and its ordered profile loops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Sketch {
    /// Globally unique sketch id.
    pub id: SketchId,
    /// Source display name, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source configuration key, when scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Source display visibility, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Placement of sketch coordinates in model space.
    pub placement: SketchPlacement,
    /// Ordered closed or open profile chains.
    #[serde(default, skip_serializing_if = "SketchProfiles::is_empty")]
    pub profiles: SketchProfiles,
    /// Identifier of the full-fidelity native input lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// Placement of a planar sketch's local coordinates in model space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchPlacement {
    /// Local geometry is decoded but its model-space frame is unresolved.
    Unresolved,
    /// Complete model-space sketch frame.
    Resolved {
        /// Checked origin and nonzero perpendicular axes.
        #[serde(flatten)]
        frame: SketchPlaneFrame,
    },
}

const EPS_SKETCH_PLANE_ORTHOGONALITY: f64 = 1.0e-9;

/// A finite origin with nonzero perpendicular sketch axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchPlaneFrameWire")]
pub struct SketchPlaneFrame {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchPlaneFrameWire {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

impl TryFrom<SketchPlaneFrameWire> for SketchPlaneFrame {
    type Error = &'static str;

    fn try_from(wire: SketchPlaneFrameWire) -> Result<Self, Self::Error> {
        let normal = wire.normal.norm();
        let u_norm = wire.u_axis.norm();
        let dot = wire.normal.x * wire.u_axis.x
            + wire.normal.y * wire.u_axis.y
            + wire.normal.z * wire.u_axis.z;
        if !normal.is_finite() || normal <= 0.0 || !u_norm.is_finite() || u_norm <= 0.0 {
            return Err("sketch normal and u_axis must have finite positive length");
        }
        if dot.abs() > EPS_SKETCH_PLANE_ORTHOGONALITY * normal * u_norm {
            return Err("sketch normal and u_axis must be perpendicular");
        }
        if !wire.origin.x.is_finite() || !wire.origin.y.is_finite() || !wire.origin.z.is_finite() {
            return Err("sketch origin must be finite");
        }
        Ok(Self {
            origin: wire.origin,
            normal: wire.normal,
            u_axis: wire.u_axis,
        })
    }
}

impl SketchPlacement {
    /// Admit a resolved sketch frame with finite origin and nonzero perpendicular axes.
    pub fn try_resolved(
        origin: Point3,
        normal: Vector3,
        u_axis: Vector3,
    ) -> Result<Self, &'static str> {
        Ok(Self::Resolved {
            frame: SketchPlaneFrameWire {
                origin,
                normal,
                u_axis,
            }
            .try_into()?,
        })
    }

    /// Return the complete frame when placement is resolved.
    pub fn resolved(self) -> Option<(Point3, Vector3, Vector3)> {
        match self {
            Self::Unresolved => None,
            Self::Resolved { frame } => Some((frame.origin, frame.normal, frame.u_axis)),
        }
    }
}

/// An ordered collection of nonempty sketch profile chains.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "Vec<Vec<SketchEntityUse>>")]
pub struct SketchProfiles(Vec<Vec<SketchEntityUse>>);

impl TryFrom<Vec<Vec<SketchEntityUse>>> for SketchProfiles {
    type Error = &'static str;

    fn try_from(profiles: Vec<Vec<SketchEntityUse>>) -> Result<Self, Self::Error> {
        if profiles.iter().any(Vec::is_empty) {
            return Err("sketch profiles must contain no empty chain");
        }
        Ok(Self(profiles))
    }
}

impl std::ops::Deref for SketchProfiles {
    type Target = [Vec<SketchEntityUse>];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a SketchProfiles {
    type Item = &'a Vec<SketchEntityUse>;
    type IntoIter = std::slice::Iter<'a, Vec<SketchEntityUse>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl IntoIterator for SketchProfiles {
    type Item = Vec<SketchEntityUse>;
    type IntoIter = std::vec::IntoIter<Vec<SketchEntityUse>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl SketchProfiles {
    /// Borrow the ordered nonempty profile chains.
    #[must_use]
    pub fn as_slice(&self) -> &[Vec<SketchEntityUse>] {
        &self.0
    }

    /// Whether the sketch has no profile chains.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Append a profile containing one entity use.
    pub fn push_single(&mut self, entity: SketchEntityUse) {
        self.0.push(vec![entity]);
    }

    /// Retain matching entity uses and remove chains emptied by the filter.
    pub fn retain_uses(&mut self, mut keep: impl FnMut(&SketchEntityUse) -> bool) {
        let mut profiles = self.0.clone();
        for profile in &mut profiles {
            profile.retain(&mut keep);
        }
        profiles.retain(|profile| !profile.is_empty());
        self.0 = profiles;
    }

    /// Append a nonempty profile chain.
    pub fn try_push(&mut self, profile: Vec<SketchEntityUse>) -> Result<(), &'static str> {
        if profile.is_empty() {
            return Err("sketch profile chain must be nonempty");
        }
        self.0.push(profile);
        Ok(())
    }

    /// Replace profile chains only after every edited chain passes admission.
    pub fn edit(
        &mut self,
        edit: impl FnOnce(&mut Vec<Vec<SketchEntityUse>>),
    ) -> Result<(), &'static str> {
        let mut profiles = self.0.clone();
        edit(&mut profiles);
        *self = profiles.try_into()?;
        Ok(())
    }

    /// Remove all profile chains.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl Sketch {
    /// Return the complete model-space frame when placement is resolved.
    pub fn resolved_placement(&self) -> Option<(Point3, Vector3, Vector3)> {
        self.placement.resolved()
    }
}

/// Oriented use of one sketch entity in a profile chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchEntityUse {
    /// Referenced sketch entity.
    pub entity: SketchEntityId,
    /// Whether traversal opposes the entity's stored direction.
    #[serde(default)]
    pub reversed: bool,
}

/// Solved geometry belonging to one sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchEntity {
    /// Globally unique entity id.
    id: SketchEntityId,
    /// Owning sketch.
    pub sketch: SketchId,
    /// Whether the entity is construction geometry.
    #[serde(default)]
    pub construction: bool,
    /// Source-native geometry record represented by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
    /// Source-native curve carrier represented by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_ref: Option<String>,
    /// Source-native endpoint records in stored entity direction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoint_refs: Vec<String>,
    /// Solved two-dimensional geometry.
    pub geometry: SketchGeometry,
}

impl SketchEntity {
    /// Construct a sketch entity from its id, owning sketch, and geometry.
    pub fn new(id: SketchEntityId, sketch: SketchId, geometry: SketchGeometry) -> Self {
        Self {
            id,
            sketch,
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        }
    }

    /// Return the globally unique entity id.
    pub fn id(&self) -> &SketchEntityId {
        &self.id
    }

    /// Set whether this entity is construction geometry.
    #[must_use]
    pub fn with_construction(mut self, construction: bool) -> Self {
        self.construction = construction;
        self
    }

    /// Set the source-native geometry record.
    #[must_use]
    pub fn with_native_ref(mut self, native_ref: Option<String>) -> Self {
        self.native_ref = native_ref;
        self
    }

    /// Set the source-native curve carrier.
    #[must_use]
    pub fn with_geometry_ref(mut self, geometry_ref: Option<String>) -> Self {
        self.geometry_ref = geometry_ref;
        self
    }

    /// Set the source-native endpoint records.
    #[must_use]
    pub fn with_endpoint_refs(mut self, endpoint_refs: Vec<String>) -> Self {
        self.endpoint_refs = endpoint_refs;
        self
    }
}

/// Solved two-dimensional sketch geometry with finite numeric coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchGeometryDefinition")]
pub struct SketchGeometry(SketchGeometryDefinition);

impl SketchGeometry {
    /// Retain source-native geometry without solved numeric fields.
    #[must_use]
    pub fn native(native_kind: NonEmptyString) -> Self {
        Self(SketchGeometryDefinition::Native { native_kind })
    }

    /// Admit an already checked planar NURBS curve.
    #[must_use]
    pub fn nurbs(curve: crate::geometry::PcurveNurbs) -> Self {
        Self(SketchGeometryDefinition::Nurbs { curve })
    }

    /// Borrow the admitted geometry definition.
    #[must_use]
    pub fn definition(&self) -> &SketchGeometryDefinition {
        &self.0
    }

    /// Extract the admitted definition.
    #[must_use]
    pub fn into_definition(self) -> SketchGeometryDefinition {
        self.0
    }

    /// Replace the definition only after its numeric invariants pass.
    pub fn edit(
        &mut self,
        edit: impl FnOnce(&mut SketchGeometryDefinition),
    ) -> Result<(), &'static str> {
        let mut definition = self.0.clone();
        edit(&mut definition);
        *self = definition.try_into()?;
        Ok(())
    }
}

impl TryFrom<SketchGeometryDefinition> for SketchGeometry {
    type Error = &'static str;

    fn try_from(definition: SketchGeometryDefinition) -> Result<Self, Self::Error> {
        let finite_point = |point: &Point2| point.u.is_finite() && point.v.is_finite();
        let positive = |length: &Length| length.get() > 0.0;
        match &definition {
            SketchGeometryDefinition::Point { position } if !finite_point(position) => {
                return Err("sketch point position must be finite");
            }
            SketchGeometryDefinition::Line { start, end }
                if !finite_point(start) || !finite_point(end) =>
            {
                return Err("sketch line endpoints must be finite");
            }
            SketchGeometryDefinition::ReferenceLine { origin, direction }
                if !finite_point(origin)
                    || !finite_point(direction)
                    || direction.u.hypot(direction.v) <= f64::EPSILON =>
            {
                return Err(
                    "sketch reference line requires finite origin and nonzero finite direction",
                );
            }
            SketchGeometryDefinition::Circle { center, radius }
            | SketchGeometryDefinition::Arc { center, radius, .. }
                if !finite_point(center) || !positive(radius) =>
            {
                return Err(
                    "sketch circular geometry requires finite center and positive finite radius",
                );
            }
            SketchGeometryDefinition::Ellipse {
                center,
                major_angle: _,
                major_radius,
                minor_radius,
                bounds: _,
            } => {
                if !finite_point(center) {
                    return Err("sketch ellipse center and major_angle must be finite");
                }
                if !positive(major_radius) || !positive(minor_radius) {
                    return Err("sketch ellipse radii must be positive and finite");
                }
                if major_radius.get() < minor_radius.get() {
                    return Err("sketch ellipse major_radius must be at least minor_radius");
                }
            }
            SketchGeometryDefinition::Hyperbola {
                center,
                major_angle: _,
                major_radius,
                minor_radius,
                bounds,
            } => {
                if !finite_point(center) {
                    return Err("sketch hyperbola center and major_angle must be finite");
                }
                if !positive(major_radius) || !positive(minor_radius) {
                    return Err("sketch hyperbola radii must be positive and finite");
                }
                if bounds.iter().flatten().any(|value| !value.is_finite()) {
                    return Err("sketch hyperbola bounds must be finite");
                }
            }
            SketchGeometryDefinition::Parabola {
                vertex,
                axis_angle: _,
                focal_length,
                bounds,
            } => {
                if !finite_point(vertex) {
                    return Err("sketch parabola vertex and axis_angle must be finite");
                }
                if !positive(focal_length) {
                    return Err("sketch parabola focal_length must be positive and finite");
                }
                if bounds.iter().flatten().any(|value| !value.is_finite()) {
                    return Err("sketch parabola bounds must be finite");
                }
            }
            SketchGeometryDefinition::Text {
                height,
                width_factor,
                placement,
                ..
            } => {
                if !positive(height) {
                    return Err("sketch text height must be positive and finite");
                }
                if width_factor.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                    return Err("sketch text width_factor must be positive and finite");
                }
                if placement.is_some_and(|placement| !finite_point(&placement.anchor)) {
                    return Err("sketch text anchor and rotation must be finite");
                }
            }
            SketchGeometryDefinition::Point { .. }
            | SketchGeometryDefinition::Line { .. }
            | SketchGeometryDefinition::ReferenceLine { .. }
            | SketchGeometryDefinition::Circle { .. }
            | SketchGeometryDefinition::Arc { .. }
            | SketchGeometryDefinition::Nurbs { .. }
            | SketchGeometryDefinition::ExternalReference { .. }
            | SketchGeometryDefinition::Native { .. } => {}
        }
        Ok(Self(definition))
    }
}

/// Definition admitted by a solved two-dimensional sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchGeometryDefinition {
    /// Isolated point.
    Point {
        /// Solved point position.
        position: Point2,
    },
    /// Bounded line segment.
    Line {
        /// Segment start.
        start: Point2,
        /// Segment end.
        end: Point2,
    },
    /// Unbounded construction or reference line.
    ReferenceLine {
        /// Point on the line.
        origin: Point2,
        /// Non-zero direction in sketch coordinates.
        direction: Point2,
    },
    /// Full circle.
    Circle {
        /// Circle center.
        center: Point2,
        /// Circle radius.
        radius: Length,
    },
    /// Circular arc with angles in radians.
    Arc {
        /// Arc center.
        center: Point2,
        /// Arc radius.
        radius: Length,
        /// Start angle.
        start_angle: Angle,
        /// End angle.
        end_angle: Angle,
    },
    /// Full or bounded ellipse.
    Ellipse {
        /// Ellipse center.
        center: Point2,
        /// Major-axis angle in sketch coordinates.
        major_angle: Angle,
        /// Semi-major radius.
        major_radius: Length,
        /// Semi-minor radius.
        minor_radius: Length,
        /// Parameter bounds for an arc; absent for a full ellipse.
        #[serde(flatten, with = "angle_bounds_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "AngleBoundsWire"))]
        bounds: Option<[Angle; 2]>,
    },
    /// Full or bounded hyperbola.
    Hyperbola {
        /// Hyperbola center.
        center: Point2,
        /// Major-axis angle in sketch coordinates.
        major_angle: Angle,
        /// Semi-major radius.
        major_radius: Length,
        /// Semi-minor radius.
        minor_radius: Length,
        /// Parameter bounds for a branch; absent for the full curve.
        #[serde(flatten, with = "parameter_bounds_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "ParameterBoundsWire"))]
        bounds: Option<[f64; 2]>,
    },
    /// Full or bounded parabola.
    Parabola {
        /// Parabola vertex.
        vertex: Point2,
        /// Symmetry-axis angle in sketch coordinates.
        axis_angle: Angle,
        /// Distance from the vertex to the focus.
        focal_length: Length,
        /// Parameter bounds for a branch; absent for the full curve.
        #[serde(flatten, with = "parameter_bounds_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "ParameterBoundsWire"))]
        bounds: Option<[f64; 2]>,
    },
    /// NURBS curve in sketch coordinates.
    Nurbs {
        /// Checked two-dimensional knot, pole, and weight payload.
        #[serde(flatten)]
        curve: crate::geometry::PcurveNurbs,
    },
    /// Text placed in sketch coordinates.
    Text {
        /// Unicode text content.
        text: NonEmptyString,
        /// Source font-family name.
        font_family: NonEmptyString,
        /// Font weight from the source text style.
        font_weight: SketchFontWeight,
        /// Nominal character height.
        height: Length,
        /// Horizontal scale relative to the nominal font width, absent when the
        /// source stores none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width_factor: Option<f64>,
        /// Text placement in sketch coordinates, absent when the source stores none.
        #[serde(flatten, with = "text_placement_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "TextPlacementWire"))]
        placement: Option<TextPlacement>,
        /// Horizontal placement about the text anchor, when the source class
        /// carries an alignment enum.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        horizontal_alignment: Option<SketchTextHorizontalAlignment>,
        /// Vertical placement about the text anchor, when the source class
        /// carries an alignment enum.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vertical_alignment: Option<SketchTextVerticalAlignment>,
    },
    /// Geometry referenced from another object when no solved sketch-space carrier is stored.
    ExternalReference {
        /// External document identity, absent for a reference within the current document.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        document: Option<String>,
        /// Referenced object identity.
        #[serde(deserialize_with = "deserialize_object")]
        object: NonEmptyString,
        /// Ordered source subelement selectors.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        subelements: Vec<String>,
    },
    /// Source-native geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        #[serde(deserialize_with = "deserialize_native_kind")]
        native_kind: NonEmptyString,
    },
}

/// Placement of sketch text about one anchor point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextPlacement {
    /// Point the text is placed and rotated about, in sketch coordinates.
    pub anchor: Point2,
    /// Counterclockwise rotation from the sketch u axis.
    pub rotation: Angle,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TextPlacementWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    anchor: Option<Point2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotation: Option<Angle>,
}

mod text_placement_wire {
    use super::{TextPlacement, TextPlacementWire};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // Serde passes the borrowed field to this adapter.
    #[allow(clippy::ref_option)]
    pub fn serialize<S>(value: &Option<TextPlacement>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        TextPlacementWire {
            anchor: value.map(|placement| placement.anchor),
            rotation: value.map(|placement| placement.rotation),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<TextPlacement>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = TextPlacementWire::deserialize(deserializer)?;
        match (wire.anchor, wire.rotation) {
            (None, None) => Ok(None),
            (Some(anchor), Some(rotation)) => Ok(Some(TextPlacement { anchor, rotation })),
            _ => Err(serde::de::Error::custom(
                "text anchor and rotation must be present together",
            )),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct AngleBoundsWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start_angle: Option<Angle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end_angle: Option<Angle>,
}

mod angle_bounds_wire {
    use super::{Angle, AngleBoundsWire};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // Serde passes the borrowed field to this adapter.
    #[allow(clippy::ref_option)]
    pub fn serialize<S>(value: &Option<[Angle; 2]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let [start_angle, end_angle] =
            (*value).map_or([None, None], |[start, end]| [Some(start), Some(end)]);
        AngleBoundsWire {
            start_angle,
            end_angle,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<[Angle; 2]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = AngleBoundsWire::deserialize(deserializer)?;
        match (wire.start_angle, wire.end_angle) {
            (None, None) => Ok(None),
            (Some(start), Some(end)) => Ok(Some([start, end])),
            _ => Err(serde::de::Error::custom(
                "start_angle and end_angle must be present together",
            )),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ParameterBoundsWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start_parameter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end_parameter: Option<f64>,
}

mod parameter_bounds_wire {
    use super::ParameterBoundsWire;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // Serde passes the borrowed field to this adapter.
    #[allow(clippy::ref_option)]
    pub fn serialize<S>(value: &Option<[f64; 2]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let [start_parameter, end_parameter] =
            value.map_or([None, None], |[start, end]| [Some(start), Some(end)]);
        ParameterBoundsWire {
            start_parameter,
            end_parameter,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<[f64; 2]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ParameterBoundsWire::deserialize(deserializer)?;
        match (wire.start_parameter, wire.end_parameter) {
            (None, None) => Ok(None),
            (Some(start), Some(end)) => Ok(Some([start, end])),
            _ => Err(serde::de::Error::custom(
                "start_parameter and end_parameter must be present together",
            )),
        }
    }
}

/// A sketch whose solved geometry is expressed directly in model space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketch {
    /// Globally unique spatial-sketch id.
    pub id: SpatialSketchId,
    /// Source display name, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source configuration key, when scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Source display visibility, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Ordered closed profile loops with profile-local planes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<SpatialSketchProfile>,
    /// Identifier of the full-fidelity native input lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

const EPS_SPATIAL_PROFILE_FRAME: f64 = 1.0e-9;

/// One closed spatial-sketch profile and its admitted model-space plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SpatialSketchProfileWire")]
pub struct SpatialSketchProfile {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
    boundary: Vec<SpatialSketchEntityUse>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SpatialSketchProfileWire {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
    boundary: Vec<SpatialSketchEntityUse>,
}

impl TryFrom<SpatialSketchProfileWire> for SpatialSketchProfile {
    type Error = &'static str;

    fn try_from(wire: SpatialSketchProfileWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.origin, wire.normal, wire.u_axis, wire.boundary)
    }
}

impl SpatialSketchProfile {
    /// Admit a finite plane with unit orthogonal axes and a nonempty distinct boundary.
    pub fn try_new(
        origin: Point3,
        normal: Vector3,
        u_axis: Vector3,
        boundary: Vec<SpatialSketchEntityUse>,
    ) -> Result<Self, &'static str> {
        if !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite() {
            return Err("spatial profile origin must be finite");
        }
        let normal_length = normal.norm();
        let u_length = u_axis.norm();
        let dot = normal.x * u_axis.x + normal.y * u_axis.y + normal.z * u_axis.z;
        if !normal_length.is_finite()
            || !u_length.is_finite()
            || !dot.is_finite()
            || (normal_length - 1.0).abs() > EPS_SPATIAL_PROFILE_FRAME
            || (u_length - 1.0).abs() > EPS_SPATIAL_PROFILE_FRAME
            || dot.abs() > EPS_SPATIAL_PROFILE_FRAME
        {
            return Err("spatial profile normal and u_axis must be unit and orthogonal");
        }
        let unique = boundary
            .iter()
            .map(|use_| &use_.entity)
            .collect::<std::collections::HashSet<_>>();
        if boundary.is_empty() || unique.len() != boundary.len() {
            return Err("spatial profile boundary must be nonempty and contain distinct entities");
        }
        Ok(Self {
            origin,
            normal,
            u_axis,
            boundary,
        })
    }

    /// Profile-plane origin in model space.
    #[must_use]
    pub fn origin(&self) -> Point3 {
        self.origin
    }

    /// Profile-plane unit normal.
    #[must_use]
    pub fn normal(&self) -> Vector3 {
        self.normal
    }

    /// Profile-plane unit u-axis.
    #[must_use]
    pub fn u_axis(&self) -> Vector3 {
        self.u_axis
    }

    /// Ordered oriented boundary uses.
    #[must_use]
    pub fn boundary(&self) -> &[SpatialSketchEntityUse] {
        &self.boundary
    }

    /// Replace the origin after finite-coordinate admission.
    pub fn set_origin(&mut self, origin: Point3) -> Result<(), &'static str> {
        if !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite() {
            return Err("spatial profile origin must be finite");
        }
        self.origin = origin;
        Ok(())
    }
}

/// Oriented use of one spatial-sketch entity in a profile boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchEntityUse {
    /// Referenced spatial-sketch entity.
    pub entity: SpatialSketchEntityId,
    /// Whether traversal opposes the entity's stored direction.
    #[serde(default)]
    pub reversed: bool,
}

/// Solved model-space geometry belonging to one spatial sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchEntity {
    /// Globally unique spatial entity id.
    id: SpatialSketchEntityId,
    /// Owning spatial sketch.
    pub sketch: SpatialSketchId,
    /// Whether the entity is construction geometry.
    #[serde(default)]
    pub construction: bool,
    /// Source-native geometry record represented by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
    /// Source-native curve carrier represented by this entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_ref: Option<String>,
    /// Source-native endpoint records in stored entity direction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoint_refs: Vec<String>,
    /// Solved model-space geometry.
    pub geometry: SpatialSketchGeometry,
}

impl SpatialSketchEntity {
    /// Construct a spatial sketch entity from its id, owning sketch, and geometry.
    pub fn new(
        id: SpatialSketchEntityId,
        sketch: SpatialSketchId,
        geometry: SpatialSketchGeometry,
    ) -> Self {
        Self {
            id,
            sketch,
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        }
    }

    /// Return the globally unique spatial entity id.
    pub fn id(&self) -> &SpatialSketchEntityId {
        &self.id
    }

    /// Set whether this entity is construction geometry.
    #[must_use]
    pub fn with_construction(mut self, construction: bool) -> Self {
        self.construction = construction;
        self
    }

    /// Set the source-native geometry record.
    #[must_use]
    pub fn with_native_ref(mut self, native_ref: Option<String>) -> Self {
        self.native_ref = native_ref;
        self
    }

    /// Set the source-native curve carrier.
    #[must_use]
    pub fn with_geometry_ref(mut self, geometry_ref: Option<String>) -> Self {
        self.geometry_ref = geometry_ref;
        self
    }

    /// Set the source-native endpoint records.
    #[must_use]
    pub fn with_endpoint_refs(mut self, endpoint_refs: Vec<String>) -> Self {
        self.endpoint_refs = endpoint_refs;
        self
    }
}

/// One geometric relation owned by a spatial sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchConstraint {
    /// Globally unique constraint id.
    pub id: SketchConstraintId,
    /// Owning spatial sketch.
    pub sketch: SpatialSketchId,
    /// Neutral relation semantics.
    pub definition: SpatialSketchConstraintDefinition,
    /// Source-native relation represented by this constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// One unordered entity pair in a repeated model-space sketch relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchEntityPair {
    /// First member in source discovery order.
    pub first: SpatialSketchEntityId,
    /// Second member in source discovery order.
    pub second: SpatialSketchEntityId,
}

const EPS_SPATIAL_CONSTRAINT_UNIT: f64 = 1.0e-9;

/// A spatial sketch constraint with admitted local members and scalar values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SpatialSketchConstraintDefinitionInput")]
pub struct SpatialSketchConstraintDefinition(SpatialSketchConstraintDefinitionInput);

impl SpatialSketchConstraintDefinition {
    /// Borrow the admitted spatial constraint kind.
    #[must_use]
    pub fn kind(&self) -> &SpatialSketchConstraintDefinitionInput {
        &self.0
    }

    /// Replace the kind only after all edited local invariants pass.
    pub fn edit<R>(
        &mut self,
        edit: impl FnOnce(&mut SpatialSketchConstraintDefinitionInput) -> R,
    ) -> Result<R, &'static str> {
        let mut kind = self.0.clone();
        let result = edit(&mut kind);
        *self = kind.try_into()?;
        Ok(result)
    }
}

impl TryFrom<SpatialSketchConstraintDefinitionInput> for SpatialSketchConstraintDefinition {
    type Error = &'static str;

    fn try_from(kind: SpatialSketchConstraintDefinitionInput) -> Result<Self, Self::Error> {
        use SpatialSketchConstraintDefinitionInput as Kind;
        let unit = |direction: &Vector3| {
            let norm = direction.norm();
            norm.is_finite() && (norm - 1.0).abs() <= EPS_SPATIAL_CONSTRAINT_UNIT
        };
        let valid = match &kind {
            Kind::Native { operands, .. } => !operands.is_empty(),
            Kind::LineLength { .. } => true,
            Kind::Coincident { first, second }
            | Kind::Tangent { first, second }
            | Kind::PointDistance { first, second, .. }
            | Kind::ParallelLineDistance { first, second, .. } => first != second,
            Kind::Symmetric {
                first,
                second,
                axis,
            } => first != second && first != axis && second != axis,
            Kind::PointOnSurface { point, surface } => point != surface,
            Kind::Midpoint { point, entity } => point != entity,
            Kind::PointLineDistance { point, line, .. } => point != line,
            Kind::RepeatedLineLength { entities, .. } | Kind::SplineGroup { entities } => {
                entities.len() >= 2
                    && entities
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == entities.len()
            }
            Kind::RepeatedParallelLineDistance { pairs, .. } => {
                let mut entities = std::collections::HashSet::new();
                pairs.len() >= 2
                    && pairs
                        .iter()
                        .all(|pair| entities.insert(&pair.first) && entities.insert(&pair.second))
            }
            Kind::ParallelLineSetDistance { first, second, .. } => {
                !first.is_empty()
                    && !second.is_empty()
                    && (first.len() > 1 || second.len() > 1)
                    && first
                        .iter()
                        .chain(second)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == first.len() + second.len()
            }
            Kind::Offset {
                sources,
                results,
                normal,
                distance,
                ..
            } => {
                !sources.is_empty()
                    && !results.is_empty()
                    && unit(normal)
                    && distance.get() > 0.0
                    && sources
                        .iter()
                        .chain(results)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == sources.len() + results.len()
            }
            Kind::ParallelToDirection { direction, .. } => unit(direction),
        };
        if !valid {
            return Err("invalid spatial sketch constraint local arity or scalar value");
        }
        Ok(Self(kind))
    }
}

/// Neutral geometric relations between model-space sketch entities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpatialSketchConstraintDefinitionInput {
    /// Source-native spatial relation without complete neutral semantics.
    Native {
        /// Source relation family.
        #[serde(deserialize_with = "deserialize_native_kind")]
        native_kind: NonEmptyString,
        /// Source relation state or subtype discriminator, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_state: Option<u64>,
        /// Neutral parameter driving the relation, when resolved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<crate::features::ParameterId>,
        /// Full-fidelity source operands in field order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        operands: Vec<SketchNativeOperand>,
    },
    /// Two model-space sketch points occupy the same solved position.
    Coincident {
        /// First coincident point.
        first: SpatialSketchEntityId,
        /// Second coincident point.
        second: SpatialSketchEntityId,
    },
    /// Two model-space sketch points are mirror images across a model-space line.
    Symmetric {
        /// First symmetric point.
        first: SpatialSketchEntityId,
        /// Second symmetric point.
        second: SpatialSketchEntityId,
        /// Bounded line whose infinite carrier is the reflection axis.
        axis: SpatialSketchEntityId,
    },
    /// A model-space point lies on a model-space surface.
    PointOnSurface {
        /// Point constrained to the surface.
        point: SpatialSketchEntityId,
        /// Surface containing the point.
        surface: SpatialSketchEntityId,
    },
    /// A model-space point lies at the midpoint of a bounded line.
    Midpoint {
        /// Point constrained to the midpoint.
        point: SpatialSketchEntityId,
        /// Bounded line whose midpoint is used.
        entity: SpatialSketchEntityId,
    },
    /// Two model-space curves are tangent.
    Tangent {
        /// First tangent curve.
        first: SpatialSketchEntityId,
        /// Second tangent curve.
        second: SpatialSketchEntityId,
    },
    /// Euclidean distance between two model-space sketch points.
    PointDistance {
        /// First measured point.
        first: SpatialSketchEntityId,
        /// Second measured point.
        second: SpatialSketchEntityId,
        /// Driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// Distance from a model-space point to an infinite model-space line.
    PointLineDistance {
        /// Measured point.
        point: SpatialSketchEntityId,
        /// Bounded entity supplying the infinite line carrier.
        line: SpatialSketchEntityId,
        /// Driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// Endpoint-to-endpoint length of one bounded model-space sketch line.
    LineLength {
        /// Measured line.
        entity: SpatialSketchEntityId,
        /// Driving length parameter.
        parameter: crate::features::ParameterId,
    },
    /// Endpoint-to-endpoint lengths of multiple bounded model-space sketch lines.
    RepeatedLineLength {
        /// Distinct measured lines in spatial-sketch entity order.
        entities: Vec<SpatialSketchEntityId>,
        /// Shared driving length parameter.
        parameter: crate::features::ParameterId,
    },
    /// Minimum separation between two parallel model-space sketch lines.
    ParallelLineDistance {
        /// First measured line.
        first: SpatialSketchEntityId,
        /// Second measured line.
        second: SpatialSketchEntityId,
        /// Driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// Repeated separation between disjoint pairs of parallel lines.
    RepeatedParallelLineDistance {
        /// Distinct line pairs in profile traversal order.
        pairs: Vec<SpatialSketchEntityPair>,
        /// Shared driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// Minimum separation between two parallel collinear model-space line sets.
    ParallelLineSetDistance {
        /// Collinear entities forming the first line carrier.
        first: Vec<SpatialSketchEntityId>,
        /// Collinear entities forming the second line carrier.
        second: Vec<SpatialSketchEntityId>,
        /// Driving distance parameter.
        parameter: crate::features::ParameterId,
    },
    /// A model-space curve set generated at one offset distance from a source set.
    Offset {
        /// Source curves in native relation order.
        sources: Vec<SpatialSketchEntityId>,
        /// Generated curves in native relation order.
        ///
        /// Position does not imply pairwise geometric correspondence with
        /// `sources`; an offset operation can change curve carriers or topology.
        results: Vec<SpatialSketchEntityId>,
        /// Unit normal of the result curve set's common plane.
        normal: Vector3,
        /// Strictly positive operation-level offset magnitude.
        distance: crate::scalar::Length,
        /// Signed driving offset-distance parameter, when dimensional.
        #[serde(flatten, with = "offset_parameter_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "OffsetParameterWire"))]
        parameter: Option<OffsetParameter>,
    },
    /// A model-space line is parallel to one fixed model-space direction.
    ParallelToDirection {
        /// Line constrained to the direction.
        entity: SpatialSketchEntityId,
        /// Unit model-space direction; either sign denotes the same axis.
        direction: Vector3,
    },
    /// A spline's defining model-space entities grouped by one native relation.
    SplineGroup {
        /// Ordered spline-group members.
        entities: Vec<SpatialSketchEntityId>,
    },
}

/// NURBS curve with positive degree and positive rational weights.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SpatialSketchNurbsCurve(crate::geometry::NurbsCurve);

impl TryFrom<crate::geometry::NurbsCurve> for SpatialSketchNurbsCurve {
    type Error = &'static str;

    fn try_from(curve: crate::geometry::NurbsCurve) -> Result<Self, Self::Error> {
        if curve.degree() == 0 {
            return Err("spatial sketch NURBS degree must be at least one");
        }
        if curve
            .weights()
            .is_some_and(|weights| weights.iter().any(|weight| *weight <= 0.0))
        {
            return Err("spatial sketch NURBS weights must be positive");
        }
        Ok(Self(curve))
    }
}

impl<'de> Deserialize<'de> for SpatialSketchNurbsCurve {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(crate::geometry::NurbsCurve::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

impl std::ops::Deref for SpatialSketchNurbsCurve {
    type Target = crate::geometry::NurbsCurve;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl SpatialSketchNurbsCurve {
    /// Atomically edit control points and preserve finite coordinates.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnOnce(&mut [Point3]),
    ) -> Result<(), crate::geometry::NurbsError> {
        self.0.edit_control_points(edit)
    }
}

const EPS_SPATIAL_LINE_LENGTH: f64 = 1.0e-12;
const EPS_SPATIAL_CIRCLE_FRAME: f64 = 1.0e-9;

/// Spatial-sketch geometry with checked analytic numeric fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SpatialSketchGeometryDefinition")]
pub struct SpatialSketchGeometry(SpatialSketchGeometryDefinition);

impl SpatialSketchGeometry {
    /// Borrow the admitted spatial geometry definition.
    #[must_use]
    pub fn definition(&self) -> &SpatialSketchGeometryDefinition {
        &self.0
    }

    /// Replace the spatial definition only after numeric admission succeeds.
    pub fn edit(
        &mut self,
        edit: impl FnOnce(&mut SpatialSketchGeometryDefinition),
    ) -> Result<(), &'static str> {
        let mut definition = self.0.clone();
        edit(&mut definition);
        *self = definition.try_into()?;
        Ok(())
    }
}

impl TryFrom<SpatialSketchGeometryDefinition> for SpatialSketchGeometry {
    type Error = &'static str;

    fn try_from(definition: SpatialSketchGeometryDefinition) -> Result<Self, Self::Error> {
        let finite_point =
            |point: &Point3| point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
        match &definition {
            SpatialSketchGeometryDefinition::Point { position } if !finite_point(position) => {
                return Err("spatial sketch point position must be finite");
            }
            SpatialSketchGeometryDefinition::Line { start, end } => {
                let distance = (end.x - start.x)
                    .hypot(end.y - start.y)
                    .hypot(end.z - start.z);
                if !finite_point(start) || !finite_point(end) || distance <= EPS_SPATIAL_LINE_LENGTH
                {
                    return Err("spatial sketch line endpoints must be finite and separated");
                }
            }
            SpatialSketchGeometryDefinition::Circle {
                center,
                normal,
                reference_direction,
                radius,
            }
            | SpatialSketchGeometryDefinition::Arc {
                center,
                normal,
                reference_direction,
                radius,
                ..
            } => {
                if !finite_point(center) || radius.get() <= 0.0 {
                    return Err("spatial circular geometry requires finite center and positive finite radius");
                }
                let normal_length = normal.norm();
                let reference_length = reference_direction.norm();
                let orthogonal = (normal.x * reference_direction.x
                    + normal.y * reference_direction.y
                    + normal.z * reference_direction.z)
                    .abs()
                    <= EPS_SPATIAL_CIRCLE_FRAME;
                if !normal_length.is_finite()
                    || !reference_length.is_finite()
                    || (normal_length - 1.0).abs() > EPS_SPATIAL_CIRCLE_FRAME
                    || (reference_length - 1.0).abs() > EPS_SPATIAL_CIRCLE_FRAME
                    || !orthogonal
                {
                    return Err("spatial circular normal and reference_direction must be unit and orthogonal");
                }
                if let SpatialSketchGeometryDefinition::Arc {
                    start_angle,
                    end_angle,
                    ..
                } = &definition
                {
                    if start_angle == end_angle {
                        return Err("spatial sketch arc angles must be finite and distinct");
                    }
                }
            }
            SpatialSketchGeometryDefinition::Point { .. }
            | SpatialSketchGeometryDefinition::Nurbs { .. }
            | SpatialSketchGeometryDefinition::NurbsSurface { .. }
            | SpatialSketchGeometryDefinition::Native { .. } => {}
        }
        Ok(Self(definition))
    }
}

/// Definition admitted by model-space spatial-sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpatialSketchGeometryDefinition {
    /// Model-space point.
    Point {
        /// Point position in model coordinates.
        position: Point3,
    },
    /// Bounded model-space line segment.
    Line {
        /// Segment start in model coordinates.
        start: Point3,
        /// Segment end in model coordinates.
        end: Point3,
    },
    /// Oriented full model-space circle.
    Circle {
        /// Circle center in model coordinates.
        center: Point3,
        /// Unit normal defining positive angular travel.
        normal: Vector3,
        /// Unit radial direction at parameter zero.
        reference_direction: Vector3,
        /// Circle radius.
        radius: Length,
    },
    /// Oriented bounded model-space circular arc.
    Arc {
        /// Arc center in model coordinates.
        center: Point3,
        /// Unit normal defining positive angular travel.
        normal: Vector3,
        /// Unit radial direction at parameter zero.
        reference_direction: Vector3,
        /// Arc radius.
        radius: Length,
        /// Inclusive start parameter in radians.
        start_angle: Angle,
        /// Inclusive end parameter in radians.
        end_angle: Angle,
    },
    /// Model-space NURBS curve.
    Nurbs {
        /// Checked model-space knot, pole, and weight payload.
        #[serde(flatten)]
        curve: SpatialSketchNurbsCurve,
    },
    /// Polynomial tensor-product B-spline surface embedded in model space.
    NurbsSurface {
        /// Checked rectangular control grid and full knot vectors.
        #[serde(flatten)]
        surface: crate::geometry::BsplineSurface,
    },
    /// Source-native spatial geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        #[serde(deserialize_with = "deserialize_native_kind")]
        native_kind: NonEmptyString,
    },
}

/// One relation constraining solved sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchConstraint {
    /// Globally unique constraint id.
    pub id: SketchConstraintId,
    /// Owning sketch.
    pub sketch: SketchId,
    /// Constraint semantics.
    #[serde(deserialize_with = "deserialize_sketch_constraint_definition")]
    pub definition: SketchConstraintDefinition,
    /// User-visible constraint name, when assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether this dimensional relation drives geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driving: Option<bool>,
    /// Whether the solver currently applies this relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    /// Whether the relation belongs to virtual sketch space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_space: Option<bool>,
    /// Whether the relation is displayed in the sketch UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Source orientation bit field, when the relation carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<u32>,
    /// Persisted label offset from the constrained geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_distance: Option<SketchLabelValue>,
    /// Persisted position along the dimension label path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_position: Option<SketchLabelValue>,
    /// Application metadata text attached to this relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    /// Source-native relation record when decoded from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

fn deserialize_sketch_constraint_definition<'de, D>(
    deserializer: D,
) -> Result<SketchConstraintDefinition, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut value = serde_json::Value::deserialize(deserializer)?;
    if let Some(object) = value.as_object_mut() {
        let axis = match object.get("kind").and_then(serde_json::Value::as_str) {
            Some("horizontal_loci" | "horizontal_points") => Some("v"),
            Some("vertical_loci" | "vertical_points") => Some("u"),
            _ => None,
        };
        if let Some(axis) = axis {
            object.insert(
                "kind".into(),
                serde_json::Value::String("same_coordinate".into()),
            );
            object.insert("axis".into(), serde_json::Value::String(axis.into()));
        }
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

/// A geometric locus on a sketch entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "entity", rename_all = "snake_case")]
pub enum SketchLocus {
    /// The complete entity.
    Entity(SketchEntityId),
    /// Stored start point of a bounded entity.
    Start(SketchEntityId),
    /// Stored end point of a bounded entity.
    End(SketchEntityId),
    /// Center of a circle, arc, or ellipse.
    Center(SketchEntityId),
}

/// Coordinate axis selected by a sketch relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SketchCoordinateAxis {
    /// First coordinate in sketch space.
    U,
    /// Second coordinate in sketch space.
    V,
}

/// Source-native field and optional role carrying one sketch operand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NativeOperandField {
    /// Non-empty source-native field name.
    pub name: NonEmptyString,
    /// Source-native role code, when the field carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<u32>,
}

/// One ordered operand retained from a native sketch relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchNativeOperand {
    /// Non-empty source-native operand family.
    pub native_kind: NonEmptyString,
    /// Source-native field and optional role containing this operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<NativeOperandField>,
    /// Source-native object index; absent when the native operand names no
    /// object (an axis, root point, or external reference slot).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_index: Option<u32>,
    /// Resolved source-native operand record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
}

/// One progenitor/result pair in a sketch offset relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchOffsetPair {
    /// Source entity whose stored direction defines the signed offset normal.
    pub source: SketchEntityId,
    /// Entity produced at the shared signed offset distance.
    pub result: SketchEntityId,
    /// Reverse the source's stored traversal before selecting its left normal.
    #[serde(default)]
    pub source_reversed: bool,
}

/// Signed use of a driving offset-distance parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetParameter {
    /// Driving parameter identity.
    pub id: ParameterId,
    /// Whether the stored positive distance is the negation of the parameter.
    pub negated: bool,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct OffsetParameterWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter: Option<ParameterId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_factor: Option<f64>,
}

mod offset_parameter_wire {
    use super::{OffsetParameter, OffsetParameterWire};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // Serde passes the borrowed field to this adapter.
    #[allow(clippy::ref_option)]
    pub fn serialize<S>(value: &Option<OffsetParameter>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        OffsetParameterWire {
            parameter: value.as_ref().map(|parameter| parameter.id.clone()),
            parameter_factor: value
                .as_ref()
                .map(|parameter| if parameter.negated { -1.0 } else { 1.0 }),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<OffsetParameter>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = OffsetParameterWire::deserialize(deserializer)?;
        match (wire.parameter, wire.parameter_factor) {
            (None, None) => Ok(None),
            (Some(id), Some(1.0)) => Ok(Some(OffsetParameter { id, negated: false })),
            (Some(id), Some(-1.0)) => Ok(Some(OffsetParameter { id, negated: true })),
            (Some(_), Some(_)) => Err(serde::de::Error::custom("parameter_factor must be -1 or 1")),
            _ => Err(serde::de::Error::custom(
                "offset parameter and parameter_factor must be present together",
            )),
        }
    }
}

/// One axis of a rectangular sketch pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPatternDirection {
    /// Unit direction in sketch coordinates.
    direction: [f64; 2],
    /// Adjacent-instance spacing along `direction`.
    spacing: Length,
    /// Driving distance parameter and the distance form it controls.
    pub distance: Option<SketchPatternDistance>,
    /// Driving instance-count parameter, when the source exposes it as a neutral parameter.
    pub count_parameter: Option<ParameterId>,
}

const EPS_PATTERN_DIRECTION_UNIT: f64 = 1.0e-9;
const EPS_PATTERN_DIRECTION_ORTHOGONALITY: f64 = 1.0e-9;

impl SketchPatternDirection {
    /// Admit a finite unit direction and finite signed spacing.
    pub fn new(
        direction: [f64; 2],
        spacing: Length,
        distance: Option<SketchPatternDistance>,
        count_parameter: Option<ParameterId>,
    ) -> Option<Self> {
        if !direction.iter().all(|value| value.is_finite())
            || (direction[0].hypot(direction[1]) - 1.0).abs() > EPS_PATTERN_DIRECTION_UNIT
        {
            return None;
        }
        Some(Self {
            direction,
            spacing,
            distance,
            count_parameter,
        })
    }

    /// Unit direction in sketch coordinates.
    #[must_use]
    pub fn direction(&self) -> [f64; 2] {
        self.direction
    }

    /// Adjacent-instance signed spacing.
    #[must_use]
    pub fn spacing(&self) -> Length {
        self.spacing
    }
}

/// Distance form controlled by a rectangular-pattern parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SketchPatternDistance {
    /// The parameter controls adjacent-instance spacing.
    Spacing(ParameterId),
    /// The parameter controls the seed-to-final-instance span.
    Span(ParameterId),
}

impl SketchPatternDistance {
    /// Parameter that controls this distance form.
    #[must_use]
    pub fn parameter(&self) -> &ParameterId {
        match self {
            Self::Spacing(parameter) | Self::Span(parameter) => parameter,
        }
    }
}

/// One resolved rectangular-pattern instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SketchPatternInstance {
    /// Entities in fixed seed-entity order.
    pub entities: Vec<SketchEntityId>,
}

/// One resolved circular-pattern instance.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchCircularPatternInstance {
    /// Signed rotation from the seed instance in radians.
    pub angle: Angle,
    /// Entities in fixed seed-entity order.
    pub entities: Vec<SketchEntityId>,
}

/// Checked two-axis rectangular sketch pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchRectangularPattern {
    directions: [SketchPatternDirection; 2],
    rows: Vec<Vec<SketchPatternInstance>>,
}

impl SketchRectangularPattern {
    /// Construct a non-empty rectangular grid whose instances have one fixed
    /// positive entity arity.
    pub fn new(
        directions: [SketchPatternDirection; 2],
        rows: Vec<Vec<SketchPatternInstance>>,
    ) -> Option<Self> {
        let row_count = u32::try_from(rows.len()).ok()?;
        let column_count = u32::try_from(rows.first()?.len()).ok()?;
        if row_count == 0
            || column_count == 0
            || rows.iter().any(|row| row.len() != column_count as usize)
        {
            return None;
        }
        let entity_arity = rows.first()?.first()?.entities.len();
        if entity_arity == 0
            || rows
                .iter()
                .flatten()
                .any(|instance| instance.entities.len() != entity_arity)
        {
            return None;
        }
        let dot = directions[0].direction[0] * directions[1].direction[0]
            + directions[0].direction[1] * directions[1].direction[1];
        let mut entities = std::collections::HashSet::new();
        if dot.abs() > EPS_PATTERN_DIRECTION_ORTHOGONALITY
            || rows.iter().flatten().any(|instance| {
                instance
                    .entities
                    .iter()
                    .any(|entity| !entities.insert(entity))
            })
        {
            return None;
        }
        Some(Self { directions, rows })
    }

    /// Ordered pattern directions.
    #[must_use]
    pub fn directions(&self) -> &[SketchPatternDirection; 2] {
        &self.directions
    }

    /// Rectangular instance rows. Outer and inner positions are the two
    /// zero-based pattern indices.
    #[must_use]
    pub fn rows(&self) -> &[Vec<SketchPatternInstance>] {
        &self.rows
    }

    /// Number of instances along each direction.
    #[must_use]
    pub fn counts(&self) -> [u32; 2] {
        [self.rows.len() as u32, self.rows[0].len() as u32]
    }
}

/// Checked circular sketch pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchCircularPattern {
    center: SketchEntityId,
    angle: Angle,
    angle_parameter: Option<ParameterId>,
    count_parameter: Option<ParameterId>,
    instances: Vec<SketchCircularPatternInstance>,
}

impl SketchCircularPattern {
    /// Construct a non-empty pattern whose instances have one fixed positive
    /// entity arity.
    pub fn new(
        center: SketchEntityId,
        angle: Angle,
        angle_parameter: Option<ParameterId>,
        count_parameter: Option<ParameterId>,
        instances: Vec<SketchCircularPatternInstance>,
    ) -> Option<Self> {
        u32::try_from(instances.len()).ok()?;
        let entity_arity = instances.first()?.entities.len();
        if entity_arity == 0
            || instances
                .iter()
                .any(|instance| instance.entities.len() != entity_arity)
        {
            return None;
        }
        let mut entities = std::collections::HashSet::new();
        if instances.first()?.angle.get() != 0.0
            || instances.iter().any(|instance| {
                instance
                    .entities
                    .iter()
                    .any(|entity| entity == &center || !entities.insert(entity))
            })
        {
            return None;
        }
        Some(Self {
            center,
            angle,
            angle_parameter,
            count_parameter,
            instances,
        })
    }

    /// Point entity defining the center of rotation.
    #[must_use]
    pub fn center(&self) -> &SketchEntityId {
        &self.center
    }

    /// Evaluated angular span stored by the native pattern.
    #[must_use]
    pub fn angle(&self) -> Angle {
        self.angle
    }

    /// Number of instances, including the seed instance.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.instances.len() as u32
    }

    /// Driving angular-span parameter.
    #[must_use]
    pub fn angle_parameter(&self) -> Option<&ParameterId> {
        self.angle_parameter.as_ref()
    }

    /// Driving instance-count parameter.
    #[must_use]
    pub fn count_parameter(&self) -> Option<&ParameterId> {
        self.count_parameter.as_ref()
    }

    /// Instances in pattern order. Slice position is the zero-based index.
    #[must_use]
    pub fn instances(&self) -> &[SketchCircularPatternInstance] {
        &self.instances
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchPatternDirectionWire {
    direction: [f64; 2],
    spacing: Length,
    count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    spacing_parameter: Option<ParameterId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    span_parameter: Option<ParameterId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    count_parameter: Option<ParameterId>,
}

impl SketchPatternDirectionWire {
    fn from_direction(value: &SketchPatternDirection, count: u32) -> Self {
        Self {
            direction: value.direction,
            spacing: value.spacing,
            count,
            spacing_parameter: match &value.distance {
                Some(SketchPatternDistance::Spacing(parameter)) => Some(parameter.clone()),
                _ => None,
            },
            span_parameter: match &value.distance {
                Some(SketchPatternDistance::Span(parameter)) => Some(parameter.clone()),
                _ => None,
            },
            count_parameter: value.count_parameter.clone(),
        }
    }

    fn into_direction(self) -> Result<SketchPatternDirection, &'static str> {
        let distance = match (self.spacing_parameter, self.span_parameter) {
            (None, None) => None,
            (Some(parameter), None) => Some(SketchPatternDistance::Spacing(parameter)),
            (None, Some(parameter)) => Some(SketchPatternDistance::Span(parameter)),
            (Some(_), Some(_)) => {
                return Err("spacing_parameter and span_parameter are mutually exclusive")
            }
        };
        SketchPatternDirection::new(self.direction, self.spacing, distance, self.count_parameter)
            .ok_or("pattern direction must be finite and unit, with finite spacing")
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchPatternInstanceWire {
    indices: [u32; 2],
    entities: Vec<SketchEntityId>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchCircularPatternInstanceWire {
    index: u32,
    angle: Angle,
    entities: Vec<SketchEntityId>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchRectangularPatternWire {
    directions: [SketchPatternDirectionWire; 2],
    instances: Vec<SketchPatternInstanceWire>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchCircularPatternWire {
    center: SketchEntityId,
    angle: Angle,
    count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle_parameter: Option<ParameterId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    count_parameter: Option<ParameterId>,
    instances: Vec<SketchCircularPatternInstanceWire>,
}

impl Serialize for SketchRectangularPattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let counts = self.counts();
        SketchRectangularPatternWire {
            directions: [
                SketchPatternDirectionWire::from_direction(&self.directions[0], counts[0]),
                SketchPatternDirectionWire::from_direction(&self.directions[1], counts[1]),
            ],
            instances: self
                .rows
                .iter()
                .enumerate()
                .flat_map(|(first, row)| {
                    row.iter().enumerate().map(move |(second, instance)| {
                        SketchPatternInstanceWire {
                            indices: [first as u32, second as u32],
                            entities: instance.entities.clone(),
                        }
                    })
                })
                .collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SketchRectangularPattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = SketchRectangularPatternWire::deserialize(deserializer)?;
        let counts = wire.directions.each_ref().map(|direction| direction.count);
        let row_count = usize::try_from(counts[0]).map_err(serde::de::Error::custom)?;
        let column_count = usize::try_from(counts[1]).map_err(serde::de::Error::custom)?;
        let expected = row_count
            .checked_mul(column_count)
            .ok_or_else(|| serde::de::Error::custom("rectangular pattern count overflows"))?;
        if row_count == 0 || column_count == 0 || wire.instances.len() != expected {
            return Err(serde::de::Error::custom(
                "rectangular pattern directions.count must match instances",
            ));
        }
        if wire
            .instances
            .iter()
            .enumerate()
            .any(|(position, instance)| {
                usize::try_from(instance.indices[0]).ok() != Some(position / column_count)
                    || usize::try_from(instance.indices[1]).ok() != Some(position % column_count)
            })
        {
            return Err(serde::de::Error::custom(
                "rectangular pattern instance indices must match their positions",
            ));
        }
        let [first, second] = wire.directions;
        let directions = [
            first.into_direction().map_err(serde::de::Error::custom)?,
            second.into_direction().map_err(serde::de::Error::custom)?,
        ];
        let mut instances = wire.instances.into_iter();
        let rows = (0..row_count)
            .map(|_| {
                instances
                    .by_ref()
                    .take(column_count)
                    .map(|instance| SketchPatternInstance {
                        entities: instance.entities,
                    })
                    .collect()
            })
            .collect();
        Self::new(directions, rows).ok_or_else(|| {
            serde::de::Error::custom(
                "rectangular pattern instances require one fixed positive entity arity",
            )
        })
    }
}

impl Serialize for SketchCircularPattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        SketchCircularPatternWire {
            center: self.center.clone(),
            angle: self.angle,
            count: self.count(),
            angle_parameter: self.angle_parameter.clone(),
            count_parameter: self.count_parameter.clone(),
            instances: self
                .instances
                .iter()
                .enumerate()
                .map(|(index, instance)| SketchCircularPatternInstanceWire {
                    index: index as u32,
                    angle: instance.angle,
                    entities: instance.entities.clone(),
                })
                .collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SketchCircularPattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = SketchCircularPatternWire::deserialize(deserializer)?;
        if usize::try_from(wire.count).ok() != Some(wire.instances.len()) {
            return Err(serde::de::Error::custom(
                "circular pattern count must match instances",
            ));
        }
        if wire
            .instances
            .iter()
            .enumerate()
            .any(|(index, instance)| u32::try_from(index).ok() != Some(instance.index))
        {
            return Err(serde::de::Error::custom(
                "circular pattern instance index must match its position",
            ));
        }
        let instances = wire
            .instances
            .into_iter()
            .map(|instance| SketchCircularPatternInstance {
                angle: instance.angle,
                entities: instance.entities,
            })
            .collect();
        Self::new(
            wire.center,
            wire.angle,
            wire.angle_parameter,
            wire.count_parameter,
            instances,
        )
        .ok_or_else(|| {
            serde::de::Error::custom(
                "circular pattern instances require one fixed positive entity arity",
            )
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for SketchRectangularPattern {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SketchRectangularPattern".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        SketchRectangularPatternWire::json_schema(generator)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for SketchCircularPattern {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SketchCircularPattern".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        SketchCircularPatternWire::json_schema(generator)
    }
}

/// One independently measured pair within a repeated linear dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchDistanceMeasurement {
    /// Euclidean separation between two loci.
    Distance {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
    },
    /// Horizontal separation between two loci.
    Horizontal {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
    },
    /// Vertical separation between two loci.
    Vertical {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
    },
}

/// One ordered pair of loci whose Euclidean separation participates in an
/// equality relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchDistancePair {
    /// First locus in the measured pair.
    pub first: SketchLocus,
    /// Second locus in the measured pair.
    pub second: SketchLocus,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SolverScalarWire {
    variable_type: u32,
    key: u32,
}

mod solver_scalar_wire {
    use super::SolverScalarWire;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // Serde passes this scalar field by reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn serialize<const CLASS: u32, S: Serializer>(
        key: &u32,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        SolverScalarWire {
            variable_type: CLASS,
            key: *key,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, const CLASS: u32, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<u32, D::Error> {
        let wire = SolverScalarWire::deserialize(deserializer)?;
        if wire.variable_type != CLASS {
            return Err(serde::de::Error::custom(format_args!(
                "variable_type must be {CLASS} for this scalar slot, got {}",
                wire.variable_type
            )));
        }
        Ok(wire.key)
    }
}

/// Meaning of an internal sketch alignment helper relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchInternalAlignment {
    /// Major diameter helper for an ellipse.
    EllipseMajorDiameter,
    /// Minor diameter helper for an ellipse.
    EllipseMinorDiameter,
    /// First ellipse focus helper.
    EllipseFocus1,
    /// Second ellipse focus helper.
    EllipseFocus2,
    /// Hyperbola major-axis helper.
    HyperbolaMajor,
    /// Hyperbola minor-axis helper.
    HyperbolaMinor,
    /// Hyperbola focus helper.
    HyperbolaFocus,
    /// Parabola focus helper.
    ParabolaFocus,
    /// B-spline control-point helper.
    BsplineControlPoint(u32),
    /// B-spline knot-point helper.
    BsplineKnotPoint(u32),
    /// Parabola focal-axis helper.
    ParabolaFocalAxis,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum SketchInternalAlignmentWireKind {
    EllipseMajorDiameter,
    EllipseMinorDiameter,
    EllipseFocus1,
    EllipseFocus2,
    HyperbolaMajor,
    HyperbolaMinor,
    HyperbolaFocus,
    ParabolaFocus,
    BsplineControlPoint,
    BsplineKnotPoint,
    ParabolaFocalAxis,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchInternalAlignmentWire {
    alignment: SketchInternalAlignmentWireKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index: Option<u32>,
}

mod internal_alignment_wire {
    use super::{
        SketchInternalAlignment as Alignment, SketchInternalAlignmentWire as Wire,
        SketchInternalAlignmentWireKind as Kind,
    };
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // Serde passes this alignment field by reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn serialize<S>(value: &Alignment, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (alignment, index) = match *value {
            Alignment::EllipseMajorDiameter => (Kind::EllipseMajorDiameter, None),
            Alignment::EllipseMinorDiameter => (Kind::EllipseMinorDiameter, None),
            Alignment::EllipseFocus1 => (Kind::EllipseFocus1, None),
            Alignment::EllipseFocus2 => (Kind::EllipseFocus2, None),
            Alignment::HyperbolaMajor => (Kind::HyperbolaMajor, None),
            Alignment::HyperbolaMinor => (Kind::HyperbolaMinor, None),
            Alignment::HyperbolaFocus => (Kind::HyperbolaFocus, None),
            Alignment::ParabolaFocus => (Kind::ParabolaFocus, None),
            Alignment::BsplineControlPoint(index) => (Kind::BsplineControlPoint, Some(index)),
            Alignment::BsplineKnotPoint(index) => (Kind::BsplineKnotPoint, Some(index)),
            Alignment::ParabolaFocalAxis => (Kind::ParabolaFocalAxis, None),
        };
        Wire { alignment, index }.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Alignment, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        match (wire.alignment, wire.index) {
            (Kind::EllipseMajorDiameter, None) => Ok(Alignment::EllipseMajorDiameter),
            (Kind::EllipseMinorDiameter, None) => Ok(Alignment::EllipseMinorDiameter),
            (Kind::EllipseFocus1, None) => Ok(Alignment::EllipseFocus1),
            (Kind::EllipseFocus2, None) => Ok(Alignment::EllipseFocus2),
            (Kind::HyperbolaMajor, None) => Ok(Alignment::HyperbolaMajor),
            (Kind::HyperbolaMinor, None) => Ok(Alignment::HyperbolaMinor),
            (Kind::HyperbolaFocus, None) => Ok(Alignment::HyperbolaFocus),
            (Kind::ParabolaFocus, None) => Ok(Alignment::ParabolaFocus),
            (Kind::BsplineControlPoint, Some(index)) => Ok(Alignment::BsplineControlPoint(index)),
            (Kind::BsplineKnotPoint, Some(index)) => Ok(Alignment::BsplineKnotPoint(index)),
            (Kind::ParabolaFocalAxis, None) => Ok(Alignment::ParabolaFocalAxis),
            (Kind::BsplineControlPoint | Kind::BsplineKnotPoint, None) => Err(
                serde::de::Error::custom("B-spline internal alignment requires index"),
            ),
            (_, Some(_)) => Err(serde::de::Error::custom(
                "internal alignment index is only valid for B-spline families",
            )),
        }
    }
}

/// Ordered polygon members with at least three distinct identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchPolygonWire")]
pub struct SketchPolygon {
    entities: Vec<SketchEntityId>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchPolygonWire {
    entities: Vec<SketchEntityId>,
}

impl TryFrom<SketchPolygonWire> for SketchPolygon {
    type Error = &'static str;

    fn try_from(wire: SketchPolygonWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.entities)
    }
}

impl SketchPolygon {
    /// Admits at least three distinct polygon members.
    pub fn try_new(entities: Vec<SketchEntityId>) -> Result<Self, &'static str> {
        if entities.len() < 3
            || entities
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != entities.len()
        {
            return Err("entities requires at least three distinct polygon members");
        }
        Ok(Self { entities })
    }

    /// Returns the ordered polygon members.
    pub fn entities(&self) -> &[SketchEntityId] {
        &self.entities
    }
}

/// Two distinct loci aligned on one sketch coordinate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchSameCoordinateWire")]
pub struct SketchSameCoordinate {
    first: SketchLocus,
    second: SketchLocus,
    axis: SketchCoordinateAxis,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchSameCoordinateWire {
    first: SketchLocus,
    second: SketchLocus,
    axis: SketchCoordinateAxis,
}

impl TryFrom<SketchSameCoordinateWire> for SketchSameCoordinate {
    type Error = &'static str;

    fn try_from(wire: SketchSameCoordinateWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.first, wire.second, wire.axis)
    }
}

impl SketchSameCoordinate {
    /// Admits two distinct loci and their shared coordinate axis.
    pub fn try_new(
        first: SketchLocus,
        second: SketchLocus,
        axis: SketchCoordinateAxis,
    ) -> Result<Self, &'static str> {
        if first == second {
            return Err("first and second require distinct loci");
        }
        Ok(Self {
            first,
            second,
            axis,
        })
    }

    /// Returns the first locus.
    pub fn first(&self) -> &SketchLocus {
        &self.first
    }

    /// Returns the second locus.
    pub fn second(&self) -> &SketchLocus {
        &self.second
    }

    /// Returns the shared coordinate axis.
    pub fn axis(&self) -> SketchCoordinateAxis {
        self.axis
    }
}

/// A finite sketch constraint label coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "f64")]
pub struct SketchLabelValue(f64);

impl TryFrom<f64> for SketchLabelValue {
    type Error = &'static str;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if !value.is_finite() {
            return Err("sketch constraint label coordinate must be finite");
        }
        Ok(Self(value))
    }
}

impl SketchLabelValue {
    /// Return the admitted label coordinate.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

const EPS_POLAR_DISTANCE_ZERO: f64 = 1.0e-12;

/// A sketch constraint definition with admitted local arity and scalar values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchConstraintDefinitionInput")]
pub struct SketchConstraintDefinition(SketchConstraintDefinitionInput);

impl SketchConstraintDefinition {
    /// Borrow the admitted constraint kind.
    #[must_use]
    pub fn kind(&self) -> &SketchConstraintDefinitionInput {
        &self.0
    }

    /// Consume the admitted definition and return its kind.
    #[must_use]
    pub fn into_kind(self) -> SketchConstraintDefinitionInput {
        self.0
    }

    /// Replace the kind only after all edited local invariants pass.
    pub fn edit<R>(
        &mut self,
        edit: impl FnOnce(&mut SketchConstraintDefinitionInput) -> R,
    ) -> Result<R, &'static str> {
        let mut kind = self.0.clone();
        let result = edit(&mut kind);
        *self = kind.try_into()?;
        Ok(result)
    }
}

impl TryFrom<SketchConstraintDefinitionInput> for SketchConstraintDefinition {
    type Error = &'static str;

    fn try_from(kind: SketchConstraintDefinitionInput) -> Result<Self, Self::Error> {
        use SketchConstraintDefinitionInput as Kind;
        let valid = match &kind {
            Kind::Coincident { entities } | Kind::SplineGroup { entities } => entities.len() >= 2,
            Kind::CoincidentLoci { loci } => loci.len() >= 2,
            Kind::Distance { entities, .. } => !entities.is_empty(),
            Kind::TextFrame { text, frame } => {
                !frame.is_empty() && frame.iter().all(|entity| entity != text)
            }
            Kind::TextPath {
                text,
                path,
                glyph_transforms,
            } => text != path && !glyph_transforms.is_empty(),
            Kind::DistanceLociValue { distance, .. } => distance.get() >= 0.0,
            Kind::PolarDistance {
                distance, angle, ..
            } => {
                distance.get() >= 0.0
                    && if distance.get() <= EPS_POLAR_DISTANCE_ZERO {
                        angle.is_none()
                    } else {
                        angle.is_some()
                    }
            }
            Kind::AngleDifference { value, .. } => {
                (0.0..=std::f64::consts::PI).contains(&value.get())
            }
            Kind::ScalarEquality { first, second } => first != second,
            Kind::RepeatedDistance { measurements, .. } => {
                let mut entities = std::collections::HashSet::new();
                !measurements.is_empty()
                    && measurements.iter().all(|measurement| {
                        let (first, second) = match measurement {
                            SketchDistanceMeasurement::Distance { first, second }
                            | SketchDistanceMeasurement::Horizontal { first, second }
                            | SketchDistanceMeasurement::Vertical { first, second } => {
                                (first, second)
                            }
                        };
                        let entity = |locus: &SketchLocus| match locus {
                            SketchLocus::Entity(id)
                            | SketchLocus::Start(id)
                            | SketchLocus::End(id)
                            | SketchLocus::Center(id) => id.clone(),
                        };
                        entities.insert(entity(first)) && entities.insert(entity(second))
                    })
            }
            Kind::RepeatedLength { entities, .. }
            | Kind::RepeatedRadius { entities, .. }
            | Kind::RepeatedDiameter { entities, .. } => {
                entities.len() >= 2
                    && entities
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == entities.len()
            }
            Kind::ParallelLineSetDistance { first, second, .. } => {
                !first.is_empty()
                    && !second.is_empty()
                    && (first.len() > 1 || second.len() > 1)
                    && first
                        .iter()
                        .chain(second)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == first.len() + second.len()
            }
            Kind::Offset {
                pairs, distance, ..
            } => {
                let mut sources = std::collections::HashSet::new();
                let mut results = std::collections::HashSet::new();
                !pairs.is_empty()
                    && distance.get() > 0.0
                    && pairs.iter().all(|pair| {
                        pair.source != pair.result
                            && sources.insert(&pair.source)
                            && results.insert(&pair.result)
                    })
            }
            Kind::ProjectedCopy { source, result } => source != result,
            Kind::Group { elements } | Kind::Text { elements, .. } => !elements.is_empty(),
            Kind::Native {
                entities, operands, ..
            } => !entities.is_empty() || !operands.is_empty(),
            Kind::Disabled => true,
            Kind::Polygon { .. } => true,
            Kind::RectangularPattern { .. } => true,
            Kind::CircularPattern { .. } => true,
            Kind::SameCoordinate { .. } => true,
            Kind::PointOnObject { .. } => true,
            Kind::Midpoint { .. } => true,
            Kind::PointCoordinateValues { .. } => true,
            Kind::MidpointCoordinate { .. } => true,
            Kind::AtIntersection { .. } => true,
            Kind::Concentric { .. } => true,
            Kind::Coradial { .. } => true,
            Kind::Collinear { .. } => true,
            Kind::Symmetric { .. } => true,
            Kind::PointSymmetric { .. } => true,
            Kind::Horizontal { .. } => true,
            Kind::Vertical { .. } => true,
            Kind::Parallel { .. } => true,
            Kind::Perpendicular { .. } => true,
            Kind::Tangent { .. } => true,
            Kind::TangentLoci { .. } => true,
            Kind::Curvature { .. } => true,
            Kind::Equal { .. } => true,
            Kind::Fixed { .. } => true,
            Kind::ArcAngle { .. } => true,
            Kind::EllipseAngle { .. } => true,
            Kind::DistanceLoci { .. } => true,
            Kind::EqualDistance { .. } => true,
            Kind::HorizontalDistance { .. } => true,
            Kind::VerticalDistance { .. } => true,
            Kind::Angle { .. } => true,
            Kind::AngleToAxis { .. } => true,
            Kind::Radius { .. } => true,
            Kind::Diameter { .. } => true,
            Kind::SnellsLaw { .. } => true,
            Kind::Weight { .. } => true,
            Kind::InternalAlignment { .. } => true,
        };
        if !valid {
            return Err("invalid sketch constraint local arity or scalar value");
        }
        Ok(Self(kind))
    }
}

/// Candidate geometric and dimensional sketch relations for checked admission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchConstraintDefinitionInput {
    /// Persisted no-op relation slot.
    Disabled,
    /// Two entity loci coincide.
    Coincident {
        /// Coincident entity loci.
        entities: Vec<SketchEntityId>,
    },
    /// Entities participate in one native polygon relation.
    Polygon {
        /// Checked polygon members.
        #[serde(flatten)]
        polygon: SketchPolygon,
    },
    /// A spline's defining entities grouped by one native spline relation.
    SplineGroup {
        /// Ordered spline-group members: the spline's defining entities and
        /// its curve entity.
        entities: Vec<SketchEntityId>,
    },
    /// A complete two-axis rectangular pattern with resolved instances.
    RectangularPattern {
        /// Checked directions and rectangular instance grid.
        #[serde(flatten)]
        pattern: SketchRectangularPattern,
    },
    /// A parameter-driven circular pattern with geometrically resolved instances.
    CircularPattern {
        /// Checked center, parameters, and positional instances.
        #[serde(flatten)]
        pattern: SketchCircularPattern,
    },
    /// Text entity bounded by ordered frame curves.
    TextFrame {
        /// Text entity owning the frame.
        text: SketchEntityId,
        /// Ordered frame curves.
        frame: Vec<SketchEntityId>,
    },
    /// Text entity laid out along a path curve.
    TextPath {
        /// Text entity placed along the path.
        text: SketchEntityId,
        /// Path curve.
        path: SketchEntityId,
        /// Character placements in text order, expressed in sketch coordinates.
        glyph_transforms: Vec<Transform>,
    },
    /// Two or more explicit entity loci coincide.
    CoincidentLoci {
        /// Coincident endpoints, centers, or complete entities.
        loci: Vec<SketchLocus>,
    },
    /// Two loci share one sketch-space coordinate.
    SameCoordinate {
        /// Checked coordinate relation.
        #[serde(flatten)]
        relation: SketchSameCoordinate,
    },
    /// A point locus lies on another sketch entity.
    PointOnObject {
        /// Point constrained to the supporting entity.
        point: SketchLocus,
        /// Entity on which the point lies.
        entity: SketchEntityId,
    },
    /// A point locus lies at the midpoint of a bounded entity.
    Midpoint {
        /// Point constrained to the midpoint.
        point: SketchLocus,
        /// Bounded entity whose midpoint is used.
        entity: SketchEntityId,
    },
    /// A point locus has fixed values on both sketch coordinate axes.
    PointCoordinateValues {
        /// Point whose two coordinates are constrained.
        point: SketchLocus,
        /// Coordinate values in sketch `u`, then `v`, order.
        values: [Length; 2],
    },
    /// One sketch coordinate is the arithmetic mean of two point loci.
    MidpointCoordinate {
        /// First point contributing to the mean.
        first: SketchLocus,
        /// Second point contributing to the mean.
        second: SketchLocus,
        /// Coordinate axis carrying the mean relation.
        axis: SketchCoordinateAxis,
        /// Source-evaluated coordinate mean.
        value: Length,
    },
    /// One or more entities offset from their progenitors by one signed distance.
    Offset {
        /// Ordered progenitor/result pairs.
        pairs: Vec<SketchOffsetPair>,
        /// Strictly positive common offset magnitude, measured along each
        /// oriented source entity's left normal.
        distance: Length,
        /// Signed driving offset-distance parameter, when dimensional.
        #[serde(flatten, with = "offset_parameter_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "OffsetParameterWire"))]
        parameter: Option<OffsetParameter>,
    },
    /// A regular profile entity copied from a projected reference entity.
    ProjectedCopy {
        /// Projected reference entity that supplies the geometry.
        source: SketchEntityId,
        /// Regular entity used by the profile.
        result: SketchEntityId,
    },
    /// A point locus lies at the intersection of two entities.
    AtIntersection {
        /// Point constrained to the intersection.
        point: SketchLocus,
        /// First intersecting entity.
        first: SketchEntityId,
        /// Second intersecting entity.
        second: SketchEntityId,
    },
    /// Circular or elliptical entities share a center.
    Concentric {
        /// First centered entity.
        first: SketchEntityId,
        /// Second centered entity.
        second: SketchEntityId,
    },
    /// Two circular entities share a center and radius.
    Coradial {
        /// First circular entity.
        first: SketchEntityId,
        /// Second circular entity.
        second: SketchEntityId,
    },
    /// Two line entities lie on one infinite line.
    Collinear {
        /// First line.
        first: SketchEntityId,
        /// Second line.
        second: SketchEntityId,
    },
    /// Two loci are symmetric about a line entity.
    Symmetric {
        /// First symmetric locus.
        first: SketchLocus,
        /// Second symmetric locus.
        second: SketchLocus,
        /// Symmetry axis.
        axis: SketchEntityId,
    },
    /// Two loci are centrally symmetric about a point.
    PointSymmetric {
        /// First symmetric locus.
        first: SketchLocus,
        /// Second symmetric locus.
        second: SketchLocus,
        /// Center of symmetry.
        center: SketchLocus,
    },
    /// Line is horizontal in sketch coordinates.
    Horizontal {
        /// Constrained entity.
        entity: SketchEntityId,
    },
    /// Line is vertical in sketch coordinates.
    Vertical {
        /// Constrained entity.
        entity: SketchEntityId,
    },
    /// Two entities are parallel.
    Parallel {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Two entities are perpendicular.
    Perpendicular {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Two entities are tangent.
    Tangent {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Two bounded entities are tangent at explicit loci.
    TangentLoci {
        /// Tangency locus on the first entity.
        first: SketchLocus,
        /// Tangency locus on the second entity.
        second: SketchLocus,
    },
    /// Two entities have equal tangent direction and curvature at contact.
    Curvature {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Two entities have equal size.
    Equal {
        /// First entity.
        first: SketchEntityId,
        /// Second entity.
        second: SketchEntityId,
    },
    /// Entity is fixed in sketch coordinates.
    Fixed {
        /// Fixed entity.
        entity: SketchEntityId,
    },
    /// Circular arc angle fixed by the relation kind.
    ArcAngle {
        /// Constrained circular arc.
        entity: SketchEntityId,
        /// Fixed positive arc angle in radians.
        angle: Angle,
    },
    /// Bounded ellipse parameter sweep fixed by the relation kind.
    EllipseAngle {
        /// Constrained bounded ellipse.
        entity: SketchEntityId,
        /// Fixed positive parameter sweep in radians.
        angle: Angle,
    },
    /// Distance controlled by a design parameter.
    Distance {
        /// Measured entities.
        entities: Vec<SketchEntityId>,
        /// Driving distance parameter.
        parameter: ParameterId,
    },
    /// Euclidean distance between two explicit loci.
    DistanceLoci {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
        /// Driving distance parameter.
        parameter: ParameterId,
    },
    /// Euclidean distance between two loci with a source-evaluated value.
    DistanceLociValue {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
        /// Non-negative measured distance in model units.
        distance: Length,
        /// Driving distance parameter, when the source supplies one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<ParameterId>,
    },
    /// A second locus is displaced from the first by a polar sketch-space
    /// distance and direction.
    PolarDistance {
        /// Origin locus of the displacement.
        first: SketchLocus,
        /// Displaced locus.
        second: SketchLocus,
        /// Non-negative displacement length in model units.
        distance: Length,
        /// Direction from the sketch-u axis; absent when the displacement is
        /// zero and therefore has no defined direction.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<Angle>,
        /// Driving distance parameter, when the source supplies one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance_parameter: Option<ParameterId>,
    },
    /// Direct difference between two angle-valued solver scalars.
    AngleDifference {
        /// Solver-local key of the first angle scalar.
        #[serde(
            serialize_with = "solver_scalar_wire::serialize::<4, _>",
            deserialize_with = "solver_scalar_wire::deserialize::<4, _>"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "SolverScalarWire"))]
        first: u32,
        /// Solver-local key of the second angle scalar.
        #[serde(
            serialize_with = "solver_scalar_wire::serialize::<4, _>",
            deserialize_with = "solver_scalar_wire::deserialize::<4, _>"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "SolverScalarWire"))]
        second: u32,
        /// Solver-local key of the difference scalar receiving `first - second`.
        #[serde(
            serialize_with = "solver_scalar_wire::serialize::<0, _>",
            deserialize_with = "solver_scalar_wire::deserialize::<0, _>"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "SolverScalarWire"))]
        difference: u32,
        /// Source-evaluated non-negative angle difference in radians.
        value: Angle,
    },
    /// Equality between two equality-class solver scalars.
    ScalarEquality {
        /// Solver-local key of the first equality-class scalar.
        #[serde(
            serialize_with = "solver_scalar_wire::serialize::<6, _>",
            deserialize_with = "solver_scalar_wire::deserialize::<6, _>"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "SolverScalarWire"))]
        first: u32,
        /// Solver-local key of the second equality-class scalar.
        #[serde(
            serialize_with = "solver_scalar_wire::serialize::<6, _>",
            deserialize_with = "solver_scalar_wire::deserialize::<6, _>"
        )]
        #[cfg_attr(feature = "schema", schemars(with = "SolverScalarWire"))]
        second: u32,
    },
    /// Two explicit Euclidean locus pairs have equal separation.
    EqualDistance {
        /// First measured locus pair.
        first: SketchDistancePair,
        /// Second measured locus pair.
        second: SketchDistancePair,
    },
    /// Horizontal separation between two explicit loci.
    HorizontalDistance {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
        /// Driving horizontal-distance parameter.
        parameter: ParameterId,
    },
    /// Vertical separation between two explicit loci.
    VerticalDistance {
        /// First measured locus.
        first: SketchLocus,
        /// Second measured locus.
        second: SketchLocus,
        /// Driving vertical-distance parameter.
        parameter: ParameterId,
    },
    /// Multiple disjoint locus pairs controlled by one linear parameter.
    RepeatedDistance {
        /// Ordered independent measurements.
        measurements: Vec<SketchDistanceMeasurement>,
        /// Shared driving distance parameter.
        parameter: ParameterId,
    },
    /// Equal-length line entities controlled by one linear parameter.
    RepeatedLength {
        /// Distinct line entities sharing the driven length.
        entities: Vec<SketchEntityId>,
        /// Shared driving length parameter.
        parameter: ParameterId,
    },
    /// Distance between two parallel collinear line-entity sets.
    ParallelLineSetDistance {
        /// Collinear entities forming the first line carrier.
        first: Vec<SketchEntityId>,
        /// Collinear entities forming the second line carrier.
        second: Vec<SketchEntityId>,
        /// Driving distance parameter.
        parameter: ParameterId,
    },
    /// Angle controlled by a design parameter.
    Angle {
        /// First angular entity.
        first: SketchEntityId,
        /// Second angular entity.
        second: SketchEntityId,
        /// Driving angle parameter.
        parameter: ParameterId,
    },
    /// Angle from a canonical sketch axis to one line entity.
    AngleToAxis {
        /// Measured line entity.
        entity: SketchEntityId,
        /// Canonical sketch reference axis.
        axis: SketchAxis,
        /// Driving angle parameter.
        parameter: ParameterId,
    },
    /// Radius controlled by a design parameter.
    Radius {
        /// Circular or elliptical entity.
        entity: SketchEntityId,
        /// Driving radius parameter.
        parameter: ParameterId,
    },
    /// Equal-radius circular entities controlled by one design parameter.
    RepeatedRadius {
        /// Distinct circular entities sharing the driven radius.
        entities: Vec<SketchEntityId>,
        /// Shared driving radius parameter.
        parameter: ParameterId,
    },
    /// Diameter controlled by a design parameter.
    Diameter {
        /// Circular entity.
        entity: SketchEntityId,
        /// Driving diameter parameter.
        parameter: ParameterId,
    },
    /// Equal-diameter circular entities controlled by one design parameter.
    RepeatedDiameter {
        /// Distinct circular entities sharing the driven diameter.
        entities: Vec<SketchEntityId>,
        /// Shared driving diameter parameter.
        parameter: ParameterId,
    },
    /// Refraction relation between two curve loci and their interface.
    SnellsLaw {
        /// Incident curve locus.
        incident: SketchLocus,
        /// Refracted curve locus.
        refracted: SketchLocus,
        /// Interface entity carrying the surface normal in sketch space.
        interface: SketchEntityId,
        /// Dimensionless refractive-index ratio.
        parameter: ParameterId,
    },
    /// Rational spline weight controlled by a dimensionless parameter.
    Weight {
        /// Weighted spline entity.
        entity: SketchEntityId,
        /// Dimensionless weight parameter.
        parameter: ParameterId,
    },
    /// Relation between generated helper geometry and its parent conic or spline.
    InternalAlignment {
        /// Generated helper geometry.
        helper: SketchEntityId,
        /// Parent geometry receiving the alignment.
        parent: SketchEntityId,
        /// Exact helper relation family, including its B-spline index when required.
        #[serde(flatten, with = "internal_alignment_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "SketchInternalAlignmentWire"))]
        alignment: SketchInternalAlignment,
    },
    /// Ordered geometry grouped under a sketch construction handle.
    Group {
        /// Group handle followed by its ordered member loci.
        elements: Vec<SketchLocus>,
    },
    /// Text constructed from an ordered set of sketch geometry.
    Text {
        /// Text handle followed by its ordered construction loci.
        elements: Vec<SketchLocus>,
        /// Displayed text.
        text: String,
        /// Font family or source font token, when carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font: Option<String>,
        /// Whether the construction dimension controls text height rather than width.
        is_text_height: bool,
    },
    /// Source-native relation not yet reduced to a neutral family.
    Native {
        /// Source constraint family.
        #[serde(deserialize_with = "deserialize_native_kind")]
        native_kind: NonEmptyString,
        /// Source-native constraint-state mask, when the format carries one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_state: Option<u64>,
        /// Source-native constraint flags, when distinct from constraint state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_flags: Option<u64>,
        /// Exact source-native scalar properties not represented by common state or flags.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        native_properties: BTreeMap<String, String>,
        /// Referenced entities.
        entities: Vec<SketchEntityId>,
        /// Driving or driven parameter attached to the relation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<ParameterId>,
        /// Native operands whose neutral loci are unresolved.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        operands: Vec<SketchNativeOperand>,
    },
}

crate::units::named_field!(
    deserialize_object,
    crate::products::NonEmptyString,
    "object"
);
crate::units::named_field!(
    deserialize_native_kind,
    crate::products::NonEmptyString,
    "native_kind"
);

#[cfg(test)]
mod tests;
