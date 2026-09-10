// SPDX-License-Identifier: Apache-2.0
use super::deserialize_local_standard;
use crate::products::NonEmptyString;
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
    #[serde(flatten)]
    construction: HoleConstruction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<HoleKindWire>"))]
    exit_kind: Option<HoleKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diameter: Option<PositiveLength>,
}

#[derive(Deserialize)]
struct HoleShapeWire {
    #[serde(flatten)]
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

impl<'de> Deserialize<'de> for HoleShape {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = HoleShapeWire::deserialize(deserializer)?;
        Self::new(wire.construction, wire.exit_kind, wire.diameter)
            .map_err(serde::de::Error::custom)
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
#[serde(try_from = "HoleConstructionWire", into = "HoleConstructionWire")]
pub enum HoleConstruction {
    /// Entry treatment with optional named standard sizing or thread metadata.
    Form {
        /// Structural entry treatment.
        kind: HoleKind,
        /// Standard sizing and thread construction, when specified.
        specification: Option<Box<HoleSpecification>>,
    },
    /// SLDPRT native thread geometry carried without a named standard specification.
    NativeThread {
        /// Nominal major diameter of the internal thread.
        major_diameter: PositiveLength,
        /// Axial length over which the thread is cut.
        thread_depth: PositiveLength,
        /// Thread pitch, when carried independently of a nominal designation.
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

/// A pair of dimensions of which at least one is present.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum PartialPair<A, B> {
    /// Only the first dimension is present.
    First(A),
    /// Only the second dimension is present.
    Second(B),
    /// Both dimensions are present.
    Both(A, B),
}

impl<A, B> PartialPair<A, B> {
    /// Admit a pair unless both dimensions are absent.
    pub fn new(first: Option<A>, second: Option<B>) -> Option<Self> {
        match (first, second) {
            (Some(first), Some(second)) => Some(Self::Both(first, second)),
            (Some(first), None) => Some(Self::First(first)),
            (None, Some(second)) => Some(Self::Second(second)),
            (None, None) => None,
        }
    }

    /// The first dimension, when present.
    pub const fn first(&self) -> Option<&A> {
        match self {
            Self::First(first) | Self::Both(first, _) => Some(first),
            Self::Second(_) => None,
        }
    }

    /// The second dimension, when present.
    pub const fn second(&self) -> Option<&B> {
        match self {
            Self::Second(second) | Self::Both(_, second) => Some(second),
            Self::First(_) => None,
        }
    }

    /// Mutable access to the first dimension, when present.
    pub const fn first_mut(&mut self) -> Option<&mut A> {
        match self {
            Self::First(first) | Self::Both(first, _) => Some(first),
            Self::Second(_) => None,
        }
    }

