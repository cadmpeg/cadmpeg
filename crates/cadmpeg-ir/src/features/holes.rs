// SPDX-License-Identifier: Apache-2.0
use super::deserialize_local_standard;
use crate::products::NonBlankString;
use crate::scalar::{InteriorAngle, Length, PositiveLength};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HoleShapeWire {
    construction: HoleConstruction,
    #[serde(default)]
    exit_kind: Option<HoleKind>,
    #[serde(default)]
    diameter: Option<PositiveLength>,
}

impl HoleShape {
    /// Admit treatments whose diameters exceed the bore diameter.
    pub fn new(
        construction: HoleConstruction,
        exit_kind: Option<HoleKind>,
        diameter: Option<PositiveLength>,
    ) -> Result<Self, &'static str> {
        let larger =
            |treatment: PositiveLength| diameter.is_some_and(|bore| treatment.get() > bore.get());
        let valid_kind = |kind: &HoleKind| match kind {
            HoleKind::Unresolved(_)
            | HoleKind::PartialCounterbore(..)
            | HoleKind::PartialCountersink(..)
            | HoleKind::Simple
            | HoleKind::SimpleDrilled { .. } => true,
            HoleKind::Chamfer { diameter, .. }
            | HoleKind::Counterbore { diameter, .. }
            | HoleKind::CounterboreDrilled { diameter, .. }
            | HoleKind::Countersink { diameter, .. } => larger(*diameter),
            HoleKind::Counterdrill { diameters, .. } => larger(diameters.diameter()),
        };
        let valid = match &construction {
            HoleConstruction::Form { kind, .. } => valid_kind(kind),
            HoleConstruction::NativeThread { major_diameter, .. } => larger(*major_diameter),
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        specification: Option<Box<HoleSpecification>>,
    },
    /// SLDPRT native thread geometry carried without a named standard specification.
    NativeThread {
        /// Nominal major diameter of the internal thread.
        major_diameter: PositiveLength,
        /// Axial length over which the thread is cut.
        thread_depth: PositiveLength,
        /// Thread pitch, when carried independently of a nominal designation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// The first dimension, when present.
    pub const fn first(&self) -> Option<&A> {
        match self {
            Self::First(first) => Some(first),
            Self::Second(_) => None,
        }
    }

    /// The second dimension, when present.
    pub const fn second(&self) -> Option<&B> {
        match self {
            Self::Second(second) => Some(second),
            Self::First(_) => None,
        }
    }

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

    /// Identified entry-treatment family of an unresolved construction.
    #[must_use]
    pub const fn unresolved_form(&self) -> Option<HoleForm> {
        match self {
            Self::Unresolved(form) => *form,
            Self::PartialCounterbore(..) => Some(HoleForm::Counterbore),
            Self::PartialCountersink(..) => Some(HoleForm::Countersink),
            _ => None,
        }
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
    Unresolved {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<HoleForm>,
    },
    PartialCounterbore {
        dimension: PartialCounterboreDimension,
    },
    PartialCountersink {
        dimension: PartialCountersinkDimension,
    },
    Simple {},
    Chamfer {
        diameter: PositiveLength,
        angle: InteriorAngle,
    },
    SimpleDrilled {
        drill_point_angle: InteriorAngle,
    },
    Counterbore {
        diameter: PositiveLength,
        depth: PositiveLength,
    },
    CounterboreDrilled {
        diameter: PositiveLength,
        depth: PositiveLength,
        drill_point_angle: InteriorAngle,
    },
    Countersink {
        diameter: PositiveLength,
        angle: InteriorAngle,
    },
    Counterdrill {
        diameter: PositiveLength,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entry_diameter: Option<PositiveLength>,
        depth: PositiveLength,
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
    Flat {},
    Angled {
        included_angle: InteriorAngle,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        designation: Option<String>,
        /// Clearance-hole fit class.
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clearance: Option<Length>,
    },
    /// Internally threaded-hole sizing and thread geometry.
    Threaded {
        /// Named thread standard family.
        #[serde(deserialize_with = "deserialize_local_standard")]
        standard: NonBlankString,
        /// Nominal size designation within the standard.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        designation: Option<String>,
        /// Tolerance or thread class.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<String>,
        /// Whether exact helical thread geometry is modeled.
        modeled: bool,
        /// Whether cosmetic thread presentation is requested.
        cosmetic: bool,
        /// Thread pitch in canonical millimeters.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pitch: Option<PositiveLength>,
        /// Nominal major thread diameter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        major_diameter: Option<PositiveLength>,
        /// Thread handedness.
        hand: ThreadHand,
        /// Axial thread-depth construction.
        depth: HoleThreadDepth,
        /// Additional radial thread clearance used for modeled geometry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum HoleThreadDepthWire {
    HoleDepth {},
    Blind { depth: PositiveLength },
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
