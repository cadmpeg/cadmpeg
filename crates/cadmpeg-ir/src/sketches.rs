// SPDX-License-Identifier: Apache-2.0
//! Neutral planar sketches, solved entities, and geometric constraints.

use crate::features::{Angle, Length, ParameterId};
use crate::math::{Point2, Point3, Vector3};
use crate::transform::Transform;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                crate::schema::serialize_reference_id(&self.0, serializer)
            }
        }

        impl $name {
            /// Borrow the underlying id string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

string_id!(SketchId, "Identifies a neutral planar sketch.");
string_id!(SketchEntityId, "Identifies solved geometry in a sketch.");
string_id!(SpatialSketchId, "Identifies a neutral spatial sketch.");
string_id!(
    SpatialSketchEntityId,
    "Identifies solved geometry in a spatial sketch."
);
string_id!(
    SketchConstraintId,
    "Identifies a geometric sketch constraint."
);

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<Vec<SketchEntityUse>>,
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
        /// Sketch-plane origin in model space.
        origin: Point3,
        /// Sketch-plane unit normal.
        normal: Vector3,
        /// Sketch-plane u-axis.
        u_axis: Vector3,
    },
}

impl SketchPlacement {
    /// Return the complete frame when placement is resolved.
    pub fn resolved(self) -> Option<(Point3, Vector3, Vector3)> {
        match self {
            Self::Unresolved => None,
            Self::Resolved {
                origin,
                normal,
                u_axis,
            } => Some((origin, normal, u_axis)),
        }
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
///
/// Prefer [`SketchEntity::new`] for invariant-bearing construction. There is no
/// public [`Default`]: an empty id is illegal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchEntityWire")]
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
        assert!(!id.0.is_empty(), "SketchEntity.id must not be empty");
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
    pub fn with_construction(mut self, construction: bool) -> Self {
        self.construction = construction;
        self
    }

    /// Set the source-native geometry record.
    pub fn with_native_ref(mut self, native_ref: Option<String>) -> Self {
        self.native_ref = native_ref;
        self
    }

    /// Set the source-native curve carrier.
    pub fn with_geometry_ref(mut self, geometry_ref: Option<String>) -> Self {
        self.geometry_ref = geometry_ref;
        self
    }

    /// Set the source-native endpoint records.
    pub fn with_endpoint_refs(mut self, endpoint_refs: Vec<String>) -> Self {
        self.endpoint_refs = endpoint_refs;
        self
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SketchEntityWire {
    id: SketchEntityId,
    sketch: SketchId,
    #[serde(default)]
    construction: bool,
    #[serde(default)]
    native_ref: Option<String>,
    #[serde(default)]
    geometry_ref: Option<String>,
    #[serde(default)]
    endpoint_refs: Vec<String>,
    geometry: SketchGeometry,
}

impl TryFrom<SketchEntityWire> for SketchEntity {
    type Error = &'static str;

    fn try_from(wire: SketchEntityWire) -> Result<Self, Self::Error> {
        if wire.id.0.is_empty() {
            return Err("SketchEntity.id must not be empty");
        }
        Ok(Self {
            id: wire.id,
            sketch: wire.sketch,
            construction: wire.construction,
            native_ref: wire.native_ref,
            geometry_ref: wire.geometry_ref,
            endpoint_refs: wire.endpoint_refs,
            geometry: wire.geometry,
        })
    }
}

/// Solved two-dimensional sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchGeometry {
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
        /// Curve degree.
        degree: u32,
        /// Full knot vector.
        knots: Vec<f64>,
        /// Control points in parameter order.
        control_points: Vec<Point2>,
        /// Per-pole weights; absent for non-rational curves.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weights: Option<Vec<f64>>,
        /// Whether the curve is periodic.
        #[serde(default)]
        periodic: bool,
    },
    /// Text placed in sketch coordinates.
    Text {
        /// Unicode text content.
        text: String,
        /// Source font-family name.
        font_family: String,
        /// Numeric font weight from the source text style.
        font_weight: i32,
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
        object: String,
        /// Ordered source subelement selectors.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        subelements: Vec<String>,
    },
    /// Source-native geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        native_kind: String,
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

    pub fn serialize<S>(value: &Option<[Angle; 2]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let [start_angle, end_angle] = value
            .clone()
            .map_or([None, None], |[start, end]| [Some(start), Some(end)]);
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

/// One closed spatial-sketch profile and its model-space plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SpatialSketchProfile {
    /// Profile-plane origin in model space.
    pub origin: Point3,
    /// Profile-plane unit normal, oriented by boundary traversal.
    pub normal: Vector3,
    /// Profile-plane unit u-axis.
    pub u_axis: Vector3,
    /// Ordered oriented boundary uses.
    pub boundary: Vec<SpatialSketchEntityUse>,
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
///
/// Prefer [`SpatialSketchEntity::new`] for invariant-bearing construction. There
/// is no public [`Default`]: an empty id is illegal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SpatialSketchEntityWire")]
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
        assert!(!id.0.is_empty(), "SpatialSketchEntity.id must not be empty");
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
    pub fn with_construction(mut self, construction: bool) -> Self {
        self.construction = construction;
        self
    }

    /// Set the source-native geometry record.
    pub fn with_native_ref(mut self, native_ref: Option<String>) -> Self {
        self.native_ref = native_ref;
        self
    }

    /// Set the source-native curve carrier.
    pub fn with_geometry_ref(mut self, geometry_ref: Option<String>) -> Self {
        self.geometry_ref = geometry_ref;
        self
    }

    /// Set the source-native endpoint records.
    pub fn with_endpoint_refs(mut self, endpoint_refs: Vec<String>) -> Self {
        self.endpoint_refs = endpoint_refs;
        self
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SpatialSketchEntityWire {
    id: SpatialSketchEntityId,
    sketch: SpatialSketchId,
    #[serde(default)]
    construction: bool,
    #[serde(default)]
    native_ref: Option<String>,
    #[serde(default)]
    geometry_ref: Option<String>,
    #[serde(default)]
    endpoint_refs: Vec<String>,
    geometry: SpatialSketchGeometry,
}

impl TryFrom<SpatialSketchEntityWire> for SpatialSketchEntity {
    type Error = &'static str;

    fn try_from(wire: SpatialSketchEntityWire) -> Result<Self, Self::Error> {
        if wire.id.0.is_empty() {
            return Err("SpatialSketchEntity.id must not be empty");
        }
        Ok(Self {
            id: wire.id,
            sketch: wire.sketch,
            construction: wire.construction,
            native_ref: wire.native_ref,
            geometry_ref: wire.geometry_ref,
            endpoint_refs: wire.endpoint_refs,
            geometry: wire.geometry,
        })
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

/// Neutral geometric relations between model-space sketch entities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpatialSketchConstraintDefinition {
    /// Source-native spatial relation without complete neutral semantics.
    Native {
        /// Source relation family.
        native_kind: String,
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
        distance: crate::features::Length,
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

/// Solved model-space spatial-sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpatialSketchGeometry {
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
        /// Curve degree.
        degree: u32,
        /// Full knot vector.
        knots: Vec<f64>,
        /// Control points in parameter order.
        control_points: Vec<Point3>,
        /// Per-pole weights; absent for non-rational curves.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weights: Option<Vec<f64>>,
        /// Whether the curve is periodic.
        #[serde(default)]
        periodic: bool,
    },
    /// Tensor-product NURBS surface embedded in model space.
    NurbsSurface {
        /// Degree in the first parameter.
        u_degree: u32,
        /// Degree in the second parameter.
        v_degree: u32,
        /// Full knot vector in the first parameter.
        u_knots: Vec<f64>,
        /// Full knot vector in the second parameter.
        v_knots: Vec<f64>,
        /// Rectangular control grid in first-parameter-major order.
        control_points: Vec<Vec<Point3>>,
    },
    /// Source-native spatial geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        native_kind: String,
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
    pub label_distance: Option<f64>,
    /// Persisted position along the dimension label path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_position: Option<f64>,
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

/// One ordered operand retained from a native sketch relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchNativeOperand {
    /// Source-native operand family.
    pub native_kind: String,
    /// Source-native field containing this operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_field: Option<String>,
    /// Source-native role code, when the field carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_role: Option<u32>,
    /// Source-native object index.
    pub object_index: u32,
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
    pub direction: [f64; 2],
    /// Adjacent-instance spacing along `direction`.
    pub spacing: Length,
    /// Driving distance parameter and the distance form it controls.
    pub distance: Option<SketchPatternDistance>,
    /// Driving instance-count parameter, when the source exposes it as a neutral parameter.
    pub count_parameter: Option<ParameterId>,
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
        Ok(SketchPatternDirection {
            direction: self.direction,
            spacing: self.spacing,
            distance,
            count_parameter: self.count_parameter,
        })
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

/// Solver class of an opaque scalar symbol in a sketch solver graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SolverScalarClass {
    /// Result scalar of an angle-difference relation (wire class 0).
    Difference,
    /// Angle operand scalar (wire class 4).
    Angle,
    /// Operand scalar of a direct scalar equality (wire class 6).
    Equality,
}

impl SolverScalarClass {
    const fn wire_value(self) -> u32 {
        match self {
            Self::Difference => 0,
            Self::Angle => 4,
            Self::Equality => 6,
        }
    }
}

impl Serialize for SolverScalarClass {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u32(self.wire_value())
    }
}

impl<'de> Deserialize<'de> for SolverScalarClass {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            0 => Ok(Self::Difference),
            4 => Ok(Self::Angle),
            6 => Ok(Self::Equality),
            value => Err(serde::de::Error::custom(format_args!(
                "variable_type must be 0, 4, or 6, got {value}"
            ))),
        }
    }
}

/// One opaque scalar symbol in a sketch solver graph.
///
/// The identity is local to the owning sketch. The class preserves the
/// solver's scalar family so relations can join only compatible symbols; it
/// has no meaning outside that solver graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SketchSolverScalar {
    /// Solver scalar class.
    #[serde(rename = "variable_type")]
    #[cfg_attr(feature = "schema", schemars(with = "u32"))]
    pub class: SolverScalarClass,
    /// Solver-local scalar key.
    pub key: u32,
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

/// Neutral geometric and dimensional sketch relations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchConstraintDefinition {
    /// Persisted no-op relation slot.
    Disabled,
    /// Two entity loci coincide.
    Coincident {
        /// Coincident entity loci.
        entities: Vec<SketchEntityId>,
    },
    /// Entities participate in one native polygon relation.
    Polygon {
        /// Ordered polygon members.
        entities: Vec<SketchEntityId>,
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
        /// First aligned locus.
        first: SketchLocus,
        /// Second aligned locus.
        second: SketchLocus,
        /// Shared sketch coordinate.
        axis: SketchCoordinateAxis,
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
        /// First angle scalar in the subtraction.
        first: SketchSolverScalar,
        /// Second angle scalar in the subtraction.
        second: SketchSolverScalar,
        /// Scalar receiving `first - second`.
        difference: SketchSolverScalar,
        /// Source-evaluated non-negative angle difference in radians.
        value: Angle,
    },
    /// Equality between two equality-class solver scalars.
    ScalarEquality {
        /// First scalar in the equality.
        first: SketchSolverScalar,
        /// Second scalar in the equality.
        second: SketchSolverScalar,
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
        native_kind: String,
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

#[cfg(test)]
mod tests;
