// SPDX-License-Identifier: Apache-2.0
//! Hole construction, placement, and compatible dimensions.

use super::{FeatureDirection3, FinitePoint3};
use crate::scalar::{InteriorAngle, Length, PositiveLength};
use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One complete spatial placement in a hole operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HolePlacement {
    /// Position and directed drilling vector recorded by the feature definition.
    Directed {
        /// Hole entry position in model space.
        position: FinitePoint3,
        /// Directed drilling vector.
        direction: FeatureDirection3,
    },
    /// Unoriented geometric axis inferred from a generated cylindrical surface.
    Axis {
        /// Point on the cylinder axis in model space.
        origin: FinitePoint3,
        /// Unoriented cylinder-axis vector; its sign has no semantic meaning.
        axis: FeatureDirection3,
    },
}

/// A counterdrill recess diameter and optional larger entry diameter.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CounterdrillDiameters {
    diameter: PositiveLength,
    entry_diameter: Option<PositiveLength>,
}

impl CounterdrillDiameters {
    /// Admit a recess diameter and an optional larger entry diameter.
    pub fn new(
        diameter: PositiveLength,
        entry_diameter: Option<PositiveLength>,
    ) -> Result<Self, &'static str> {
        if entry_diameter.is_some_and(|entry| entry.get() <= diameter.get()) {
            return Err("entry_diameter must exceed diameter");
        }
        Ok(Self {
            diameter,
            entry_diameter,
        })
    }

    /// Return the cylindrical recess diameter.
    pub const fn diameter(self) -> PositiveLength {
        self.diameter
    }

    /// Return the optional diameter before the conical transition.
    pub const fn entry_diameter(self) -> Option<PositiveLength> {
        self.entry_diameter
    }
}

/// A hole bore and its compatible entry, exit, and thread dimensions.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct HoleShape {
    construction: HoleConstruction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<HoleKindWire>"))]
    exit_kind: Option<HoleKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diameter: Option<PositiveLength>,
}

