// SPDX-License-Identifier: Apache-2.0
//! Join projections derived from retained native key records.
//!
//! The key records are the document's only statement of the ASM join keys. A
//! reader that wants the key map builds it here; the map is never written
//! beside the records it projects.

use crate::brep::records::FaceNativeKey;
use cadmpeg_ir::ids::FaceId;
use std::collections::HashMap;

/// Face id to ASM face key, for the records that state a key.
#[must_use]
pub fn face_keys(
    faces: &[cadmpeg_ir::topology::Face],
    records: &[FaceNativeKey],
) -> HashMap<FaceId, u64> {
    let native: HashMap<_, _> = records
        .iter()
        .filter_map(|record| record.asm_face_key.map(|key| (record.id(), key)))
        .collect();
    faces
        .iter()
        .filter_map(|face| {
            let owner = face.native_ref.as_deref()?;
            native.get(owner).map(|key| (face.id.clone(), *key))
        })
        .collect()
}
