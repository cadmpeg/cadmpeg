// SPDX-License-Identifier: Apache-2.0
//! Sweep, coil and helix constructions.

use crate::math::{Point3, Vector3};
use crate::scalar::{
    Angle, Fraction, Length, NonNegativeLength, NonZeroLength, PositiveLength, PositiveReal,
    SlopeAngle,
};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;

use super::geometry::{FeatureUnitPlaneFrame, FinitePoint3};
use super::operation_settings::BooleanOp;
use super::profiles::{PathRef, PlanarProfileRef};
use super::selections::{FaceSelection, SelectionReference};

use crate::units::UnitVector3;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_allow_multi_profile_faces,
    bool,
    "allow_multi_profile_faces"
);

cadmpeg_core::named_optional_field!(deserialize_section_native, String, "value");

cadmpeg_core::named_optional_field!(deserialize_tolerance, PositiveReal, "tolerance");

cadmpeg_core::named_optional_field!(deserialize_wall_thickness, PositiveLength, "wall_thickness");

/// Native algorithm used to construct faces from wires.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FaceMaker {
    /// Builds each wire independently.
    Simple,
    /// Builds one face with non-nested holes.
    Cheese,
    /// Builds the extrusion-compatible face form.
    Extrusion,
    /// Builds planar faces with nested islands.
    Bullseye,
    /// Builds overlapping, nested, or non-planar faces.
    Unified,
    /// Retains an extension face-maker class.
    Other(NonBlankString),
}

impl FaceMaker {
    /// Parses a non-empty runtime class name.
    pub fn new(class: impl Into<String>) -> Option<Self> {
        let class = class.into();
        Some(match class.as_str() {
            "Part::FaceMakerSimple" => Self::Simple,
            "Part::FaceMakerCheese" => Self::Cheese,
            "Part::FaceMakerExtrusion" => Self::Extrusion,
            "Part::FaceMakerBullseye" => Self::Bullseye,
            "Part::FaceMakerUnified" => Self::Unified,
            _ => Self::Other(NonBlankString::new(class)?),
        })
    }

    /// Returns the `FreeCAD` runtime class name.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Simple => "Part::FaceMakerSimple",
            Self::Cheese => "Part::FaceMakerCheese",
            Self::Extrusion => "Part::FaceMakerExtrusion",
            Self::Bullseye => "Part::FaceMakerBullseye",
            Self::Unified => "Part::FaceMakerUnified",
            Self::Other(class) => class.as_str(),
        }
    }

    /// Returns the persisted `FreeCAD` extrusion enumeration value.
    pub const fn mode(&self) -> u32 {
        match self {
            Self::Simple => 0,
            Self::Cheese => 1,
            Self::Extrusion => 2,
            Self::Bullseye => 3,
            Self::Unified | Self::Other(_) => 4,
        }
    }
}

impl Serialize for FaceMaker {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FaceMaker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("face maker class must not be empty"))
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for FaceMaker {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FaceMaker".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }
}

/// Relationship between outer-wire and inner-wire taper directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum InnerWireTaper {
    /// Inner wires taper opposite to outer wires.
    Inverted,
    /// Inner wires taper in the same direction as outer wires.
    SameAsOuter,
}

/// Persisted construction algorithm used for a parametric helix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HelixConstructionStyle {
    /// Historical construction retained for document compatibility.
    Legacy,
    /// Corrected construction used by newly created features.
    Corrected,
}

/// Axial or radial construction law of a helix feature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelixShape {
    /// Constant-radius helix with signed axial rise per revolution.
    Cylindrical {
        /// Signed axial rise per revolution.
        pitch: NonZeroLength,
    },
    /// Conical helix with signed axial rise and cone half-angle.
    Conical {
        /// Signed axial rise per revolution.
        pitch: NonZeroLength,
        /// Cone half-angle.
        cone_angle: SlopeAngle,
    },
    /// Planar spiral with signed radial growth per revolution.
    Spiral {
        /// Signed radial growth per revolution.
        radial_growth: Length,
    },
}

impl TryFrom<BooleanOp> for SolidSweepOperation {
    type Error = BooleanOp;

    fn try_from(value: BooleanOp) -> Result<Self, Self::Error> {
        match value {
            BooleanOp::NewBody => Ok(Self::NewBody),
            BooleanOp::Join => Ok(Self::Join),
            BooleanOp::Cut => Ok(Self::Cut),
            BooleanOp::Intersect => Ok(Self::Intersect),
            BooleanOp::Unresolved => Err(value),
        }
    }
}