/// A failed edit of the length-bearing dimensions of an admitted hole.
#[derive(Debug)]
pub enum HoleLengthEditError<E> {
    /// A changed length was refused by the caller's map.
    Field(E),
    /// A scaled counterdrill entry no longer exceeds its recess diameter.
    Counterdrill(&'static str),
    /// A scaled treatment diameter no longer exceeds the bore diameter.
    Treatment(&'static str),
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HoleShapeWire {
    construction: HoleConstruction,
    #[serde(default, deserialize_with = "deserialize_exit_kind")]
    exit_kind: Option<HoleKind>,
    #[serde(default, deserialize_with = "deserialize_diameter")]
    diameter: Option<PositiveLength>,
}

impl HoleShape {
    /// Admit treatments whose diameters exceed the bore diameter.
    pub fn new(
        construction: HoleConstruction,
        exit_kind: Option<HoleKind>,
        diameter: Option<PositiveLength>,
    ) -> Result<Self, &'static str> {
        let valid_kind = |kind: &HoleKind| match diameter {
            Some(bore) => treatment_exceeds_bore(kind, bore),
            None => matches!(
                kind,
                HoleKind::Unresolved(_)
                    | HoleKind::PartialCounterbore(..)
                    | HoleKind::PartialCountersink(..)
                    | HoleKind::Simple
                    | HoleKind::SimpleDrilled { .. }
            ),
        };
        let valid = match &construction {
            HoleConstruction::Form { kind, .. } => valid_kind(kind),
            HoleConstruction::NativeThread { major_diameter, .. } => {
                diameter.is_some_and(|bore| major_diameter.get() > bore.get())
            }
        };
        if !valid || exit_kind.as_ref().is_some_and(|kind| !valid_kind(kind)) {
            return Err(
                "construction and exit_kind treatment diameters must exceed the bore diameter",
            );
        }
        Ok(Self {
            construction,
            exit_kind,
            diameter,
        })
    }

    /// Return the hole construction.
    pub fn construction(&self) -> &HoleConstruction {
        &self.construction
    }

    /// Return the exit treatment.
    pub fn exit_kind(&self) -> &Option<HoleKind> {
        &self.exit_kind
    }

    /// Return the bore diameter.
    pub const fn diameter(&self) -> Option<PositiveLength> {
        self.diameter
    }

    /// Map length fields without changing the hole's bore presence, treatment
    /// kinds, or non-length fields. Recheck strict diameter relations, which
    /// can collapse when two positive dimensions are rounded after scaling.
    pub fn try_map_lengths<E>(
        &self,
        map_positive: &mut impl FnMut(PositiveLength) -> Result<PositiveLength, E>,
        map_length: &mut impl FnMut(Length) -> Result<Length, E>,
    ) -> Result<Self, HoleLengthEditError<E>> {
        let mut mapped = self.clone();
        map_hole_construction_lengths(&mut mapped.construction, map_positive, map_length)?;
        if let Some(kind) = &mut mapped.exit_kind {
            map_hole_kind_lengths(kind, map_positive)?;
        }
        if let Some(diameter) = &mut mapped.diameter {
            *diameter = map_positive(*diameter).map_err(HoleLengthEditError::Field)?;
        }
        if let Some(bore) = mapped.diameter {
            let valid = match &mapped.construction {
                HoleConstruction::Form { kind, .. } => treatment_exceeds_bore(kind, bore),
                HoleConstruction::NativeThread { major_diameter, .. } => {
                    major_diameter.get() > bore.get()
                }
            };
            if !valid
                || mapped
                    .exit_kind
                    .as_ref()
                    .is_some_and(|kind| !treatment_exceeds_bore(kind, bore))
            {
                return Err(HoleLengthEditError::Treatment(
                    "construction and exit_kind treatment diameters must exceed the bore diameter",
                ));
            }
        }
        Ok(mapped)
    }

    /// Admit edited construction and dimensions before replacing the hole shape.
    pub fn try_edit(
        &mut self,
        edit: impl FnOnce(&mut HoleConstruction, &mut Option<HoleKind>, &mut Option<PositiveLength>),
    ) -> Result<(), &'static str> {
        let mut construction = self.construction.clone();
        let mut exit_kind = self.exit_kind;
        let mut diameter = self.diameter;
        edit(&mut construction, &mut exit_kind, &mut diameter);
        *self = Self::new(construction, exit_kind, diameter)?;
        Ok(())
    }
}

fn treatment_exceeds_bore(kind: &HoleKind, bore: PositiveLength) -> bool {
    match kind {
        HoleKind::Unresolved(_)
        | HoleKind::PartialCounterbore(..)
        | HoleKind::PartialCountersink(..)
        | HoleKind::Simple
        | HoleKind::SimpleDrilled { .. } => true,
        HoleKind::Chamfer { diameter, .. }
        | HoleKind::Counterbore { diameter, .. }
        | HoleKind::CounterboreDrilled { diameter, .. }
        | HoleKind::Countersink { diameter, .. } => diameter.get() > bore.get(),
        HoleKind::Counterdrill { diameters, .. } => diameters.diameter().get() > bore.get(),
    }
}

fn map_hole_kind_lengths<E>(
    kind: &mut HoleKind,
    map_positive: &mut impl FnMut(PositiveLength) -> Result<PositiveLength, E>,
) -> Result<(), HoleLengthEditError<E>> {
    let mut edit = |length: &mut PositiveLength| {
        *length = map_positive(*length).map_err(HoleLengthEditError::Field)?;
        Ok(())
    };
    match kind {
        HoleKind::Unresolved(_) | HoleKind::Simple | HoleKind::SimpleDrilled { .. } => {}
        HoleKind::PartialCounterbore(pair) => {
            if let Some(diameter) = pair.first_mut() {
                edit(diameter)?;
            }
            if let Some(depth) = pair.second_mut() {
                edit(depth)?;
            }
        }
        HoleKind::PartialCountersink(pair) => {
            if let Some(diameter) = pair.first_mut() {
                edit(diameter)?;
            }
        }
        HoleKind::Chamfer { diameter, .. } | HoleKind::Countersink { diameter, .. } => {
            edit(diameter)?;
        }
        HoleKind::Counterbore { diameter, depth }
        | HoleKind::CounterboreDrilled {
            diameter, depth, ..
        } => {
            edit(diameter)?;
            edit(depth)?;
        }
        HoleKind::Counterdrill {
            diameters, depth, ..
        } => {
            let mut diameter = diameters.diameter();
            let mut entry_diameter = diameters.entry_diameter();
            edit(&mut diameter)?;
            if let Some(entry) = &mut entry_diameter {
                edit(entry)?;
            }
            *diameters = CounterdrillDiameters::new(diameter, entry_diameter)
                .map_err(HoleLengthEditError::Counterdrill)?;
            edit(depth)?;
        }
    }
    Ok(())
}

impl HoleKind {
    /// Map this treatment's length fields while retaining its form and angles.
    /// Strict counterdrill diameter ordering is checked after the map.
    pub fn try_map_lengths<E>(
        &self,
        map_positive: &mut impl FnMut(PositiveLength) -> Result<PositiveLength, E>,
    ) -> Result<Self, HoleLengthEditError<E>> {
        let mut mapped = *self;
        map_hole_kind_lengths(&mut mapped, map_positive)?;
        Ok(mapped)
    }
}

fn map_hole_construction_lengths<E>(
    construction: &mut HoleConstruction,
    map_positive: &mut impl FnMut(PositiveLength) -> Result<PositiveLength, E>,
    map_length: &mut impl FnMut(Length) -> Result<Length, E>,
) -> Result<(), HoleLengthEditError<E>> {
    match construction {
        HoleConstruction::Form {
            kind,
            specification,
        } => {
            map_hole_kind_lengths(kind, map_positive)?;
            if let Some(specification) = specification {
                map_hole_specification_lengths(specification, map_positive, map_length)?;
            }
        }
        HoleConstruction::NativeThread {
            major_diameter,
            thread_depth,
            pitch,
            ..
        } => {
            *major_diameter = map_positive(*major_diameter).map_err(HoleLengthEditError::Field)?;
            *thread_depth = map_positive(*thread_depth).map_err(HoleLengthEditError::Field)?;
            if let Some(pitch) = pitch {
                *pitch = map_positive(*pitch).map_err(HoleLengthEditError::Field)?;
            }
        }
    }
    Ok(())
}

fn map_hole_specification_lengths<E>(
    specification: &mut HoleSpecification,
    map_positive: &mut impl FnMut(PositiveLength) -> Result<PositiveLength, E>,
    map_length: &mut impl FnMut(Length) -> Result<Length, E>,
) -> Result<(), HoleLengthEditError<E>> {
    let (pitch, major_diameter, clearance, depth) = match specification {
        HoleSpecification::Clearance {
            clearance, depth, ..
        } => (None, None, clearance, depth),
        HoleSpecification::Threaded {
            pitch,
            major_diameter,
            clearance,
            depth,
            ..
        } => (Some(pitch), Some(major_diameter), clearance, depth),
    };
    if let Some(Some(pitch)) = pitch {
        *pitch = map_positive(*pitch).map_err(HoleLengthEditError::Field)?;
    }
    if let Some(Some(major_diameter)) = major_diameter {
        *major_diameter = map_positive(*major_diameter).map_err(HoleLengthEditError::Field)?;
    }
    if let Some(clearance) = clearance {
        *clearance = map_length(*clearance).map_err(HoleLengthEditError::Field)?;
    }
    if let HoleThreadDepth::Blind { depth } = depth {
        *depth = map_positive(*depth).map_err(HoleLengthEditError::Field)?;
    }
    Ok(())
}

impl TryFrom<HoleShapeWire> for HoleShape {
    type Error = &'static str;

