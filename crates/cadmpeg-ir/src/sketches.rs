// SPDX-License-Identifier: Apache-2.0
//! Neutral planar sketches, solved entities, and geometric constraints.

use crate::math::{Point2, Point3, Vector3};
use crate::transform::Transform;
use crate::{
    features::{FinitePoint3, FiniteVector3, ParameterId},
    scalar::{Angle, FiniteReal, Length, PositiveAngle, PositiveLength, PositiveReal},
    units::{FinitePoint2, UnitVector2, UnitVector3},
};
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Collection admission or decode resource refusal.
#[derive(Debug, thiserror::Error)]
pub enum SketchCollectionError {
    /// Geometric or collection invariant failure.
    #[error("{0}")]
    Invalid(&'static str),
    /// Storage or work allowance exhausted.
    #[error("resource refusal: {0:?}")]
    Resource(cadmpeg_core::decode::ResourceLimit),
    /// A fallible admission operation could not complete.
    #[error("{0}")]
    Admission(String),
}

impl From<cadmpeg_core::CodecError> for SketchCollectionError {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        match error {
            cadmpeg_core::CodecError::ResourceLimit(limit) => Self::Resource(limit),
            error => Self::Admission(error.to_string()),
        }
    }
}

fn distinct_sketch_members<'id>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    identities: impl ExactSizeIterator<Item = &'id str>,
    operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError> {
    // Drop the temporary identities before their storage reservation.
    let (storage, mut members) = {
        let mut values = Vec::new();
        let reservation = ctx.reserve_temporary_vec(&mut values, identities.len(), operation)?;
        (reservation, values)
    };
    for identity in identities {
        ctx.charge_work(1, operation)?;
        let mut low = 0;
        let mut high = members.len();
        while low < high {
            ctx.charge_work(1, operation)?;
            let middle = low + (high - low) / 2;
            let candidate: &str = members[middle];
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(candidate.len().min(identity.len())),
                operation,
            )?;
            match candidate.cmp(identity) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Ok(false),
            }
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(members.len() - low),
            operation,
        )?;
        ctx.charge_work(1, operation)?;
        members.insert(low, identity);
    }
    drop(members);
    drop(storage);
    Ok(true)
}

pub mod scaling;

crate::ids::id_type!(
    /// Identifies a neutral planar sketch.
    SketchId, compose, into_string
);
crate::ids::id_type!(
    /// Identifies solved geometry in a sketch.
    SketchEntityId, compose
);
crate::ids::id_type!(
    /// Identifies a neutral spatial sketch.
    SpatialSketchId, compose
);
crate::ids::id_type!(
    /// Identifies solved geometry in a spatial sketch.
    SpatialSketchEntityId, compose
);
crate::ids::id_type!(
    /// Identifies a geometric sketch constraint.
    SketchConstraintId, compose
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
        match value {
            SketchFontWeight::Regular => 400,
            SketchFontWeight::Medium => 500,
            SketchFontWeight::Bold => 750,
        }
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub enum SketchAxis {
    /// Positive sketch-u direction.
    Horizontal,
    /// Positive sketch-v direction.
    Vertical,
}

/// A planar sketch and its ordered profile loops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Sketch {
    /// Globally unique sketch id.
    pub id: SketchId,
    /// Source display name, when recorded.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Source configuration key, when scoped.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<String>,
    /// Source display visibility, when recorded.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// Placement of sketch coordinates in model space.
    pub placement: SketchPlacement,
    /// Ordered closed or open profile chains.
    #[serde(default, skip_serializing_if = "SketchProfiles::is_empty")]
    pub profiles: SketchProfiles,
    /// Identifier of the full-fidelity native input lane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// Placement of a planar sketch's local coordinates in model space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SketchPlacement {
    /// Local geometry is decoded but its model-space frame is unresolved.
    Unresolved {},
    /// Complete model-space sketch frame.
    Resolved {
        /// Checked origin and nonzero perpendicular axes.
        frame: SketchPlaneFrame,
    },
}

const EPS_SKETCH_PLANE_ORTHOGONALITY: f64 = 1.0e-9;

/// A finite origin with nonzero perpendicular sketch axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchPlaneFrameWire")]
pub struct SketchPlaneFrame {
    origin: FinitePoint3,
    normal: FiniteVector3,
    u_axis: FiniteVector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SketchPlaneFrameWire {
    /// Sketch-plane origin in model space.
    origin: Point3,
    /// Sketch-plane normal.
    normal: Vector3,
    /// In-plane first axis, perpendicular to `normal`.
    u_axis: Vector3,
}

impl TryFrom<SketchPlaneFrameWire> for SketchPlaneFrame {
    type Error = &'static str;

    fn try_from(wire: SketchPlaneFrameWire) -> Result<Self, Self::Error> {
        let normal =
            FiniteVector3::new(wire.normal).ok_or("sketch normal must be finite and nonzero")?;
        let unit_normal = normal
            .unit_nonzero()
            .ok_or("sketch normal must be finite and nonzero")?;
        let u_axis =
            FiniteVector3::new(wire.u_axis).ok_or("sketch u_axis must be finite and nonzero")?;
        let unit_u_axis = u_axis
            .unit_nonzero()
            .ok_or("sketch u_axis must be finite and nonzero")?;
        if unit_normal.dot(unit_u_axis).abs() > EPS_SKETCH_PLANE_ORTHOGONALITY {
            return Err("sketch normal and u_axis must be perpendicular");
        }
        let origin = FinitePoint3::new(wire.origin).ok_or("sketch origin must be finite")?;
        Ok(Self {
            origin,
            normal,
            u_axis,
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

    /// Replace the origin of a resolved frame and keep its admitted axes.
    /// The axes alone satisfy the frame contract, so the result needs no new
    /// admission. An unresolved placement has no origin and stays unresolved.
    #[must_use]
    pub fn with_origin(self, origin: FinitePoint3) -> Self {
        match self {
            Self::Unresolved {} => Self::Unresolved {},
            Self::Resolved { frame } => Self::Resolved {
                frame: SketchPlaneFrame { origin, ..frame },
            },
        }
    }

    /// Return the complete frame when placement is resolved.
    pub fn resolved(self) -> Option<(FinitePoint3, FiniteVector3, FiniteVector3)> {
        match self {
            Self::Unresolved {} => None,
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

    /// Append a profile containing one entity use after resource admission.
    pub fn push_single(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        entity: SketchEntityUse,
    ) -> Result<(), cadmpeg_core::CodecError> {
        const OPERATION: &str = "append sketch profile use";
        ctx.charge_work(2, OPERATION)?;
        let mut profile = Vec::new();
        ctx.reserve_vec(&mut profile, 1, OPERATION)?;
        if self.0.len() == self.0.capacity() {
            ctx.charge_work(
                u64::try_from(self.0.len())
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
        }
        ctx.reserve_vec(&mut self.0, 1, OPERATION)?;
        profile.push(entity);
        self.0.push(profile);
        Ok(())
    }

    /// Retain matching uses and remove empty chains after every predicate returns.
    pub fn retain_uses(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut keep: impl FnMut(&SketchEntityUse) -> bool,
    ) -> Result<(), cadmpeg_core::CodecError> {
        const OPERATION: &str = "filter sketch profile uses";
        let count = self.0.iter().try_fold(0usize, |count, profile| {
            ctx.charge_work(1, OPERATION)?;
            count
                .checked_add(profile.len())
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
        })?;
        ctx.charge_work(
            u64::try_from(count)
                .ok()
                .and_then(|count| count.checked_mul(4))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        let (mut decisions, _decision_storage) = ctx.temporary_vec(count, OPERATION)?;
        for profile in &self.0 {
            for usage in profile {
                ctx.charge_work(
                    u64::try_from(usage.entity.as_str().len())
                        .ok()
                        .and_then(|len| len.checked_add(1))
                        .and_then(|work| work.checked_mul(4))
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                decisions.push(keep(usage));
            }
        }
        let mut decision_index = 0;
        for profile in &mut self.0 {
            profile.retain(|_| {
                let decision = decisions[decision_index];
                decision_index += 1;
                decision
            });
        }
        self.0.retain(|profile| !profile.is_empty());
        Ok(())
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
}

impl Sketch {
    /// Return the complete model-space frame when placement is resolved.
    pub fn resolved_placement(&self) -> Option<(FinitePoint3, FiniteVector3, FiniteVector3)> {
        self.placement.resolved()
    }
}

/// Oriented use of one sketch entity in a profile chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchEntityUse {
    /// Referenced sketch entity.
    pub entity: SketchEntityId,
    /// Whether traversal opposes the entity's stored direction.
    #[serde(default)]
    pub reversed: bool,
}

impl cadmpeg_core::decode::cost::DecodeCost for SketchEntityUse {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.entity, self.reversed),
            ctx,
            operation,
        )
    }
}

/// Solved geometry belonging to one sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchEntity {
    /// Globally unique entity id.
    id: SketchEntityId,
    /// Owning sketch.
    pub sketch: SketchId,
    /// Whether the entity is construction geometry.
    #[serde(default)]
    pub construction: bool,
    /// Source-native geometry record represented by this entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
    /// Source-native curve carrier represented by this entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_geometry_ref"
    )]
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

/// A finite reference-line direction longer than machine epsilon.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ReferenceLineDirection(FinitePoint2);

impl ReferenceLineDirection {
    /// Admit the direction used by a reference line.
    pub fn new(direction: FinitePoint2) -> Option<Self> {
        (direction.u.hypot(direction.v) > f64::EPSILON).then_some(Self(direction))
    }

    /// Return the direction coordinates.
    pub fn get(self) -> Point2 {
        self.0.get()
    }
}

impl std::ops::Deref for ReferenceLineDirection {
    type Target = Point2;

    fn deref(&self) -> &Point2 {
        self.0.as_raw()
    }
}

/// An ellipse's radii before their numeric relationship is admitted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EllipseRadii<L> {
    /// Semi-major radius.
    pub major_radius: L,
    /// Semi-minor radius.
    pub minor_radius: L,
}

/// Positive ellipse radii with a major radius at least the minor radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderedMajorRadius {
    major: PositiveLength,
    minor: PositiveLength,
}

impl OrderedMajorRadius {
    /// Admit the relation between two positive radii.
    pub fn new(major: PositiveLength, minor: PositiveLength) -> Option<Self> {
        (major.get() >= minor.get()).then_some(Self { major, minor })
    }

    /// Return the major radius.
    pub const fn major(self) -> PositiveLength {
        self.major
    }

    /// Return the minor radius.
    pub const fn minor(self) -> PositiveLength {
        self.minor
    }
}

impl Serialize for OrderedMajorRadius {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut radii = serializer.serialize_struct("EllipseRadii", 2)?;
        radii.serialize_field("major_radius", &self.major)?;
        radii.serialize_field("minor_radius", &self.minor)?;
        radii.end()
    }
}

/// Solved two-dimensional sketch geometry with finite numeric coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(into = "SketchGeometryDefinition"))]
#[serde(try_from = "SketchGeometryDefinition")]
pub struct SketchGeometry(
    SketchGeometryDefinition<
        FinitePoint2,
        PositiveLength,
        FiniteReal,
        PositiveReal,
        ReferenceLineDirection,
        OrderedMajorRadius,
    >,
);

impl cadmpeg_core::decode::cost::DecodeCost for SketchGeometry {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        use cadmpeg_core::decode::cost::DecodeCost;

