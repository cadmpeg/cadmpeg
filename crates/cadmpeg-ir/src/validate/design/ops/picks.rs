// SPDX-License-Identifier: Apache-2.0
//! What a selection's picks target, as far as the picks state it.

use std::fmt;

use super::super::names::derived_kinds;
use crate::design::names::{NameKind, TopoName};
use crate::design::selection::{
    Keyed, MemberOrder, PickTarget, Picked, Picks, PrincipalEntity, Selection, SelectionRule,
};

/// The kind of entity a pick targets, where its target states one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    /// A topological entity or carrier, by its name's derived kind.
    Name(NameKind),
    /// A whole sketch.
    Sketch,
    /// A planar sketch entity.
    SketchEntity,
    /// A spatial sketch entity.
    SpatialSketchEntity,
    /// A region of a sketch.
    SketchRegion,
    /// A sketch's plane, axes, normal, or origin.
    SketchFrame,
    /// A datum feature or a part of one.
    Datum,
    /// A principal entity of a definition or a container.
    Principal(PrincipalEntity),
    /// A feature as an object.
    Feature,
    /// A feature's whole result.
    FeatureResult,
    /// The occurrence the pick's context names.
    Component,
    /// A target whose kind the pick does not state: a native member, a
    /// template input, a publication, or a name whose kind does not derive.
    Unknown,
}

impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(kind) => write!(f, "{}", name_kind(*kind)),
            Self::Sketch => f.write_str("a sketch"),
            Self::SketchEntity => f.write_str("a sketch entity"),
            Self::SpatialSketchEntity => f.write_str("a spatial-sketch entity"),
            Self::SketchRegion => f.write_str("a sketch region"),
            Self::SketchFrame => f.write_str("a sketch frame entity"),
            Self::Datum => f.write_str("a datum"),
            Self::Principal(_) => f.write_str("a principal entity"),
            Self::Feature => f.write_str("a feature"),
            Self::FeatureResult => f.write_str("a feature result"),
            Self::Component => f.write_str("a component"),
            Self::Unknown => f.write_str("an entity of unstated kind"),
        }
    }
}

const fn name_kind(kind: NameKind) -> &'static str {
    match kind {
        NameKind::Body => "a body",
        NameKind::Surface => "a surface",
        NameKind::Face => "a face",
        NameKind::Curve => "a curve",
        NameKind::Edge => "an edge",
        NameKind::Vertex => "a vertex",
    }
}

/// The derived kind of a name (§5.7). `None` for a `Merged` name with no
/// members or with members of several kinds.
fn kind_of(name: &TopoName) -> Option<NameKind> {
    let kinds = derived_kinds(name);
    match kinds.first() {
        Some(kind) if kinds.len() == 1 => Some(*kind),
        _ => None,
    }
}

/// The class of a pick's target.
pub(super) fn class(pick: &Picked) -> Class {
    match &pick.target {
        PickTarget::Entity(name) => kind_of(name).map_or(Class::Unknown, Class::Name),
        PickTarget::Sketch(_) => Class::Sketch,
        PickTarget::SketchEntity(_) => Class::SketchEntity,
        PickTarget::SpatialSketchEntity(_) => Class::SpatialSketchEntity,
        PickTarget::SketchRegion { .. } => Class::SketchRegion,
        PickTarget::SketchFrame { .. } => Class::SketchFrame,
        PickTarget::Datum { .. } => Class::Datum,
        PickTarget::Principal { entity, .. } => Class::Principal(*entity),
        PickTarget::Feature(_) => Class::Feature,
        PickTarget::FeatureResult(_) => Class::FeatureResult,
        PickTarget::Component => Class::Component,
        PickTarget::Native { .. }
        | PickTarget::TemplateInput { .. }
        | PickTarget::Publication(_) => Class::Unknown,
    }
}

