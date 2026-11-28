// SPDX-License-Identifier: Apache-2.0
//! Selections: every operand that picks geometry or design entities.
//!
//! A selection states the state its picks resolve in (`at`), the picks or
//! the rule that gives them (`picks`), and the source's group-level
//! selection record (`native`).

use std::cmp::Ordering;
use std::marker::PhantomData;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};

use super::construction::FrameValue;
use super::dimension::Dimension;
use super::ids::ContainerId;
use super::names::{CodecName, NameKind, NativeName, TopoName};
use super::refs::{ConfigurationRef, EntityRef, Position};
use crate::document::Topology;
use crate::features::geometry::FinitePoint3;
use crate::features::selections::NativeSelections;
use crate::features::FeatureId;
use crate::geometry::{InlineCurve, InlineSurface};
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::scalar::{Angle, FiniteReal, Length};
use crate::sketches::{SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId};
use crate::structural_order::StructuralOrd;
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

/// A member of a [`Members`] list, distinct from the list's other members by
/// its identity key.
pub trait Keyed {
    /// The identity key, in the order of §2.5.
    type Key<'a>: Ord
    where
        Self: 'a;

    /// The member's identity key.
    fn key(&self) -> Self::Key<'_>;
}

/// A list of members that are distinct by identity. Distinctness and order
/// read only each member's identity key.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    try_from = "Vec<T>",
    bound(deserialize = "T: ::serde::de::DeserializeOwned + Keyed, O: MemberOrder")
)]
pub struct Members<T, O> {
    items: Vec<T>,
    order: PhantomData<O>,
}

impl<T, O> Default for Members<T, O> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            order: PhantomData,
        }
    }
}

impl<T: Keyed, O: MemberOrder> Members<T, O> {
    /// Admits a list of members that are distinct by identity. An
    /// `Unordered` list is sorted by identity key.
    ///
    /// # Errors
    ///
    /// Refuses a list that states a member twice.
    pub fn new(mut items: Vec<T>) -> Result<Self, &'static str> {
        let repeats = {
            let mut keys: Vec<T::Key<'_>> = Vec::with_capacity(items.len());
            for item in &items {
                keys.push(item.key());
            }
            keys.sort();
            keys.windows(2).any(|pair| pair[0] == pair[1])
        };
        if repeats {
            return Err("a member list states a member twice");
        }
        if O::SORTED {
            items.sort_by(|left, right| left.key().cmp(&right.key()));
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

    #[doc = "Consume the checked list in its stored order."]
    pub fn into_vec(self) -> Vec<T> {
        self.items
    }
}

impl<T: Serialize, O> Serialize for Members<T, O> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(&self.items)
    }
}

impl<T: Keyed, O: MemberOrder> TryFrom<Vec<T>> for Members<T, O> {
    type Error = &'static str;

    /// Reads a list of members that are distinct by identity. An `Unordered`
    /// list states its members in identity-key order, its one spelling.
    fn try_from(items: Vec<T>) -> Result<Self, Self::Error> {
        if O::SORTED
            && items.windows(2).any(|pair| pair[0].key() > pair[1].key())
            && !cadmpeg_core::read_session::repair(
                cadmpeg_core::read_session::Repair::Spelling,
                &|| "an unordered member list states its members out of identity-key order".into(),
            )
        {
            return Err("an unordered member list states its members out of identity-key order");
        }
        Self::new(items)
    }
}

#[cfg(feature = "schema")]
impl<T: JsonSchema, O: MemberOrder> JsonSchema for Members<T, O> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        let order = if O::SORTED { "Unordered" } else { "Ordered" };
        format!("{order}Members_{}", T::schema_name()).into()
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
        deserialize_with = "deserialize_native_selections"
    )]
    pub native: Option<NativeSelections>,
}

/// A selection whose picks keep source order.
pub type OrderedSelection = Selection<Ordered>;

/// A selection whose picks carry no order.
pub type SetSelection = Selection<Unordered>;