        let fields = match self.definition() {
            SketchGeometryDefinition::Point { position } => {
                DecodeCost::decode_cost(&(0_u8, position.get()), ctx, operation)?
            }
            SketchGeometryDefinition::Line { start, end } => {
                DecodeCost::decode_cost(&(1_u8, start.get(), end.get()), ctx, operation)?
            }
            SketchGeometryDefinition::ReferenceLine { origin, direction } => {
                DecodeCost::decode_cost(&(2_u8, origin.get(), direction.get()), ctx, operation)?
            }
            SketchGeometryDefinition::Circle { center, radius } => {
                DecodeCost::decode_cost(&(3_u8, center.get(), radius.get()), ctx, operation)?
            }
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => DecodeCost::decode_cost(
                &(
                    4_u8,
                    center.get(),
                    radius.get(),
                    start_angle.get(),
                    end_angle.get(),
                ),
                ctx,
                operation,
            )?,
            SketchGeometryDefinition::Ellipse {
                center,
                major_angle,
                radii,
                bounds,
            } => DecodeCost::decode_cost(
                &(
                    5_u8,
                    center.get(),
                    major_angle.get(),
                    radii.major().get(),
                    radii.minor().get(),
                    bounds
                        .as_ref()
                        .map(|[start, end]| (start.get(), end.get())),
                ),
                ctx,
                operation,
            )?,
            SketchGeometryDefinition::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds,
            } => DecodeCost::decode_cost(
                &(
                    6_u8,
                    center.get(),
                    major_angle.get(),
                    major_radius.get(),
                    minor_radius.get(),
                    bounds
                        .as_ref()
                        .map(|[start, end]| (start.get(), end.get())),
                ),
                ctx,
                operation,
            )?,
            SketchGeometryDefinition::Parabola {
                vertex,
                axis_angle,
                focal_length,
                bounds,
            } => DecodeCost::decode_cost(
                &(
                    7_u8,
                    vertex.get(),
                    axis_angle.get(),
                    focal_length.get(),
                    bounds
                        .as_ref()
                        .map(|[start, end]| (start.get(), end.get())),
                ),
                ctx,
                operation,
            )?,
            SketchGeometryDefinition::Nurbs { curve } => DecodeCost::decode_cost(
                &(8_u8, curve),
                ctx,
                operation,
            )?,
            SketchGeometryDefinition::Text {
                text,
                font_family,
                font_weight,
                height,
                width_factor,
                placement,
                horizontal_alignment,
                vertical_alignment,
            } => {
                let font_weight_tag = match font_weight {
                    SketchFontWeight::Regular => 0_u8,
                    SketchFontWeight::Medium => 1_u8,
                    SketchFontWeight::Bold => 2_u8,
                };
                let string_bytes = DecodeCost::decode_cost(&(text, font_family), ctx, operation)?;
                let scalar_bytes = DecodeCost::decode_cost(
                    &(
                        9_u8,
                        font_weight_tag,
                        height.get(),
                        width_factor.as_ref().map(|factor| factor.get()),
                    ),
                    ctx,
                    operation,
                )?;
                let placement_bytes = DecodeCost::decode_cost(
                    &placement.as_ref().map(|placement| {
                        (placement.anchor.get(), placement.rotation.get())
                    }),
                    ctx,
                    operation,
                )?;
                let horizontal_bytes = match horizontal_alignment {
                    None => DecodeCost::decode_cost(&0_u8, ctx, operation)?,
                    Some(SketchTextHorizontalAlignment::Left) => {
                        DecodeCost::decode_cost(&(1_u8, 0_u8), ctx, operation)?
                    }
                    Some(SketchTextHorizontalAlignment::Center) => {
                        DecodeCost::decode_cost(&(1_u8, 1_u8), ctx, operation)?
                    }
                    Some(SketchTextHorizontalAlignment::Right) => {
                        DecodeCost::decode_cost(&(1_u8, 2_u8), ctx, operation)?
                    }
                    Some(SketchTextHorizontalAlignment::Native(value)) => {
                        DecodeCost::decode_cost(&(1_u8, 3_u8, value), ctx, operation)?
                    }
                };
                let vertical_bytes = match vertical_alignment {
                    None => DecodeCost::decode_cost(&0_u8, ctx, operation)?,
                    Some(SketchTextVerticalAlignment::Top) => {
                        DecodeCost::decode_cost(&(1_u8, 0_u8), ctx, operation)?
                    }
                    Some(SketchTextVerticalAlignment::Middle) => {
                        DecodeCost::decode_cost(&(1_u8, 1_u8), ctx, operation)?
                    }
                    Some(SketchTextVerticalAlignment::Bottom) => {
                        DecodeCost::decode_cost(&(1_u8, 2_u8), ctx, operation)?
                    }
                    Some(SketchTextVerticalAlignment::Native(value)) => {
                        DecodeCost::decode_cost(&(1_u8, 3_u8, value), ctx, operation)?
                    }
                };
                let fields = (string_bytes).checked_add(scalar_bytes).ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
                let fields = (fields).checked_add(placement_bytes).ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
                let fields = (fields).checked_add(horizontal_bytes).ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
                (fields).checked_add(vertical_bytes).ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?
            }
            SketchGeometryDefinition::ExternalReference {
                document,
                object,
                subelements,
            } => {
                let fields = DecodeCost::decode_cost(&(10_u8, document, object), ctx, operation)?;
                (fields).checked_add(DecodeCost::decode_cost(subelements, ctx, operation)?).ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?
            }
            SketchGeometryDefinition::Native { native_kind } => {
                DecodeCost::decode_cost(&(11_u8, native_kind), ctx, operation)?
            }
        };
        Ok(fields)
    }
}

impl SketchGeometry {
    /// Copy geometry after charging each retained nested allocation.
    pub fn try_clone_for_decode(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        use SketchGeometryDefinition as Definition;
        ctx.charge_work(1, operation)?;
        let definition = match self.definition() {
            Definition::Nurbs { curve } => Definition::Nurbs {
                curve: curve.try_clone_for_decode(ctx, operation)?,
            },
            Definition::Text {
                text,
                font_family,
                font_weight,
                height,
                width_factor,
                placement,
                horizontal_alignment,
                vertical_alignment,
            } => Definition::Text {
                text: text.try_clone_for_decode(ctx, operation)?,
                font_family: font_family.try_clone_for_decode(ctx, operation)?,
                font_weight: *font_weight,
                height: *height,
                width_factor: *width_factor,
                placement: *placement,
                horizontal_alignment: *horizontal_alignment,
                vertical_alignment: *vertical_alignment,
            },
            Definition::ExternalReference {
                document,
                object,
                subelements,
            } => {
                let document = document
                    .as_deref()
                    .map(|document| ctx.copy_retained_text(document, operation))
                    .transpose()?;
                let object = object.try_clone_for_decode(ctx, operation)?;
                let mut copied_subelements = Vec::new();
                ctx.reserve_vec(&mut copied_subelements, subelements.len(), operation)?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(subelements.len()),
                    operation,
                )?;
                for subelement in subelements {
                    copied_subelements.push(ctx.copy_retained_text(subelement, operation)?);
                }
                Definition::ExternalReference {
                    document,
                    object,
                    subelements: copied_subelements,
                }
            }
            Definition::Native { native_kind } => Definition::Native {
                native_kind: native_kind.try_clone_for_decode(ctx, operation)?,
            },
            Definition::Point { .. }
            | Definition::Line { .. }
            | Definition::ReferenceLine { .. }
            | Definition::Circle { .. }
            | Definition::Arc { .. }
            | Definition::Ellipse { .. }
            | Definition::Hyperbola { .. }
            | Definition::Parabola { .. } => return Ok(self.clone()),
        };
        Ok(Self::from_admitted_definition(definition))
    }

    /// Build from a definition whose field types already carry the admitted invariants.
    #[must_use]
    pub fn from_admitted_definition(
        definition: SketchGeometryDefinition<
            FinitePoint2,
            PositiveLength,
            FiniteReal,
            PositiveReal,
            ReferenceLineDirection,
            OrderedMajorRadius,
        >,
    ) -> Self {
        Self(definition)
    }

    /// Retain source-native geometry without solved numeric fields.
    #[must_use]
    pub fn native(native_kind: NonBlankString) -> Self {
        Self(SketchGeometryDefinition::Native { native_kind })
    }

    /// Admit an already checked planar NURBS curve.
    #[must_use]
    pub fn nurbs(curve: crate::geometry::pcurve::PcurveNurbs) -> Self {
        Self(SketchGeometryDefinition::Nurbs { curve })
    }

    /// Build the geometry from an admitted definition. The field types
    /// state finiteness and positivity, so only the conditions between
    /// fields are tested: a reference-line direction longer than machine
    /// epsilon and an ellipse major radius at least its minor radius.
    pub fn from_parts(
        definition: SketchGeometryDefinition<
            FinitePoint2,
            PositiveLength,
            FiniteReal,
            PositiveReal,
        >,
    ) -> Result<Self, &'static str> {
        use SketchGeometryDefinition as Definition;
        Ok(Self(match definition {
            Definition::Point { position } => Definition::Point { position },
            Definition::Line { start, end } => Definition::Line { start, end },
            Definition::ReferenceLine { origin, direction } => Definition::ReferenceLine {
                origin,
                direction: ReferenceLineDirection::new(direction).ok_or(
                    "sketch reference line requires finite origin and nonzero finite direction",
                )?,
            },
            Definition::Circle { center, radius } => Definition::Circle { center, radius },
            Definition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => Definition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            },
            Definition::Ellipse {
                center,
                major_angle,
                radii,
                bounds,
            } => Definition::Ellipse {
                center,
                major_angle,
                radii: OrderedMajorRadius::new(radii.major_radius, radii.minor_radius)
                    .ok_or("sketch ellipse major_radius must be at least minor_radius")?,
                bounds,
            },
            Definition::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds,
            } => Definition::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds,
            },
            Definition::Parabola {
                vertex,
                axis_angle,
                focal_length,
                bounds,
            } => Definition::Parabola {
                vertex,
                axis_angle,
                focal_length,
                bounds,
            },
            Definition::Nurbs { curve } => Definition::Nurbs { curve },
            Definition::Text {
                text,
                font_family,
                font_weight,
                height,
                width_factor,
                placement,
                horizontal_alignment,
                vertical_alignment,
            } => Definition::Text {
                text,
                font_family,
                font_weight,
                height,
                width_factor,
                placement,
                horizontal_alignment,
                vertical_alignment,
            },
            Definition::ExternalReference {
                document,
                object,
                subelements,
            } => Definition::ExternalReference {
                document,
                object,
                subelements,
            },
            Definition::Native { native_kind } => Definition::Native { native_kind },
        }))
    }

    /// Borrow the admitted geometry definition.
    #[must_use]
    pub fn definition(
        &self,
    ) -> &SketchGeometryDefinition<
        FinitePoint2,
        PositiveLength,
        FiniteReal,
        PositiveReal,
        ReferenceLineDirection,
        OrderedMajorRadius,
    > {
        &self.0
    }

    /// Extract the admitted definition.
    #[must_use]
    pub fn into_definition(
        self,
    ) -> SketchGeometryDefinition<
        FinitePoint2,
        PositiveLength,
        FiniteReal,
        PositiveReal,
        ReferenceLineDirection,
        OrderedMajorRadius,
    > {
        self.0
    }
}

