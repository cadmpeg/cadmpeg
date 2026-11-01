// SPDX-License-Identifier: Apache-2.0
//! Borrowed native metadata indexes for source-less generation.

use std::collections::HashMap;

use crate::native::F3dNative;
use cadmpeg_asm::brep::records::{
    EdgeContinuity, EdgeOwnership, FaceNativeKey, FaceSidedness, TolerantCoedgeParameters,
    TolerantEdgeTail, TolerantVertexTail, TransformHints, VertexOwnership, WireTopology,
};

pub(super) struct NativeGenerationIndex<'a> {
    pub(super) edge_continuities: HashMap<&'a str, &'a EdgeContinuity>,
    pub(super) edge_ownerships: &'a [EdgeOwnership],
    pub(super) face_sidedness: HashMap<&'a str, &'a FaceSidedness>,
    pub(super) face_native_keys: HashMap<&'a str, &'a FaceNativeKey>,
    pub(super) tolerant_coedges: HashMap<&'a str, &'a TolerantCoedgeParameters>,
    pub(super) tolerant_edges: HashMap<&'a str, &'a TolerantEdgeTail>,
    pub(super) tolerant_vertices: HashMap<&'a str, &'a TolerantVertexTail>,
    pub(super) transform_hints: HashMap<&'a str, &'a TransformHints>,
    pub(super) vertex_ownerships: HashMap<&'a str, &'a VertexOwnership>,
    pub(super) wires_by_shell: HashMap<&'a str, Vec<&'a WireTopology>>,
}