/// The state a selection's picks resolve in.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Input,
    /// One fixed state.
    Pinned(Position),
    /// The current geometry of the scope the pick resolves in. It follows
    /// every edit.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Current,
    /// A state of a definition that has no scope in this document, by the
    /// source's own name for it.
    Native(NativeName),
}

/// The members of a selection.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
            deserialize_with = "deserialize_resolved_members"
        )]
        resolved: Option<Members<Picked, O>>,
    },
}

/// One pick of a selection. Its identity key is
/// `(target, context, native, domain)`, and a `Native` pick that carries no
/// native name adds its witness. `resolved` is never part of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields, from = "PickedWire")]
pub struct Picked {
    /// What the pick picks.
    pub target: PickTarget,
    /// Where the pick resolves, where it is not the definition of the
    /// selection's owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<PickContext>,
    /// The source's own name for the pick.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeName>,
    /// The part of the entity that the pick is restricted to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<PickDomain>,
    /// The pick's evaluated geometry as the source cached it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub witness: Option<Witness>,
    /// The entities the pick resolves to in its anchored state, as the source
    /// identifies them there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<Vec<EntityRef>>,
}

/// The identity key of a [`Picked`], `(target, context, native, domain)`, in
/// the order of §2.5. A `Native` pick that carries no native name adds its
/// witness, which is then the one statement of what it picks; the witness of
/// any other pick, and `resolved`, are not part of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PickKey<'a> {
    target: &'a PickTarget,
    context: Option<&'a PickContext>,
    native: Option<&'a NativeName>,
    domain: Option<PickDomainKey<'a>>,
    witness: Option<WitnessKey<'a>>,
}

impl Keyed for Picked {
    type Key<'a> = PickKey<'a>;

    fn key(&self) -> PickKey<'_> {
        let witness = match (&self.target, &self.native) {
            (PickTarget::Native { .. }, None) => self.witness.as_ref().map(WitnessKey),
            _ => None,
        };
        PickKey {
            target: &self.target,
            context: self.context.as_ref(),
            native: self.native.as_ref(),
            domain: self.domain.as_ref().map(PickDomainKey),
            witness,
        }
    }
}

/// A witness in a pick's identity key, compared in the derived structural
/// order of §2.5.
#[derive(Debug, Clone, Copy)]
pub struct WitnessKey<'a>(&'a Witness);

impl PartialEq for WitnessKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for WitnessKey<'_> {}

impl PartialOrd for WitnessKey<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WitnessKey<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.structural_cmp(other.0)
    }
}

/// The reading form of [`Picked`].
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PickedWire {
    /// What the pick picks.
    target: PickTarget,
    /// Where the pick resolves, where it is not the definition of the
    /// selection's owner.
    #[serde(default, deserialize_with = "deserialize_context")]
    context: Option<PickContext>,
    /// The source's own name for the pick.
    #[serde(default, deserialize_with = "deserialize_native_name")]
    native: Option<NativeName>,
    /// The part of the entity that the pick is restricted to.
    #[serde(default, deserialize_with = "deserialize_domain")]
    domain: Option<PickDomain>,
    /// The pick's evaluated geometry as the source cached it.
    #[serde(default, deserialize_with = "deserialize_witness")]
    witness: Option<Witness>,
    /// The entities the pick resolves to in its anchored state, as the source
    /// identifies them there.
    #[serde(default, deserialize_with = "deserialize_resolved_entities")]
    resolved: Option<Vec<EntityRef>>,
}

impl From<PickedWire> for Picked {
    fn from(wire: PickedWire) -> Self {
        let PickedWire {
            target,
            context,
            native,
            domain,
            witness,
            resolved,
        } = wire;
        Self {
            target,
            context,
            native,
            domain,
            witness,
            resolved,
        }
    }
}

/// What a pick picks.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, cadmpeg_derive::DeserializeTagged,
)]
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
            deserialize_with = "deserialize_within"
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
            deserialize_with = "deserialize_part"
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
            deserialize_with = "deserialize_container"
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Component,
}