    fn try_from(wire: HoleShapeWire) -> Result<Self, Self::Error> {
        Self::new(wire.construction, wire.exit_kind, wire.diameter)
    }
}

impl<'de> Deserialize<'de> for HoleShape {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(HoleShapeWire::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Structural drilling, entry-treatment, and threading form of a hole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HoleKindWire", into = "HoleKindWire")]
pub enum HoleKind {
    /// Entry-treatment family whose dimensions remain unresolved.
    Unresolved(Option<HoleForm>),
    /// Independently retained counterbore diameter and depth.
    PartialCounterbore(PartialPair<PositiveLength, PositiveLength>),
    /// Independently retained countersink diameter and included angle.
    PartialCountersink(PartialPair<PositiveLength, InteriorAngle>),
    /// Plain cylindrical hole with no entry feature.
    Simple,
    /// Hole with a chamfered entry.
    Chamfer {
        /// Entry chamfer diameter.
        diameter: PositiveLength,
        /// Included chamfer angle.
        angle: InteriorAngle,
    },
    /// Plain cylindrical hole terminating in a conical drill point.
    SimpleDrilled {
        /// Included angle of the conical drill point.
        drill_point_angle: InteriorAngle,
    },
    /// Hole with a wider, flat-bottomed counterbore at the entry.
    Counterbore {
        /// Counterbore diameter, wider than the hole diameter.
        diameter: PositiveLength,
        /// Counterbore depth.
        depth: PositiveLength,
    },
    /// Counterbored hole terminating in a conical drill point.
    CounterboreDrilled {
        /// Counterbore diameter, wider than the hole diameter.
        diameter: PositiveLength,
        /// Axial depth of the counterbore.
        depth: PositiveLength,
        /// Included angle of the conical drill point.
        drill_point_angle: InteriorAngle,
    },
    /// Hole with a conical countersink at the entry.
    Countersink {
        /// Countersink diameter at the surface, wider than the hole diameter.
        diameter: PositiveLength,
        /// Countersink included angle.
        angle: InteriorAngle,
    },
    /// Hole with a conical entry followed by a wider cylindrical recess.
    Counterdrill {
        /// Recess diameter and optional larger entry diameter.
        diameters: CounterdrillDiameters,
        /// Cylindrical recess depth.
        depth: PositiveLength,
        /// Included conical entry angle.
        angle: InteriorAngle,
    },
}

/// Mutually exclusive ordinary and source-native threaded hole constructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "construction", rename_all = "snake_case", deny_unknown_fields)]
pub enum HoleConstruction {
    /// Entry treatment with optional named standard sizing or thread metadata.
    Form {
        /// Structural entry treatment.
        kind: HoleKind,
        /// Standard sizing and thread construction, when specified.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_specification"
        )]
        specification: Option<Box<HoleSpecification>>,
    },
    /// SLDPRT native thread geometry carried without a named standard specification.
    NativeThread {
        /// Nominal major diameter of the internal thread.
        major_diameter: PositiveLength,
        /// Axial length over which the thread is cut.
        thread_depth: PositiveLength,
        /// Thread pitch, when carried independently of a nominal designation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pitch"
        )]
        pitch: Option<PositiveLength>,
        /// Included angle of the conical drill point.
        drill_point_angle: InteriorAngle,
    },
}

