// SPDX-License-Identifier: Apache-2.0
//! The sketch entity that generates a side face of an extrusion, and the
//! blend picks whose path entries it settles (`docs/formats/sldprt.md` §5).
//!
//! The local identifier of a side face joins to its profile line through the
//! `+80` and `+84` words of the profile's 92-byte line records. A record
//! whose `+80` is zero stores in `+84` the identifier of the side face that
//! the next record sweeps; the first record sweeps the one side identifier
//! that no `+84` names. The decoder states nothing for a face the rule does
//! not settle.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometryDefinition, SketchId};

use crate::records::{
    FeatureHistory, FeatureInputEdgeSelection, FeatureInputGeneratedSurfaceIdentity,
    FeatureInputLane, HistoryFeatures, LineSides, SketchInputEntity, SketchInputKind,
};

/// Distance within which two profile endpoints are one corner, in the units
/// of the sketch.
const EPS_SAME_CORNER: f64 = 1.0e-6;

/// Classes of the `*SurfIdRep_c` family whose first object of a lane is
/// declared inline: the class declaration is followed by the identity's
/// two zero bytes and its first component signature.
const DECLARATION_PREFIX: u64 = 6;
const INLINE_IDENTITY_GAP: u64 = 2;

const SIDE_CLASSES: [&str; 2] = ["moFromSktEntSurfIdRep_c", "moFromSktEnt3IntSurfIdRep_c"];
const END_CLASSES: [&str; 2] = ["moEndFaceSurfIdRep_c", "moEndFace3IntSurfIdRep_c"];

/// The instance cells that mark the two classes of generated surface of a
/// lane, and the offset of the side-face identity declared inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceClasses {
    /// The instance cell of a side-face identity.
    pub(crate) side: u16,
    /// The instance cell of an end-face identity.
    pub(crate) end: u16,
    side_inline: u64,
}

/// What a path entry of an extrusion's blend names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathFace {
    /// A side face, by its local identifier.
    Side(u32),
    /// An end face.
    End,
}

impl SurfaceClasses {
    /// The side-face and end-face instance cells of `lane`.
    ///
    /// Each class of the surface-identity family whose first identity the
    /// lane declares inline takes, in stream order, the next instance cell in
    /// ascending order among the cells the lane's identities carry, where
    /// that cell is first used after the inline identity. A lane whose
    /// classes and cells do not pair one to one, or that holds no side-face or
    /// no end-face class, states none.
    pub(crate) fn of(lane: &FeatureInputLane) -> Option<Self> {
        let mut inline = Vec::new();
        for class in &lane.classes {
            if !class.name.ends_with("SurfIdRep_c") {
                continue;
            }
            let length = u64::try_from(class.name.len()).ok()?;
            let first = class
                .offset
                .checked_add(DECLARATION_PREFIX)?
                .checked_add(length)?
                .checked_add(INLINE_IDENTITY_GAP)?;
            if lane
                .generated_surface_identities
                .iter()
                .any(|identity| identity.offset == first && last_instance(identity).is_none())
            {
                inline.push((class.offset, first, class.name.as_str()));
            }
        }
        inline.sort_unstable();
        let mut cells = BTreeSet::new();
        let mut first_use = HashMap::new();
        for identity in &lane.generated_surface_identities {
            if let Some(cell) = last_instance(identity) {
                cells.insert(cell);
                first_use
                    .entry(cell)
                    .and_modify(|first: &mut u64| *first = (*first).min(identity.offset))
                    .or_insert(identity.offset);
            }
        }
        if inline.len() != cells.len() {
            return None;
        }
        let mut side = None;
        let mut end = None;
        for ((_, inline_at, name), cell) in inline.into_iter().zip(cells) {
            if first_use.get(&cell).is_none_or(|first| *first <= inline_at) {
                return None;
            }
            if SIDE_CLASSES.contains(&name) {
                side = Some((cell, inline_at));
            } else if END_CLASSES.contains(&name) {
                end = Some((cell, inline_at));
            }
        }
        let ((side, side_inline), (end, _)) = (side?, end?);
        Some(Self {
            side,
            end,
            side_inline,
        })
    }

    /// Whether `identity` is a side-face identity of a lane with these
    /// classes.
    fn is_side(&self, identity: &FeatureInputGeneratedSurfaceIdentity) -> bool {
        match last_instance(identity) {
            Some(cell) => cell == self.side,
            None => identity.offset == self.side_inline,
        }
    }