impl<'a> NativeGenerationIndex<'a> {
    pub(super) fn new(target: &'a cadmpeg_ir::CadIr, native: &'a F3dNative) -> Self {
        let mut wires_by_shell: HashMap<_, Vec<_>> = HashMap::new();
        // Canonical native row id to the neutral entity linked from it.
        let mut row_owner: HashMap<_, _> = HashMap::new();
        for body in &target.model.bodies {
            if let Some(native_ref) = body.native_ref.as_deref() {
                row_owner.entry(native_ref).or_insert(body.id.as_str());
            }
        }
        for face in &target.model.faces {
            if let Some(native_ref) = face.native_ref.as_deref() {
                row_owner.entry(native_ref).or_insert(face.id.as_str());
            }
        }
        for edge in &target.model.edges {
            if let Some(native_ref) = edge.native_ref.as_deref() {
                row_owner.entry(native_ref).or_insert(edge.id.as_str());
            }
        }
        for vertex in &target.model.vertices {
            if let Some(native_ref) = vertex.native_ref.as_deref() {
                row_owner.entry(native_ref).or_insert(vertex.id.as_str());
            }
        }
        for wire in &native.wire_topologies {
            let members = wire
                .members
                .edges()
                .iter()
                .map(|member| member.as_str())
                .chain(wire.members.free_vertex().map(|vertex| vertex.as_str()))
                .filter_map(|member| row_owner.get(member).copied())
                .collect::<Vec<_>>();
            for shell in &target.model.shells {
                let owns = members.iter().any(|neutral| {
                    shell
                        .wire_edges()
                        .iter()
                        .any(|edge| edge.as_str() == *neutral)
                        || shell
                            .free_vertices()
                            .iter()
                            .any(|vertex| vertex.as_str() == *neutral)
                });
                if owns {
                    wires_by_shell
                        .entry(shell.id.as_str())
                        .or_default()
                        .push(wire);
                }
            }
        }
        let face_native_keys = target
            .model
            .faces
            .iter()
            .filter_map(|face| {
                native
                    .face_native_keys
                    .iter()
                    .find(|record| {
                        record.owns_native_ref(face.native_ref.as_deref(), &native.face_sidedness)
                    })
                    .map(|record| (face.id.as_str(), record))
            })
            .collect();
        Self {
            edge_continuities: native
                .edge_continuities
                .iter()
                .filter_map(|row| {
                    row_owner
                        .get(row.owner.as_str())
                        .map(|edge| (*edge, row))
                })
                .collect(),
            edge_ownerships: &native.edge_ownerships,
            face_sidedness: target
                .model
                .faces
                .iter()
                .filter_map(|face| {
                    native
                        .face_sidedness
                        .iter()
                        .find(|row| {
                            row.owns_native_ref(
                                face.native_ref.as_deref(),
                                &native.face_native_keys,
                            )
                        })
                        .map(|row| (face.id.as_str(), row))
                })
                .collect(),
            face_native_keys,
            tolerant_coedges: native
                .tolerant_coedge_parameters
                .iter()
                .filter_map(|row| {
                    cadmpeg_asm::brep::records::native_entity_id(
                        &row.source_namespace,
                        row.record_index,
                    )
                    .and_then(|entity| {
                        target
                            .model
                            .coedges
                            .iter()
                            .find(|coedge| coedge.id.as_str() == entity)
                            .map(|coedge| (coedge.id.as_str(), row))
                    })
                })
                .collect(),
            tolerant_edges: native
                .tolerant_edge_tails
                .iter()
                .filter_map(|row| {
                    row_owner
                        .get(row.owner.as_str())
                        .map(|edge| (*edge, row))
                })
                .collect(),
            tolerant_vertices: native
                .tolerant_vertex_tails
                .iter()
                .filter_map(|row| {
                    row_owner
                        .get(row.owner.as_str())
                        .map(|vertex| (*vertex, row))
                })
                .collect(),
            transform_hints: native
                .transform_hints
                .iter()
                .filter_map(|row| {
                    row_owner
                        .get(row.owner.as_str())
                        .map(|body| (*body, row))
                })
                .collect(),
            vertex_ownerships: native
                .vertex_ownerships
                .iter()
                .filter_map(|row| {
                    row_owner
                        .get(row.id().as_str())
                        .map(|vertex| (*vertex, row))
                })
                .collect(),
            wires_by_shell,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_asm::brep::records::{identity::NativeRecordNamespace, FaceContainment};
    use cadmpeg_ir::topology::Sense;

    fn side() -> FaceSidedness {
        FaceSidedness {
            source_namespace: NativeRecordNamespace::new(cadmpeg_asm::asm_format!("f3d")),
            record_index: 17,
            face_key: None,
            native_sense: Sense::Forward,
            carrier_flipped: true,
            containment: Some(FaceContainment::Out),
        }
    }

    #[test]
    fn generation_sidedness_resolves_source_keys_and_preserves_shared_native_refs() {
        let mut target = cadmpeg_ir::examples::unit_cube().unwrap();
        let key = FaceNativeKey {
            source_namespace: NativeRecordNamespace::new(cadmpeg_asm::asm_format!("f3d")),
            record_index: 17,
            asm_face_key: Some(42),
        };
        target.model.faces[0].native_ref = Some(key.id());
        target.model.faces[1].native_ref = Some(key.id());
        let mut owned = side();
        owned.face_key = Some(key.id());
        let native = F3dNative {
            face_sidedness: vec![owned],
            face_native_keys: vec![key],
            ..F3dNative::default()
        };
        let index = NativeGenerationIndex::new(&target, &native);
        for face in &target.model.faces[..2] {
            let row = index.face_sidedness[face.id.as_str()];
            assert_eq!(row.record_index, 17);
            assert_eq!(row.containment, Some(FaceContainment::Out));
            assert!(row.carrier_flipped);
        }
        assert_eq!(index.face_sidedness.len(), 2);
    }

    #[test]
    fn generation_sidedness_direct_reference_does_not_guess_neutral_id_spelling() {
        let mut target = cadmpeg_ir::examples::unit_cube().unwrap();
        let row = side();
        target.model.faces[0].native_ref = Some(row.id());
        target.model.faces[1].native_ref = Some("sat:asm:face-sidedness#17".into());
        target.model.faces[2].native_ref = Some("f3d:asm:face-native-key#17".into());
        let native = F3dNative {
            face_sidedness: vec![row],
            ..F3dNative::default()
        };
        let index = NativeGenerationIndex::new(&target, &native);
        assert_eq!(index.face_sidedness.len(), 1);
        assert_eq!(
            index.face_sidedness[target.model.faces[0].id.as_str()].record_index,
            17
        );
    }
}