/// Placement and parameterization of a solid Coil primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CoilConstruction {
    /// Axis frame and angular origin.
    pub placement: CoilPlacement,
    /// Diameter of the reference trajectory at its start.
    pub diameter: PositiveLength,
    /// Independent driving dimensions retained from the source feature.
    pub extent: CoilExtent,
    /// Generated section swept along the trajectory.
    pub section: CoilSection,
    /// Radial position of the section relative to the reference trajectory.
    pub section_placement: CoilSectionPlacement,
    /// Angular travel direction when viewed from the axis origin along the positive axis.
    pub clockwise: bool,
    /// Signed cone half-angle of an axial coil; zero produces a cylindrical helix.
    pub taper: Angle,
}

/// Geometric placement of a Coil trajectory.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CoilPlacementWire", into = "CoilPlacementWire")]
pub enum CoilPlacement {
    /// Complete model-space axis and radial frame.
    Explicit {
        /// Origin, positive trajectory axis, and angular-zero radial direction.
        frame: FeatureUnitPlaneFrame,
    },
    /// Placement retained in one source-native construction aggregate.
    Native {
        /// Nonblank native record or scope containing the placement semantics.
        native_ref: SelectionReference,
    },
}

#[derive(Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
enum CoilPlacementWire {
    /// Complete model-space axis and radial frame.
    Explicit {
        /// Model-space origin of the trajectory frame.
        origin: Point3,
        /// Positive trajectory axis direction.
        axis: Vector3,
        /// Angular-zero radial direction.
        radial: Vector3,
    },
    /// Placement retained in one source-native construction aggregate.
    Native {
        /// Nonblank native record or scope containing the placement semantics.
        native_ref: SelectionReference,
    },
}

impl TryFrom<CoilPlacementWire> for CoilPlacement {
    type Error = &'static str;
    fn try_from(wire: CoilPlacementWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            CoilPlacementWire::Explicit {
                origin,
                axis,
                radial,
            } => Self::Explicit {
                frame: FeatureUnitPlaneFrame::new(origin, axis, radial).ok_or(
                    "coil placement requires finite origin and perpendicular unit directions",
                )?,
            },
            CoilPlacementWire::Native { native_ref } => Self::Native { native_ref },
        })
    }
}

/// Independent driving dimensions of a Coil trajectory.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CoilExtent {
    /// Axial coil driven by revolution count and total signed height.
    RevolutionsHeight {
        /// Positive angular-turn count.
        revolutions: PositiveReal,
        /// Signed axial travel.
        height: Length,
    },
    /// Axial coil driven by revolution count and signed pitch per revolution.
    RevolutionsPitch {
        /// Positive angular-turn count.
        revolutions: PositiveReal,
        /// Signed axial travel per revolution.
        pitch: NonZeroLength,
    },
    /// Axial coil driven by total signed height and signed pitch per revolution.
    HeightPitch {
        /// Signed axial travel.
        height: NonZeroLength,
        /// Signed axial travel per revolution.
        pitch: NonZeroLength,
    },
    /// Planar spiral driven by revolution count and signed radial pitch.
    Spiral {
        /// Positive angular-turn count.
        revolutions: PositiveReal,
        /// Signed radial growth per revolution.
        radial_pitch: NonZeroLength,
    },
}

/// Generated cross-section of a Coil primitive.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CoilSection {
    /// Circular section whose size is its diameter.
    Circular {
        /// Circle diameter.
        diameter: PositiveLength,
    },
    /// Square section whose size is its edge length.
    Square {
        /// Edge length.
        size: PositiveLength,
    },
    /// Equilateral triangle pointing radially away from the axis.
    ExternalTriangle {
        /// Edge length.
        size: PositiveLength,
    },
    /// Equilateral triangle pointing radially toward the axis.
    InternalTriangle {
        /// Edge length.
        size: PositiveLength,
    },
}

/// Radial placement of a generated Coil section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CoilSectionPlacement {
    /// Section lies inside the reference trajectory.
    Inside,
    /// Section centroid lies on the reference trajectory.
    Center,
    /// Section lies outside the reference trajectory.
    Outside,
}

