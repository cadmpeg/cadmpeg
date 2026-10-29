// SPDX-License-Identifier: Apache-2.0
//! Join projections derived from retained native key records.
//!
//! The key records are the document's only statement of the ASM join keys. A
//! reader that wants the key map builds it here; the map is never written
//! beside the records it projects.

use crate::brep::records::{FaceNativeKey, FaceSidedness};
use cadmpeg_ir::ids::FaceId;
use std::collections::HashMap;

/// Face id to ASM face key, for the records that state a key.
#[must_use]
pub fn face_keys(
    faces: &[cadmpeg_ir::topology::Face],
    records: &[FaceNativeKey],
    sides: &[FaceSidedness],
) -> HashMap<FaceId, u64> {
    faces
        .iter()
        .filter_map(|face| {
            records.iter().find_map(|record| {
                record
                    .owns_native_ref(face.native_ref.as_deref(), sides)
                    .then_some(record.asm_face_key)
                    .flatten()
                    .map(|key| (face.id.clone(), key))
            })
        })
        .collect()
}
