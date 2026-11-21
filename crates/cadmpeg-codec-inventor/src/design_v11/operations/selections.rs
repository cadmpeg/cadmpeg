// SPDX-License-Identifier: Apache-2.0
//! Input selections for feature operands.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::selection::{
    Anchor, BoundaryUse, MemberOrder, Members, NativeOwner, OrderedSelection, PickTarget, Picked,
    Picks, Selection, SetSelection, SketchRegion,
};
use cadmpeg_ir::features::{
    self as old, BodySelection, EdgeSelection, FaceSelection, PlanarProfileRef,
};
use cadmpeg_ir::sketches::Sketch;

use super::Projected;

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

pub(super) fn named(text: &str, within: Option<NativeOwner>) -> Projected<Picked> {
    let mut pick = picked(PickTarget::Native { within });
    pick.native = Some(NativeName {
        codec: cadmpeg_core::nonblank_literal!("inventor").into(),
        value: NativeNameValue::Text(
            NonBlankString::new(text).ok_or("a native selection is blank")?,
        ),
    });
    Ok(pick)
}

pub(super) fn selection<O: MemberOrder>(items: Vec<Picked>) -> Projected<Selection<O>> {
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(items)?),
        native: None,
    })
}

pub(super) fn native<O: MemberOrder>(text: &str) -> Projected<Selection<O>> {
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Unread,
        native: Some(
            vec![text.to_owned()]
                .try_into()
                .map_err(|_| "a native selection is blank")?,
        ),
    })
}

pub(super) fn set(selection: OrderedSelection) -> Projected<SetSelection> {
    Ok(Selection {
        at: selection.at,
        picks: match selection.picks {
            Picks::Named(members) => Picks::Named(Members::new(members.into_vec())?),
            Picks::Unread => Picks::Unread,
            Picks::Rule { .. } => return Err("the selection has a rule"),
        },
        native: selection.native,
    })
}

pub(super) fn face(value: &FaceSelection) -> Projected<SetSelection> {
    match value {
        FaceSelection::Faces(faces) if faces.is_empty() => selection(Vec::new()),
        FaceSelection::Native(text) | FaceSelection::Resolved { native: text, .. } => native(text),
        FaceSelection::Historical { native: text, .. }
        | FaceSelection::HistoricalPartial { native: text, .. } => native(text.as_str()),
        FaceSelection::Generated { native: text, .. } => native(text.as_str()),
        _ => Err("the face selection has no persistent name"),
    }
}

pub(super) fn edge(value: &EdgeSelection) -> Projected<OrderedSelection> {
    match value {
        EdgeSelection::Native(text) | EdgeSelection::Resolved { native: text, .. } => native(text),
        EdgeSelection::Historical { native: text, .. }
        | EdgeSelection::HistoricalPartial { native: text, .. } => native(text.as_str()),
        EdgeSelection::Generated { native: text, .. } => native(text.as_str()),
        _ => Err("the edge selection has no persistent name"),
    }
}

pub(super) fn bodies(value: &BodySelection) -> Projected<OrderedSelection> {
    match value {
        BodySelection::Native(text) | BodySelection::Resolved { native: text, .. } => native(text),
        BodySelection::Historical { native: text, .. }
        | BodySelection::Local { native: text, .. }
        | BodySelection::Generated { native: text, .. } => native(text.as_str()),
        BodySelection::NativeSet(names) => selection(
            names
                .iter()
                .map(|name| named(name, None))
                .collect::<Projected<_>>()?,
        ),
        BodySelection::ResolvedSet { members } => selection(
            members
                .iter()
                .map(|member| named(&member.clone().into_parts().1, None))
                .collect::<Projected<_>>()?,
        ),
        BodySelection::HistoricalSet { members, .. } => selection(
            members
                .iter()
                .map(|member| named(&member.clone().into_parts().1, None))
                .collect::<Projected<_>>()?,
        ),
        _ => Err("the body selection has no persistent name"),
    }
}