/// Result semantics of a swept profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SweepMode {
    /// Native sweep family is known but its result subtype is unresolved.
    Unresolved {},
    /// Sweep creates or modifies a solid body.
    Solid {
        /// Independent-body creation or boolean combination with existing bodies.
        op: SolidSweepOperation,
    },
    /// Sweep creates a sheet body.
    Surface {},
}

/// How a solid sweep result meets the existing bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SolidSweepOperation {
    /// Create an independent body.
    NewBody,
    /// Add the swept volume to existing bodies.
    Join,
    /// Remove the swept volume from existing bodies.
    Cut,
    /// Keep the intersection with existing bodies.
    Intersect,
}

/// Directed fractions of a sweep path consumed from the profile location.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SweepPathExtent {
    /// Fraction consumed in the path's forward traversal direction.
    pub along_fraction: Fraction,
    /// Fraction consumed in the path's reverse traversal direction.
    pub against_fraction: Fraction,
}

/// Guide rail controlling a sweep, with its directed consumed extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SweepGuideRail {
    /// Ordered guide trajectory.
    pub path: PathRef,
    /// Fractions consumed on either side of the profile location.
    pub extent: SweepPathExtent,
}

/// The generated cross-section of a result that generates none.
///
/// A sheet sweep owns no geometry of its own, so this type has no value and
/// [`SweepSection::Generated`] is unreachable for a sheet section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum NoGeneratedSection {}

/// A cross-section of a result that generates no geometry of its own.
pub type SheetSweepSection = SweepSection<NoGeneratedSection>;

/// Cross-section owned or referenced by a sweep construction.
///
/// `G` names the geometry the result may generate. A solid result generates a
/// circular region; a sheet result generates nothing, and its `G` is
/// uninhabited.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SweepSection<G = GeneratedSweepSection> {
    /// The source requires a cross-section, but its carrier is unresolved.
    Unresolved(#[serde(deserialize_with = "deserialize_section_native")] Option<String>),
    /// Cross-section supplied by referenced profile geometry.
    Profile(PlanarProfileRef),
    /// Cross-section generated by the sweep construction itself.
    Generated(G),
}

impl<G> SweepSection<G> {
    /// Returns the referenced profile when this section does not own its geometry.
    pub const fn referenced_profile(&self) -> Option<&PlanarProfileRef> {
        match self {
            Self::Profile(profile) => Some(profile),
            Self::Unresolved(_) | Self::Generated(_) => None,
        }
    }

    /// Returns the mutable referenced profile when this section does not own its geometry.
    pub fn referenced_profile_mut(&mut self) -> Option<&mut PlanarProfileRef> {
        match self {
            Self::Profile(profile) => Some(profile),
            Self::Unresolved(_) | Self::Generated(_) => None,
        }
    }

    /// Whether the source requires a cross-section it did not resolve.
    pub const fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved(_))
    }

    /// The native carrier named by an unresolved cross-section.
    pub fn unresolved_native(&self) -> Option<&str> {
        match self {
            Self::Unresolved(native) => native.as_deref(),
            Self::Profile(_) | Self::Generated(_) => None,
        }
    }
}

/// A sweep circular section with an optional wall thinner than its radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SweepCircularRegion {
    outer_radius: PositiveLength,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wall_thickness: Option<PositiveLength>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepCircularRegionWire {
    outer_radius: PositiveLength,
    #[serde(default, deserialize_with = "deserialize_wall_thickness")]
    wall_thickness: Option<PositiveLength>,
}

impl SweepCircularRegion {
    /// Admit a disk or a wall thinner than the outer radius.
    pub fn new(
        outer_radius: PositiveLength,
        wall_thickness: Option<PositiveLength>,
    ) -> Result<Self, &'static str> {
        if wall_thickness.is_some_and(|wall| wall.get() >= outer_radius.get()) {
            return Err("wall_thickness must be less than outer_radius");
        }
        Ok(Self {
            outer_radius,
            wall_thickness,
        })
    }

    /// Return the outer radius.
    pub const fn outer_radius(self) -> PositiveLength {
        self.outer_radius
    }

    /// Return the inward radial wall thickness.
    pub const fn wall_thickness(self) -> Option<PositiveLength> {
        self.wall_thickness
    }
}

impl<'de> Deserialize<'de> for SweepCircularRegion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SweepCircularRegionWire::deserialize(deserializer)?;
        Self::new(wire.outer_radius, wire.wall_thickness).map_err(serde::de::Error::custom)
    }
}