    /// Mutable access to the second dimension, when present.
    pub const fn second_mut(&mut self) -> Option<&mut B> {
        match self {
            Self::Second(second) | Self::Both(_, second) => Some(second),
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
#[serde(tag = "kind", rename_all = "snake_case")]
enum HoleKindWire {
    Unresolved {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<HoleForm>,
    },
    PartialCounterbore {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diameter: Option<PositiveLength>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<PositiveLength>,
    },
    PartialCountersink {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diameter: Option<PositiveLength>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<InteriorAngle>,
    },
    Simple,
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
    Threaded {
        major_diameter: PositiveLength,
        thread_depth: PositiveLength,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pitch: Option<PositiveLength>,
        drill_point_angle: InteriorAngle,
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
                diameter: pair.first().copied(),
                depth: pair.second().copied(),
            },
            HoleKind::PartialCountersink(pair) => Self::PartialCountersink {
                diameter: pair.first().copied(),
                angle: pair.second().copied(),
            },
            HoleKind::Simple => Self::Simple,
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
            HoleKindWire::PartialCounterbore { diameter, depth } => Self::PartialCounterbore(
                PartialPair::new(diameter, depth)
                    .ok_or_else(|| "partial_counterbore carries no dimension".to_string())?,
            ),
            HoleKindWire::PartialCountersink { diameter, angle } => Self::PartialCountersink(
                PartialPair::new(diameter, angle)
                    .ok_or_else(|| "partial_countersink carries no dimension".to_string())?,
            ),
            HoleKindWire::Simple => Self::Simple,
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
            HoleKindWire::Threaded { .. } => {
                return Err("threaded hole kind requires a HoleConstruction".to_string())
            }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HoleConstructionWire {
    kind: HoleKindWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    specification: Option<Box<HoleSpecification>>,
}

impl From<HoleConstruction> for HoleConstructionWire {
    fn from(value: HoleConstruction) -> Self {
        match value {
            HoleConstruction::Form {
                kind,
                specification,
            } => Self {
                kind: kind.into(),
                specification,
            },
            HoleConstruction::NativeThread {
                major_diameter,
                thread_depth,
                pitch,
                drill_point_angle,
            } => Self {
                kind: HoleKindWire::Threaded {
                    major_diameter,
                    thread_depth,
                    pitch,
                    drill_point_angle,
                },
                specification: None,
            },
        }
    }
}

impl TryFrom<HoleConstructionWire> for HoleConstruction {
    type Error = String;

    fn try_from(value: HoleConstructionWire) -> Result<Self, Self::Error> {
        match value.kind {
            HoleKindWire::Threaded {
                major_diameter,
                thread_depth,
                pitch,
                drill_point_angle,
            } => {
                if value.specification.is_some() {
                    return Err(
                        "a native threaded hole cannot also carry a standard specification"
                            .to_string(),
                    );
                }
                Ok(Self::NativeThread {
                    major_diameter,
                    thread_depth,
                    pitch,
                    drill_point_angle,
                })
            }
            kind => Ok(Self::Form {
                kind: kind.try_into()?,
                specification: value.specification,
            }),
        }
    }
}

/// Profile geometry families accepted as hole-location generators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HoleProfileFilterWire", into = "HoleProfileFilterWire")]
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

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HoleProfileFilterWire {
    points: bool,
    circles: bool,
    arcs: bool,
}

impl TryFrom<HoleProfileFilterWire> for HoleProfileFilter {
    type Error = &'static str;

    fn try_from(value: HoleProfileFilterWire) -> Result<Self, Self::Error> {
        match (value.points, value.circles, value.arcs) {
            (true, false, false) => Ok(Self::Points),
            (false, true, false) => Ok(Self::Circles),
            (true, true, false) => Ok(Self::PointsAndCircles),
            (false, false, true) => Ok(Self::Arcs),
            (true, false, true) => Ok(Self::PointsAndArcs),
            (false, true, true) => Ok(Self::CirclesAndArcs),
            (true, true, true) => Ok(Self::All),
            (false, false, false) => Err("hole profile filter requires points, circles, or arcs"),
        }
    }
}

impl From<HoleProfileFilter> for HoleProfileFilterWire {
    fn from(value: HoleProfileFilter) -> Self {
        let (points, circles, arcs) = match value {
            HoleProfileFilter::Points => (true, false, false),
            HoleProfileFilter::Circles => (false, true, false),
            HoleProfileFilter::PointsAndCircles => (true, true, false),
            HoleProfileFilter::Arcs => (false, false, true),
            HoleProfileFilter::PointsAndArcs => (true, false, true),
            HoleProfileFilter::CirclesAndArcs => (false, true, true),
            HoleProfileFilter::All => (true, true, true),
        };
        Self {
            points,
            circles,
            arcs,
        }
    }
}

/// Blind-end construction of a drilled hole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
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

/// Standard sizing and optional physical-thread construction for a hole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HoleSpecificationWire", into = "HoleSpecificationWire")]
pub enum HoleSpecification {
    /// Clearance-hole sizing, which may carry a fit but cannot carry thread data.
    Clearance {
        /// Named fastener standard family.
        standard: NonEmptyString,
        /// Nominal size designation within the standard.
        designation: Option<String>,
        /// Clearance-hole fit class.
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
        clearance: Option<Length>,
    },
    /// Internally threaded-hole sizing and thread geometry.
    Threaded {
        /// Named thread standard family.
        standard: NonEmptyString,
        /// Nominal size designation within the standard.
        designation: Option<String>,
        /// Tolerance or thread class.
        class: Option<String>,
        /// Whether exact helical thread geometry is modeled.
        modeled: bool,
        /// Whether cosmetic thread presentation is requested.
        cosmetic: bool,
        /// Thread pitch in canonical millimeters.
        pitch: Option<PositiveLength>,
        /// Nominal major thread diameter.
        major_diameter: Option<PositiveLength>,
        /// Thread handedness.
        hand: ThreadHand,
        /// Axial thread-depth construction.
        depth: HoleThreadDepth,
        /// Additional radial thread clearance used for modeled geometry.
        clearance: Option<Length>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HoleSpecificationWire {
    #[serde(deserialize_with = "deserialize_local_standard")]
    standard: NonEmptyString,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    designation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fit: Option<String>,
    threaded: bool,
    modeled: bool,
    cosmetic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pitch: Option<PositiveLength>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    major_diameter: Option<PositiveLength>,
    hand: ThreadHand,
    depth: HoleThreadDepth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clearance: Option<Length>,
}

impl From<HoleSpecification> for HoleSpecificationWire {
    fn from(value: HoleSpecification) -> Self {
        match value {
            HoleSpecification::Clearance {
                standard,
                designation,
                fit,
                modeled,
                cosmetic,
                hand,
                depth,
                clearance,
            } => Self {
                standard,
                designation,
                class: None,
                fit,
                threaded: false,
                modeled,
                cosmetic,
                pitch: None,
                major_diameter: None,
                hand,
                depth,
                clearance,
            },
            HoleSpecification::Threaded {
                standard,
                designation,
                class,
                modeled,
                cosmetic,
                pitch,
                major_diameter,
                hand,
                depth,
                clearance,
            } => Self {
                standard,
                designation,
                class,
                fit: None,
                threaded: true,
                modeled,
                cosmetic,
                pitch,
                major_diameter,
                hand,
                depth,
                clearance,
            },
        }
    }
}

impl TryFrom<HoleSpecificationWire> for HoleSpecification {
    type Error = String;

    fn try_from(value: HoleSpecificationWire) -> Result<Self, Self::Error> {
        let HoleSpecificationWire {
            standard,
            designation,
            class,
            fit,
            threaded,
            modeled,
            cosmetic,
            pitch,
            major_diameter,
            hand,
            depth,
            clearance,
        } = value;
        if threaded {
            if fit.is_some() {
                return Err("fit must be absent for a threaded hole specification".to_string());
            }
            Ok(Self::Threaded {
                standard,
                designation,
                class,
                modeled,
                cosmetic,
                pitch,
                major_diameter,
                hand,
                depth,
                clearance,
            })
        } else {
            if class.is_some() || pitch.is_some() || major_diameter.is_some() {
                return Err(
                    "class, pitch, and major_diameter must be absent for a clearance hole specification"
                        .to_string(),
                );
            }
            Ok(Self::Clearance {
                standard,
                designation,
                fit,
                modeled,
                cosmetic,
                hand,
                depth,
                clearance,
            })
        }
    }
}

/// Thread handedness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThreadHand {
    /// Right-hand thread.
    Right,
    /// Left-hand thread.
    Left,
}

/// Axial extent rule for a hole thread.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
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

/// Structural form of a hole entry treatment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HoleForm {
    /// Chamfered entry.
    Chamfer,
    /// Wider, flat-bottomed entry.
    Counterbore,
    /// Conical entry.
    Countersink,
}