impl HoleConstruction {
    /// Creates an entry treatment without standard sizing metadata.
    #[must_use]
    pub const fn form(kind: HoleKind) -> Self {
        Self::Form {
            kind,
            specification: None,
        }
    }
}

/// A pair of dimensions of which exactly one is present.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum PartialPair<A, B> {
    /// Only the first dimension is present.
    First(A),
    /// Only the second dimension is present.
    Second(B),
}

impl<A, B> PartialPair<A, B> {
    /// Mutable access to the first dimension, when present.
    pub const fn first_mut(&mut self) -> Option<&mut A> {
        match self {
            Self::First(first) => Some(first),
            Self::Second(_) => None,
        }
    }

    /// Mutable access to the second dimension, when present.
    pub const fn second_mut(&mut self) -> Option<&mut B> {
        match self {
            Self::Second(second) => Some(second),
            Self::First(_) => None,
        }
    }
}

impl HoleKind {
    /// Whether the entry treatment still lacks required construction data.
    #[must_use]
    pub const fn is_unresolved(&self) -> bool {
        matches!(
            self,
            Self::Unresolved(_) | Self::PartialCounterbore(..) | Self::PartialCountersink(..)
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PartialCounterboreDimension {
    Diameter { diameter: PositiveLength },
    Depth { depth: PositiveLength },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PartialCountersinkDimension {
    Diameter { diameter: PositiveLength },
    Angle { angle: InteriorAngle },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum HoleKindWire {
    /// Entry-treatment family whose dimensions remain unresolved.
    Unresolved {
        /// Identified entry-treatment family of an unresolved construction.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_form"
        )]
        form: Option<HoleForm>,
    },
    /// Independently retained counterbore diameter and depth.
    PartialCounterbore {
        /// Independently retained counterbore diameter and depth.
        dimension: PartialCounterboreDimension,
    },
    /// Independently retained countersink diameter and included angle.
    PartialCountersink {
        /// Independently retained countersink diameter and included angle.
        dimension: PartialCountersinkDimension,
    },
    /// Plain cylindrical hole with no entry feature.
    Simple {},
    /// Hole with a chamfered entry.
    Chamfer {
        /// Entry chamfer diameter.
        diameter: PositiveLength,
        /// Included chamfer angle.
        angle: InteriorAngle,
    },
    /// Plain cylindrical hole terminating in a conical drill point.
    SimpleDrilled {
        /// Included angle of the conical drill point.
        drill_point_angle: InteriorAngle,
    },
    /// Hole with a wider, flat-bottomed counterbore at the entry.
    Counterbore {
        /// Counterbore diameter, wider than the hole diameter.
        diameter: PositiveLength,
        /// Counterbore depth.
        depth: PositiveLength,
    },
    /// Counterbored hole terminating in a conical drill point.
    CounterboreDrilled {
        /// Counterbore diameter, wider than the hole diameter.
        diameter: PositiveLength,
        /// Axial depth of the counterbore.
        depth: PositiveLength,
        /// Included angle of the conical drill point.
        drill_point_angle: InteriorAngle,
    },
    /// Hole with a conical countersink at the entry.
    Countersink {
        /// Countersink diameter at the surface, wider than the hole diameter.
        diameter: PositiveLength,
        /// Countersink included angle.
        angle: InteriorAngle,
    },
    /// Hole with a conical entry followed by a wider cylindrical recess.
    Counterdrill {
        /// Cylindrical recess diameter.
        diameter: PositiveLength,
        /// Larger entry diameter, when the source states one.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_entry_diameter"
        )]
        entry_diameter: Option<PositiveLength>,
        /// Cylindrical recess depth.
        depth: PositiveLength,
        /// Included conical entry angle.
        angle: InteriorAngle,
    },
}

impl From<HoleKind> for HoleKindWire {
    fn from(value: HoleKind) -> Self {
        match value {
            HoleKind::Unresolved(form) => Self::Unresolved { form },
            HoleKind::PartialCounterbore(pair) => Self::PartialCounterbore {
                dimension: match pair {
                    PartialPair::First(diameter) => {
                        PartialCounterboreDimension::Diameter { diameter }
                    }
                    PartialPair::Second(depth) => PartialCounterboreDimension::Depth { depth },
                },
            },
            HoleKind::PartialCountersink(pair) => Self::PartialCountersink {
                dimension: match pair {
                    PartialPair::First(diameter) => {
                        PartialCountersinkDimension::Diameter { diameter }
                    }
                    PartialPair::Second(angle) => PartialCountersinkDimension::Angle { angle },
                },
            },
            HoleKind::Simple => Self::Simple {},
            HoleKind::Chamfer { diameter, angle } => Self::Chamfer { diameter, angle },
            HoleKind::SimpleDrilled { drill_point_angle } => {
                Self::SimpleDrilled { drill_point_angle }
            }
            HoleKind::Counterbore { diameter, depth } => Self::Counterbore { diameter, depth },
            HoleKind::CounterboreDrilled {
                diameter,
                depth,
                drill_point_angle,
            } => Self::CounterboreDrilled {
                diameter,
                depth,
                drill_point_angle,
            },
            HoleKind::Countersink { diameter, angle } => Self::Countersink { diameter, angle },
            HoleKind::Counterdrill {
                diameters,
                depth,
                angle,
            } => Self::Counterdrill {
                diameter: diameters.diameter(),
                entry_diameter: diameters.entry_diameter(),
                depth,
                angle,
            },
        }
    }
}

impl TryFrom<HoleKindWire> for HoleKind {
    type Error = String;

