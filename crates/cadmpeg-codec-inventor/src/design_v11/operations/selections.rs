// SPDX-License-Identifier: Apache-2.0
//! Input selections for feature operands.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::selection::{
    Anchor, BoundaryUse, MemberOrder, Members, NativeOwner, OrderedSelection, PickTarget, Picked,
    Picks, Selection, SetSelection, SketchRegion,
};
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::features::selections::{BodySelection, EdgeSelection, FaceSelection};
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
        FaceSelection::Native(text) => native(text),
        FaceSelection::Resolved {
            faces,
            native: text,
        } => {
            let mut pick = named(text, None)?;
            pick.resolved = Some(
                faces
                    .iter()
                    .cloned()
                    .map(cadmpeg_ir::design::refs::EntityRef::Face)
                    .collect(),
            );
            selection(vec![pick])
        }
        FaceSelection::Generated {
            faces,
            native: text,
        } => {
            let mut result = selection(
                faces
                    .iter()
                    .map(|face| {
                        named(
                            face.local_id.as_str(),
                            Some(NativeOwner::Feature(face.feature.clone())),
                        )
                    })
                    .collect::<Projected<_>>()?,
            )?;
            result.native = Some(
                vec![text.as_str().to_owned()]
                    .try_into()
                    .map_err(|_| "a face group is blank")?,
            );
            Ok(result)
        }
        FaceSelection::Historical {
            faces,
            native: text,
            ..
        } => named_group(
            text.as_str(),
            faces.iter().map(cadmpeg_ir::ids::HistoricalFaceId::as_str),
        ),
        FaceSelection::HistoricalPartial {
            faces,
            unresolved,
            native: text,
            ..
        } => named_group(
            text.as_str(),
            faces
                .iter()
                .map(cadmpeg_ir::ids::HistoricalFaceId::as_str)
                .chain(unresolved.iter().map(String::as_str)),
        ),

        _ => Err("the face selection has no persistent name"),
    }
}

fn named_group<'a, O: MemberOrder>(
    group: &str,
    names: impl Iterator<Item = &'a str>,
) -> Projected<Selection<O>> {
    let mut value = selection(
        names
            .map(|name| named(name, None))
            .collect::<Projected<_>>()?,
    )?;
    value.native = Some(
        vec![group.to_owned()]
            .try_into()
            .map_err(|_| "a native group is blank")?,
    );
    Ok(value)
}

fn historical_edges<'a>(
    state: &cadmpeg_ir::ids::FeatureInputTopologyId,
    edges: impl Iterator<Item = &'a cadmpeg_ir::ids::HistoricalEdgeId>,
    unresolved: &[String],
    group: &str,
    inputs: &[old::FeatureInputTopology],
) -> Projected<OrderedSelection> {
    let mut states = inputs.iter().filter(|input| &input.id == state);
    let input = states.next().ok_or("the selected edge state is absent")?;
    if states.next().is_some() {
        return Err("the selected edge state is ambiguous");
    }
    let mut picks = Vec::new();
    for edge in edges {
        if !input.edges.contains(edge) {
            return Err("the selected edge is absent from its state");
        }
        let mut pick = named(edge.as_str(), None)?;
        let mut samples = input
            .edge_geometry
            .iter()
            .filter(|sample| &sample.edge == edge);
        if let Some(sample) = samples.next() {
            if samples.next().is_some() {
                return Err("the selected edge sample is ambiguous");
            }
            pick.witness = Some(cadmpeg_ir::design::selection::Witness {
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
            });
        }
        picks.push(pick);
    }
    for name in unresolved {
        picks.push(named(name, None)?);
    }
    let mut value = selection(picks)?;
    value.native = Some(
        vec![group.to_owned()]
            .try_into()
            .map_err(|_| "an edge group is blank")?,
    );
    Ok(value)
}

pub(super) fn edge(
    value: &EdgeSelection,
    inputs: &[old::FeatureInputTopology],
) -> Projected<OrderedSelection> {
    match value {
        EdgeSelection::Native(text) => native(text),
        EdgeSelection::Resolved {
            edges,
            native: text,
        } => {
            let mut pick = named(text, None)?;
            pick.resolved = Some(
                edges
                    .iter()
                    .cloned()
                    .map(cadmpeg_ir::design::refs::EntityRef::Edge)
                    .collect(),
            );
            selection(vec![pick])
        }
        EdgeSelection::Generated {
            edges,
            native: text,
        } => {
            let mut result = selection(
                edges
                    .iter()
                    .map(|edge| {
                        named(
                            edge.local_id.as_str(),
                            Some(NativeOwner::Feature(edge.feature.clone())),
                        )
                    })
                    .collect::<Projected<_>>()?,
            )?;
            result.native = Some(
                vec![text.as_str().to_owned()]
                    .try_into()
                    .map_err(|_| "an edge group is blank")?,
            );
            Ok(result)
        }
        EdgeSelection::Historical {
            state,
            edges,
            native: text,
        } => historical_edges(state, edges.iter(), &[], text.as_str(), inputs),
        EdgeSelection::HistoricalPartial {
            state,
            edges,
            unresolved,
            native: text,
        } => historical_edges(
            state,
            edges.iter(),
            unresolved.as_slice(),
            text.as_str(),
            inputs,
        ),

        _ => Err("the edge selection has no persistent name"),
    }
}