/// The sketch or feature a native pick lies in.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, cadmpeg_derive::DeserializeTagged,
)]
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
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, cadmpeg_derive::DeserializeTagged,
)]
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
            deserialize_with = "deserialize_own"
        )]
        own: Option<OccurrencePath>,
        /// The path from the assembly to the target.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_target"
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
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<ConfigurationRef>,
}

/// A path of occurrences, outermost first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct OccurrencePath(pub Vec<PathStep>);

/// One step of an occurrence path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<ConfigurationRef>,
}

/// The occurrence of a path step.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, cadmpeg_derive::DeserializeTagged,
)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
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
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, cadmpeg_derive::DeserializeTagged,
)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Witness {
    /// A point.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_point"
    )]
    pub point: Option<FinitePoint3>,
    /// A direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub direction: Option<UnitVector3>,
    /// An edge sample: its two end points.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_endpoints"
    )]
    pub endpoints: Option<[FinitePoint3; 2]>,
    /// A frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_frame"
    )]
    pub frame: Option<FrameValue>,
    /// The chain curves.
    pub curves: Vec<InlineCurve>,
    /// A surface.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_surface"
    )]
    pub surface: Option<InlineSurface>,
    /// The face group or body that the source backs up for the pick.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_body"
    )]
    pub body: Option<Box<Topology>>,
}

impl Witness {
    /// The witness of an edge as a feature's stored input state samples it:
    /// its interior point, the carrier tangent there, and its two end points
    /// where both are stored.
    #[must_use]
    pub fn edge_sample(sample: &crate::features::evaluation::HistoricalEdgeGeometry) -> Self {
        Self {
            point: Some(sample.interior),
            direction: sample.tangent,
            endpoints: sample
                .start
                .zip(sample.end)
                .map(|(start, end)| [start, end]),
            frame: None,
            curves: Vec::new(),
            surface: None,
            body: None,
        }
    }
}

/// The part of an entity that a pick is restricted to.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
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
            deserialize_with = "deserialize_start"
        )]
        start: Option<Box<Picked>>,
        /// The element that limits the end.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_end"
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

/// The identity key of a [`PickDomain`]: its value, with each limiting pick
/// compared by its own [`PickKey`].
#[derive(Debug, Clone, Copy)]
pub struct PickDomainKey<'a>(&'a PickDomain);

impl PartialEq for PickDomainKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for PickDomainKey<'_> {}

impl PartialOrd for PickDomainKey<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PickDomainKey<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        fn limit(pick: Option<&Picked>) -> Option<PickKey<'_>> {
            pick.map(Picked::key)
        }
        match (self.0, other.0) {
            (
                PickDomain::EdgeInterval { start, end },
                PickDomain::EdgeInterval {
                    start: other_start,
                    end: other_end,
                },
            ) => by_value(start.get(), other_start.get())
                .then_with(|| by_value(end.get(), other_end.get())),
            (
                PickDomain::Limits { start, end },
                PickDomain::Limits {
                    start: other_start,
                    end: other_end,
                },
            ) => (limit(start.as_deref()), limit(end.as_deref()))
                .cmp(&(limit(other_start.as_deref()), limit(other_end.as_deref()))),
            (PickDomain::Fragment(fragment), PickDomain::Fragment(other_fragment)) => {
                fragment.cmp(other_fragment)
            }
            (PickDomain::Containing(point), PickDomain::Containing(other_point)) => {
                by_value(point.x, other_point.x)
                    .then_with(|| by_value(point.y, other_point.y))
                    .then_with(|| by_value(point.z, other_point.z))
            }
            (left, right) => left.rank().cmp(&right.rank()),
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
        deserialize_with = "deserialize_propagation"
    )]
    pub propagation: Option<Propagation>,
}

