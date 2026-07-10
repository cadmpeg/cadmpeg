// SPDX-License-Identifier: Apache-2.0
//! ASM construction-history container and `delta_state` headers.

use cadmpeg_ir::history::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmHistory,
    AsmHistoryRecord,
};
use cadmpeg_ir::provenance::{EntityMeta, Exactness, Provenance};

const DELTA: &[u8] = b"\x11\x0d\x0bdelta_state";
const PREAMBLE: &[u8] = b"\x0d\x0ehistory_stream";

/// Decode the construction-history tail of an ASM stream: every `delta_state`
/// record (spec §2.3) from `bytes`, each with its `BulletinBoard` chain of
/// per-entity insert/delete/update changes and the raw history-entity records
/// framed between it and the next `delta_state`. `stream` is the source ZIP
/// entry name, recorded in each decoded item's provenance. Returns `None` when
/// `bytes` carries no `delta_state` record (the stream is a construction
/// snapshot with no history tail) or a malformed history body.
pub fn decode(bytes: &[u8], stream: &str) -> Option<AsmHistory> {
    let mut delta_offsets = Vec::new();
    let mut search = 0usize;
    while let Some(relative) = bytes[search..]
        .windows(DELTA.len())
        .position(|window| window == DELTA)
    {
        let offset = search + relative;
        delta_offsets.push(offset);
        search = offset + DELTA.len();
    }
    let mut states = Vec::new();
    for (ordinal, &offset) in delta_offsets.iter().enumerate() {
        let mut position = offset + DELTA.len();
        let state_id = take_i64(bytes, &mut position, 0x04)?;
        let version_flag = take_i64(bytes, &mut position, 0x04)?;
        let state_flag = take_i64(bytes, &mut position, 0x04)?;
        let previous = take_i64(bytes, &mut position, 0x0c)?;
        let next = take_i64(bytes, &mut position, 0x0c)?;
        let node_index = take_i64(bytes, &mut position, 0x0c)?;
        let partner = take_i64(bytes, &mut position, 0x0c)?;
        let owner_ref = take_i64(bytes, &mut position, 0x0c)?;
        if bytes.get(position) != Some(&0x0b) {
            continue;
        }
        let (bulletin_boards, body_end) = decode_bulletin_boards(bytes, position + 1)?;
        let records = decode_history_records(
            bytes,
            body_end,
            delta_offsets.get(ordinal + 1).copied(),
            stream,
        );
        states.push(AsmDeltaState {
            state_id,
            version_flag,
            state_flag,
            previous_ref: (previous >= 0).then_some(previous),
            next_ref: (next >= 0).then_some(next),
            node_index,
            partner_ref: (partner >= 0).then_some(partner),
            owner_ref,
            bulletin_boards,
            records,
            meta: meta(stream, offset, "delta_state"),
        });
    }
    if states.is_empty() {
        return None;
    }

    let preamble_offset = bytes
        .windows(PREAMBLE.len())
        .position(|window| window == PREAMBLE);
    let (stream_size, high_water_mark) = preamble_offset
        .and_then(|offset| decode_preamble(bytes, offset + PREAMBLE.len()))
        .map_or((None, None), |(size, high)| (Some(size), Some(high)));
    let offset = preamble_offset.unwrap_or(states[0].meta.provenance.offset as usize);
    Some(AsmHistory {
        stream_size,
        high_water_mark,
        states,
        meta: meta(stream, offset, "history_stream"),
    })
}

fn decode_bulletin_boards(
    bytes: &[u8],
    mut position: usize,
) -> Option<(Vec<AsmBulletinBoard>, usize)> {
    if bytes.get(position) == Some(&0x11) {
        return Some((Vec::new(), position));
    }
    let mut boards = Vec::new();
    loop {
        let present = take_i64(bytes, &mut position, 0x04)?;
        if present == 0 {
            break;
        }
        let owner_ref = take_i64(bytes, &mut position, 0x0c)?;
        let number = take_i64(bytes, &mut position, 0x04)?;
        let mut changes = Vec::new();
        loop {
            let present = take_i64(bytes, &mut position, 0x04)?;
            if present == 0 {
                break;
            }
            let old = take_i64(bytes, &mut position, 0x0c)?;
            let new = take_i64(bytes, &mut position, 0x0c)?;
            let kind = match (old >= 0, new >= 0) {
                (false, true) => AsmEntityChangeKind::Insert,
                (true, false) => AsmEntityChangeKind::Delete,
                (true, true) => AsmEntityChangeKind::Update,
                (false, false) => return None,
            };
            changes.push(AsmEntityChange {
                kind,
                old_ref: (old >= 0).then_some(old),
                new_ref: (new >= 0).then_some(new),
            });
        }
        boards.push(AsmBulletinBoard {
            owner_ref,
            number,
            changes,
        });
    }
    Some((boards, position))
}

fn decode_history_records(
    bytes: &[u8],
    state_end: usize,
    next_delta: Option<usize>,
    stream: &str,
) -> Vec<AsmHistoryRecord> {
    let start = state_end + usize::from(bytes.get(state_end) == Some(&0x11));
    let limit = next_delta.map_or(bytes.len(), |offset| offset + 1);
    if start >= limit {
        return Vec::new();
    }
    match crate::sab::frame(bytes, start, limit, 8) {
        Ok(records) => records
            .into_iter()
            .map(|record| AsmHistoryRecord {
                index: record.index as u64,
                name: record.name,
                raw_bytes: bytes[record.offset..record.offset + record.len].to_vec(),
                meta: meta(stream, record.offset, "history_entity"),
            })
            .collect(),
        Err(_) => vec![AsmHistoryRecord {
            index: 0,
            name: "opaque_history_payload".into(),
            raw_bytes: bytes[start..limit].to_vec(),
            meta: meta(stream, start, "opaque_history_payload"),
        }],
    }
}

fn decode_preamble(bytes: &[u8], mut position: usize) -> Option<(i64, i64)> {
    let size = take_i64(bytes, &mut position, 0x04)?;
    let duplicate = take_i64(bytes, &mut position, 0x04)?;
    let zero = take_i64(bytes, &mut position, 0x04)?;
    let high_water = take_i64(bytes, &mut position, 0x04)?;
    (size == duplicate && zero == 0).then_some((size, high_water))
}

fn take_i64(bytes: &[u8], position: &mut usize, tag: u8) -> Option<i64> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let value = i64::from_le_bytes(bytes.get(*position + 1..*position + 9)?.try_into().ok()?);
    *position += 9;
    Some(value)
}

fn meta(stream: &str, offset: usize, tag: &str) -> EntityMeta {
    EntityMeta {
        provenance: Provenance {
            format: "f3d".into(),
            stream: stream.into(),
            offset: offset as u64,
            tag: Some(tag.into()),
        },
        exactness: Exactness::ByteExact,
    }
}
