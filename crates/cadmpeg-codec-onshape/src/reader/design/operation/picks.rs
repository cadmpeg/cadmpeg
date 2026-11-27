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
    Picks, Propagation, RuleTerm, Selection, SelectionRule, Witness,
};
use cadmpeg_ir::features::evaluation::FeatureInputTopology;
use cadmpeg_ir::features::profiles::{PathRef, PlanarProfileRef, SketchProfileRegion};

use super::Context;
use crate::reader::{features, rows, Mapping as SourceMapping};
use serde_json::Value;

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
pub(super) fn native(id: &str, within: Option<NativeOwner>) -> Mapping<Picked> {
    let mut picked = pick(PickTarget::Native { within });
    picked.native = Some(NativeName {
        codec: cadmpeg_ir::codec_id!(crate::dialect::FORMAT),
        value: NativeNameValue::Text(NonBlankString::new(id).ok_or("a selected id is blank")?),
    });
    Ok(picked)
}

/// The explicit ids in the query list, in source order.
pub(super) fn ids<'a>(feature: &'a Value, key: &str) -> SourceMapping<Vec<&'a str>> {
    let mut ids = Vec::new();
    for query in rows(features::parameter(feature, key)?, "queries")? {
        for value in rows(query, "deterministicIds")? {
            ids.push(value.as_str().ok_or("a selected id is not a string")?);
        }
    }
    Ok(ids)
}

pub(super) fn edge_picks(
    ids: &[&str],
    input: Option<&FeatureInputTopology>,
) -> Mapping<Vec<Picked>> {
    ids.iter()
        .map(|id| {
            let mut picked = native(id, None)?;
            picked.witness = input
                .and_then(|input| {
                    input
                        .edge_geometry
                        .iter()
                        .find(|sample| sample.edge.as_str() == *id)
                })
                .map(Witness::edge_sample);
            Ok(picked)
        })
        .collect()
}

pub(super) fn blend<O: MemberOrder>(
    seeds: Vec<Picked>,
    tangent: bool,
    excluded: Vec<Picked>,
) -> Mapping<Selection<O>> {
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Rule {
            terms: vec![RuleTerm {
                rule: SelectionRule::Explicit,
                seeds: Members::new(seeds)?,
                propagation: Some(if tangent {
                    Propagation::Connected {
                        continuity: Some(Continuity::G1),
                        angle: None,
                        gap: None,
                    }
                } else {
                    Propagation::Off
                }),
            }],
            excluded: if excluded.is_empty() {
                Vec::new()
            } else {
                vec![RuleTerm {
                    rule: SelectionRule::Explicit,
                    seeds: Members::new(excluded)?,
                    propagation: Some(Propagation::Off),
                }]
            },
            resolved: None,
        },
        native: None,
    })
}

pub(super) fn selected<O: MemberOrder>(feature: &Value, key: &str) -> SourceMapping<Selection<O>> {
    let ids = ids(feature, key)?;
    if ids.is_empty() {
        return Err(format!("{key} has no selected ids"));
    }
    let picks = ids
        .into_iter()
        .map(|id| native(id, None))
        .collect::<Mapping<_>>()?;
    Ok(selection(picks)?)
}

/// The bodies in source order, each within the feature that made it.
pub(super) fn bodies<O: MemberOrder>(values: &[(String, String)]) -> Mapping<Selection<O>> {
    selection(
        values
            .iter()
            .map(|(body, producer)| {
                let feature = cadmpeg_ir::features::FeatureId::mint(producer)
                    .map_err(|_| "a selected body has an invalid producer identity")?;
                native(body, Some(NativeOwner::Feature(feature)))
            })
            .collect::<Mapping<_>>()?,
    )
}

/// The profile `profile`: a whole sketch, regions of a sketch, or sketch
/// entities, of a sketch the design states earlier in the timeline.
pub(super) fn profile(
    profile: &PlanarProfileRef,
    context: &Context<'_, '_>,
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
pub(super) fn path(path: &PathRef, context: &Context<'_, '_>) -> Mapping<OrderedSelection> {
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
fn stated_before(sketch: &str, context: &Context<'_, '_>) -> Mapping<()> {
    if !context.sketches.contains(sketch) {
        return Err("the profile sketch is not stated in the design");
    }
    if !context.preceding.contains(sketch) {
        return Err("the profile sketch does not precede the feature in the timeline");
    }
    Ok(())
}
