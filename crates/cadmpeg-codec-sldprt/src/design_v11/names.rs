// SPDX-License-Identifier: Apache-2.0
//! The name tables of a part's states (§5.7).
//!
//! A Parasolid partition stores a persistent id on each face (`FACE_ID_2001`)
//! and on each body (`ATOM_ID_2001`); the whole stored list is the entity's
//! native name. A face's list is a chain of hops, each `[class, producer,
//! time stamp, 0, class fields...]`, where the class is a
//! `mo*SurfIdRep_c` identity class, the producer the history feature that
//! made the face, and a wrapping class is followed by the hop it wraps
//! (`docs/formats/sldprt.md` §5). The partition reader keeps lists of four to
//! seven values, which are single hops, so a face of a pattern, mirror, copy
//! or shell, whose list wraps a hop, has no name entry; nor has a face of
//! merged faces whose lists disagree. A face whose producer is a feature of
//! the timeline is named by that feature, its role and index read from the
//! class: a side swept from a sketch entity, a start or end cap with its
//! region as index, a fillet or chamfer face with its ordinal as index, a
//! feature-scoped set that names no role; every other class is a role whose
//! meaning is not established, `Other("sldprt:<class>")`. The producer id and
//! time stamp are allocations and stay in the native name. The sketch-entity
//! id also stays there, and the name states no generator: that it is the
//! sketch segment's id is not established. A face whose producer is no
//! feature of the document, one the state does not depend on, or one whose
//! source id several history records carry, and a body, whose list is one
//! identity that names no feature, keep their native name and have no
//! structured name.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::names::{
    CodecId, GenerationRole, NameEntry, NameKind, NativeName, NativeNameValue, TopoName,
};
use cadmpeg_ir::design::refs::EntityRef;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{BodyId, FaceId};

use crate::brep::attrib::NativeIdentity;
use crate::records::FeatureHistory;

/// The `mo*SurfIdRep_c` classes whose role the hop states, by class code.
mod class {
    /// `moFromSktEntSurfIdRep_c`: a face swept from a sketch entity, whose
    /// one field is the entity's id.
    pub(super) const FROM_SKETCH_ENTITY: u32 = 74;
    /// `moFromSktEnt3IntSurfIdRep_c`: the same, with two fields more.
    pub(super) const FROM_SKETCH_ENTITY_3INT: u32 = 83;
    /// `moEndFaceSurfIdRep_c`: a cap, by its end (0 start, 1 end) and region.
    pub(super) const END_FACE: u32 = 40;
    /// `moEndFace3IntSurfIdRep_c`: the same, with a zero field after them;
    /// what a nonzero third field states is not established.
    pub(super) const END_FACE_3INT: u32 = 81;
    /// `moFilletSurfIdRep_c`: a fillet or chamfer face, by its ordinal.
    pub(super) const FILLET: u32 = 72;
    /// `moSurfaceIdRep_c`: the four-word set of a feature's faces.
    pub(super) const SURFACE: u32 = 36;
}

/// The persistent ids the Parasolid partitions store on decoded faces and
/// bodies.
#[derive(Clone, Copy)]
pub(crate) struct Persistent<'a> {
    /// Each face's whole `FACE_ID_2001` list.
    pub(crate) faces: &'a [NativeIdentity<FaceId>],
    /// Each body's whole `ATOM_ID_2001` list.
    pub(crate) bodies: &'a [NativeIdentity<BodyId>],
}

/// What the history record of one native source id is.
#[derive(Clone, Copy)]
enum Producer<'a> {
    /// A member of the timeline.
    Feature(&'a FeatureId),
    /// A tree node that is no member: a folder or another node with no
    /// design meaning.
    Node,
    /// Several history records carry the id.
    Ambiguous,
}

/// The persistent ids of a part's entities and the timeline features their
/// producers join.
pub(super) struct Names<'a> {
    faces: HashMap<&'a str, &'a [u32]>,
    bodies: HashMap<&'a str, &'a [u32]>,
    producers: HashMap<u32, Producer<'a>>,
}