    /// What the path entry with instance cell `instance` and local identifier
    /// `local` names, or `None` where its class is not one of the two.
    pub(crate) fn face(&self, instance: Option<u16>, local: Option<u32>) -> Option<PathFace> {
        let cell = instance?;
        if cell == self.side {
            local.map(PathFace::Side)
        } else if cell == self.end {
            Some(PathFace::End)
        } else {
            None
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(side: u16, end: u16) -> Self {
        Self {
            side,
            end,
            side_inline: 0,
        }
    }
}

fn last_instance(identity: &FeatureInputGeneratedSurfaceIdentity) -> Option<u16> {
    identity
        .components
        .last()
        .and_then(|component| component.instance)
}

/// One 92-byte line record of an extruded polygon.
#[derive(Debug, Clone)]
struct Record {
    sides: LineSides,
    /// The two ends of the record's line where the decoder reads its
    /// geometry, with the entity that carries it.
    read: Option<([f64; 2], [f64; 2], SketchEntityId)>,
}

/// The local identifier of the side face each record sweeps, where the rule
/// settles it. `records` is in stream order, `sides` the side identifiers of
/// the extrusion.
fn join(records: &[Record], sides: &BTreeSet<u32>) -> Vec<Option<u32>> {
    let none = vec![None; records.len()];
    let Some((last, others)) = records.split_last() else {
        return none;
    };
    if last.sides.state == 0 && last.sides.next_side != 0 {
        return none;
    }
    let mut faces = vec![None; records.len()];
    let mut named = BTreeSet::new();
    for (index, record) in others.iter().enumerate() {
        if record.sides.state != 0 {
            continue;
        }
        let face = record.sides.next_side;
        if !sides.contains(&face) || !named.insert(face) {
            return none;
        }
        faces[index + 1] = Some(face);
    }
    let unnamed = faces.iter().filter(|face| face.is_none()).count();
    let mut remaining = sides.difference(&named);
    if let (1, Some(&face), None) = (unnamed, remaining.next(), remaining.next()) {
        faces[0] = Some(face);
    }
    faces
}

/// Whether the read lines of consecutive records share a corner, the last
/// record with the first.
fn contour_closes(records: &[Record]) -> bool {
    let same = |a: [f64; 2], b: [f64; 2]| {
        (a[0] - b[0]).abs() <= EPS_SAME_CORNER && (a[1] - b[1]).abs() <= EPS_SAME_CORNER
    };
    let shares = |left: &Record, right: &Record| match (&left.read, &right.read) {
        (Some((a, b, _)), Some((c, d, _))) => {
            same(*a, *c) || same(*a, *d) || same(*b, *c) || same(*b, *d)
        }
        _ => true,
    };
    records.len() >= 3
        && records
            .iter()
            .zip(records.iter().cycle().skip(1))
            .all(|(left, right)| shares(left, right))
}

/// The faces of `records` where they form a closed polygon, else none.
fn polygon_faces(records: &[Record], sides: &BTreeSet<u32>) -> Vec<Option<u32>> {
    if contour_closes(records) {
        join(records, sides)
    } else {
        vec![None; records.len()]
    }
}

/// The sketch entity that generates each side face the join settles, by the
/// source identifier of the extruding feature and the face's local
/// identifier.
#[derive(Debug, Default)]
pub(crate) struct SideGenerators {
    by_face: HashMap<(u32, u32), Vec<SketchEntityId>>,
}

impl SideGenerators {
    /// The generators of the extrusions of `model`.
    pub(crate) fn derive(
        model: &Model,
        histories: &[FeatureHistory],
        lanes: &[FeatureInputLane],
    ) -> Self {
        let history = HistoryFeatures::new(histories);
        let mut markers = HashMap::<&str, (&FeatureInputLane, &SketchInputEntity)>::new();
        for lane in lanes {
            for marker in &lane.sketch_entities {
                markers.insert(marker.id(), (lane, marker));
            }
        }
        let mut entities = HashMap::<&SketchId, Vec<&SketchEntity>>::new();
        for entity in &model.sketch_entities {
            entities.entry(&entity.sketch).or_default().push(entity);
        }
        let mut out = Self::default();
        for feature in &model.features {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
                feature.evaluation.definition()
            else {
                continue;
            };
            let ProfileRef::Planar(
                PlanarProfileRef::Sketch(sketch)
                | PlanarProfileRef::SketchProfiles { sketch, .. }
                | PlanarProfileRef::SketchRegions { sketch, .. }
                | PlanarProfileRef::SketchEntities { sketch, .. },
            ) = profile
            else {
                continue;
            };
            let Some(source) = feature
                .native_ref
                .as_deref()
                .and_then(|record| history.get(record))
                .and_then(crate::records::Feature::source_value)
            else {
                continue;
            };
            let Some(entities) = entities.get(sketch) else {
                continue;
            };
            out.extrusion(source, entities, &markers);
        }
        out
    }

    fn extrusion(
        &mut self,
        source: u32,
        entities: &[&SketchEntity],
        markers: &HashMap<&str, (&FeatureInputLane, &SketchInputEntity)>,
    ) {
        let Some((lane, first)) = entities
            .iter()
            .find_map(|entity| markers.get(entity.native_ref.as_deref()?))
        else {
            return;
        };
        let Some(owner) = first.feature_ref.as_deref() else {
            return;
        };
        let Some(classes) = SurfaceClasses::of(lane) else {
            return;
        };
        let sides = lane
            .generated_surface_identities
            .iter()
            .filter(|identity| {
                identity.feature_source_id.value() == source && classes.is_side(identity)
            })
            .map(|identity| identity.local_identity)
            .collect::<BTreeSet<_>>();
        let by_marker = entities
            .iter()
            .filter_map(|entity| Some((entity.native_ref.as_deref()?, *entity)))
            .collect::<HashMap<_, _>>();
        let mut stream = lane
            .sketch_entities
            .iter()
            .filter(|marker| {
                marker.feature_ref.as_deref() == Some(owner)
                    && marker.kind() == SketchInputKind::LineOrCircle
                    && marker.line_sides().is_some()
            })
            .collect::<Vec<_>>();
        stream.sort_unstable_by_key(|marker| marker.offset());
        let record_ids = stream.iter().map(|marker| marker.id()).collect::<Vec<_>>();
        for entity in entities.iter().filter(|entity| !entity.construction) {
            let admitted = match entity.geometry.definition() {
                SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::Point { .. } => {
                    true
                }
                SketchGeometryDefinition::Native { .. } => entity
                    .native_ref
                    .as_deref()
                    .is_some_and(|marker| record_ids.contains(&marker)),
                _ => false,
            };
            if !admitted {
                return;
            }
        }
        let records = stream
            .iter()
            .filter_map(|marker| {
                let sides = marker.line_sides()?;
                let entity = by_marker.get(marker.id()).copied();
                if entity.is_some_and(|entity| entity.construction) {
                    return None;
                }
                let read = entity.and_then(|entity| {
                    let SketchGeometryDefinition::Line { start, end } =
                        entity.geometry.definition()
                    else {
                        return None;
                    };
                    let (start, end) = (start.get(), end.get());
                    Some(([start.u, start.v], [end.u, end.v], entity.id().clone()))
                });
                Some(Record { sides, read })
            })
            .collect::<Vec<_>>();
        for (record, face) in records.iter().zip(polygon_faces(&records, &sides)) {
            if let (Some(face), Some((_, _, entity))) = (face, &record.read) {
                self.by_face
                    .entry((source, face))
                    .or_default()
                    .push(entity.clone());
            }
        }
    }

    /// The generator of each face that has one, whose entity satisfies
    /// `stated`, by source identifier and local identifier.
    pub(crate) fn into_stated(
        self,
        stated: impl Fn(&SketchEntityId) -> bool,
    ) -> HashMap<(u32, u32), SketchEntityId> {
        self.by_face
            .into_iter()
            .filter_map(|(key, mut entities)| match entities.as_mut_slice() {
                [one] if stated(one) => Some((key, one.clone())),
                _ => None,
            })
            .collect()
    }

    /// The one sketch entity that generates the side face with local
    /// identifier `face` of the feature with source identifier `source`.
    pub(crate) fn generator(&self, source: u32, face: u32) -> Option<&SketchEntityId> {
        match self.by_face.get(&(source, face))?.as_slice() {
            [one] => Some(one),
            _ => None,
        }
    }

    /// Whether every side face that the path of `selection` names has a
    /// generator, where the path lists its four entries and each names a
    /// side face or an end face of `classes`. `source` is the source
    /// identifier of the feature that holds the terminal component.
    pub(crate) fn settle(
        &self,
        selection: &FeatureInputEdgeSelection,
        classes: &SurfaceClasses,
        source: u32,
    ) -> bool {
        selection.references.is_empty()
            && selection.components.len() == 4
            && selection.components.iter().all(|entry| {
                match classes.face(entry.instance, entry.local_id) {
                    Some(PathFace::Side(face)) => self.generator(source, face).is_some(),
                    Some(PathFace::End) => true,
                    None => false,
                }
            })
    }

    #[cfg(test)]
    pub(crate) fn with_face(mut self, source: u32, face: u32, entity: SketchEntityId) -> Self {
        self.by_face.entry((source, face)).or_default().push(entity);
        self
    }
}

#[cfg(test)]
mod tests;