impl
    SketchGeometryDefinition<
        FinitePoint2,
        PositiveLength,
        FiniteReal,
        PositiveReal,
        ReferenceLineDirection,
        OrderedMajorRadius,
    >
{
    /// The definition with raw points, lengths, bounds and width factor.
    #[must_use]
    pub fn to_raw(&self) -> SketchGeometryDefinition {
        use SketchGeometryDefinition as Definition;
        match self {
            Self::Point { position } => Definition::Point {
                position: position.get(),
            },
            Self::Line { start, end } => Definition::Line {
                start: start.get(),
                end: end.get(),
            },
            Self::ReferenceLine { origin, direction } => Definition::ReferenceLine {
                origin: origin.get(),
                direction: direction.get(),
            },
            Self::Circle { center, radius } => Definition::Circle {
                center: center.get(),
                radius: Length::from(*radius),
            },
            Self::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => Definition::Arc {
                center: center.get(),
                radius: Length::from(*radius),
                start_angle: *start_angle,
                end_angle: *end_angle,
            },
            Self::Ellipse {
                center,
                major_angle,
                radii,
                bounds,
            } => Definition::Ellipse {
                center: center.get(),
                major_angle: *major_angle,
                radii: EllipseRadii {
                    major_radius: Length::from(radii.major()),
                    minor_radius: Length::from(radii.minor()),
                },
                bounds: *bounds,
            },
            Self::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds,
            } => Definition::Hyperbola {
                center: center.get(),
                major_angle: *major_angle,
                major_radius: Length::from(*major_radius),
                minor_radius: Length::from(*minor_radius),
                bounds: bounds.map(FiniteReal::raw_array),
            },
            Self::Parabola {
                vertex,
                axis_angle,
                focal_length,
                bounds,
            } => Definition::Parabola {
                vertex: vertex.get(),
                axis_angle: *axis_angle,
                focal_length: Length::from(*focal_length),
                bounds: bounds.map(FiniteReal::raw_array),
            },
            Self::Nurbs { curve } => Definition::Nurbs {
                curve: curve.clone(),
            },
            Self::Text {
                text,
                font_family,
                font_weight,
                height,
                width_factor,
                placement,
                horizontal_alignment,
                vertical_alignment,
            } => Definition::Text {
                text: text.clone(),
                font_family: font_family.clone(),
                font_weight: *font_weight,
                height: Length::from(*height),
                width_factor: width_factor.map(PositiveReal::get),
                placement: placement.map(|placement| TextPlacement {
                    anchor: placement.anchor.get(),
                    rotation: placement.rotation,
                }),
                horizontal_alignment: *horizontal_alignment,
                vertical_alignment: *vertical_alignment,
            },
            Self::ExternalReference {
                document,
                object,
                subelements,
            } => Definition::ExternalReference {
                document: document.clone(),
                object: object.clone(),
                subelements: subelements.clone(),
            },
            Self::Native { native_kind } => Definition::Native {
                native_kind: native_kind.clone(),
            },
        }
    }
}

/// Admit a sketch point, or refuse it with `message`.
fn admit_sketch_point(point: Point2, message: &'static str) -> Result<FinitePoint2, &'static str> {
    FinitePoint2::new(point).ok_or(message)
}

/// Admit a positive sketch length, or refuse it with `message`.
fn admit_sketch_length(
    length: Length,
    message: &'static str,
) -> Result<PositiveLength, &'static str> {
    PositiveLength::try_from(length).map_err(|_| message)
}

/// Admit optional finite bounds, or refuse them with `message`.
fn admit_sketch_bounds(
    bounds: Option<[f64; 2]>,
    message: &'static str,
) -> Result<Option<[FiniteReal; 2]>, &'static str> {
    bounds
        .map(|bounds| FiniteReal::array(bounds).ok_or(message))
        .transpose()
}

impl TryFrom<SketchGeometryDefinition> for SketchGeometry {
    type Error = &'static str;

    fn try_from(definition: SketchGeometryDefinition) -> Result<Self, Self::Error> {
        use SketchGeometryDefinition as Definition;
        const CIRCULAR: &str =
            "sketch circular geometry requires finite center and positive finite radius";
        Self::from_parts(match definition {
            Definition::Point { position } => Definition::Point {
                position: admit_sketch_point(position, "sketch point position must be finite")?,
            },
            Definition::Line { start, end } => {
                const LINE: &str = "sketch line endpoints must be finite";
                Definition::Line {
                    start: admit_sketch_point(start, LINE)?,
                    end: admit_sketch_point(end, LINE)?,
                }
            }
            Definition::ReferenceLine { origin, direction } => {
                const REFERENCE: &str =
                    "sketch reference line requires finite origin and nonzero finite direction";
                Definition::ReferenceLine {
                    origin: admit_sketch_point(origin, REFERENCE)?,
                    direction: admit_sketch_point(direction, REFERENCE)?,
                }
            }
            Definition::Circle { center, radius } => Definition::Circle {
                center: admit_sketch_point(center, CIRCULAR)?,
                radius: admit_sketch_length(radius, CIRCULAR)?,
            },
            Definition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => Definition::Arc {
                center: admit_sketch_point(center, CIRCULAR)?,
                radius: admit_sketch_length(radius, CIRCULAR)?,
                start_angle,
                end_angle,
            },
            Definition::Ellipse {
                center,
                major_angle,
                radii,
                bounds,
            } => {
                const RADII: &str = "sketch ellipse radii must be positive and finite";
                Definition::Ellipse {
                    center: admit_sketch_point(
                        center,
                        "sketch ellipse center and major_angle must be finite",
                    )?,
                    major_angle,
                    radii: EllipseRadii {
                        major_radius: admit_sketch_length(radii.major_radius, RADII)?,
                        minor_radius: admit_sketch_length(radii.minor_radius, RADII)?,
                    },
                    bounds,
                }
            }
            Definition::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds,
            } => {
                const RADII: &str = "sketch hyperbola radii must be positive and finite";
                Definition::Hyperbola {
                    center: admit_sketch_point(
                        center,
                        "sketch hyperbola center and major_angle must be finite",
                    )?,
                    major_angle,
                    major_radius: admit_sketch_length(major_radius, RADII)?,
                    minor_radius: admit_sketch_length(minor_radius, RADII)?,
                    bounds: admit_sketch_bounds(bounds, "sketch hyperbola bounds must be finite")?,
                }
            }
            Definition::Parabola {
                vertex,
                axis_angle,
                focal_length,
                bounds,
            } => Definition::Parabola {
                vertex: admit_sketch_point(
                    vertex,
                    "sketch parabola vertex and axis_angle must be finite",
                )?,
                axis_angle,
                focal_length: admit_sketch_length(
                    focal_length,
                    "sketch parabola focal_length must be positive and finite",
                )?,
                bounds: admit_sketch_bounds(bounds, "sketch parabola bounds must be finite")?,
            },
            Definition::Nurbs { curve } => Definition::Nurbs { curve },
            Definition::Text {
                text,
                font_family,
                font_weight,
                height,
                width_factor,
                placement,
                horizontal_alignment,
                vertical_alignment,
            } => Definition::Text {
                text,
                font_family,
                font_weight,
                height: admit_sketch_length(
                    height,
                    "sketch text height must be positive and finite",
                )?,
                width_factor: width_factor
                    .map(|value| {
                        PositiveReal::new(value)
                            .ok_or("sketch text width_factor must be positive and finite")
                    })
                    .transpose()?,
                placement: placement
                    .map(|placement| {
                        Ok::<_, &'static str>(TextPlacement {
                            anchor: admit_sketch_point(
                                placement.anchor,
                                "sketch text anchor and rotation must be finite",
                            )?,
                            rotation: placement.rotation,
                        })
                    })
                    .transpose()?,
                horizontal_alignment,
                vertical_alignment,
            },
            Definition::ExternalReference {
                document,
                object,
                subelements,
            } => Definition::ExternalReference {
                document,
                object,
                subelements,
            },
            Definition::Native { native_kind } => Definition::Native { native_kind },
        })
    }
}

/// Definition admitted by a solved two-dimensional sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(bound(
    deserialize = "P: Deserialize<'de>, L: Deserialize<'de>, R: Deserialize<'de>, W: Deserialize<'de>, D: Deserialize<'de>, M: Deserialize<'de>"
))]
pub enum SketchGeometryDefinition<
    P = Point2,
    L = Length,
    R = f64,
    W = f64,
    D = P,
    M = EllipseRadii<L>,
> {
    /// Isolated point.
    Point {
        /// Solved point position.
        position: P,
    },
    /// Bounded line segment.
    Line {
        /// Segment start.
        start: P,
        /// Segment end.
        end: P,
    },
    /// Unbounded construction or reference line.
    ReferenceLine {
        /// Point on the line.
        origin: P,
        /// Non-zero direction in sketch coordinates.
        direction: D,
    },
    /// Full circle.
    Circle {
        /// Circle center.
        center: P,
        /// Circle radius.
        radius: L,
    },
    /// Circular arc with angles in radians.
    Arc {
        /// Arc center.
        center: P,
        /// Arc radius.
        radius: L,
        /// Start angle.
        start_angle: Angle,
        /// End angle.
        end_angle: Angle,
    },
    /// Full or bounded ellipse.
    Ellipse {
        /// Ellipse center.
        center: P,
        /// Major-axis angle in sketch coordinates.
        major_angle: Angle,
        /// Semi-major and semi-minor radii.
        #[serde(flatten)]
        radii: M,
        /// Parameter bounds for an arc; absent for a full ellipse.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_bounds"
        )]
        bounds: Option<[Angle; 2]>,
    },
    /// Full or bounded hyperbola.
    Hyperbola {
        /// Hyperbola center.
        center: P,
        /// Major-axis angle in sketch coordinates.
        major_angle: Angle,
        /// Semi-major radius.
        major_radius: L,
        /// Semi-minor radius.
        minor_radius: L,
        /// Parameter bounds for a branch; absent for the full curve.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_sketch_geometry_definition_bounds"
        )]
        bounds: Option<[R; 2]>,
    },
    /// Full or bounded parabola.
    Parabola {
        /// Parabola vertex.
        vertex: P,
        /// Symmetry-axis angle in sketch coordinates.
        axis_angle: Angle,
        /// Distance from the vertex to the focus.
        focal_length: L,
        /// Bounds on the local transverse coordinate `y`, in sketch length units.
        /// The axial coordinate is `x = y² / (4 * focal_length)`.
        /// Absent for the full curve.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_sketch_geometry_definition_bounds"
        )]
        bounds: Option<[R; 2]>,
    },
    /// NURBS curve in sketch coordinates.
    Nurbs {
        /// Checked two-dimensional knot, pole, and weight payload.
        curve: crate::geometry::pcurve::PcurveNurbs,
    },
    /// Text placed in sketch coordinates.
    Text {
        /// Unicode text content.
        text: NonBlankString,
        /// Source font-family name.
        font_family: NonBlankString,
        /// Font weight from the source text style.
        font_weight: SketchFontWeight,
        /// Nominal character height.
        height: L,
        /// Horizontal scale relative to the nominal font width, absent when the
        /// source stores none.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_width_factor"
        )]
        width_factor: Option<W>,
        /// Text placement in sketch coordinates, absent when the source stores none.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_placement"
        )]
        placement: Option<TextPlacement<P>>,
        /// Horizontal placement about the text anchor, when the source class
        /// carries an alignment enum.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_horizontal_alignment"
        )]
        horizontal_alignment: Option<SketchTextHorizontalAlignment>,
        /// Vertical placement about the text anchor, when the source class
        /// carries an alignment enum.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_vertical_alignment"
        )]
        vertical_alignment: Option<SketchTextVerticalAlignment>,
    },
    /// Geometry referenced from another object when no solved sketch-space carrier is stored.
    ExternalReference {
        /// External document identity, absent for a reference within the current document.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_document"
        )]
        document: Option<String>,
        /// Referenced object identity.
        #[serde(deserialize_with = "deserialize_object")]
        object: NonBlankString,
        /// Ordered source subelement selectors.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        subelements: Vec<String>,
    },
    /// Source-native geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        #[serde(deserialize_with = "deserialize_native_kind")]
        native_kind: NonBlankString,
    },
}

/// Placement of sketch text about one anchor point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TextPlacement<P = Point2> {
    /// Point the text is placed and rotated about, in sketch coordinates.
    pub anchor: P,
    /// Counterclockwise rotation from the sketch u axis.
    pub rotation: Angle,
}

/// A sketch whose solved geometry is expressed directly in model space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SpatialSketch {
    /// Globally unique spatial-sketch id.
    pub id: SpatialSketchId,
    /// Source display name, when recorded.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Source configuration key, when scoped.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<String>,
    /// Source display visibility, when recorded.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// Ordered closed profile loops with profile-local planes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<SpatialSketchProfile>,
    /// Identifier of the full-fidelity native input lane.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

