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
pub fn face_keys(records: &[FaceNativeKey]) -> HashMap<FaceId, u64> {
    records
        .iter()
        .filter_map(|record| record.asm_face_key.map(|key| (record.face.clone(), key)))
        .collect()
}