impl From<SheetSweepSection> for SweepSection {
    fn from(section: SheetSweepSection) -> Self {
        match section {
            SweepSection::Unresolved(native) => Self::Unresolved(native),
            SweepSection::Profile(profile) => Self::Profile(profile),
            SweepSection::Generated(generated) => match generated {},
        }
    }
}

/// Sweep cross-sections nested under the result mode they belong to.
///
/// A sheet result generates no geometry, so its sections carry an uninhabited
/// generated arm and a generated circular region under a sheet result has no
/// spelling.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SweepShape {
    /// Native sweep family is known but its result subtype is unresolved.
    Unresolved {
        /// Primary cross-section.
        section: SheetSweepSection,
        /// Additional cross-sections in path order.
        sections: Vec<SheetSweepSection>,
    },
    /// Sweep creates or modifies a solid body.
    Solid {
        /// Independent-body creation or boolean combination with existing bodies.
        op: SolidSweepOperation,
        /// Primary cross-section.
        section: SweepSection,
        /// Additional cross-sections in path order.
        sections: Vec<SweepSection>,
    },
    /// Sweep creates a sheet body.
    Surface {
        /// Primary cross-section.
        section: SheetSweepSection,
        /// Additional cross-sections in path order.
        sections: Vec<SheetSweepSection>,
    },
}

impl SweepShape {
    /// Construct an unresolved sweep section and result mode.
    pub fn unresolved(native: Option<String>) -> Self {
        Self::Unresolved {
            section: SweepSection::Unresolved(native),
            sections: Vec::new(),
        }
    }

    /// A sweep under `mode` whose cross-sections generate no geometry.
    ///
    /// Referenced and unresolved cross-sections are admissible under every
    /// result, so a caller that generated nothing may name any mode.
    pub fn sheet_sections(
        mode: SweepMode,
        section: SheetSweepSection,
        sections: Vec<SheetSweepSection>,
    ) -> Self {
        match mode {
            SweepMode::Unresolved {} => Self::Unresolved { section, sections },
            SweepMode::Surface {} => Self::Surface { section, sections },
            SweepMode::Solid { op } => Self::Solid {
                op,
                section: section.into(),
                sections: sections.into_iter().map(Into::into).collect(),
            },
        }
    }

    /// Return the result mode.
    pub const fn mode(&self) -> SweepMode {
        match self {
            Self::Unresolved { .. } => SweepMode::Unresolved {},
            Self::Solid { op, .. } => SweepMode::Solid { op: *op },
            Self::Surface { .. } => SweepMode::Surface {},
        }
    }

    /// Whether the primary cross-section is unresolved.
    pub const fn section_is_unresolved(&self) -> bool {
        match self {
            Self::Unresolved { section, .. } | Self::Surface { section, .. } => {
                section.is_unresolved()
            }
            Self::Solid { section, .. } => section.is_unresolved(),
        }
    }

    /// Whether any cross-section is unresolved.
    pub fn any_section_is_unresolved(&self) -> bool {
        match self {
            Self::Unresolved { section, sections } | Self::Surface { section, sections } => {
                section.is_unresolved() || sections.iter().any(SweepSection::is_unresolved)
            }
            Self::Solid {
                section, sections, ..
            } => section.is_unresolved() || sections.iter().any(SweepSection::is_unresolved),
        }
    }

    /// Whether any cross-section is unresolved and names a native carrier.
    pub fn any_section_names_an_unresolved_carrier(&self) -> bool {
        match self {
            Self::Unresolved { section, sections } | Self::Surface { section, sections } => {
                std::iter::once(section)
                    .chain(sections)
                    .any(|section| section.unresolved_native().is_some())
            }
            Self::Solid {
                section, sections, ..
            } => std::iter::once(section)
                .chain(sections)
                .any(|section| section.unresolved_native().is_some()),
        }
    }

    /// Number of additional cross-sections beyond the primary one.
    pub fn additional_section_count(&self) -> usize {
        match self {
            Self::Unresolved { sections, .. } | Self::Surface { sections, .. } => sections.len(),
            Self::Solid { sections, .. } => sections.len(),
        }
    }

    /// Referenced profile of the primary cross-section, when it references one.
    pub const fn referenced_profile(&self) -> Option<&PlanarProfileRef> {
        match self {
            Self::Unresolved { section, .. } | Self::Surface { section, .. } => {
                section.referenced_profile()
            }
            Self::Solid { section, .. } => section.referenced_profile(),
        }
    }