const EPS_SPATIAL_PROFILE_FRAME: f64 = 1.0e-9;
const SPATIAL_PROFILE_AXES_ERROR: &str =
    "spatial profile normal and u_axis must be unit and orthogonal";

/// One closed spatial-sketch profile and its admitted model-space plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SpatialSketchProfileWire")]
pub struct SpatialSketchProfile {
    origin: FinitePoint3,
    normal: UnitVector3,
    u_axis: UnitVector3,
    boundary: Vec<SpatialSketchEntityUse>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SpatialSketchProfileWire {
    /// Profile-plane origin in model space.
    origin: Point3,
    /// Profile-plane unit normal.
    normal: Vector3,
    /// Profile-plane unit u-axis.
    u_axis: Vector3,
    /// Ordered oriented boundary uses.
    boundary: Vec<SpatialSketchEntityUse>,
}

impl TryFrom<SpatialSketchProfileWire> for SpatialSketchProfile {
    type Error = SketchCollectionError;

    fn try_from(wire: SpatialSketchProfileWire) -> Result<Self, Self::Error> {
        let origin = FinitePoint3::new(wire.origin).ok_or(SketchCollectionError::Invalid(
            "spatial profile origin must be finite",
        ))?;
        let normal = UnitVector3::new(wire.normal)
            .ok_or(SketchCollectionError::Invalid(SPATIAL_PROFILE_AXES_ERROR))?;
        let u_axis = UnitVector3::new(wire.u_axis)
            .ok_or(SketchCollectionError::Invalid(SPATIAL_PROFILE_AXES_ERROR))?;
        let [n, u] = [normal.as_raw(), u_axis.as_raw()];
        if (n.x * u.x + n.y * u.y + n.z * u.z).abs() > EPS_SPATIAL_PROFILE_FRAME {
            return Err(SketchCollectionError::Invalid(SPATIAL_PROFILE_AXES_ERROR));
        }
        let invalid = "spatial profile boundary must be nonempty and contain distinct entities";
        if wire.boundary.is_empty() {
            return Err(SketchCollectionError::Invalid(invalid));
        }
        let mut members = std::collections::HashSet::new();
        members
            .try_reserve(wire.boundary.len())
            .map_err(|error| SketchCollectionError::Admission(error.to_string()))?;
        for use_ in &wire.boundary {
            if !members.insert(&use_.entity) {
                return Err(SketchCollectionError::Invalid(invalid));
            }
        }
        Ok(Self {
            origin,
            normal,
            u_axis,
            boundary: wire.boundary,
        })
    }
}

impl SpatialSketchProfile {
    /// Build a decoded profile through the shared typed-parts admission body.
    pub fn try_new(
        origin: Point3,
        normal: Vector3,
        u_axis: Vector3,
        boundary: Vec<SpatialSketchEntityUse>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Result<Self, &'static str>, cadmpeg_core::CodecError> {
        let Some(origin) = FinitePoint3::new(origin) else {
            return Ok(Err("spatial profile origin must be finite"));
        };
        let Some(normal) = UnitVector3::new(normal) else {
            return Ok(Err(SPATIAL_PROFILE_AXES_ERROR));
        };
        let Some(u_axis) = UnitVector3::new(u_axis) else {
            return Ok(Err(SPATIAL_PROFILE_AXES_ERROR));
        };
        Self::from_parts(origin, normal, u_axis, boundary, ctx, operation)
    }

    /// Admit typed plane controls and charge temporary uniqueness storage.
    pub fn from_parts(
        origin: FinitePoint3,
        normal: UnitVector3,
        u_axis: UnitVector3,
        boundary: Vec<SpatialSketchEntityUse>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Result<Self, &'static str>, cadmpeg_core::CodecError> {
        let [n, u] = [normal.as_raw(), u_axis.as_raw()];
        ctx.charge_work(1, operation)?;
        let dot = n.x * u.x + n.y * u.y + n.z * u.z;
        if dot.abs() > EPS_SPATIAL_PROFILE_FRAME {
            return Ok(Err(SPATIAL_PROFILE_AXES_ERROR));
        }
        if boundary.is_empty() {
            return Ok(Err(
                "spatial profile boundary must be nonempty and contain distinct entities",
            ));
        }
        if !distinct_sketch_members(
            ctx,
            boundary.iter().map(|use_| use_.entity.as_str()),
            operation,
        )? {
            return Ok(Err(
                "spatial profile boundary must be nonempty and contain distinct entities",
            ));
        }
        Ok(Ok(Self {
            origin,
            normal,
            u_axis,
            boundary,
        }))
    }

    /// Profile-plane origin in model space.
    #[must_use]
    pub fn origin(&self) -> FinitePoint3 {
        self.origin
    }

    /// Profile-plane unit normal.
    #[must_use]
    pub fn normal(&self) -> UnitVector3 {
        self.normal
    }

    /// Profile-plane unit u-axis.
    #[must_use]
    pub fn u_axis(&self) -> UnitVector3 {
        self.u_axis
    }

    /// Ordered oriented boundary uses.
    #[must_use]
    pub fn boundary(&self) -> &[SpatialSketchEntityUse] {
        &self.boundary
    }

    /// Replace the origin after finite-coordinate admission.
    pub fn set_origin(&mut self, origin: Point3) -> Result<(), &'static str> {
        self.origin = FinitePoint3::new(origin).ok_or("spatial profile origin must be finite")?;
        Ok(())
    }
}

/// Oriented use of one spatial-sketch entity in a profile boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct SpatialSketchEntity {
    /// Globally unique spatial entity id.
    id: SpatialSketchEntityId,
    /// Owning spatial sketch.
    pub sketch: SpatialSketchId,
    /// Whether the entity is construction geometry.
    #[serde(default)]
    pub construction: bool,
    /// Source-native geometry record represented by this entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
    /// Source-native curve carrier represented by this entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_geometry_ref"
    )]
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
#[serde(deny_unknown_fields)]
pub struct SpatialSketchConstraint {
    /// Globally unique constraint id.
    pub id: SketchConstraintId,
    /// Owning spatial sketch.
    pub sketch: SpatialSketchId,
    /// Neutral relation semantics.
    pub definition: SpatialSketchConstraintDefinition,
    /// Source-native relation represented by this constraint.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// One unordered entity pair in a repeated model-space sketch relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SpatialSketchEntityPair {
    /// First member in source discovery order.
    pub first: SpatialSketchEntityId,
    /// Second member in source discovery order.
    pub second: SpatialSketchEntityId,
}

/// A spatial sketch constraint with admitted local members and scalar values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(into = "SpatialSketchConstraintDefinitionInput")
)]
#[serde(try_from = "SpatialSketchConstraintDefinitionInput")]
pub struct SpatialSketchConstraintDefinition(
    SpatialSketchConstraintDefinitionInput<UnitVector3, PositiveLength>,
);

impl SpatialSketchConstraintDefinition {
    /// Borrow the admitted spatial constraint kind.
    #[must_use]
    pub fn kind(&self) -> &SpatialSketchConstraintDefinitionInput<UnitVector3, PositiveLength> {
        &self.0
    }

    /// Edit the raw kind and replace it only after all edited local
    /// invariants pass.
    pub fn edit<R>(
        &mut self,
        edit: impl FnOnce(&mut SpatialSketchConstraintDefinitionInput) -> R,
    ) -> Result<R, &'static str> {
        let mut kind = self.0.to_raw();
        let result = edit(&mut kind);
        *self = kind.try_into()?;
        Ok(result)
    }
}

impl TryFrom<SpatialSketchConstraintDefinitionInput> for SpatialSketchConstraintDefinition {
    type Error = &'static str;

    fn try_from(kind: SpatialSketchConstraintDefinitionInput) -> Result<Self, Self::Error> {
        use SpatialSketchConstraintDefinitionInput as Kind;
        const INVALID: &str = "invalid spatial sketch constraint local arity or scalar value";
        let kind = kind.map_values(
            |direction| UnitVector3::new(direction).ok_or(INVALID),
            |distance| PositiveLength::try_from(distance).map_err(|_| INVALID),
        )?;
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
                sources, results, ..
            } => {
                !sources.is_empty()
                    && !results.is_empty()
                    && sources
                        .iter()
                        .chain(results)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == sources.len() + results.len()
            }
            Kind::ParallelToDirection { .. } => true,
        };
        if !valid {
            return Err(INVALID);
        }
        Ok(Self(kind))
    }
}

/// Neutral geometric relations between model-space sketch entities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SpatialSketchConstraintDefinitionInput<U = Vector3, L = crate::scalar::Length> {
    /// Source-native spatial relation without complete neutral semantics.
    Native {
        /// Source relation family.
        #[serde(deserialize_with = "deserialize_native_kind")]
        native_kind: NonBlankString,
        /// Source relation state or subtype discriminator, when present.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_native_state"
        )]
        native_state: Option<u64>,
        /// Neutral parameter driving the relation, when resolved.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_parameter"
        )]
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
        normal: U,
        /// Strictly positive operation-level offset magnitude.
        distance: L,
        /// Signed driving offset-distance parameter, when dimensional.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_spatial_sketch_constraint_definition_input_parameter"
        )]
        parameter: Option<OffsetParameter>,
    },
    /// A model-space line is parallel to one fixed model-space direction.
    ParallelToDirection {
        /// Line constrained to the direction.
        entity: SpatialSketchEntityId,
        /// Unit model-space direction; either sign denotes the same axis.
        direction: U,
    },
    /// A spline's defining model-space entities grouped by one native relation.
    SplineGroup {
        /// Ordered spline-group members.
        entities: Vec<SpatialSketchEntityId>,
    },
}

impl<U, L> SpatialSketchConstraintDefinitionInput<U, L> {
    /// The kind with its unit directions and its offset distance mapped, or
    /// the first refusal of either map.
    fn map_values<V, M, E>(
        self,
        mut direction: impl FnMut(U) -> Result<V, E>,
        length: impl FnOnce(L) -> Result<M, E>,
    ) -> Result<SpatialSketchConstraintDefinitionInput<V, M>, E> {
        use SpatialSketchConstraintDefinitionInput as Kind;
        Ok(match self {
            Self::Native {
                native_kind,
                native_state,
                parameter,
                operands,
            } => Kind::Native {
                native_kind,
                native_state,
                parameter,
                operands,
            },
            Self::Coincident { first, second } => Kind::Coincident { first, second },
            Self::Symmetric {
                first,
                second,
                axis,
            } => Kind::Symmetric {
                first,
                second,
                axis,
            },
            Self::PointOnSurface { point, surface } => Kind::PointOnSurface { point, surface },
            Self::Midpoint { point, entity } => Kind::Midpoint { point, entity },
            Self::Tangent { first, second } => Kind::Tangent { first, second },
            Self::PointDistance {
                first,
                second,
                parameter,
            } => Kind::PointDistance {
                first,
                second,
                parameter,
            },
            Self::PointLineDistance {
                point,
                line,
                parameter,
            } => Kind::PointLineDistance {
                point,
                line,
                parameter,
            },
            Self::LineLength { entity, parameter } => Kind::LineLength { entity, parameter },
            Self::RepeatedLineLength {
                entities,
                parameter,
            } => Kind::RepeatedLineLength {
                entities,
                parameter,
            },
            Self::ParallelLineDistance {
                first,
                second,
                parameter,
            } => Kind::ParallelLineDistance {
                first,
                second,
                parameter,
            },
            Self::RepeatedParallelLineDistance { pairs, parameter } => {
                Kind::RepeatedParallelLineDistance { pairs, parameter }
            }
            Self::ParallelLineSetDistance {
                first,
                second,
                parameter,
            } => Kind::ParallelLineSetDistance {
                first,
                second,
                parameter,
            },
            Self::Offset {
                sources,
                results,
                normal,
                distance,
                parameter,
            } => Kind::Offset {
                sources,
                results,
                normal: direction(normal)?,
                distance: length(distance)?,
                parameter,
            },
            Self::ParallelToDirection {
                entity,
                direction: value,
            } => Kind::ParallelToDirection {
                entity,
                direction: direction(value)?,
            },
            Self::SplineGroup { entities } => Kind::SplineGroup { entities },
        })
    }
}