    fn try_from(value: HoleKindWire) -> Result<Self, Self::Error> {
        Ok(match value {
            HoleKindWire::Unresolved { form } => Self::Unresolved(form),
            HoleKindWire::PartialCounterbore { dimension } => {
                Self::PartialCounterbore(match dimension {
                    PartialCounterboreDimension::Diameter { diameter } => {
                        PartialPair::First(diameter)
                    }
                    PartialCounterboreDimension::Depth { depth } => PartialPair::Second(depth),
                })
            }
            HoleKindWire::PartialCountersink { dimension } => {
                Self::PartialCountersink(match dimension {
                    PartialCountersinkDimension::Diameter { diameter } => {
                        PartialPair::First(diameter)
                    }
                    PartialCountersinkDimension::Angle { angle } => PartialPair::Second(angle),
                })
            }
            HoleKindWire::Simple {} => Self::Simple,
            HoleKindWire::Chamfer { diameter, angle } => Self::Chamfer { diameter, angle },
            HoleKindWire::SimpleDrilled { drill_point_angle } => {
                Self::SimpleDrilled { drill_point_angle }
            }
            HoleKindWire::Counterbore { diameter, depth } => Self::Counterbore { diameter, depth },
            HoleKindWire::CounterboreDrilled {
                diameter,
                depth,
                drill_point_angle,
            } => Self::CounterboreDrilled {
                diameter,
                depth,
                drill_point_angle,
            },
            HoleKindWire::Countersink { diameter, angle } => Self::Countersink { diameter, angle },
            HoleKindWire::Counterdrill {
                diameter,
                entry_diameter,
                depth,
                angle,
            } => Self::Counterdrill {
                diameters: CounterdrillDiameters::new(diameter, entry_diameter)?,
                depth,
                angle,
            },
        })
    }
}

/// Profile geometry families accepted as hole-location generators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HoleProfileFilter {
    /// Points generate holes.
    Points,
    /// Circles generate holes.
    Circles,
    /// Points and circles generate holes.
    PointsAndCircles,
    /// Arcs generate holes.
    Arcs,
    /// Points and arcs generate holes.
    PointsAndArcs,
    /// Circles and arcs generate holes.
    CirclesAndArcs,
    /// All profile families generate holes.
    All,
}

