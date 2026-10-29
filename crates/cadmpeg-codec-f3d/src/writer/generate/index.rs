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
        macro_rules! first_by_id {
            ($items:expr, $field:ident) => {{
                let mut map = HashMap::new();
                for item in $items {
                    map.entry(item.$field.as_str()).or_insert(item);
                }
                map
            }};
        }
        let mut wires_by_shell: HashMap<_, Vec<_>> = HashMap::new();
        for wire in &native.wire_topologies {
            wires_by_shell
                .entry(wire.shell.as_str())
                .or_default()
                .push(wire);
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
            edge_continuities: first_by_id!(&native.edge_continuities, edge),
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
            tolerant_coedges: first_by_id!(&native.tolerant_coedge_parameters, coedge),
            tolerant_edges: first_by_id!(&native.tolerant_edge_tails, edge),
            tolerant_vertices: first_by_id!(&native.tolerant_vertex_tails, vertex),
            transform_hints: first_by_id!(&native.transform_hints, body),
            vertex_ownerships: first_by_id!(&native.vertex_ownerships, vertex),
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