impl SpatialSketchConstraintDefinitionInput<UnitVector3, PositiveLength> {
    /// The kind with raw directions and a raw offset distance.
    #[must_use]
    pub fn to_raw(&self) -> SpatialSketchConstraintDefinitionInput {
        let Ok(raw) = self.clone().map_values(
            |direction| Ok::<_, std::convert::Infallible>(Vector3::from(direction)),
            |distance| Ok(crate::scalar::Length::from(distance)),
        );
        raw
    }
}

/// NURBS curve with positive degree and positive rational weights.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct SpatialSketchNurbsCurve(crate::geometry::nurbs::NurbsCurve);

impl TryFrom<crate::geometry::nurbs::NurbsCurve> for SpatialSketchNurbsCurve {
    type Error = &'static str;

    fn try_from(curve: crate::geometry::nurbs::NurbsCurve) -> Result<Self, Self::Error> {
        if curve.degree() == 0 {
            return Err("spatial sketch NURBS degree must be at least one");
        }
        if curve
            .weights()
            .is_some_and(|weights| weights.iter().any(|weight| weight.get() <= 0.0))
        {
            return Err("spatial sketch NURBS weights must be positive");
        }
        Ok(Self(curve))
    }
}

impl<'de> Deserialize<'de> for SpatialSketchNurbsCurve {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(crate::geometry::nurbs::NurbsCurve::deserialize(
            deserializer,
        )?)
        .map_err(serde::de::Error::custom)
    }
}

impl std::ops::Deref for SpatialSketchNurbsCurve {
    type Target = crate::geometry::nurbs::NurbsCurve;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl SpatialSketchNurbsCurve {
    /// Map pole positions after every result passes admission.
    pub fn try_map_control_points<E>(
        &mut self,
        map: impl Fn(usize, FinitePoint3) -> Result<FinitePoint3, E>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<(), E>, cadmpeg_core::CodecError> {
        self.0.try_map_control_points(map, ctx)
    }
}

const EPS_SPATIAL_LINE_LENGTH: f64 = 1.0e-12;
const EPS_SPATIAL_CIRCLE_FRAME: f64 = 1.0e-9;

/// Spatial-sketch geometry with checked analytic numeric fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(into = "SpatialSketchGeometryDefinition"))]
#[serde(try_from = "SpatialSketchGeometryDefinition")]
pub struct SpatialSketchGeometry(
    SpatialSketchGeometryDefinition<FinitePoint3, UnitVector3, PositiveLength>,
);

impl SpatialSketchGeometry {
    /// Copy the retained geometry payload through the decode budget.
    pub fn try_clone_for_decode(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        use SpatialSketchGeometryDefinition as Definition;
        ctx.charge_work(1, operation)?;
        let definition = match &self.0 {
            Definition::Nurbs { curve } => Definition::Nurbs {
                curve: SpatialSketchNurbsCurve(curve.0.try_clone_for_decode(ctx, operation)?),
            },
            Definition::NurbsSurface { surface } => Definition::NurbsSurface {
                surface: surface.try_clone_for_decode(ctx, operation)?,
            },
            Definition::Native { native_kind } => Definition::Native {
                native_kind: native_kind.try_clone_for_decode(ctx, operation)?,
            },
            Definition::Point { .. }
            | Definition::Line { .. }
            | Definition::Circle { .. }
            | Definition::Arc { .. } => return Ok(self.clone()),
        };
        Ok(Self(definition))
    }

    /// Borrow the admitted spatial geometry definition.
    #[must_use]
    pub fn definition(
        &self,
    ) -> &SpatialSketchGeometryDefinition<FinitePoint3, UnitVector3, PositiveLength> {
        &self.0
    }

    /// Build a line from admitted endpoints. Only their separation is checked.
    pub fn try_line_from_parts(
        start: FinitePoint3,
        end: FinitePoint3,
    ) -> Result<Self, &'static str> {
        let start_point = start.get();
        let end_point = end.get();
        let distance = (end_point.x - start_point.x)
            .hypot(end_point.y - start_point.y)
            .hypot(end_point.z - start_point.z);
        if distance <= EPS_SPATIAL_LINE_LENGTH {
            return Err("spatial sketch line endpoints must be finite and separated");
        }
        Ok(Self(SpatialSketchGeometryDefinition::Line { start, end }))
    }
}

/// An admitted circular frame: center, radius, unit normal and unit
/// reference direction.
type CircularFrame = (FinitePoint3, PositiveLength, UnitVector3, UnitVector3);

/// Admit a circular center and radius, then a unit orthogonal frame.
fn admit_spatial_circle(
    center: Point3,
    radius: Length,
    normal: Vector3,
    reference_direction: Vector3,
) -> Result<CircularFrame, &'static str> {
    let (Some(center), Ok(radius)) = (FinitePoint3::new(center), PositiveLength::try_from(radius))
    else {
        return Err("spatial circular geometry requires finite center and positive finite radius");
    };
    let (Some(normal), Some(reference_direction)) = (
        UnitVector3::new(normal),
        UnitVector3::new(reference_direction),
    ) else {
        return Err("spatial circular normal and reference_direction must be unit and orthogonal");
    };
    if normal.as_raw().dot(*reference_direction.as_raw()).abs() > EPS_SPATIAL_CIRCLE_FRAME {
        return Err("spatial circular normal and reference_direction must be unit and orthogonal");
    }
    Ok((center, radius, normal, reference_direction))
}

impl TryFrom<SpatialSketchGeometryDefinition> for SpatialSketchGeometry {
    type Error = &'static str;

    fn try_from(definition: SpatialSketchGeometryDefinition) -> Result<Self, Self::Error> {
        use SpatialSketchGeometryDefinition as Definition;
        Ok(Self(match definition {
            Definition::Point { position } => Definition::Point {
                position: FinitePoint3::new(position)
                    .ok_or("spatial sketch point position must be finite")?,
            },
            Definition::Line { start, end } => {
                let distance = (end.x - start.x)
                    .hypot(end.y - start.y)
                    .hypot(end.z - start.z);
                let (Some(start), Some(end)) = (FinitePoint3::new(start), FinitePoint3::new(end))
                else {
                    return Err("spatial sketch line endpoints must be finite and separated");
                };
                if distance <= EPS_SPATIAL_LINE_LENGTH {
                    return Err("spatial sketch line endpoints must be finite and separated");
                }
                Definition::Line { start, end }
            }
            Definition::Circle {
                center,
                normal,
                reference_direction,
                radius,
            } => {
                let (center, radius, normal, reference_direction) =
                    admit_spatial_circle(center, radius, normal, reference_direction)?;
                Definition::Circle {
                    center,
                    normal,
                    reference_direction,
                    radius,
                }
            }
            Definition::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => {
                let (center, radius, normal, reference_direction) =
                    admit_spatial_circle(center, radius, normal, reference_direction)?;
                if start_angle == end_angle {
                    return Err("spatial sketch arc angles must be finite and distinct");
                }
                Definition::Arc {
                    center,
                    normal,
                    reference_direction,
                    radius,
                    start_angle,
                    end_angle,
                }
            }
            Definition::Nurbs { curve } => Definition::Nurbs { curve },
            Definition::NurbsSurface { surface } => Definition::NurbsSurface { surface },
            Definition::Native { native_kind } => Definition::Native { native_kind },
        }))
    }
}

impl SpatialSketchGeometryDefinition<FinitePoint3, UnitVector3, PositiveLength> {
    /// The definition with raw points, directions and radii.
    #[must_use]
    pub fn to_raw(&self) -> SpatialSketchGeometryDefinition {
        use SpatialSketchGeometryDefinition as Definition;
        match self {
            Self::Point { position } => Definition::Point {
                position: position.get(),
            },
            Self::Line { start, end } => Definition::Line {
                start: start.get(),
                end: end.get(),
            },
            Self::Circle {
                center,
                normal,
                reference_direction,
                radius,
            } => Definition::Circle {
                center: center.get(),
                normal: Vector3::from(*normal),
                reference_direction: Vector3::from(*reference_direction),
                radius: Length::from(*radius),
            },
            Self::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => Definition::Arc {
                center: center.get(),
                normal: Vector3::from(*normal),
                reference_direction: Vector3::from(*reference_direction),
                radius: Length::from(*radius),
                start_angle: *start_angle,
                end_angle: *end_angle,
            },
            Self::Nurbs { curve } => Definition::Nurbs {
                curve: curve.clone(),
            },
            Self::NurbsSurface { surface } => Definition::NurbsSurface {
                surface: surface.clone(),
            },
            Self::Native { native_kind } => Definition::Native {
                native_kind: native_kind.clone(),
            },
        }
    }
}

/// Definition admitted by model-space spatial-sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SpatialSketchGeometryDefinition<P = Point3, V = Vector3, L = Length> {
    /// Model-space point.
    Point {
        /// Point position in model coordinates.
        position: P,
    },
    /// Bounded model-space line segment.
    Line {
        /// Segment start in model coordinates.
        start: P,
        /// Segment end in model coordinates.
        end: P,
    },
    /// Oriented full model-space circle.
    Circle {
        /// Circle center in model coordinates.
        center: P,
        /// Unit normal defining positive angular travel.
        normal: V,
        /// Unit radial direction at parameter zero.
        reference_direction: V,
        /// Circle radius.
        radius: L,
    },
    /// Oriented bounded model-space circular arc.
    Arc {
        /// Arc center in model coordinates.
        center: P,
        /// Unit normal defining positive angular travel.
        normal: V,
        /// Unit radial direction at parameter zero.
        reference_direction: V,
        /// Arc radius.
        radius: L,
        /// Inclusive start parameter in radians.
        start_angle: Angle,
        /// Inclusive end parameter in radians.
        end_angle: Angle,
    },
    /// Model-space NURBS curve.
    Nurbs {
        /// Checked model-space knot, pole, and weight payload.
        curve: SpatialSketchNurbsCurve,
    },
    /// Polynomial tensor-product B-spline surface embedded in model space.
    NurbsSurface {
        /// Checked rectangular control grid and full knot vectors.
        surface: crate::geometry::nurbs::BsplineSurface,
    },
    /// Source-native spatial geometry not yet reduced to a neutral family.
    Native {
        /// Source geometry family.
        #[serde(deserialize_with = "deserialize_native_kind")]
        native_kind: NonBlankString,
    },
}

/// One relation constraining solved sketch geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchConstraint {
    /// Globally unique constraint id.
    pub id: SketchConstraintId,
    /// Owning sketch.
    pub sketch: SketchId,
    /// Constraint semantics.
    pub definition: SketchConstraintDefinition,
    /// User-visible constraint name, when assigned.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Whether this dimensional relation drives geometry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_driving"
    )]
    pub driving: Option<bool>,
    /// Whether the solver currently applies this relation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_active"
    )]
    pub active: Option<bool>,
    /// Whether the relation belongs to virtual sketch space.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_virtual_space"
    )]
    pub virtual_space: Option<bool>,
    /// Whether the relation is displayed in the sketch UI.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visible"
    )]
    pub visible: Option<bool>,
    /// Source orientation bit field, when the relation carries one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_orientation"
    )]
    pub orientation: Option<u32>,
    /// Persisted label offset from the constrained geometry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_label_distance"
    )]
    pub label_distance: Option<SketchLabelValue>,
    /// Persisted position along the dimension label path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_label_position"
    )]
    pub label_position: Option<SketchLabelValue>,
    /// Application metadata text attached to this relation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_metadata"
    )]
    pub metadata: Option<String>,
    /// Source-native relation record when decoded from one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A geometric locus on a sketch entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "entity", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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

