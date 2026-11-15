// SPDX-License-Identifier: Apache-2.0
//! Selections: every operand that picks geometry or design entities.
//!
//! A selection states the state its picks resolve in (`at`), the picks or
//! the rule that gives them (`picks`), and the source's group-level
//! selection record (`native`).

use std::cmp::Ordering;
use std::marker::PhantomData;

use cadmpeg_core::text::NonBlankString;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::construction::FrameValue;
use super::dimension::Dimension;
use super::ids::{ContainerId, PublicationId};
use super::names::{NameKind, NativeName, TopoName};
use super::read::{enter_de, Ceiling};
use super::refs::{ConfigurationRef, EntityRef, Position};
use super::topology::Topology;
use crate::features::{FeatureId, FinitePoint3, NativeSelections};
use crate::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::scalar::{Angle, FiniteReal, Length};
use crate::sketches::{SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId};
use crate::units::UnitVector3;

/// The order marker of a [`Members`] list.
pub trait MemberOrder {
    /// Whether the list is sorted by its members' identity keys.
    const SORTED: bool;
}

/// A member list that keeps source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Ordered;

/// A member list with no order, sorted by its members' identity keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Unordered;

impl MemberOrder for Ordered {
    const SORTED: bool = false;
}

impl MemberOrder for Unordered {
    const SORTED: bool = true;
}

/// A list of members that are distinct by identity. A member type's `Ord`
/// is its identity key, so distinctness and order read only that key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Members<T, O> {
    items: Vec<T>,
    order: PhantomData<O>,
}

impl<T: Ord, O: MemberOrder> Members<T, O> {
    /// Admits a list of members that are distinct by identity. An
    /// `Unordered` list is sorted by identity key.
    ///
    /// # Errors
    ///
    /// Refuses a list that states a member twice.
    pub fn new(mut items: Vec<T>) -> Result<Self, &'static str> {
        let mut keys: Vec<&T> = items.iter().collect();
        keys.sort();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("a member list states a member twice");
        }
        if O::SORTED {
            items.sort();
        }
        Ok(Self {
            items,
            order: PhantomData,
        })
    }
}

impl<T, O> Members<T, O> {
    /// The members, in list order.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }
}

impl<T: Serialize, O> Serialize for Members<T, O> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(&self.items)
    }
}

impl<'de, T: Deserialize<'de> + Ord, O: MemberOrder> Deserialize<'de> for Members<T, O> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::<T>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl<T: JsonSchema, O> JsonSchema for Members<T, O> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("Members_{}", T::schema_name()).into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "array",
            "items": generator.subschema_for::<T>(),
            "uniqueItems": true
        })
    }
}

/// An operand that picks geometry or design entities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields, bound = "")]
pub struct Selection<O: MemberOrder> {
    /// The state the picks resolve in.
    pub at: Anchor,
    /// The picks, or the rule that gives them.
    pub picks: Picks<O>,
    /// The source's group-level selection record, where it has one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub native: Option<NativeSelections>,
}

/// A selection whose picks keep source order.
pub type OrderedSelection = Selection<Ordered>;

/// A selection whose picks carry no order.
pub type SetSelection = Selection<Unordered>;

/// The state a selection's picks resolve in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Anchor {
    /// The state entering the feature whose definition holds the selection.
    /// It follows edits that insert features before that feature.
    Input,
    /// One fixed state.
    Pinned(Position),
    /// The current geometry of the scope the pick resolves in. It follows
    /// every edit.
    Current,
    /// A state of a definition that has no scope in this document, by the
    /// source's own name for it.
    Native(NativeName),
}

/// The members of a selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound = ""
)]
pub enum Picks<O: MemberOrder> {
    /// The source holds a selection whose members the decoder did not read.
    Unread,
    /// A list of picks.
    Named(Members<Picked, O>),
    /// A rule the source stores instead of a list.
    Rule {
        /// The terms whose results the selection unites.
        terms: Vec<RuleTerm<O>>,
        /// The terms whose results the selection removes.
        excluded: Vec<RuleTerm<O>>,
        /// The rule's result in the anchored state, only as the source cached
        /// it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        resolved: Option<Members<Picked, O>>,
    },
}

