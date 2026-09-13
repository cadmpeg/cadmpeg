// SPDX-License-Identifier: Apache-2.0
//! Wire-only join maps derived from retained native key records.

pub(super) mod faces {
    use crate::brep::records::FaceNativeKey;
    use cadmpeg_ir::ids::FaceId;
    use serde::{Serialize, Serializer};
    use std::collections::HashMap;

    pub(crate) fn serialize<S: Serializer>(
        records: &[FaceNativeKey],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            face_keys: HashMap<&'a FaceId, u64>,
            face_native_keys: &'a [FaceNativeKey],
        }
        Wire {
            face_keys: records
                .iter()
                .filter_map(|record| record.asm_face_key.map(|key| (&record.face, key)))
                .collect(),
            face_native_keys: records,
        }
        .serialize(serializer)
    }

    /// Whether the stated join map is the one the key records project.
    pub(crate) fn agrees(keys: &HashMap<FaceId, u64>, records: &[FaceNativeKey]) -> bool {
        let projected = records
            .iter()
            .filter_map(|record| record.asm_face_key.map(|key| (record.face.clone(), key)))
            .collect::<HashMap<_, _>>();
        *keys == projected
    }
}

pub(super) mod bodies {
    use crate::brep::records::BodyNativeKey;
    use cadmpeg_ir::ids::BodyId;
    use serde::{Serialize, Serializer};
    use std::collections::HashMap;

    pub(crate) fn serialize<S: Serializer>(
        records: &[BodyNativeKey],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            body_keys: HashMap<&'a BodyId, u64>,
            body_native_keys: &'a [BodyNativeKey],
        }
        Wire {
            body_keys: records
                .iter()
                .filter_map(|record| record.asm_body_key.map(|key| (&record.body, key)))
                .collect(),
            body_native_keys: records,
        }
        .serialize(serializer)
    }

    /// Whether the stated join map is the one the key records project.
    pub(crate) fn agrees(keys: &HashMap<BodyId, u64>, records: &[BodyNativeKey]) -> bool {
        let projected = records
            .iter()
            .filter_map(|record| record.asm_body_key.map(|key| (record.body.clone(), key)))
            .collect::<HashMap<_, _>>();
        *keys == projected
    }
}