impl cadmpeg_core::decode::cost::DecodeCost for SketchLocus {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        let id = match self {
            Self::Entity(id) | Self::Start(id) | Self::End(id) | Self::Center(id) => id,
        };
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(&(1_u8, id), ctx, operation)
    }
}

/// Coordinate axis selected by a sketch relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
    pub name: NonBlankString,
    /// Source-native role code, when the field carries one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_role"
    )]
    pub role: Option<u32>,
}

/// One ordered operand retained from a native sketch relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchNativeOperand {
    /// Non-empty source-native operand family.
    pub native_kind: NonBlankString,
    /// Source-native field and optional role containing this operand.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_field"
    )]
    pub field: Option<NativeOperandField>,
    /// Source-native object index; absent when the native operand names no
    /// object (an axis, root point, or external reference slot).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_object_index"
    )]
    pub object_index: Option<u32>,
    /// Resolved source-native operand record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// One progenitor/result pair in a sketch offset relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OffsetParameter {
    /// Driving parameter identity.
    pub id: ParameterId,
    /// Whether the stored positive distance is the negation of the parameter.
    pub negated: bool,
}

/// One axis of a rectangular sketch pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPatternDirection {
    /// Unit direction in sketch coordinates.
    direction: UnitVector2,
    /// Adjacent-instance spacing along `direction`.
    spacing: Length,
    /// Driving distance parameter and the distance form it controls.
    pub distance: Option<SketchPatternDistance>,
    /// Driving instance-count parameter, when the source exposes it as a neutral parameter.
    pub count_parameter: Option<ParameterId>,
}

const EPS_PATTERN_DIRECTION_ORTHOGONALITY: f64 = 1.0e-9;

impl SketchPatternDirection {
    /// Admit a finite unit direction and finite signed spacing.
    pub fn new(
        direction: [f64; 2],
        spacing: Length,
        distance: Option<SketchPatternDistance>,
        count_parameter: Option<ParameterId>,
    ) -> Option<Self> {
        let direction = UnitVector2::new(direction)?;
        Some(Self {
            direction,
            spacing,
            distance,
            count_parameter,
        })
    }

    /// Unit direction in sketch coordinates.
    #[must_use]
    pub fn direction(&self) -> UnitVector2 {
        self.direction
    }

    /// Adjacent-instance signed spacing.
    #[must_use]
    pub fn spacing(&self) -> Length {
        self.spacing
    }
}

/// Distance form controlled by a rectangular-pattern parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SketchPatternDistance {
    /// The parameter controls adjacent-instance spacing.
    Spacing {
        /// Driving parameter identity.
        parameter: ParameterId,
    },
    /// The parameter controls the seed-to-final-instance span.
    Span {
        /// Driving parameter identity.
        parameter: ParameterId,
    },
}

impl SketchPatternDistance {
    /// Parameter that controls this distance form.
    #[must_use]
    pub fn parameter(&self) -> &ParameterId {
        match self {
            Self::Spacing { parameter } | Self::Span { parameter } => parameter,
        }
    }
}

/// One resolved rectangular-pattern instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchPatternInstance {
    /// Entities in fixed seed-entity order.
    pub entities: Vec<SketchEntityId>,
}

/// One resolved circular-pattern instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchCircularPatternInstance {
    /// Signed rotation from the seed instance in radians.
    pub angle: crate::scalar::NonZeroAngle,
    /// Entities in fixed seed-entity order.
    pub entities: Vec<SketchEntityId>,
}

/// Checked two-axis rectangular sketch pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "SketchRectangularPatternWire",
    into = "SketchRectangularPatternWire"
)]
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
            || rows
                .iter()
                .any(|row| row.len() != cadmpeg_core::decode::index_from_u32(column_count))
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
        let [first, second] = [directions[0].direction.get(), directions[1].direction.get()];
        let dot = first[0] * second[0] + first[1] * second[1];
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
}

/// Checked circular sketch pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "SketchCircularPatternWire",
    into = "SketchCircularPatternWire"
)]
pub struct SketchCircularPattern {
    center: SketchEntityId,
    angle: Angle,
    angle_parameter: Option<ParameterId>,
    count_parameter: Option<ParameterId>,
    seed: Vec<SketchEntityId>,
    /// Instances after the seed, at a population whose count the IR width can
    /// state. The type carries the bound and the figure, so the pattern
    /// stores no count of its own.
    instances: SeededMembers<SketchCircularPatternInstance>,
}

/// A nonempty population whose count, with the seed counted, fits the IR
/// width.
///
/// The pattern states its instance count as the population plus the seed, in
/// a `u32`. The bound therefore belongs on the population, not on the count:
/// this type proves the bound at construction and refuses a population the
/// width cannot count. `TryFrom<Vec<T>>` is the only constructor, and the type
/// hands out a slice, so no value can grow past the count it proved.
#[derive(Debug, Clone, PartialEq)]
struct SeededMembers<T> {
    members: Vec<T>,
}

impl<T> TryFrom<Vec<T>> for SeededMembers<T> {
    type Error = &'static str;

    fn try_from(members: Vec<T>) -> Result<Self, Self::Error> {
        if members.is_empty() {
            return Err("population states no member");
        }
        let Some(_) = u32::try_from(members.len())
            .ok()
            .and_then(|population| population.checked_add(1))
        else {
            return Err("population holds more members than the seeded count can state");
        };
        Ok(Self { members })
    }
}

impl<T> std::ops::Deref for SeededMembers<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.members
    }
}

impl<T> IntoIterator for SeededMembers<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.members.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a SeededMembers<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.members.iter()
    }
}

/// The refusal every arity and ownership check of a circular pattern states.
const CIRCULAR_PATTERN_ARITY: &str =
    "circular pattern seed and instances must share one fixed positive entity arity over distinct entities";

impl SketchCircularPattern {
    /// Construct a non-empty pattern whose instances have one fixed positive
    /// entity arity.
    ///
    /// This is the recognizer shape, for a caller that holds no channel to
    /// state a cause in. The route that has one is
    /// `TryFrom<SketchCircularPatternWire>`, which states the cause
    /// `Self::admit` gives.
    pub fn new(
        center: SketchEntityId,
        angle: Angle,
        angle_parameter: Option<ParameterId>,
        count_parameter: Option<ParameterId>,
        seed: Vec<SketchEntityId>,
        instances: Vec<SketchCircularPatternInstance>,
    ) -> Option<Self> {
        Self::admit(
            center,
            angle,
            angle_parameter,
            count_parameter,
            seed,
            instances,
        )
        .ok()
    }

    /// Admit the pattern, naming the cause of every refusal.
    ///
    /// The population bound is the one `SeededMembers` proves, and its two
    /// texts are the ones this route states: an empty `instances` array is
    /// refused as a population that states no member, not as an arity
    /// disagreement.
    fn admit(
        center: SketchEntityId,
        angle: Angle,
        angle_parameter: Option<ParameterId>,
        count_parameter: Option<ParameterId>,
        seed: Vec<SketchEntityId>,
        instances: Vec<SketchCircularPatternInstance>,
    ) -> Result<Self, &'static str> {
        let entity_arity = seed.len();
        if entity_arity == 0
            || instances
                .iter()
                .any(|instance| instance.entities.len() != entity_arity)
        {
            return Err(CIRCULAR_PATTERN_ARITY);
        }
        let mut entities = std::collections::HashSet::new();
        if seed
            .iter()
            .chain(
                instances
                    .iter()
                    .flat_map(|instance| instance.entities.iter()),
            )
            .any(|entity| entity == &center || !entities.insert(entity))
        {
            return Err(CIRCULAR_PATTERN_ARITY);
        }
        Ok(Self {
            center,
            angle,
            angle_parameter,
            count_parameter,
            seed,
            instances: SeededMembers::try_from(instances)?,
        })
    }

    /// Point entity defining the center of rotation.
    #[must_use]
    pub fn center(&self) -> &SketchEntityId {
        &self.center
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

    /// Instances in pattern order, after the seed. Slice position plus one is
    /// the zero-based pattern index.
    #[must_use]
    pub fn instances(&self) -> &[SketchCircularPatternInstance] {
        &self.instances
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SketchPatternDirectionWire {
    direction: [f64; 2],
    spacing: Length,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_distance"
    )]
    distance: Option<SketchPatternDistance>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_count_parameter"
    )]
    count_parameter: Option<ParameterId>,
}

impl SketchPatternDirectionWire {
    fn from_direction(value: &SketchPatternDirection) -> Self {
        Self {
            direction: value.direction.get(),
            spacing: value.spacing,
            distance: value.distance.clone(),
            count_parameter: value.count_parameter.clone(),
        }
    }

    fn into_direction(self) -> Result<SketchPatternDirection, &'static str> {
        SketchPatternDirection::new(
            self.direction,
            self.spacing,
            self.distance,
            self.count_parameter,
        )
        .ok_or("pattern direction must be finite and unit, with finite spacing")
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "SketchRectangularPattern"))]
#[serde(deny_unknown_fields)]
struct SketchRectangularPatternWire {
    /// Ordered pattern directions.
    directions: [SketchPatternDirectionWire; 2],
    /// Rectangular instance rows.
    rows: Vec<Vec<SketchPatternInstance>>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "SketchCircularPattern"))]
#[serde(deny_unknown_fields)]
struct SketchCircularPatternWire {
    /// Point entity defining the center of rotation.
    center: SketchEntityId,
    /// Evaluated angular span stored by the native pattern.
    angle: Angle,
    /// Driving angular-span parameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle_parameter"
    )]
    angle_parameter: Option<ParameterId>,
    /// Driving instance-count parameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_count_parameter"
    )]
    count_parameter: Option<ParameterId>,
    /// Seed entities in fixed order.
    seed: Vec<SketchEntityId>,
    /// Instances after the seed, in pattern order.
    instances: Vec<SketchCircularPatternInstance>,
}

impl From<SketchRectangularPattern> for SketchRectangularPatternWire {
    fn from(pattern: SketchRectangularPattern) -> Self {
        Self {
            directions: [
                SketchPatternDirectionWire::from_direction(&pattern.directions[0]),
                SketchPatternDirectionWire::from_direction(&pattern.directions[1]),
            ],
            rows: pattern.rows,
        }
    }
}

impl TryFrom<SketchRectangularPatternWire> for SketchRectangularPattern {
    type Error = &'static str;

    fn try_from(wire: SketchRectangularPatternWire) -> Result<Self, Self::Error> {
        let [first, second] = wire.directions;
        let directions = [first.into_direction()?, second.into_direction()?];
        Self::new(directions, wire.rows).ok_or(
            "rectangular pattern rows must form one non-empty grid whose instances have one fixed positive entity arity",
        )
    }
}

impl From<SketchCircularPattern> for SketchCircularPatternWire {
    fn from(pattern: SketchCircularPattern) -> Self {
        Self {
            center: pattern.center,
            angle: pattern.angle,
            angle_parameter: pattern.angle_parameter,
            count_parameter: pattern.count_parameter,
            seed: pattern.seed,
            instances: pattern.instances.into_iter().collect(),
        }
    }
}

impl TryFrom<SketchCircularPatternWire> for SketchCircularPattern {
    type Error = &'static str;

    fn try_from(wire: SketchCircularPatternWire) -> Result<Self, Self::Error> {
        Self::admit(
            wire.center,
            wire.angle,
            wire.angle_parameter,
            wire.count_parameter,
            wire.seed,
            wire.instances,
        )
    }
}

/// One independently measured pair within a repeated linear dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct SketchDistancePair {
    /// First locus in the measured pair.
    pub first: SketchLocus,
    /// Second locus in the measured pair.
    pub second: SketchLocus,
}

/// Meaning of an internal sketch alignment helper relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    from = "SketchInternalAlignmentWire",
    into = "SketchInternalAlignmentWire"
)]
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