    /// Mutable referenced profile of the primary cross-section.
    pub fn referenced_profile_mut(&mut self) -> Option<&mut PlanarProfileRef> {
        match self {
            Self::Unresolved { section, .. } | Self::Surface { section, .. } => {
                section.referenced_profile_mut()
            }
            Self::Solid { section, .. } => section.referenced_profile_mut(),
        }
    }

    /// Every referenced profile, the primary cross-section first.
    pub fn referenced_profiles(&self) -> impl Iterator<Item = &PlanarProfileRef> + Clone {
        let (primary, sheet_sections, solid_sections): (
            Option<&PlanarProfileRef>,
            &[SheetSweepSection],
            &[SweepSection],
        ) = match self {
            Self::Unresolved { section, sections } | Self::Surface { section, sections } => {
                (section.referenced_profile(), sections, &[])
            }
            Self::Solid {
                section, sections, ..
            } => (section.referenced_profile(), &[], sections),
        };
        primary
            .into_iter()
            .chain(
                sheet_sections
                    .iter()
                    .filter_map(SweepSection::referenced_profile),
            )
            .chain(
                solid_sections
                    .iter()
                    .filter_map(SweepSection::referenced_profile),
            )
    }

    /// Generated cross-sections, mutable, the primary cross-section first.
    ///
    /// Only a solid result generates geometry.
    pub fn generated_sections_mut(&mut self) -> impl Iterator<Item = &mut GeneratedSweepSection> {
        let (section, sections) = match self {
            Self::Unresolved { .. } | Self::Surface { .. } => (None, &mut [][..]),
            Self::Solid {
                section, sections, ..
            } => (Some(section), sections.as_mut_slice()),
        };
        section
            .into_iter()
            .chain(sections.iter_mut())
            .filter_map(|section| match section {
                SweepSection::Generated(generated) => Some(generated),
                SweepSection::Unresolved(_) | SweepSection::Profile(_) => None,
            })
    }

    /// The generated region of the primary cross-section, when the sweep owns it.
    pub const fn generated_section(&self) -> Option<&GeneratedSweepSection> {
        match self {
            Self::Unresolved { .. } | Self::Surface { .. } => None,
            Self::Solid { section, .. } => match section {
                SweepSection::Generated(generated) => Some(generated),
                SweepSection::Unresolved(_) | SweepSection::Profile(_) => None,
            },
        }
    }

    /// A solid sweep under `op` with the given cross-sections.
    #[must_use]
    pub fn solid_sections(
        op: SolidSweepOperation,
        section: SweepSection,
        sections: Vec<SweepSection>,
    ) -> Self {
        Self::Solid {
            op,
            section,
            sections,
        }
    }

    /// Take the cross-sections out of the shape, in path order.
    #[must_use]
    pub fn into_sections(self) -> (SweepSection, Vec<SweepSection>) {
        match self {
            Self::Unresolved { section, sections } | Self::Surface { section, sections } => (
                section.into(),
                sections.into_iter().map(Into::into).collect(),
            ),
            Self::Solid {
                section, sections, ..
            } => (section, sections),
        }
    }

    /// Replaces the primary cross-section with referenced profile geometry.
    pub fn set_referenced_profile(&mut self, profile: PlanarProfileRef) {
        match self {
            Self::Unresolved { section, .. } | Self::Surface { section, .. } => {
                *section = SweepSection::Profile(profile);
            }
            Self::Solid { section, .. } => *section = SweepSection::Profile(profile),
        }
    }
}

/// Cross-section geometry generated and owned by a sweep construction.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum GeneratedSweepSection {
    /// Filled or hollow circular region centered on the sweep path.
    CircularRegion {
        /// Outer radius and optional smaller radial wall thickness.
        region: SweepCircularRegion,
    },
}

/// Cross-section orientation law along a sweep path.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SweepOrientation {
    /// Rotation-minimizing corrected-Frenet frame.
    CorrectedFrenet {},
    /// Fixed section frame.
    Fixed {},
    /// Exact Frenet frame from path derivatives.
    Frenet {},
    /// Frame constrained by a secondary path.
    Auxiliary {
        /// Secondary orientation path.
        path: PathRef,
        /// Whether tangent-connected edges extend the secondary path.
        tangent: bool,
        /// Whether corresponding points use curvilinear rather than parameter distance.
        curvilinear: bool,
    },
    /// Frame constrained to remain normal to selected guide faces.
    GuideSurface {
        /// Ordered guide faces controlling the section frame.
        faces: FaceSelection,
    },
    /// Frame constrained by a fixed binormal direction.
    Binormal {
        /// Unit binormal direction.
        direction: UnitVector3,
    },
}