/// One pick of a selection. Its identity key is
/// `(target, context, native, domain)`; neither the witness nor `resolved`
/// is part of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Picked {
    /// What the pick picks.
    pub target: PickTarget,
    /// Where the pick resolves, where it is not the definition of the
    /// selection's owner.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub context: Option<PickContext>,
    /// The source's own name for the pick.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub native: Option<NativeName>,
    /// The part of the entity that the pick is restricted to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub domain: Option<PickDomain>,
    /// The pick's evaluated geometry as the source cached it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub witness: Option<Witness>,
    /// The entities the pick resolves to in its anchored state, as the source
    /// identifies them there.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub resolved: Option<Vec<EntityRef>>,
}

impl Picked {
    fn identity(
        &self,
    ) -> (
        &PickTarget,
        &Option<PickContext>,
        &Option<NativeName>,
        &Option<PickDomain>,
    ) {
        (&self.target, &self.context, &self.native, &self.domain)
    }
}

impl Eq for Picked {}

impl PartialOrd for Picked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Picked {
    fn cmp(&self, other: &Self) -> Ordering {
        self.identity().cmp(&other.identity())
    }
}

/// The reading form of [`Picked`]. A pick nested in it reads through
/// [`Picked`]'s reader, which holds one level of the pick ceiling.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PickedWire {
    target: PickTarget,
    #[serde(default, deserialize_with = "super::wire::present")]
    context: Option<PickContext>,
    #[serde(default, deserialize_with = "super::wire::present")]
    native: Option<NativeName>,
    #[serde(default, deserialize_with = "super::wire::present")]
    domain: Option<PickDomain>,
    #[serde(default, deserialize_with = "super::wire::present")]
    witness: Option<Witness>,
    #[serde(default, deserialize_with = "super::wire::present")]
    resolved: Option<Vec<EntityRef>>,
}

impl<'de> Deserialize<'de> for Picked {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let _level = enter_de::<D::Error>(Ceiling::Pick)?;
        let PickedWire {
            target,
            context,
            native,
            domain,
            witness,
            resolved,
        } = PickedWire::deserialize(deserializer)?;
        Ok(Self {
            target,
            context,
            native,
            domain,
            witness,
            resolved,
        })
    }
}

/// What a pick picks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PickTarget {
    /// A geometric entity, by its structured name.
    Entity(TopoName),
    /// A pick known only by its native name or its witness.
    Native {
        /// The sketch or feature the pick lies in.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        within: Option<NativeOwner>,
    },
    /// A sketch.
    Sketch(SketchId),
    /// A sketch entity.
    SketchEntity(SketchEntityId),
    /// A spatial-sketch entity.
    SpatialSketchEntity(SpatialSketchEntityId),
    /// One region of a sketch, by its boundary.
    SketchRegion {
        /// The sketch.
        sketch: SketchId,
        /// The region.
        region: SketchRegion,
    },
    /// A sketch's plane, axes, normal, or origin.
    SketchFrame {
        /// The sketch.
        sketch: SketchId,
        /// The principal entity of the sketch's frame.
        entity: PrincipalEntity,
    },
    /// A datum feature, or one part of it.
    Datum {
        /// The datum feature.
        feature: FeatureId,
        /// The part of the datum.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        part: Option<DatumPart>,
    },
    /// An origin plane, axis, point, or frame of the definition the pick
    /// resolves in, or of an ordered container that carries its own origin.
    Principal {
        /// The principal entity.
        entity: PrincipalEntity,
        /// The container whose origin the pick picks.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        container: Option<ContainerId>,
    },
    /// A feature as an object.
    Feature(FeatureId),
    /// The whole geometric result of a feature.
    FeatureResult(FeatureId),
    /// An input of a template instance, from inside the features the
    /// instance generates.
    TemplateInput {
        /// The template instance.
        instance: FeatureId,
        /// The input.
        input: u32,
    },
    /// The occurrence that the pick's context names.
    Component,
    /// A published entity.
    Publication(PublicationId),
}

