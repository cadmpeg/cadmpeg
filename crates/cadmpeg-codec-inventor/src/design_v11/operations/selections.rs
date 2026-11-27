// SPDX-License-Identifier: Apache-2.0
//! Input selections for feature operands.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::selection::{
    Anchor, BoundaryUse, MemberOrder, Members, NativeOwner, OrderedSelection, PickTarget, Picked,
    Picks, Selection, SetSelection, SketchRegion,
};
use cadmpeg_ir::features::evaluation::FeatureInputTopology;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::features::selections::{BodySelection, EdgeSelection, FaceSelection};
use cadmpeg_ir::sketches::{Sketch, SketchEntity};

use super::Projected;

pub(super) mod trimmed;

/// The planar sketches of the feature layer and their entities, and the
/// arrangements built from them so far. Without a cache, each use builds its
/// own.
#[derive(Clone, Copy, Default)]
pub(super) struct SketchData<'a> {
    pub(super) sketches: &'a [Sketch],
    pub(super) entities: &'a [SketchEntity],
    pub(super) arrangements: Option<&'a trimmed::Arrangements>,
}

impl SketchData<'_> {
    fn arrangement(
        self,
        sketch: &cadmpeg_ir::sketches::SketchId,
    ) -> Projected<std::rc::Rc<trimmed::Arrangement>> {
        match self.arrangements {
            Some(cache) => cache.get(self.entities, sketch),
            None => trimmed::Arrangement::new(self.entities, sketch).map(std::rc::Rc::new),
        }
    }
}

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
        codec: cadmpeg_ir::codec_id!("inventor"),
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
        FaceSelection::Historical {
            faces,
            native: text,
            ..
        } => source_group(
            text.as_str(),
            faces.iter().map(cadmpeg_ir::ids::HistoricalFaceId::as_str),
            &[],
        ),
        FaceSelection::HistoricalPartial {
            faces,
            unresolved,
            native: text,
            ..
        } => source_group(
            text.as_str(),
            faces.iter().map(cadmpeg_ir::ids::HistoricalFaceId::as_str),
            unresolved,
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

/// The native name of a historical face or edge: the source selector index
/// that the input state keys it by within the owning feature's segment.
fn source_pick(id: &str) -> Projected<Picked> {
    let selector = id
        .rsplit_once('-')
        .and_then(|(_, selector)| selector.parse::<u32>().ok())
        .ok_or("a historical selection has no source selector")?;
    let mut pick = picked(PickTarget::Native { within: None });
    pick.native = Some(NativeName {
        codec: cadmpeg_ir::codec_id!("inventor"),
        value: NativeNameValue::Integers(vec![i64::from(selector)]),
    });
    Ok(pick)
}

fn source_group<'a, O: MemberOrder>(
    group: &str,
    ids: impl Iterator<Item = &'a str>,
    unresolved: &[String],
) -> Projected<Selection<O>> {
    let mut picks = ids.map(source_pick).collect::<Projected<Vec<_>>>()?;
    for name in unresolved {
        picks.push(named(name, None)?);
    }
    let mut value = selection(picks)?;
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
    group: &str,
    inputs: &[FeatureInputTopology],
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
        let mut pick = source_pick(edge.as_str())?;
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
    inputs: &[FeatureInputTopology],
) -> Projected<OrderedSelection> {
    match value {
        EdgeSelection::Native(text) => native(text),
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
        } => historical_edges(state, edges.iter(), text.as_str(), inputs),

        _ => Err("the edge selection has no persistent name"),
    }
}

/// The support face of an oriented chamfer: the one face that its input state
/// holds beside the selected edge.
pub(super) fn support_face(
    edges: &EdgeSelection,
    inputs: &[FeatureInputTopology],
) -> Projected<SetSelection> {
    let EdgeSelection::Historical { state, .. } = edges else {
        return Err("the chamfer support side has no v11 reference selection");
    };
    let mut states = inputs.iter().filter(|input| &input.id == state);
    let input = states.next().ok_or("the selected edge state is absent")?;
    if states.next().is_some() {
        return Err("the selected edge state is ambiguous");
    }
    let [face] = input.faces.as_slice() else {
        return Err("the chamfer support side has no v11 reference selection");
    };
    selection(vec![source_pick(face.as_str())?])
}

pub(super) fn bodies(value: &BodySelection) -> Projected<OrderedSelection> {
    match value {
        BodySelection::Native(text) => native(text),
        // The bodies are those of the final geometry, not of the stored
        // anchored state, so the pick keeps only its native name.
        BodySelection::Resolved { native: text, .. } => selection(vec![named(text, None)?]),
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
    sketches: SketchData<'_>,
) -> Projected<OrderedSelection> {
    match value {
        cadmpeg_ir::features::profiles::ProfileRef::Planar(value) => planar(value, sketches),
        _ => Err("the spatial profile has no v11 sketch"),
    }
}

pub(super) fn planar(
    value: &PlanarProfileRef,
    data: SketchData<'_>,
) -> Projected<OrderedSelection> {
    let sketches = data.sketches;
    let loop_region =
        |sketch: &cadmpeg_ir::sketches::SketchId, outer, holes: &[u32]| -> Projected<Picked> {
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
        PlanarProfileRef::SketchProfiles { .. } => {
            Err("the selected loops have no explicit region nesting")
        }
        PlanarProfileRef::SketchRegions { sketch, regions } => {
            let mut arrangement: Option<std::rc::Rc<trimmed::Arrangement>> = None;
            let mut picks = Vec::new();
            for region in regions.iter() {
                picks.push(match region {
                    cadmpeg_ir::features::profiles::SketchProfileRegion::Loops { loops } => {
                        loop_region(sketch, loops.outer(), loops.holes())?
                    }
                    cadmpeg_ir::features::profiles::SketchProfileRegion::Trimmed {
                        outer_boundary,
                        hole_boundaries,
                    } => {
                        let arrangement = match &arrangement {
                            Some(built) => built,
                            None => arrangement.insert(data.arrangement(sketch)?),
                        };
                        picked(PickTarget::SketchRegion {
                            sketch: sketch.clone(),
                            region: arrangement.region(outer_boundary, hole_boundaries)?,
                        })
                    }
                });
            }
            selection(picks)
        }
        PlanarProfileRef::SketchEntities { entities, .. } => selection(
            entities
                .iter()
                .map(|id| picked(PickTarget::SketchEntity(id.clone())))
                .collect(),
        ),
        PlanarProfileRef::SketchSelection { sketch, selections } => selection(
            selections
                .iter()
                .map(|text| named(text, Some(NativeOwner::Sketch(sketch.clone()))))
                .collect::<Projected<_>>()?,
        ),
        PlanarProfileRef::HistoricalFaces { native: names, .. } => Ok(Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: Some(names.clone()),
        }),
        PlanarProfileRef::Feature(id) => {
            selection(vec![picked(PickTarget::FeatureResult(id.clone()))])
        }
        PlanarProfileRef::Generated { native: text, .. } => native(text.as_str()),
        _ => Err("the profile selection has unresolved members"),
    }
}

#[cfg(test)]
mod tests;
