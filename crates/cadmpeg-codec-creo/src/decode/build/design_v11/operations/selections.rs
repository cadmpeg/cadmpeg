// SPDX-License-Identifier: Apache-2.0
//! Operand selections, anchored at the state entering their feature.

use cadmpeg_ir::design::selection::{
    Anchor, MemberOrder, Members, OrderedSelection, PickTarget, Picked, Picks, Selection,
};
use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

use super::Stated;

pub(super) fn picked(target: PickTarget) -> Picked {
    Picked {
        target,
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

pub(super) fn named<O: MemberOrder>(picks: Vec<Picked>) -> Option<Selection<O>> {
    Some(Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(picks).ok()?),
        native: None,
    })
}

/// A selection the source holds whose members the decoder did not read,
/// with the source's record of it.
pub(super) fn unread<O: MemberOrder>(native: &str) -> Option<Selection<O>> {
    Some(Selection {
        at: Anchor::Input,
        picks: Picks::Unread,
        native: Some(vec![native.to_owned()].try_into().ok()?),
    })
}

/// A feature section: the whole sketch where its v11 sketch is stated, the
/// section's native record where the decoder holds the section without its
/// sketch, and nothing where the section is not read.
pub(super) fn profile(value: &ProfileRef, stated: &Stated<'_>) -> Option<OrderedSelection> {
    match value {
        ProfileRef::Planar(planar) => planar_profile(planar, stated),
        _ => None,
    }
}

pub(super) fn planar_profile(
    value: &PlanarProfileRef,
    stated: &Stated<'_>,
) -> Option<OrderedSelection> {
    match value {
        PlanarProfileRef::Sketch(sketch) if stated.sketches.contains_key(sketch) => {
            named(vec![picked(PickTarget::Sketch(sketch.clone()))])
        }
        PlanarProfileRef::Sketch(sketch) => unread(sketch.as_str()),
        PlanarProfileRef::Native(native) => unread(native),
        _ => None,
    }
}

/// The sketch a section names, where it is stated.
pub(super) fn profile_sketch<'a>(
    value: &'a ProfileRef,
    stated: &Stated<'_>,
) -> Option<&'a cadmpeg_ir::sketches::SketchId> {
    match value {
        ProfileRef::Planar(PlanarProfileRef::Sketch(sketch))
            if stated.sketches.contains_key(sketch) =>
        {
            Some(sketch)
        }
        _ => None,
    }
}
