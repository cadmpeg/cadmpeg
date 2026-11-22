// SPDX-License-Identifier: Apache-2.0
//! The history folder's items: one `moHistoryFeatItemData_c` per user
//! feature (`docs/formats/sldprt.md` §2.3).

use super::archive::Archive;
use super::tree::unit_component;
use super::{Feature, TIME_STAMP};

/// The four u32 of a history item before its string.
const ITEM_WORDS: [u32; 4] = [1, 0x4000_0000, u32::MAX, 0];

/// The item count and the items of `features`, in source order: u16 `0`,
/// [`ITEM_WORDS`], an empty `CString` and a `moCompFeature_c` naming the
/// feature by its object identifier.
pub(super) fn items(archive: &mut Archive, features: &[Feature]) {
    let count = archive.count16(features.len(), "history item count");
    archive.u16(count);
    for feature in features {
        let id = match feature {
            Feature::Sketch(sketch) => sketch.feature_id,
            Feature::Extrusion(extrusion) => extrusion.feature_id,
        };
        archive.object("moHistoryFeatItemData_c");
        archive.u16(0);
        archive.u32s(&ITEM_WORDS);
        archive.cstring("");
        archive.object("moCompFeature_c");
        unit_component(archive, 0);
        archive.u32(id);
        archive.u32(TIME_STAMP);
    }
}