/// Corner continuation used by a sweep path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SweepTransition {
    /// Transform the section continuously across the corner.
    Transformed,
    /// Form a sharp right-corner intersection.
    RightCorner,
    /// Insert a rounded corner transition.
    RoundCorner,
}

/// Cross-section interpolation law for a multi-section sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum SweepTransformation {
    /// Keep one constant section along the path.
    Constant,
    /// Interpolate through explicit ordered sections.
    MultiSection,
    /// Apply linear section interpolation.
    Linear,
    /// Apply an S-shaped interpolation law.
    SShape,
    /// Apply the native smooth interpolation law.
    Interpolation,
}

/// Signed axial travel and radial growth, named by which components move.
///
/// Each arm carries only the components that move, and each is a
/// [`NonZeroLength`], which refuses zero and every non-finite value. A helix
/// that neither rises nor grows has no spelling.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HelicalSweepTravel {
    /// The helix rises along its axis at a constant radius.
    Axial {
        /// Total signed axial travel.
        height: NonZeroLength,
    },
    /// The helix grows radially in its base plane.
    Radial {
        /// Signed radial change per turn.
        radial_growth: NonZeroLength,
    },
    /// The helix rises and grows.
    Conical {
        /// Total signed axial travel.
        height: NonZeroLength,
        /// Signed radial change per turn.
        radial_growth: NonZeroLength,
    },
}

impl HelicalSweepTravel {
    /// Admit finite signed travel with nonzero height or radial growth.
    pub fn new(height: Length, radial_growth: Length) -> Option<Self> {
        match (
            NonZeroLength::try_from(height).ok(),
            NonZeroLength::try_from(radial_growth).ok(),
        ) {
            (None, None) => None,
            (Some(height), None) => Some(Self::Axial { height }),
            (None, Some(radial_growth)) => Some(Self::Radial { radial_growth }),
            (Some(height), Some(radial_growth)) => Some(Self::Conical {
                height,
                radial_growth,
            }),
        }
    }

    /// Return the total axial travel.
    pub fn height(self) -> Length {
        match self {
            Self::Radial { .. } => Length::ZERO,
            Self::Axial { height } | Self::Conical { height, .. } => height.into(),
        }
    }

    /// Return the radial change per turn.
    pub fn radial_growth(self) -> Length {
        match self {
            Self::Axial { .. } => Length::ZERO,
            Self::Radial { radial_growth } | Self::Conical { radial_growth, .. } => {
                radial_growth.into()
            }
        }
    }
}

/// Complete construction of a solid helical sweep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HelicalSweepConstruction {
    /// Profile swept along the helical path.
    pub profile: PlanarProfileRef,
    /// Point at the start of the helix axis.
    pub axis_origin: FinitePoint3,
    /// Unit direction of positive axial travel.
    pub axis_direction: UnitVector3,
    /// Persisted authoring law identifying the independent parameters.
    pub law: HelicalSweepLaw,
    /// Nonnegative axial advance per turn; zero gives a planar spiral.
    pub pitch: NonNegativeLength,
    /// Signed axial travel and radial change per turn.
    pub travel: HelicalSweepTravel,
    /// Positive number of turns.
    pub turns: PositiveReal,
    /// Cone half-angle corresponding to radial growth.
    pub cone_angle: Angle,
    /// Whether angular travel is left-handed along the positive axis.
    pub left_handed: bool,
    /// Whether path travel runs opposite the declared axis direction.
    pub reversed: bool,
    /// Relative tolerance used while joining the generated sweep, when persisted.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tolerance"
    )]
    pub tolerance: Option<PositiveReal>,
    /// Whether a profile containing multiple faces is accepted as one operation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_allow_multi_profile_faces"
    )]
    pub allow_multi_profile_faces: Option<bool>,
}

/// Independent-parameter law used to author a helical sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HelicalSweepLaw {
    /// Pitch, height, and cone angle are independent.
    PitchHeightAngle,
    /// Pitch, turn count, and cone angle are independent.
    PitchTurnsAngle,
    /// Height, turn count, and cone angle are independent.
    HeightTurnsAngle,
    /// Height, turn count, and radial growth are independent.
    HeightTurnsGrowth,
}