/// The sketch or feature a native pick lies in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NativeOwner {
    /// A sketch.
    Sketch(SketchId),
    /// A spatial sketch.
    SpatialSketch(SpatialSketchId),
    /// A feature.
    Feature(FeatureId),
}

/// Where a pick resolves.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PickContext {
    /// In the prototype that the path reaches from the owner's definition.
    Occurrence(OccurrencePath),
    /// In a definition, without a placement.
    Definition(DefinitionRef),
    /// Through an assembly that is not the owner's definition.
    InContext {
        /// The assembly.
        assembly: DefinitionRef,
        /// The owner's own place in the assembly.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        own: Option<OccurrencePath>,
        /// The path from the assembly to the target.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        target: Option<OccurrencePath>,
    },
}

/// A definition, in a configuration where the source states one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DefinitionRef {
    /// The definition.
    pub definition: ProductDefinitionId,
    /// The configuration of the definition.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub configuration: Option<ConfigurationRef>,
}

/// A path of occurrences, outermost first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct OccurrencePath(pub Vec<PathStep>);

/// One step of an occurrence path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PathStep {
    /// The occurrence.
    pub occurrence: StepRef,
    /// The configuration of the step's prototype that the path reads, where
    /// the source stores one with the path. `None` reads the configuration
    /// the occurrence uses.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub configuration: Option<ConfigurationRef>,
}

/// The occurrence of a path step.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StepRef {
    /// An occurrence of this document.
    Occurrence(OccurrenceId),
    /// The source's own key for an occurrence that the document does not
    /// hold. Every step after a `Native` step is `Native`.
    Native(NativeName),
}

/// An origin plane, axis, point, or frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PrincipalEntity {
    /// The XY plane.
    XyPlane,
    /// The XZ plane.
    XzPlane,
    /// The YZ plane.
    YzPlane,
    /// The X axis.
    XAxis,
    /// The Y axis.
    YAxis,
    /// The Z axis.
    ZAxis,
    /// The origin point.
    Origin,
    /// The frame.
    Frame,
}

/// A part of a datum feature.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DatumPart {
    /// One principal entity of a coordinate-frame datum.
    Principal(PrincipalEntity),
    /// One point of a multi-point datum.
    Point(u32),
}

/// A pick's evaluated geometry as the source cached it: a cache of the pick
/// in its anchored state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Witness {
    /// A point.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub point: Option<FinitePoint3>,
    /// A direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub direction: Option<UnitVector3>,
    /// An edge sample: its two end points.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub endpoints: Option<[FinitePoint3; 2]>,
    /// A frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub frame: Option<FrameValue>,
    /// The chain curves.
    pub curves: Vec<SolvedCurveGeometry>,
    /// A surface.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub surface: Option<SolvedSurfaceGeometry>,
    /// The face group or body that the source backs up for the pick.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub body: Option<Box<Topology>>,
}

/// The part of an entity that a pick is restricted to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PickDomain {
    /// An interval of an edge.
    EdgeInterval {
        /// The start of the interval.
        start: FiniteReal,
        /// The end of the interval.
        end: FiniteReal,
    },
    /// An edge between two limiting elements.
    Limits {
        /// The element that limits the start.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        start: Option<Box<Picked>>,
        /// The element that limits the end.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        end: Option<Box<Picked>>,
    },
    /// One fragment of a sketch curve cut by its neighbours.
    Fragment(Fragment),
    /// The piece that contains a point.
    Containing(FinitePoint3),
}

impl PickDomain {
    const fn rank(&self) -> u8 {
        match self {
            Self::EdgeInterval { .. } => 0,
            Self::Limits { .. } => 1,
            Self::Fragment(_) => 2,
            Self::Containing(_) => 3,
        }
    }
}

/// The order of finite reals by value, with −0 equal to 0.
fn by_value(left: f64, right: f64) -> Ordering {
    (left + 0.0).total_cmp(&(right + 0.0))
}

impl Eq for PickDomain {}