#[derive(Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "alignment", rename_all = "snake_case", deny_unknown_fields)]
enum SketchInternalAlignmentWire {
    /// Major diameter helper for an ellipse.
    EllipseMajorDiameter {},
    /// Minor diameter helper for an ellipse.
    EllipseMinorDiameter {},
    /// First ellipse focus helper.
    EllipseFocus1 {},
    /// Second ellipse focus helper.
    EllipseFocus2 {},
    /// Hyperbola major-axis helper.
    HyperbolaMajor {},
    /// Hyperbola minor-axis helper.
    HyperbolaMinor {},
    /// Hyperbola focus helper.
    HyperbolaFocus {},
    /// Parabola focus helper.
    ParabolaFocus {},
    /// B-spline control-point helper.
    BsplineControlPoint {
        /// Zero-based control-point index.
        index: u32,
    },
    /// B-spline knot-point helper.
    BsplineKnotPoint {
        /// Zero-based knot-point index.
        index: u32,
    },
    /// Parabola focal-axis helper.
    ParabolaFocalAxis {},
}

impl From<SketchInternalAlignmentWire> for SketchInternalAlignment {
    fn from(wire: SketchInternalAlignmentWire) -> Self {
        match wire {
            SketchInternalAlignmentWire::EllipseMajorDiameter {} => Self::EllipseMajorDiameter,
            SketchInternalAlignmentWire::EllipseMinorDiameter {} => Self::EllipseMinorDiameter,
            SketchInternalAlignmentWire::EllipseFocus1 {} => Self::EllipseFocus1,
            SketchInternalAlignmentWire::EllipseFocus2 {} => Self::EllipseFocus2,
            SketchInternalAlignmentWire::HyperbolaMajor {} => Self::HyperbolaMajor,
            SketchInternalAlignmentWire::HyperbolaMinor {} => Self::HyperbolaMinor,
            SketchInternalAlignmentWire::HyperbolaFocus {} => Self::HyperbolaFocus,
            SketchInternalAlignmentWire::ParabolaFocus {} => Self::ParabolaFocus,
            SketchInternalAlignmentWire::BsplineControlPoint { index } => {
                Self::BsplineControlPoint(index)
            }
            SketchInternalAlignmentWire::BsplineKnotPoint { index } => {
                Self::BsplineKnotPoint(index)
            }
            SketchInternalAlignmentWire::ParabolaFocalAxis {} => Self::ParabolaFocalAxis,
        }
    }
}

impl From<SketchInternalAlignment> for SketchInternalAlignmentWire {
    fn from(value: SketchInternalAlignment) -> Self {
        match value {
            SketchInternalAlignment::EllipseMajorDiameter => Self::EllipseMajorDiameter {},
            SketchInternalAlignment::EllipseMinorDiameter => Self::EllipseMinorDiameter {},
            SketchInternalAlignment::EllipseFocus1 => Self::EllipseFocus1 {},
            SketchInternalAlignment::EllipseFocus2 => Self::EllipseFocus2 {},
            SketchInternalAlignment::HyperbolaMajor => Self::HyperbolaMajor {},
            SketchInternalAlignment::HyperbolaMinor => Self::HyperbolaMinor {},
            SketchInternalAlignment::HyperbolaFocus => Self::HyperbolaFocus {},
            SketchInternalAlignment::ParabolaFocus => Self::ParabolaFocus {},
            SketchInternalAlignment::BsplineControlPoint(index) => {
                Self::BsplineControlPoint { index }
            }
            SketchInternalAlignment::BsplineKnotPoint(index) => Self::BsplineKnotPoint { index },
            SketchInternalAlignment::ParabolaFocalAxis => Self::ParabolaFocalAxis {},
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
#[serde(deny_unknown_fields)]
struct SketchPolygonWire {
    /// Ordered polygon members.
    entities: Vec<SketchEntityId>,
}

impl TryFrom<SketchPolygonWire> for SketchPolygon {
    type Error = SketchCollectionError;

    fn try_from(wire: SketchPolygonWire) -> Result<Self, Self::Error> {
        let invalid = "entities requires at least three distinct polygon members";
        if wire.entities.len() < 3 {
            return Err(SketchCollectionError::Invalid(invalid));
        }
        let mut members = std::collections::HashSet::new();
        members
            .try_reserve(wire.entities.len())
            .map_err(|error| SketchCollectionError::Admission(error.to_string()))?;
        for entity in &wire.entities {
            if !members.insert(entity) {
                return Err(SketchCollectionError::Invalid(invalid));
            }
        }
        Ok(Self {
            entities: wire.entities,
        })
    }
}

impl SketchPolygon {
    /// Admit decoded members and charge temporary uniqueness storage.
    pub fn try_new(
        entities: Vec<SketchEntityId>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Result<Self, &'static str>, cadmpeg_core::CodecError> {
        if entities.len() < 3 {
            return Ok(Err(
                "entities requires at least three distinct polygon members",
            ));
        }
        if !distinct_sketch_members(ctx, entities.iter().map(SketchEntityId::as_str), operation)? {
            return Ok(Err(
                "entities requires at least three distinct polygon members",
            ));
        }
        Ok(Ok(Self { entities }))
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
#[serde(deny_unknown_fields)]
struct SketchSameCoordinateWire {
    /// First locus.
    first: SketchLocus,
    /// Second locus.
    second: SketchLocus,
    /// Shared coordinate axis.
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

const EPS_POLAR_DISTANCE_ZERO: f64 = 1.0e-12;

/// A sketch constraint definition with admitted local arity and scalar values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SketchConstraintDefinitionInput")]
pub struct SketchConstraintDefinition(SketchConstraintDefinitionInput);

impl SketchConstraintDefinition {
    /// Construct a native relation with one source operand and its resolved entities.
    ///
    /// The operand supplies the required nonempty binding. State, flags, and
    /// parameter are absent; callers with those fields use input admission.
    #[must_use]
    pub fn native_with_operand(
        native_kind: NonBlankString,
        native_properties: BTreeMap<String, String>,
        entities: Vec<SketchEntityId>,
        operand: SketchNativeOperand,
    ) -> Self {
        Self(SketchConstraintDefinitionInput::Native {
            native_kind,
            native_state: None,
            native_flags: None,
            native_properties,
            entities,
            parameter: None,
            operands: vec![operand],
        })
    }

    /// Borrow the admitted constraint kind.
    #[must_use]
    pub fn kind(&self) -> &SketchConstraintDefinitionInput {
        &self.0
    }

    /// Set the driving parameter of an admitted planar offset relation.
    /// The parameter does not change offset-pair admission.
    pub fn set_offset_parameter(&mut self, driving: OffsetParameter) -> bool {
        let SketchConstraintDefinitionInput::Offset { parameter, .. } = &mut self.0 else {
            return false;
        };
        *parameter = Some(driving);
        true
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
            Kind::Disabled {} => true,
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
#[serde(deny_unknown_fields)]
pub enum SketchConstraintDefinitionInput {
    /// Persisted no-op relation slot.
    Disabled {},
    /// Two entity loci coincide.
    Coincident {
        /// Coincident entity loci.
        entities: Vec<SketchEntityId>,
    },
    /// Entities participate in one native polygon relation.
    Polygon {
        /// Checked polygon members.
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
        pattern: SketchRectangularPattern,
    },
    /// A parameter-driven circular pattern with geometrically resolved instances.
    CircularPattern {
        /// Checked center, parameters, and positional instances.
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_spatial_sketch_constraint_definition_input_parameter"
        )]
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
        angle: PositiveAngle,
    },
    /// Bounded ellipse parameter sweep fixed by the relation kind.
    EllipseAngle {
        /// Constrained bounded ellipse.
        entity: SketchEntityId,
        /// Fixed positive parameter sweep in radians.
        angle: PositiveAngle,
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_sketch_constraint_definition_input_parameter"
        )]
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Angle>,
        /// Driving distance parameter, when the source supplies one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_distance_parameter"
        )]
        distance_parameter: Option<ParameterId>,
    },
    /// Direct difference between two angle-valued solver scalars.
    AngleDifference {
        /// Solver-local key of the first angle scalar.
        first: u32,
        /// Solver-local key of the second angle scalar.
        second: u32,
        /// Solver-local key of the difference scalar receiving `first - second`.
        difference: u32,
        /// Source-evaluated non-negative angle difference in radians.
        value: Angle,
    },
    /// Equality between two equality-class solver scalars.
    ScalarEquality {
        /// Solver-local key of the first equality-class scalar.
        first: u32,
        /// Solver-local key of the second equality-class scalar.
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_font"
        )]
        font: Option<String>,
        /// Whether the construction dimension controls text height rather than width.
        is_text_height: bool,
    },
    /// Source-native relation not yet reduced to a neutral family.
    Native {
        /// Source constraint family.
        #[serde(deserialize_with = "deserialize_native_kind")]
        native_kind: NonBlankString,
        /// Source-native constraint-state mask, when the format carries one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_native_state"
        )]
        native_state: Option<u64>,
        /// Source-native constraint flags, when distinct from constraint state.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_native_flags"
        )]
        native_flags: Option<u64>,
        /// Exact source-native scalar properties not represented by common state or flags.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
        native_properties: BTreeMap<String, String>,
        /// Referenced entities.
        entities: Vec<SketchEntityId>,
        /// Driving or driven parameter attached to the relation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_sketch_constraint_definition_input_parameter"
        )]
        parameter: Option<ParameterId>,
        /// Native operands whose neutral loci are unresolved.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        operands: Vec<SketchNativeOperand>,
    },
}

impl SketchConstraintDefinitionInput {
    /// The entity whose neutral geometry kind this relation restricts, with
    /// the kind it admits. Midpoint, arc-angle and ellipse-angle relations
    /// restrict their entity; other relations restrict no entity kind.
    #[must_use]
    pub fn entity_kind_restriction(
        &self,
    ) -> Option<(&SketchEntityId, SketchEntityKindRestriction)> {
        match self {
            Self::Midpoint { entity, .. } => {
                Some((entity, SketchEntityKindRestriction::BoundedCurve))
            }
            Self::ArcAngle { entity, .. } => {
                Some((entity, SketchEntityKindRestriction::CircularArc))
            }
            Self::EllipseAngle { entity, .. } => {
                Some((entity, SketchEntityKindRestriction::BoundedEllipse))
            }
            _ => None,
        }
    }
}

/// The neutral geometry kinds that a relation admits for its restricted entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchEntityKindRestriction {
    /// A line, circular arc, bounded ellipse, hyperbola or parabola, or NURBS curve.
    BoundedCurve,
    /// A circular arc.
    CircularArc,
    /// A bounded ellipse.
    BoundedEllipse,
}

impl SketchEntityKindRestriction {
    /// Whether `geometry` has an admitted kind. External and native geometry
    /// has no neutral kind, so it is admitted.
    #[must_use]
    pub fn admits<P, L, R, W, D, M>(
        self,
        geometry: &SketchGeometryDefinition<P, L, R, W, D, M>,
    ) -> bool {
        if matches!(
            geometry,
            SketchGeometryDefinition::ExternalReference { .. }
                | SketchGeometryDefinition::Native { .. }
        ) {
            return true;
        }
        match self {
            Self::BoundedCurve => matches!(
                geometry,
                SketchGeometryDefinition::Line { .. }
                    | SketchGeometryDefinition::Arc { .. }
                    | SketchGeometryDefinition::Ellipse {
                        bounds: Some(_),
                        ..
                    }
                    | SketchGeometryDefinition::Hyperbola {
                        bounds: Some(_),
                        ..
                    }
                    | SketchGeometryDefinition::Parabola {
                        bounds: Some(_),
                        ..
                    }
                    | SketchGeometryDefinition::Nurbs { .. }
            ),
            Self::CircularArc => matches!(geometry, SketchGeometryDefinition::Arc { .. }),
            Self::BoundedEllipse => matches!(
                geometry,
                SketchGeometryDefinition::Ellipse {
                    bounds: Some(_),
                    ..
                }
            ),
        }
    }
}