/// The members of a `Named` selection.
pub(super) fn named<O: MemberOrder>(selection: &Selection<O>) -> Option<&[Picked]> {
    match &selection.picks {
        Picks::Named(members) => Some(members.as_slice()),
        Picks::Unread | Picks::Rule { .. } => None,
    }
}

/// The number of picks a `Named` selection holds.
pub(super) fn count<O: MemberOrder>(selection: &Selection<O>) -> Option<usize> {
    named(selection).map(<[Picked]>::len)
}

/// Whether the selection states no pick: a `Named` selection with no
/// members, or a rule with no terms.
pub(super) fn is_empty<O: MemberOrder>(selection: &Selection<O>) -> bool {
    match &selection.picks {
        Picks::Unread => false,
        Picks::Named(members) => members.as_slice().is_empty(),
        Picks::Rule { terms, .. } => terms.is_empty(),
    }
}

/// The classes a selection states: each `Named` pick's, the kind an `All` or
/// `Of` term selects, and each seed of an `Explicit` term. Other terms
/// select entities whose kind the rule's operands decide, and are skipped.
pub(super) fn classes<O: MemberOrder>(selection: &Selection<O>) -> Vec<Class> {
    match &selection.picks {
        Picks::Unread => Vec::new(),
        Picks::Named(members) => members.as_slice().iter().map(class).collect(),
        Picks::Rule { terms, .. } => terms
            .iter()
            .flat_map(|term| match &term.rule {
                SelectionRule::All { kind } | SelectionRule::Of { kind } => {
                    vec![Class::Name(*kind)]
                }
                SelectionRule::Explicit => term.seeds.as_slice().iter().map(class).collect(),
                _ => Vec::new(),
            })
            .collect(),
    }
}

/// The first stated class of `selection` that `allowed` refuses. An
/// `Unknown` class is never refused.
pub(super) fn refused<O: MemberOrder>(
    selection: &Selection<O>,
    allowed: impl Fn(Class) -> bool,
) -> Option<Class> {
    classes(selection)
        .into_iter()
        .find(|&class| class != Class::Unknown && !allowed(class))
}

/// Whether two selections name one pick: equal identity keys.
pub(super) fn share_pick<A: MemberOrder, B: MemberOrder>(
    first: &Selection<A>,
    second: &Selection<B>,
) -> bool {
    let (Some(first), Some(second)) = (named(first), named(second)) else {
        return false;
    };
    first
        .iter()
        .any(|a| second.iter().any(|b| a.key() == b.key()))
}

/// The pick-kind predicates the rules name.
pub(super) const fn is_body(class: Class) -> bool {
    matches!(class, Class::Name(NameKind::Body) | Class::FeatureResult)
}

pub(super) const fn is_face(class: Class) -> bool {
    matches!(class, Class::Name(NameKind::Face))
}

pub(super) const fn is_edge(class: Class) -> bool {
    matches!(class, Class::Name(NameKind::Edge))
}

pub(super) const fn is_vertex(class: Class) -> bool {
    matches!(class, Class::Name(NameKind::Vertex))
}

/// A plane: a face or surface, a datum, a principal plane, or a sketch's
/// plane.
pub(super) const fn is_plane(class: Class) -> bool {
    matches!(
        class,
        Class::Name(NameKind::Face | NameKind::Surface)
            | Class::Datum
            | Class::SketchFrame
            | Class::Principal(
                PrincipalEntity::XyPlane | PrincipalEntity::XzPlane | PrincipalEntity::YzPlane
            )
    )
}

/// A point: a vertex, a datum point, a principal origin, a sketch's origin,
/// or a sketch entity, whose curve the sketch rules check.
pub(super) const fn is_point(class: Class) -> bool {
    matches!(
        class,
        Class::Name(NameKind::Vertex)
            | Class::Datum
            | Class::SketchFrame
            | Class::SketchEntity
            | Class::SpatialSketchEntity
            | Class::Principal(PrincipalEntity::Origin)
    )
}