pub(super) fn path(value: &old::PathRef) -> Projected<OrderedSelection> {
    match value {
        old::PathRef::Native(text) => native(text),
        old::PathRef::Sketch(id) => selection(vec![picked(PickTarget::Sketch(id.clone()))]),
        old::PathRef::SketchCurves { curves, .. } => selection(
            curves
                .iter()
                .map(|id| picked(PickTarget::SketchEntity(id.clone())))
                .collect(),
        ),
        old::PathRef::SpatialSketchCurves { curves, .. } => selection(
            curves
                .iter()
                .map(|id| picked(PickTarget::SpatialSketchEntity(id.clone())))
                .collect(),
        ),
        old::PathRef::SpatialSketchSelection { sketch, selections } => selection(
            selections
                .iter()
                .map(|text| named(text, Some(NativeOwner::SpatialSketch(sketch.clone()))))
                .collect::<Projected<_>>()?,
        ),
        old::PathRef::HistoricalEdges { native: text, .. } => native(text.as_str()),
        _ => Err("the path selection has unresolved members"),
    }
}

pub(super) fn profile(value: &old::ProfileRef, sketches: &[Sketch]) -> Projected<OrderedSelection> {
    match value {
        old::ProfileRef::Planar(value) => planar(value, sketches),
        _ => Err("the spatial profile has no v11 sketch"),
    }
}

pub(super) fn planar(value: &PlanarProfileRef, sketches: &[Sketch]) -> Projected<OrderedSelection> {
    let loop_region = |sketch: &cadmpeg_ir::sketches::SketchId, outer, holes: &[u32]| {
        let source = sketches
            .iter()
            .find(|item| &item.id == sketch)
            .ok_or("the profile sketch is absent")?;
        let ring = |index: u32| -> Projected<Vec<BoundaryUse>> {
            Ok(source
                .profiles
                .get(usize::try_from(index).map_err(|_| "profile index overflow")?)
                .ok_or("profile index is outside its sketch")?
                .iter()
                .map(|usage| BoundaryUse {
                    entity: usage.entity.clone(),
                    fragment: None,
                })
                .collect())
        };
        Ok(picked(PickTarget::SketchRegion {
            sketch: sketch.clone(),
            region: SketchRegion {
                outer: ring(outer)?,
                holes: holes
                    .iter()
                    .map(|index| ring(*index))
                    .collect::<Projected<_>>()?,
            },
        }))
    };
    match value {
        PlanarProfileRef::Native(text) => native(text),
        PlanarProfileRef::Sketch(id) => selection(vec![picked(PickTarget::Sketch(id.clone()))]),
        PlanarProfileRef::SketchProfiles { .. } => Err("the selected loops have no explicit region nesting"),
        PlanarProfileRef::SketchRegions { sketch, regions } => selection(regions.iter().map(|region| match region {
            old::SketchProfileRegion::Loops { loops } => loop_region(sketch, loops.outer(), loops.holes()),
            old::SketchProfileRegion::Trimmed { .. } => Err("a trimmed profile stores parameter intervals outside the v11 boundary contract"),
        }).collect::<Projected<_>>()?),
        PlanarProfileRef::SketchEntities { entities, .. } => selection(entities.iter().map(|id| picked(PickTarget::SketchEntity(id.clone()))).collect()),
        PlanarProfileRef::SketchSelection { sketch, selections } => selection(selections.iter().map(|text| named(text, Some(NativeOwner::Sketch(sketch.clone())))).collect::<Projected<_>>()?),
        PlanarProfileRef::HistoricalFaces { native: names, .. } => Ok(Selection { at: Anchor::Input, picks: Picks::Unread, native: Some(names.clone()) }),
        PlanarProfileRef::Feature(id) => selection(vec![picked(PickTarget::FeatureResult(id.clone()))]),
        PlanarProfileRef::Generated { native: text, .. } => native(text.as_str()),
        _ => Err("the profile selection has unresolved members"),
    }
}

#[cfg(test)]
mod tests;