crate::units::named_field!(
    deserialize_object,
    cadmpeg_core::text::NonBlankString,
    "object"
);
crate::units::named_field!(
    deserialize_native_kind,
    cadmpeg_core::text::NonBlankString,
    "native_kind"
);

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_configuration, String, "configuration");
cadmpeg_core::named_optional_field!(deserialize_visible, bool, "visible");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_geometry_ref, String, "geometry_ref");
cadmpeg_core::named_optional_field!(deserialize_bounds, [Angle; 2], "bounds");
cadmpeg_core::named_optional_field!(
    deserialize_sketch_geometry_definition_bounds<R>,
    [R; 2],
    "bounds"
);
cadmpeg_core::named_optional_field!(deserialize_width_factor<W>, W, "width_factor");
cadmpeg_core::named_optional_field!(deserialize_placement<P>, TextPlacement<P>, "placement");
cadmpeg_core::named_optional_field!(
    deserialize_horizontal_alignment,
    SketchTextHorizontalAlignment,
    "horizontal_alignment"
);
cadmpeg_core::named_optional_field!(
    deserialize_vertical_alignment,
    SketchTextVerticalAlignment,
    "vertical_alignment"
);
cadmpeg_core::named_optional_field!(deserialize_document, String, "document");
cadmpeg_core::named_optional_field!(deserialize_native_state, u64, "native_state");
cadmpeg_core::named_optional_field!(
    deserialize_parameter,
    crate::features::ParameterId,
    "parameter"
);
cadmpeg_core::named_optional_field!(
    deserialize_spatial_sketch_constraint_definition_input_parameter,
    OffsetParameter,
    "parameter"
);
cadmpeg_core::named_optional_field!(deserialize_driving, bool, "driving");
cadmpeg_core::named_optional_field!(deserialize_active, bool, "active");
cadmpeg_core::named_optional_field!(deserialize_virtual_space, bool, "virtual_space");
cadmpeg_core::named_optional_field!(deserialize_orientation, u32, "orientation");
cadmpeg_core::named_optional_field!(
    deserialize_label_distance,
    SketchLabelValue,
    "label_distance"
);
cadmpeg_core::named_optional_field!(
    deserialize_label_position,
    SketchLabelValue,
    "label_position"
);
cadmpeg_core::named_optional_field!(deserialize_metadata, String, "metadata");
cadmpeg_core::named_optional_field!(deserialize_role, u32, "role");
cadmpeg_core::named_optional_field!(deserialize_field, NativeOperandField, "field");
cadmpeg_core::named_optional_field!(deserialize_object_index, u32, "object_index");
cadmpeg_core::named_optional_field!(deserialize_distance, SketchPatternDistance, "distance");
cadmpeg_core::named_optional_field!(deserialize_count_parameter, ParameterId, "count_parameter");
cadmpeg_core::named_optional_field!(deserialize_angle_parameter, ParameterId, "angle_parameter");
cadmpeg_core::named_optional_field!(
    deserialize_sketch_constraint_definition_input_parameter,
    ParameterId,
    "parameter"
);
cadmpeg_core::named_optional_field!(deserialize_angle, Angle, "angle");
cadmpeg_core::named_optional_field!(
    deserialize_distance_parameter,
    ParameterId,
    "distance_parameter"
);
cadmpeg_core::named_optional_field!(deserialize_font, String, "font");
cadmpeg_core::named_optional_field!(deserialize_native_flags, u64, "native_flags");

mod identity_rewrite;

impl DecodeCost for SketchConstraint {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        let identity = (
            &self.id,
            &self.sketch,
            &self.definition,
            &self.name,
            &self.driving,
            &self.active,
        )
            .decode_cost(ctx, operation)?;
        let metadata = (
            &self.virtual_space,
            &self.visible,
            &self.orientation,
            &self.label_distance,
            &self.label_position,
            (&self.metadata, &self.native_ref),
        )
            .decode_cost(ctx, operation)?;
        identity
            .checked_add(metadata)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }
}

impl DecodeCost for SketchAxis {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        1_u8.decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchCoordinateAxis {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        1_u8.decode_cost(ctx, operation)
    }
}

impl DecodeCost for NativeOperandField {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (&self.name, &self.role).decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchNativeOperand {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (
            &self.native_kind,
            &self.field,
            &self.object_index,
            &self.native_ref,
        )
            .decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchOffsetPair {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (&self.source, &self.result, self.source_reversed).decode_cost(ctx, operation)
    }
}

impl DecodeCost for OffsetParameter {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (&self.id, self.negated).decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchPatternDirection {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (
            self.direction.get(),
            self.spacing.get(),
            &self.distance,
            &self.count_parameter,
        )
            .decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchPatternDistance {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Spacing { parameter } | Self::Span { parameter } => {
                (1_u8, parameter).decode_cost(ctx, operation)
            }
        }
    }
}

impl DecodeCost for SketchPatternInstance {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        self.entities.decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchCircularPatternInstance {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (self.angle.get(), &self.entities).decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchRectangularPattern {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (&self.directions, &self.rows).decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchCircularPattern {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (
            &self.center,
            self.angle.get(),
            &self.angle_parameter,
            &self.count_parameter,
            &self.seed,
            &self.instances.members,
        )
            .decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchDistanceMeasurement {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Distance { first, second }
            | Self::Horizontal { first, second }
            | Self::Vertical { first, second } => (1_u8, first, second).decode_cost(ctx, operation),
        }
    }
}

impl DecodeCost for SketchDistancePair {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (&self.first, &self.second).decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchInternalAlignment {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::EllipseMajorDiameter
            | Self::EllipseMinorDiameter
            | Self::EllipseFocus1
            | Self::EllipseFocus2
            | Self::HyperbolaMajor
            | Self::HyperbolaMinor
            | Self::HyperbolaFocus
            | Self::ParabolaFocus
            | Self::ParabolaFocalAxis => 1_u8.decode_cost(ctx, operation),
            Self::BsplineControlPoint(index) | Self::BsplineKnotPoint(index) => {
                (1_u8, index).decode_cost(ctx, operation)
            }
        }
    }
}

impl DecodeCost for SketchPolygon {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        self.entities.decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchSameCoordinate {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        (&self.first, &self.second, &self.axis).decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchLabelValue {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        self.0.decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchConstraintDefinition {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        self.0.decode_cost(ctx, operation)
    }
}

impl DecodeCost for SketchConstraintDefinitionInput {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            SketchConstraintDefinitionInput::Disabled {} => 1_u8.decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::Coincident { entities }
            | SketchConstraintDefinitionInput::SplineGroup { entities } => {
                (1_u8, entities).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::Polygon { polygon } => {
                (1_u8, polygon).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::RectangularPattern { pattern } => {
                (1_u8, pattern).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::CircularPattern { pattern } => {
                (1_u8, pattern).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::TextFrame { text, frame } => {
                (1_u8, text, frame).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::TextPath {
                text,
                path,
                glyph_transforms,
            } => (1_u8, text, path, glyph_transforms).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::CoincidentLoci { loci } => {
                (1_u8, loci).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::SameCoordinate { relation } => {
                (1_u8, relation).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::PointOnObject { point, entity }
            | SketchConstraintDefinitionInput::Midpoint { point, entity } => {
                (1_u8, point, entity).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::PointCoordinateValues { point, values } => {
                (1_u8, point, (*values).map(Length::get)).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::MidpointCoordinate {
                first,
                second,
                axis,
                value,
            } => (1_u8, first, second, axis, value.get()).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::Offset {
                pairs,
                distance,
                parameter,
            } => (1_u8, pairs, distance.get(), parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::ProjectedCopy { source, result } => {
                (1_u8, source, result).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::AtIntersection {
                point,
                first,
                second,
            } => (1_u8, point, first, second).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::Concentric { first, second }
            | SketchConstraintDefinitionInput::Coradial { first, second }
            | SketchConstraintDefinitionInput::Collinear { first, second }
            | SketchConstraintDefinitionInput::Parallel { first, second }
            | SketchConstraintDefinitionInput::Perpendicular { first, second }
            | SketchConstraintDefinitionInput::Tangent { first, second }
            | SketchConstraintDefinitionInput::Curvature { first, second }
            | SketchConstraintDefinitionInput::Equal { first, second } => {
                (1_u8, first, second).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::Symmetric {
                first,
                second,
                axis,
            } => (1_u8, first, second, axis).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::PointSymmetric {
                first,
                second,
                center,
            } => (1_u8, first, second, center).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::Horizontal { entity }
            | SketchConstraintDefinitionInput::Vertical { entity }
            | SketchConstraintDefinitionInput::Fixed { entity } => {
                (1_u8, entity).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::TangentLoci { first, second } => {
                (1_u8, first, second).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::ArcAngle { entity, angle }
            | SketchConstraintDefinitionInput::EllipseAngle { entity, angle } => {
                (1_u8, entity, angle.get()).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::Distance {
                entities,
                parameter,
            } => (1_u8, entities, parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::DistanceLoci {
                first,
                second,
                parameter,
            } => (1_u8, first, second, parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::DistanceLociValue {
                first,
                second,
                distance,
                parameter,
            } => (1_u8, first, second, distance.get(), parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::PolarDistance {
                first,
                second,
                distance,
                angle,
                distance_parameter,
            } => (
                1_u8,
                first,
                second,
                distance.get(),
                angle.as_ref().map(|angle| angle.get()),
                distance_parameter,
            )
                .decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::AngleDifference {
                first,
                second,
                difference,
                value,
            } => (1_u8, first, second, difference, value.get()).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::ScalarEquality { first, second } => {
                (1_u8, first, second).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::EqualDistance { first, second } => {
                (1_u8, first, second).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::HorizontalDistance {
                first,
                second,
                parameter,
            }
            | SketchConstraintDefinitionInput::VerticalDistance {
                first,
                second,
                parameter,
            } => (1_u8, first, second, parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::RepeatedDistance {
                measurements,
                parameter,
            } => (1_u8, measurements, parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::RepeatedLength {
                entities,
                parameter,
            }
            | SketchConstraintDefinitionInput::RepeatedRadius {
                entities,
                parameter,
            }
            | SketchConstraintDefinitionInput::RepeatedDiameter {
                entities,
                parameter,
            } => (1_u8, entities, parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::ParallelLineSetDistance {
                first,
                second,
                parameter,
            } => (1_u8, first, second, parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::Angle {
                first,
                second,
                parameter,
            } => (1_u8, first, second, parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::AngleToAxis {
                entity,
                axis,
                parameter,
            } => (1_u8, entity, axis, parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::Radius { entity, parameter }
            | SketchConstraintDefinitionInput::Diameter { entity, parameter } => {
                (1_u8, entity, parameter).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::SnellsLaw {
                incident,
                refracted,
                interface,
                parameter,
            } => (1_u8, incident, refracted, interface, parameter).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::Weight { entity, parameter } => {
                (1_u8, entity, parameter).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::InternalAlignment {
                helper,
                parent,
                alignment,
            } => (1_u8, helper, parent, alignment).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::Group { elements } => {
                (1_u8, elements).decode_cost(ctx, operation)
            }
            SketchConstraintDefinitionInput::Text {
                elements,
                text,
                font,
                is_text_height,
            } => (1_u8, elements, text, font, is_text_height).decode_cost(ctx, operation),
            SketchConstraintDefinitionInput::Native {
                native_kind,
                native_state,
                native_flags,
                native_properties,
                entities,
                parameter,
                operands,
            } => {
                let prefix =
                    (1_u8, native_kind, native_state, native_flags).decode_cost(ctx, operation)?;
                let mut properties = 0_u64;
                for (key, value) in ctx.admit_iter(native_properties, operation)? {
                    properties = properties
                        .checked_add((key, value).decode_cost(ctx, operation)?)
                        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
                }
                let references = (entities, parameter, operands).decode_cost(ctx, operation)?;
                prefix
                    .checked_add(properties)
                    .and_then(|bytes| bytes.checked_add(references))
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
            }
        }
    }
}