/// Blind-end construction of a drilled hole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(from = "HoleBottomWire", into = "HoleBottomWire")]
#[serde(deny_unknown_fields)]
pub enum HoleBottom {
    /// Flat-bottomed cylindrical end.
    Flat,
    /// Conical drill point.
    Angled {
        /// Included drill-point angle.
        included_angle: InteriorAngle,
        /// Whether the declared blind depth reaches the tip instead of the shoulder.
        depth_to_tip: bool,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum HoleBottomWire {
    /// Flat-bottomed cylindrical end.
    Flat {},
    /// Conical drill point.
    Angled {
        /// Included drill-point angle.
        included_angle: InteriorAngle,
        /// Whether the declared blind depth reaches the tip instead of the shoulder.
        depth_to_tip: bool,
    },
}

impl From<HoleBottom> for HoleBottomWire {
    fn from(value: HoleBottom) -> Self {
        match value {
            HoleBottom::Flat => Self::Flat {},
            HoleBottom::Angled {
                included_angle,
                depth_to_tip,
            } => Self::Angled {
                included_angle,
                depth_to_tip,
            },
        }
    }
}

impl From<HoleBottomWire> for HoleBottom {
    fn from(value: HoleBottomWire) -> Self {
        match value {
            HoleBottomWire::Flat {} => Self::Flat,
            HoleBottomWire::Angled {
                included_angle,
                depth_to_tip,
            } => Self::Angled {
                included_angle,
                depth_to_tip,
            },
        }
    }
}

/// Standard sizing and optional physical-thread construction for a hole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HoleSpecification {
    /// Clearance-hole sizing, which may carry a fit but cannot carry thread data.
    Clearance {
        /// Named fastener standard family.
        #[serde(deserialize_with = "deserialize_local_standard")]
        standard: NonBlankString,
        /// Nominal size designation within the standard.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_designation"
        )]
        designation: Option<String>,
        /// Clearance-hole fit class.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fit"
        )]
        fit: Option<String>,
        /// Whether exact standard geometry is modeled.
        modeled: bool,
        /// Whether cosmetic presentation is requested.
        cosmetic: bool,
        /// Standard direction value retained from the source record.
        hand: ThreadHand,
        /// Standard depth rule retained from the source record.
        depth: HoleThreadDepth,
        /// Additional radial clearance used for modeled geometry.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_clearance"
        )]
        clearance: Option<Length>,
    },
    /// Internally threaded-hole sizing and thread geometry.
    Threaded {
        /// Named thread standard family.
        #[serde(deserialize_with = "deserialize_local_standard")]
        standard: NonBlankString,
        /// Nominal size designation within the standard.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_designation"
        )]
        designation: Option<String>,
        /// Tolerance or thread class.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_class"
        )]
        class: Option<String>,
        /// Whether exact helical thread geometry is modeled.
        modeled: bool,
        /// Whether cosmetic thread presentation is requested.
        cosmetic: bool,
        /// Thread pitch in canonical millimeters.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pitch"
        )]
        pitch: Option<PositiveLength>,
        /// Nominal major thread diameter.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_major_diameter"
        )]
        major_diameter: Option<PositiveLength>,
        /// Thread handedness.
        hand: ThreadHand,
        /// Axial thread-depth construction.
        depth: HoleThreadDepth,
        /// Additional radial thread clearance used for modeled geometry.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_clearance"
        )]
        clearance: Option<Length>,
    },
}

