// SPDX-License-Identifier: Apache-2.0
//! Wire-only join maps derived from retained native key records.

pub(super) mod faces {
    use crate::brep::records::FaceNativeKey;
    use cadmpeg_ir::ids::FaceId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
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

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<FaceNativeKey>, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            face_keys: HashMap<FaceId, u64>,
            face_native_keys: Vec<FaceNativeKey>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let expected = wire
            .face_native_keys
            .iter()
            .filter_map(|record| record.asm_face_key.map(|key| (record.face.clone(), key)))
            .collect::<HashMap<_, _>>();
        if wire.face_keys != expected {
            return Err(serde::de::Error::custom(
                "face_keys must match face_native_keys",
            ));
        }
        Ok(wire.face_native_keys)
    }
}

pub(super) mod bodies {
    use crate::brep::records::BodyNativeKey;
    use cadmpeg_ir::ids::BodyId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
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

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<BodyNativeKey>, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            body_keys: HashMap<BodyId, u64>,
            body_native_keys: Vec<BodyNativeKey>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let expected = wire
            .body_native_keys
            .iter()
            .filter_map(|record| record.asm_body_key.map(|key| (record.body.clone(), key)))
            .collect::<HashMap<_, _>>();
        if wire.body_keys != expected {
            return Err(serde::de::Error::custom(
                "body_keys must match body_native_keys",
            ));
        }
        Ok(wire.body_native_keys)
    }
}
