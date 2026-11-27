// SPDX-License-Identifier: Apache-2.0
//! The selections of a design operation (§5.8).
//!
//! A face or edge an Onshape feature selects is a deterministic id that the
//! reader found in the feature's stored input state. It is a `Native` pick
//! named by that id, with the edge's stored sample as its witness. A body a
//! derived Boolean selects is a `Native` pick within the feature that made
//! it, named by its local id.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::selection::{
    Anchor, Continuity, MemberOrder, Members, NativeOwner, OrderedSelection, PickTarget, Picked,
    Picks, Propagation, RuleTerm, Selection, SelectionRule, SetSelection, Witness,
};
use cadmpeg_ir::features::evaluation::FeatureInputTopology;
use cadmpeg_ir::features::profiles::{PathRef, PlanarProfileRef, SketchProfileRegion};
use cadmpeg_ir::features::selections::{BodySelection, EdgeSelection, FaceSelection};

use super::Context;

pub(super) type Mapping<T> = Result<T, &'static str>;

pub(super) fn pick(target: PickTarget) -> Picked {
    Picked {
        target,
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

pub(super) fn selection<O: MemberOrder>(picks: Vec<Picked>) -> Mapping<Selection<O>> {
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(picks).map_err(|_| "the selection is empty or repeats a pick")?,
        ),
        native: None,
    })
}

/// A `Native` pick named by the Onshape id `id`.
fn native(id: &str, within: Option<NativeOwner>) -> Mapping<Picked> {
    let mut picked = pick(PickTarget::Native { within });
    picked.native = Some(NativeName {
        codec: cadmpeg_ir::codec_id!(crate::dialect::FORMAT),
        value: NativeNameValue::Text(NonBlankString::new(id).ok_or("a selected id is blank")?),
    });
    Ok(picked)
}

/// The edges `ids` of the input state `state`, each with its stored sample
/// as its witness.
fn edge_picks<'i>(
    state: &str,
    ids: impl Iterator<Item = &'i str>,
    inputs: &[FeatureInputTopology],
) -> Mapping<Vec<Picked>> {
    let input = inputs
        .iter()
        .find(|input| input.id.as_str() == state)
        .ok_or("the selected edge state is absent")?;
    ids.map(|id| {
        let mut picked = native(id, None)?;
        picked.witness = input
            .edge_geometry
            .iter()
            .find(|sample| sample.edge.as_str() == id)
            .map(Witness::edge_sample);
        Ok(picked)
    })
    .collect()
}

/// The historical edges of `value`.
pub(super) fn edges(value: &EdgeSelection, context: &Context<'_>) -> Mapping<Vec<Picked>> {
    let EdgeSelection::Historical { state, edges, .. } = value else {
        return Err("the edge selection is not resolved in the input state");
    };
    edge_picks(
        state.as_str(),
        edges.iter().map(cadmpeg_ir::ids::HistoricalEdgeId::as_str),
        context.inputs,
    )
}

/// The edges of `value`, or, where the source propagates the selection
/// along tangent edges, one `Explicit` term on the selected `seeds` with
/// tangent propagation. `tangent` is the stored propagation flag.
pub(super) fn blend_edges<O: MemberOrder>(
    value: &EdgeSelection,
    tangent: Option<bool>,
    seeds: &[String],
    context: &Context<'_>,
) -> Mapping<Selection<O>> {
    let propagation = match tangent {
        None => return selection(edges(value, context)?),
        Some(false) => Propagation::Off,
        Some(true) => Propagation::Connected {
            continuity: Some(Continuity::G1),
            angle: None,
            gap: None,
        },
    };
    let EdgeSelection::Historical { state, .. } = value else {
        return Err("the edge selection is not resolved in the input state");
    };
    let seeds = if tangent == Some(true) {
        edge_picks(
            state.as_str(),
            seeds.iter().map(String::as_str),
            context.inputs,
        )?
    } else {
        edges(value, context)?
    };
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Rule {
            terms: vec![RuleTerm {
                rule: SelectionRule::Explicit,
                seeds: Members::new(seeds).map_err(|_| "the seed edges are empty or repeat")?,
                propagation: Some(propagation),
            }],
            excluded: Vec::new(),
            resolved: None,
        },
        native: None,
    })
}