impl PartialOrd for PickDomain {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PickDomain {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (
                Self::EdgeInterval { start, end },
                Self::EdgeInterval {
                    start: other_start,
                    end: other_end,
                },
            ) => by_value(start.get(), other_start.get())
                .then_with(|| by_value(end.get(), other_end.get())),
            (
                Self::Limits { start, end },
                Self::Limits {
                    start: other_start,
                    end: other_end,
                },
            ) => (start, end).cmp(&(other_start, other_end)),
            (Self::Fragment(fragment), Self::Fragment(other_fragment)) => {
                fragment.cmp(other_fragment)
            }
            (Self::Containing(point), Self::Containing(other_point)) => {
                by_value(point.x, other_point.x)
                    .then_with(|| by_value(point.y, other_point.y))
                    .then_with(|| by_value(point.z, other_point.z))
            }
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

/// One term of a rule selection: a rule applied to seeds, extended by a
/// propagation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields, bound = "")]
pub struct RuleTerm<O: MemberOrder> {
    /// The rule.
    pub rule: SelectionRule,
    /// The seeds the rule reads.
    pub seeds: Members<Picked, O>,
    /// The propagation that extends the rule's result. `None` means the
    /// source does not state it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub propagation: Option<Propagation>,
}

/// The rule of a rule term. The kind of a seed decides what a rule reads
/// from it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SelectionRule {
    /// The seeds themselves.
    Explicit,
    /// Every entity of a kind in the anchored state. It takes no seeds.
    All {
        /// The kind of entity.
        kind: NameKind,
    },
    /// The entities of a kind that contain or bound a seed, or that a seed
    /// feature or sketch produced.
    Of {
        /// The kind of entity.
        kind: NameKind,
    },
    /// The edges of a loop.
    LoopEdges,
    /// The edges that bound exactly one face of the seed faces, or of the
    /// seed bodies' faces.
    Boundary,
    /// The edges or curves connected end to end from each seed.
    Chain {
        /// The pick the chain ends at. `None` is the end of the chain.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        end: Option<Box<Picked>>,
    },
    /// The faces reachable from the seed faces without crossing a boundary
    /// face, or the region that boundary curves enclose.
    Region {
        /// The boundary.
        boundary: Members<Picked, Unordered>,
    },
    /// The edges where the seeds' faces meet other faces.
    Intersection {
        /// The faces the seeds' faces meet.
        with: Members<Picked, Unordered>,
    },
    /// A source rule whose meaning is not established, written
    /// `codec:rule`.
    Native(NonBlankString),
}

/// How a rule term extends its result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Propagation {
    /// The source holds propagation off.
    Off,
    /// The operation continues past the seeds only where it cannot end
    /// otherwise.
    Minimal,
    /// The result extends to the entities connected to it.
    Connected {
        /// The least continuity of a connection.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        continuity: Option<Continuity>,
        /// The angle tolerance.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        angle: Option<Dimension<Angle>>,
        /// The gap tolerance.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::wire::present"
        )]
        gap: Option<Dimension<Length>>,
    },
    /// A source propagation whose meaning is not established.
    Native(NonBlankString),
}

/// The continuity of a connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Continuity {
    /// Touching.
    G0,
    /// Tangent.
    G1,
    /// Curvature continuous.
    G2,
    /// G3 continuous.
    G3,
    /// A source continuity whose meaning is not established.
    Native(NonBlankString),
}

/// One region of a sketch, by its boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SketchRegion {
    /// The outer boundary.
    pub outer: Vec<BoundaryUse>,
    /// The boundary of each hole.
    pub holes: Vec<Vec<BoundaryUse>>,
}

/// A use of a sketch entity in a region's boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BoundaryUse {
    /// The sketch entity.
    pub entity: SketchEntityId,
    /// The fragment of the entity that the boundary uses.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::wire::present"
    )]
    pub fragment: Option<Fragment>,
}

/// One fragment of a curve that its neighbours cut into pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Fragment {
    /// The fragment, counted from 0.
    pub index: u32,
    /// The number of fragments.
    pub count: u32,
}
