// SPDX-License-Identifier: Apache-2.0
//! Read the document up axis from the Design `AssetSettings` record.
//!
//! Record 0 (`base_feature_asset_settings`) stores a direction pair, the front
//! direction and the up direction, and stores the same pair a second time
//! later in the record. A Z-up document stores `(0, 1, 0)` and `(0, 0, 1)`; a
//! Y-up document stores `(0, 0, -1)` and `(0, 1, 0)`. The second vector of the
//! pair is the up axis.

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::up_axis::UpAxis;

use crate::container::ContainerScan;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::base_feature_asset_settings as settings;
use crate::loss::F3dLossCode;

/// The record index of `AssetSettings`.
const RECORD_INDEX: u32 = 0;
/// The offset of the LP-ASCII key `AssetSettings` from the record start.
const KEY_AT: usize = 15;
/// The length of the LP-ASCII key.
const KEY_LEN: usize = 17;
/// The stored (front, up) pair of a Z-up document.
const Z_UP_PAIR: ([f64; 3], [f64; 3]) = ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
/// The stored (front, up) pair of a Y-up document.
const Y_UP_PAIR: ([f64; 3], [f64; 3]) = ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);

/// What the `AssetSettings` record states about the up axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum UpAxisRead {
    /// No design stream holds an `AssetSettings` record.
    Absent,
    /// Both stored pairs agree and are one of the two stored forms.
    Stated(UpAxis),
    /// The record holds a pair that is not one of the two forms, or its two
    /// copies differ. The first copy is named.
    Unrecognized {
        front: [f64; 3],
        up: [f64; 3],
        copies_agree: bool,
    },
}

type Pair = ([f64; 3], [f64; 3]);

fn vector_at(bytes: &[u8], at: usize) -> Option<[f64; 3]> {
    Some([
        View::f64_le_at(bytes, at)?,
        View::f64_le_at(bytes, at.checked_add(8)?)?,
        View::f64_le_at(bytes, at.checked_add(16)?)?,
    ])
}

/// Whether two vectors store the same components, with `-0` equal to `0`.
fn same(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x + 0.0).to_bits() == (y + 0.0).to_bits())
}

/// The two stored pairs of the record whose header starts at `at`, when the
/// record is whole and holds the `AssetSettings` key.
fn pairs(bytes: &[u8], at: usize) -> Option<(Pair, Pair)> {
    let end = at.checked_add(settings::TEMPLATE.len())?;
    let record = bytes.get(at..end)?;
    (record.get(KEY_AT..KEY_AT + KEY_LEN)? == &settings::TEMPLATE[KEY_AT..KEY_AT + KEY_LEN])
        .then_some(())?;
    Some((
        (
            vector_at(record, settings::FRONT_A)?,
            vector_at(record, settings::UP_A)?,
        ),
        (
            vector_at(record, settings::FRONT_B)?,
            vector_at(record, settings::UP_B)?,
        ),
    ))
}

fn read_stream(bytes: &[u8]) -> Option<UpAxisRead> {
    let offsets = IndexedRecordOffsets::build(bytes);
    offsets
        .offsets(RECORD_INDEX)
        .iter()
        .find_map(|at| pairs(bytes, *at))
        .map(|(first, second)| {
            let copies_agree = same(first.0, second.0) && same(first.1, second.1);
            let form = |pair: Pair| {
                if same(pair.0, Z_UP_PAIR.0) && same(pair.1, Z_UP_PAIR.1) {
                    Some(UpAxis::Z)
                } else if same(pair.0, Y_UP_PAIR.0) && same(pair.1, Y_UP_PAIR.1) {
                    Some(UpAxis::Y)
                } else {
                    None
                }
            };
            match form(first) {
                Some(axis) if copies_agree => UpAxisRead::Stated(axis),
                _ => UpAxisRead::Unrecognized {
                    front: first.0,
                    up: first.1,
                    copies_agree,
                },
            }
        })
}

/// The document up axis, read from the first design `BulkStream` that holds
/// an `AssetSettings` record. An entry whose bytes cannot be read is skipped.
pub(crate) fn decode_document_up_axis(scan: &ContainerScan) -> UpAxisRead {
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        if let Ok(bytes) = scan.entry_bytes(&entry.name) {
            if let Some(read) = read_stream(bytes) {
                return read;
            }
        }
    }
    UpAxisRead::Absent
}

/// Record the read: the stated axis on `up_axis`, or a loss for a pair that is
/// not one of the two stored forms, leaving `up_axis` unstated.
pub(crate) fn apply(read: UpAxisRead, up_axis: &mut Option<UpAxis>, losses: &mut Vec<LossNote>) {
    match read {
        UpAxisRead::Absent => {}
        UpAxisRead::Stated(axis) => *up_axis = Some(axis),
        UpAxisRead::Unrecognized {
            front,
            up,
            copies_agree,
        } => losses.push(F3dLossCode::SettingsUpAxisUnrecognized.note(format!(
            "AssetSettings stores the direction pair front {front:?}, up {up:?}{}; \
             neither is a Y-up or Z-up document form, so the document states no up axis",
            if copies_agree {
                ""
            } else {
                ", and its two copies differ"
            }
        ))),
    }
}

#[cfg(test)]
mod tests;
