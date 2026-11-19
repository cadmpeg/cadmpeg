// SPDX-License-Identifier: Apache-2.0
//! The name tables of a part's states (§5.7).
//!
//! A Parasolid partition stores a persistent id on each face (`FACE_ID_2001`)
//! and on each body (`ATOM_ID_2001`); the whole stored list is the entity's
//! native name. A face's list names the history feature that produced it
//! (value 1), that feature's time stamp (value 2) and a face identity local to
//! the feature (value 4), with optional path fields after it. A face whose
//! producer is a feature of the timeline is `Generated` by that feature. Which
//! of the list's other values is a role and which a discriminator is not
//! established, so the name states neither (interim, gap 20261119-SW2-2). A
//! face whose producer is no feature of the document, and a body, whose list
//! is one identity that names no feature, have no structured name.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::design::names::{
    CodecId, NameEntry, NameKind, NativeName, NativeNameValue, TopoName,
};
use cadmpeg_ir::design::refs::EntityRef;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{BodyId, FaceId};

use crate::brep::attrib::NativeIdentity;
use crate::records::FeatureHistory;

/// The value of a `FACE_ID_2001` list that names the producing feature.
const PRODUCER: usize = 1;

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
    /// id several history records carry, has no entry.
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
            let producer = values
                .get(PRODUCER)
                .map(|source| self.producers.get(source).copied());
            let name = match producer {
                Some(Some(Producer::Feature(feature))) if depends(feature) => {
                    Some(TopoName::Generated {
                        kind: NameKind::Face,
                        feature: feature.clone(),
                        role: None,
                        generator: None,
                        index: None,
                    })
                }
                Some(None | Some(Producer::Node)) => None,
                Some(Some(Producer::Feature(_) | Producer::Ambiguous)) | None => continue,
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