impl<'a> Names<'a> {
    /// `features` maps each timeline member's native record to its feature.
    pub(super) fn new(
        persistent: Persistent<'a>,
        histories: &[FeatureHistory],
        features: &'a HashMap<String, FeatureId>,
    ) -> Self {
        let mut producers = HashMap::new();
        for record in histories.iter().flat_map(|history| &history.features) {
            let Some(source) = record.source_value() else {
                continue;
            };
            let producer = features
                .get(&record.id)
                .map_or(Producer::Node, Producer::Feature);
            producers
                .entry(source)
                .and_modify(|joined| *joined = Producer::Ambiguous)
                .or_insert(producer);
        }
        Self {
            faces: persistent
                .faces
                .iter()
                .map(|identity| (identity.target.as_str(), identity.values.as_slice()))
                .collect(),
            bodies: persistent
                .bodies
                .iter()
                .map(|identity| (identity.target.as_str(), identity.values.as_slice()))
                .collect(),
            producers,
        }
    }

    /// The name entries of `bodies` and of the faces of `topology` that
    /// belong to them. A face whose producer is a feature `depends` does not
    /// admit, one the state does not depend on (§5.7, Order), or whose source
    /// id several history records carry, keeps its native name and has no
    /// structured name.
    pub(super) fn entries(
        &self,
        topology: &Topology,
        bodies: &[BodyId],
        depends: impl Fn(&FeatureId) -> bool,
    ) -> BTreeMap<EntityRef, NameEntry> {
        let mut table = BTreeMap::new();
        for body in bodies {
            if let Some(values) = self.bodies.get(body.as_str()) {
                table.insert(
                    EntityRef::Body(body.clone()),
                    NameEntry {
                        name: None,
                        native: Some(native(values)),
                    },
                );
            }
        }
        for face in faces_of(topology, bodies) {
            let Some(values) = self.faces.get(face.as_str()) else {
                continue;
            };
            let &[code, producer, _, 0, ref fields @ ..] = *values else {
                continue;
            };
            let name = match self.producers.get(&producer).copied() {
                Some(Producer::Feature(feature)) if depends(feature) => {
                    Some(face_name(feature, code, fields))
                }
                _ => None,
            };
            table.insert(
                EntityRef::Face(face.clone()),
                NameEntry {
                    name,
                    native: Some(native(values)),
                },
            );
        }
        table
    }
}

/// The name of a face that `feature` produced, from the class and fields of
/// its one hop (§5.7).
fn face_name(feature: &FeatureId, code: u32, fields: &[u32]) -> TopoName {
    let generated = |role: Option<GenerationRole>, index: Option<u32>| TopoName::Generated {
        kind: NameKind::Face,
        feature: feature.clone(),
        role,
        generator: None,
        index,
    };
    match (code, fields) {
        (class::FROM_SKETCH_ENTITY, [_]) | (class::FROM_SKETCH_ENTITY_3INT, [_, _, _]) => {
            generated(Some(GenerationRole::Side), None)
        }
        (class::END_FACE, &[0, region]) | (class::END_FACE_3INT, &[0, region, 0]) => {
            generated(Some(GenerationRole::Start), Some(region))
        }
        (class::END_FACE, &[1, region]) | (class::END_FACE_3INT, &[1, region, 0]) => {
            generated(Some(GenerationRole::End), Some(region))
        }
        (class::FILLET, &[ordinal]) => TopoName::Blend {
            feature: feature.clone(),
            between: Vec::new(),
            index: Some(ordinal),
        },
        (class::SURFACE, []) => generated(None, None),
        _ => generated(
            NonBlankString::new(format!("sldprt:{code}")).map(GenerationRole::Other),
            None,
        ),
    }
}

fn native(values: &[u32]) -> NativeName {
    NativeName {
        codec: CodecId::from(cadmpeg_core::nonblank_literal!("sldprt")),
        value: NativeNameValue::Integers(values.iter().copied().map(i64::from).collect()),
    }
}

/// The faces of `topology` whose shell's region belongs to one of `bodies`.
fn faces_of<'t>(topology: &'t Topology, bodies: &[BodyId]) -> Vec<&'t FaceId> {
    let bodies = bodies.iter().collect::<HashSet<_>>();
    let regions = topology
        .regions
        .iter()
        .filter(|region| bodies.contains(&region.body))
        .map(|region| &region.id)
        .collect::<HashSet<_>>();
    let shells = topology
        .shells
        .iter()
        .filter(|shell| regions.contains(&shell.region))
        .map(|shell| &shell.id)
        .collect::<HashSet<_>>();
    topology
        .faces
        .iter()
        .filter(|face| shells.contains(&face.shell))
        .map(|face| &face.id)
        .collect()
}