/// Thread handedness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ThreadHand {
    /// Right-hand thread.
    Right,
    /// Left-hand thread.
    Left,
}

/// Axial extent rule for a hole thread.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(from = "HoleThreadDepthWire", into = "HoleThreadDepthWire")]
#[serde(deny_unknown_fields)]
pub enum HoleThreadDepth {
    /// Thread follows the complete hole depth.
    HoleDepth,
    /// Explicit thread length.
    Blind {
        /// Explicit axial thread length.
        depth: PositiveLength,
    },
    /// Standard tapped-hole runout is subtracted from the hole depth.
    TappedStandard,
}

#[cfg(test)]
mod length_mapping_tests {
    use super::{HoleConstruction, HoleKind, HoleShape};
    use crate::scalar::{InteriorAngle, Length, PositiveLength};

    fn positive(value: f64) -> PositiveLength {
        PositiveLength::new(value).expect("positive test length")
    }

    #[test]
    fn hole_length_mapping_carries_bore_presence_and_angles() {
        let angle = InteriorAngle::new(1.0).expect("interior test angle");
        let source = HoleShape::new(
            HoleConstruction::form(HoleKind::Counterbore {
                diameter: positive(4.0),
                depth: positive(3.0),
            }),
            Some(HoleKind::Chamfer {
                diameter: positive(5.0),
                angle,
            }),
            Some(positive(2.0)),
        )
        .expect("valid source hole");
        let mapped = source
            .try_map_lengths(
                &mut |value| PositiveLength::new(value.get() * 2.0).ok_or("scaled positive length"),
                &mut |value: Length| -> Result<Length, &'static str> { Ok(value) },
            )
            .expect("valid scaled hole");
        let expected = HoleShape::new(
            HoleConstruction::form(HoleKind::Counterbore {
                diameter: positive(8.0),
                depth: positive(6.0),
            }),
            Some(HoleKind::Chamfer {
                diameter: positive(10.0),
                angle,
            }),
            Some(positive(4.0)),
        )
        .expect("valid expected hole");
        assert_eq!(mapped, expected);
        assert_eq!(source.diameter(), Some(positive(2.0)));

        let without_bore = HoleShape::new(HoleConstruction::form(HoleKind::Simple), None, None)
            .expect("valid hole without bore");
        assert_eq!(
            without_bore
                .try_map_lengths(
                    &mut |value| {
                        PositiveLength::new(value.get() * 2.0).ok_or("scaled positive length")
                    },
                    &mut |value: Length| -> Result<Length, &'static str> { Ok(value) },
                )
                .expect("valid mapped hole without bore"),
            without_bore
        );
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum HoleThreadDepthWire {
    /// Thread follows the complete hole depth.
    HoleDepth {},
    /// Explicit thread length.
    Blind {
        /// Explicit axial thread length.
        depth: PositiveLength,
    },
    /// Standard tapped-hole runout is subtracted from the hole depth.
    TappedStandard {},
}

impl From<HoleThreadDepth> for HoleThreadDepthWire {
    fn from(value: HoleThreadDepth) -> Self {
        match value {
            HoleThreadDepth::HoleDepth => Self::HoleDepth {},
            HoleThreadDepth::Blind { depth } => Self::Blind { depth },
            HoleThreadDepth::TappedStandard => Self::TappedStandard {},
        }
    }
}

impl From<HoleThreadDepthWire> for HoleThreadDepth {
    fn from(value: HoleThreadDepthWire) -> Self {
        match value {
            HoleThreadDepthWire::HoleDepth {} => Self::HoleDepth,
            HoleThreadDepthWire::Blind { depth } => Self::Blind { depth },
            HoleThreadDepthWire::TappedStandard {} => Self::TappedStandard,
        }
    }
}

/// Structural form of a hole entry treatment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HoleForm {
    /// Chamfered entry.
    Chamfer,
    /// Wider, flat-bottomed entry.
    Counterbore,
    /// Conical entry.
    Countersink,
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_exit_kind, HoleKind, "exit_kind");
cadmpeg_core::named_optional_field!(deserialize_diameter, PositiveLength, "diameter");
cadmpeg_core::named_optional_field!(
    deserialize_specification,
    Box<HoleSpecification>,
    "specification"
);
cadmpeg_core::named_optional_field!(deserialize_pitch, PositiveLength, "pitch");
cadmpeg_core::named_optional_field!(deserialize_form, HoleForm, "form");
cadmpeg_core::named_optional_field!(deserialize_entry_diameter, PositiveLength, "entry_diameter");
cadmpeg_core::named_optional_field!(deserialize_designation, String, "designation");
cadmpeg_core::named_optional_field!(deserialize_fit, String, "fit");
cadmpeg_core::named_optional_field!(deserialize_clearance, Length, "clearance");
cadmpeg_core::named_optional_field!(deserialize_class, String, "class");
cadmpeg_core::named_optional_field!(deserialize_major_diameter, PositiveLength, "major_diameter");

selection_field_deserializer!(deserialize_local_standard, "standard");