/// The historical faces of `value`.
pub(super) fn faces(value: &FaceSelection) -> Mapping<SetSelection> {
    let FaceSelection::Historical { faces, .. } = value else {
        return Err("the face selection is not resolved in the input state");
    };
    selection(
        faces
            .iter()
            .map(|face| native(face.as_str(), None))
            .collect::<Mapping<_>>()?,
    )
}

/// The bodies of `value`, each within the feature that made it.
pub(super) fn bodies<O: MemberOrder>(value: &BodySelection) -> Mapping<Selection<O>> {
    let BodySelection::Generated { bodies, .. } = value else {
        return Err("the body selection names no generating feature");
    };
    selection(
        bodies
            .iter()
            .map(|body| {
                native(
                    body.local_id.as_str(),
                    Some(NativeOwner::Feature(body.feature.clone())),
                )
            })
            .collect::<Mapping<_>>()?,
    )
}

/// The profile `profile`: a whole sketch, regions of a sketch, or sketch
/// entities, of a sketch the design states earlier in the timeline.
pub(super) fn profile(
    profile: &PlanarProfileRef,
    context: &Context<'_>,
) -> Mapping<OrderedSelection> {
    let (sketch, picks) = match profile {
        PlanarProfileRef::Sketch(sketch) => {
            (sketch, vec![pick(PickTarget::Sketch(sketch.clone()))])
        }
        PlanarProfileRef::SketchEntities { sketch, entities } => (
            sketch,
            entities
                .iter()
                .map(|entity| {
                    context
                        .entities
                        .contains_key(entity.as_str())
                        .then(|| pick(PickTarget::SketchEntity(entity.clone())))
                        .ok_or("a profile entity is not stated in the design")
                })
                .collect::<Mapping<_>>()?,
        ),
        PlanarProfileRef::SketchRegions { sketch, regions } => {
            let arrangement = context.arrangements.get(context.model_entities, sketch)?;
            let picks = regions
                .iter()
                .map(|region| {
                    let SketchProfileRegion::Trimmed {
                        outer_boundary,
                        hole_boundaries,
                    } = region
                    else {
                        return Err("the profile region is no trimmed region");
                    };
                    let region = arrangement.region(outer_boundary, hole_boundaries)?;
                    if region
                        .outer
                        .iter()
                        .chain(region.holes.iter().flatten())
                        .any(|usage| !context.entities.contains_key(usage.entity.as_str()))
                    {
                        return Err("a region boundary entity is not stated in the design");
                    }
                    Ok(pick(PickTarget::SketchRegion {
                        sketch: sketch.clone(),
                        region,
                    }))
                })
                .collect::<Mapping<_>>()?;
            (sketch, picks)
        }
        _ => return Err("the profile is no sketch, sketch regions, or sketch entities"),
    };
    stated_before(sketch.as_str(), context)?;
    selection(picks)
}

/// The sketch curves of the path `path`, in path order.
pub(super) fn path(path: &PathRef, context: &Context<'_>) -> Mapping<OrderedSelection> {
    let PathRef::SketchCurves { sketch, curves } = path else {
        return Err("the path is no chain of sketch curves");
    };
    stated_before(sketch.as_str(), context)?;
    selection(
        curves
            .iter()
            .map(|curve| {
                context
                    .entities
                    .contains_key(curve.as_str())
                    .then(|| pick(PickTarget::SketchEntity(curve.clone())))
                    .ok_or("a path curve is not stated in the design")
            })
            .collect::<Mapping<_>>()?,
    )
}

/// Whether the design states the sketch `sketch` earlier in the timeline.
fn stated_before(sketch: &str, context: &Context<'_>) -> Mapping<()> {
    if !context.sketches.contains(sketch) {
        return Err("the profile sketch is not stated in the design");
    }
    if !context.preceding.contains(sketch) {
        return Err("the profile sketch does not precede the feature in the timeline");
    }
    Ok(())
}