pub(super) fn bodies(value: &BodySelection) -> Projected<OrderedSelection> {
    match value {
        BodySelection::Native(text) => native(text),
        BodySelection::Resolved {
            bodies,
            native: text,
        } => {
            let mut pick = named(text, None)?;
            pick.resolved = Some(
                bodies
                    .iter()
                    .cloned()
                    .map(cadmpeg_ir::design::refs::EntityRef::Body)
                    .collect(),
            );
            selection(vec![pick])
        }
        BodySelection::Generated {
            bodies,
            native: text,
        } => {
            let mut result = selection(
                bodies
                    .iter()
                    .map(|body| {
                        named(
                            body.local_id.as_str(),
                            Some(NativeOwner::Feature(body.feature.clone())),
                        )
                    })
                    .collect::<Projected<_>>()?,
            )?;
            result.native = Some(
                vec![text.as_str().to_owned()]
                    .try_into()
                    .map_err(|_| "a body group is blank")?,
            );
            Ok(result)
        }
        BodySelection::Historical {
            bodies,
            native: text,
            ..
        } => named_group(
            text.as_str(),
            bodies.iter().map(cadmpeg_ir::ids::HistoricalBodyId::as_str),
        ),
        BodySelection::Local {
            bodies,
            native: text,
        } => named_group(text.as_str(), bodies.iter().map(String::as_str)),
        BodySelection::NativeSet(names) => selection(
            names
                .iter()
                .map(|name| named(name, None))
                .collect::<Projected<_>>()?,
        ),
        BodySelection::ResolvedSet { members } => selection(
            members
                .iter()
                .map(|member| {
                    let (body, text) = member.clone().into_parts();
                    let mut pick = named(&text, None)?;
                    pick.resolved = Some(vec![cadmpeg_ir::design::refs::EntityRef::Body(body)]);
                    Ok(pick)
                })
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

pub(super) fn path(value: &cadmpeg_ir::features::profiles::PathRef) -> Projected<OrderedSelection> {
    match value {
        cadmpeg_ir::features::profiles::PathRef::Native(text) => native(text),
        cadmpeg_ir::features::profiles::PathRef::Sketch(id) => {
            selection(vec![picked(PickTarget::Sketch(id.clone()))])
        }
        cadmpeg_ir::features::profiles::PathRef::SketchCurves { curves, .. } => selection(
            curves
                .iter()
                .map(|id| picked(PickTarget::SketchEntity(id.clone())))
                .collect(),
        ),
        cadmpeg_ir::features::profiles::PathRef::SpatialSketchCurves { curves, .. } => selection(
            curves
                .iter()
                .map(|id| picked(PickTarget::SpatialSketchEntity(id.clone())))
                .collect(),
        ),
        cadmpeg_ir::features::profiles::PathRef::SpatialSketchSelection { sketch, selections } => {
            selection(
                selections
                    .iter()
                    .map(|text| named(text, Some(NativeOwner::SpatialSketch(sketch.clone()))))
                    .collect::<Projected<_>>()?,
            )
        }
        cadmpeg_ir::features::profiles::PathRef::HistoricalEdges { native: text, .. } => {
            native(text.as_str())
        }
        _ => Err("the path selection has unresolved members"),
    }
}

pub(super) fn profile(
    value: &cadmpeg_ir::features::profiles::ProfileRef,
    sketches: &[Sketch],
) -> Projected<OrderedSelection> {
    match value {
        cadmpeg_ir::features::profiles::ProfileRef::Planar(value) => planar(value, sketches),
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
            cadmpeg_ir::features::profiles::SketchProfileRegion::Loops { loops } => loop_region(sketch, loops.outer(), loops.holes()),
            cadmpeg_ir::features::profiles::SketchProfileRegion::Trimmed { .. } => Err("a trimmed profile stores parameter intervals outside the v11 boundary contract"),
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