/// The rule of a rule term. The kind of a seed decides what a rule reads
/// from it.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SelectionRule {
    /// The seeds themselves.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    LoopEdges,
    /// The edges that bound exactly one face of the seed faces, or of the
    /// seed bodies' faces.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Boundary,
    /// The edges or curves connected end to end from each seed.
    Chain {
        /// The pick the chain ends at. `None` is the end of the chain.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_end"
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
    /// Every component instance below the owner's definition that exists in
    /// the anchored state and whose bodies differ when the operation of the
    /// selection's feature is applied to that instance alone. It takes no
    /// seeds and no propagation, and occurs only as a term of an assembly
    /// feature's participants.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Affected,
    /// A source rule whose meaning is not established, written
    /// `codec:rule`.
    Native(CodecName),
}

/// How a rule term extends its result.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Propagation {
    /// The source holds propagation off.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Off,
    /// The operation continues past the seeds only where it cannot end
    /// otherwise.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Minimal,
    /// The result extends to the entities connected to it.
    Connected {
        /// The least continuity of a connection.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_connected_continuity"
        )]
        continuity: Option<Continuity>,
        /// The angle tolerance.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_angle"
        )]
        angle: Option<Dimension<Angle>>,
        /// The gap tolerance.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_gap"
        )]
        gap: Option<Dimension<Length>>,
    },
    /// A source propagation whose meaning is not established, written
    /// `codec:propagation`.
    Native(CodecName),
}

/// The continuity of a connection.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Continuity {
    /// Touching.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    G0,
    /// Tangent.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    G1,
    /// Curvature continuous.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    G2,
    /// G3 continuous.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
        deserialize_with = "deserialize_fragment"
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

cadmpeg_core::named_optional_field!(deserialize_angle, Dimension<Angle>, "angle");
cadmpeg_core::named_optional_field!(deserialize_body, Box<Topology>, "body");
cadmpeg_core::named_optional_field!(deserialize_configuration, ConfigurationRef, "configuration");
cadmpeg_core::named_optional_field!(deserialize_container, ContainerId, "container");
cadmpeg_core::named_optional_field!(deserialize_context, PickContext, "context");
cadmpeg_core::named_optional_field!(deserialize_connected_continuity, Continuity, "continuity");
cadmpeg_core::named_optional_field!(deserialize_direction, UnitVector3, "direction");
cadmpeg_core::named_optional_field!(deserialize_domain, PickDomain, "domain");
cadmpeg_core::named_optional_field!(deserialize_end, Box<Picked>, "end");
cadmpeg_core::named_optional_field!(deserialize_endpoints, [FinitePoint3; 2], "endpoints");
cadmpeg_core::named_optional_field!(deserialize_fragment, Fragment, "fragment");
cadmpeg_core::named_optional_field!(deserialize_frame, FrameValue, "frame");
cadmpeg_core::named_optional_field!(deserialize_gap, Dimension<Length>, "gap");
cadmpeg_core::named_optional_field!(deserialize_native_name, NativeName, "native");
cadmpeg_core::named_optional_field!(deserialize_native_selections, NativeSelections, "native");
cadmpeg_core::named_optional_field!(deserialize_own, OccurrencePath, "own");
cadmpeg_core::named_optional_field!(deserialize_part, DatumPart, "part");
cadmpeg_core::named_optional_field!(deserialize_point, FinitePoint3, "point");
cadmpeg_core::named_optional_field!(deserialize_propagation, Propagation, "propagation");
cadmpeg_core::named_optional_field!(deserialize_resolved_members<O>, Members<Picked, O>, "resolved");
cadmpeg_core::named_optional_field!(deserialize_resolved_entities, Vec<EntityRef>, "resolved");
cadmpeg_core::named_optional_field!(deserialize_start, Box<Picked>, "start");
cadmpeg_core::named_optional_field!(deserialize_surface, InlineSurface, "surface");
cadmpeg_core::named_optional_field!(deserialize_target, OccurrencePath, "target");
cadmpeg_core::named_optional_field!(deserialize_within, NativeOwner, "within");
cadmpeg_core::named_optional_field!(deserialize_witness, Witness, "witness");
