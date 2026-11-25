// SPDX-License-Identifier: Apache-2.0
//! The ASM history that installs the current bodies (f3d.md §3.1, "History
//! of a Base Feature" and "History states"; asm.md §3): a root state and one
//! state for each item that commits a state, whose one bulletin board inserts
//! the entity slots of the item's result body. A Base Feature commits one
//! state that inserts every entity slot of the solved model except slot `0`.

use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_core::CodecError;

use crate::writer::generate::native_bytes::{
    native_i64, native_ident, native_ref, native_subident,
};

/// The identity of the root `delta_state`.
pub(super) const ROOT_STATE: u32 = 2;
/// The identity of the head `delta_state`, the state the Base Feature
/// commits.
pub(super) const HEAD_STATE: u32 = 3;
/// The null history-state identity of an item tail.
pub(super) const NULL_STATE: u32 = u32::MAX;
/// The preamble's history entry count of a two-state history.
const HISTORY_ENTRY_COUNT: i64 = 1;
/// The flags word of a snapshot stream: no history partition, save-format
/// revision 1.
const SNAPSHOT_FLAGS: u64 = 2;
/// The `scale` word of a snapshot stream.
const SNAPSHOT_SCALE: f64 = 10.0;

/// `smbh`, written with no history, with the insertion history in place of
/// its empty history tail. `states` holds, for each state after the root
/// state in state order, the position of the state's result body among the
/// `body` records of `smbh`; with at most one entry, one state inserts every
/// slot.
pub(in crate::writer::generate) fn with_insertion_history(
    smbh: &[u8],
    states: &[usize],
) -> Result<Vec<u8>, CodecError> {
    let framed = cadmpeg_asm::edit::AsmEditSet::frame(smbh)?;
    let boards = boards(framed.records(), states)?;
    let mut bytes = model_part(smbh)?.to_vec();
    let root = i64::from(ROOT_STATE);
    let count = i64::try_from(boards.len())
        .map_err(|_| CodecError::NotImplemented("history state count exceeds i64".into()))?;
    let head = root + count;
    for part in ["Begin", "of", "ASM", "History"] {
        native_subident(&mut bytes, part)?;
    }
    native_ident(&mut bytes, "Data")?;
    native_ident(&mut bytes, "history_stream")?;
    for value in [head, head, 0, HISTORY_ENTRY_COUNT] {
        native_i64(&mut bytes, value);
    }
    for reference in [-1, 0, 1, -1] {
        native_ref(&mut bytes, reference);
    }
    bytes.push(0x11);
    // Node `n` is the state `head - n / 2` for even `n` and `root + n / 2`
    // for odd `n`; the records stand in node order.
    let order = node_order(root, head);
    let node = |state: i64| -> i64 {
        order
            .iter()
            .position(|&at| at == state)
            .and_then(|at| i64::try_from(at).ok())
            .unwrap_or(-1)
    };
    for &state in &order {
        delta_state(
            &mut bytes,
            state,
            [node(state + 1), node(state - 1), node(state)],
        )?;
        if let Some(slots) = usize::try_from(state - root - 1)
            .ok()
            .and_then(|at| boards.get(at))
        {
            native_i64(&mut bytes, 1);
            native_ref(&mut bytes, node(state));
            native_i64(&mut bytes, 2);
            for &slot in slots {
                native_i64(&mut bytes, 1);
                native_ref(&mut bytes, -1);
                native_ref(&mut bytes, slot);
            }
            native_i64(&mut bytes, 0);
        }
        end_state(&mut bytes);
    }
    for part in ["End", "of", "ASM", "History"] {
        native_subident(&mut bytes, part)?;
    }
    native_ident(&mut bytes, "Section")?;
    bytes.push(0x11);
    for part in ["End", "of", "ASM"] {
        native_subident(&mut bytes, part)?;
    }
    native_ident(&mut bytes, "data")?;
    Ok(bytes)
}

/// The states from `root` to `head` in node order: the head, the root, the
/// state before the head, the state after the root, and so on.
fn node_order(root: i64, head: i64) -> Vec<i64> {
    let (mut low, mut high) = (root, head);
    let mut order = Vec::new();
    while low <= high {
        order.push(high);
        if low < high {
            order.push(low);
        }
        high -= 1;
        low += 1;
    }
    order
}

/// The slots each state after the root state inserts, in state order and
/// ascending: every slot but `0` in one state where `states` holds at most
/// one body, else the slots the `body` record `states[k]` of `records`
/// reaches by its references in state `k`, and the slots no body reaches in
/// the first state.
fn boards(records: &[Record], states: &[usize]) -> Result<Vec<Vec<i64>>, CodecError> {
    let slots = records.len();
    let slot = |index: usize| {
        i64::try_from(index)
            .map_err(|_| CodecError::NotImplemented("ASM record count exceeds i64".into()))
    };
    if states.len() <= 1 {
        return Ok(vec![(1..slots).map(slot).collect::<Result<_, _>>()?]);
    }
    let bodies = records
        .iter()
        .filter(|record| record.name == "body")
        .map(|record| record.index)
        .collect::<Vec<_>>();
    let mut owner = vec![None::<usize>; records.len()];
    for (state, &body) in states.iter().enumerate() {
        let start = *bodies
            .get(body)
            .ok_or_else(|| CodecError::malformed(format_args!("no ASM body {body}")))?;
        let mut stack = vec![start];
        while let Some(index) = stack.pop() {
            match owner.get(index).copied().flatten() {
                Some(earlier) if earlier == state => continue,
                Some(_) => {
                    return Err(CodecError::NotImplemented(
                        "an ASM record that the bodies of two history states share".into(),
                    ))
                }
                None => {}
            }
            let record = records
                .get(index)
                .ok_or_else(|| CodecError::malformed(format_args!("ASM reference {index}")))?;
            owner[index] = Some(state);
            for token in record.tokens.iter() {
                let Token::Ref(reference) = token else {
                    continue;
                };
                let Ok(reference) = usize::try_from(*reference) else {
                    continue;
                };
                if reference != 0 && !bodies.contains(&reference) {
                    stack.push(reference);
                }
            }
        }
    }
    let mut boards = vec![Vec::new(); states.len()];
    for (index, state) in owner.into_iter().enumerate().skip(1) {
        boards[state.unwrap_or(0)].push(slot(index)?);
    }
    Ok(boards)
}

/// The snapshot stream of a Base Feature design from `stream`, a B-rep
/// stream written with no history whose body carries no name: its records,
/// the flags word `2`, `scale` `10.0`, and `End-of-ASM-data` (f3d.md §3.1,
/// "Archive members of a Base Feature design").
pub(in crate::writer::generate) fn snapshot(stream: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut bytes = model_part(stream)?.to_vec();
    for part in ["End", "of", "ASM"] {
        native_subident(&mut bytes, part)?;
    }
    native_ident(&mut bytes, "data")?;
    with_header(&bytes, SNAPSHOT_FLAGS, SNAPSHOT_SCALE)
}

/// The solved-model records of `stream`, a B-rep stream written with no
/// history: the stream without its empty history tail.
fn model_part(stream: &[u8]) -> Result<&[u8], CodecError> {
    let mut empty_tail = Vec::new();
    native_ident(&mut empty_tail, "delta_state")?;
    stream.strip_suffix(empty_tail.as_slice()).ok_or_else(|| {
        CodecError::Malformed(
            "the generated B-rep stream does not end with an empty history tail".into(),
        )
    })
}

/// A `delta_state` header: its identity, the constants `1` and `0`, the
/// previous, next and node references, `-1`, `0`, and the false sentinel.
fn delta_state(bytes: &mut Vec<u8>, state: i64, links: [i64; 3]) -> Result<(), CodecError> {
    native_ident(bytes, "delta_state")?;
    for value in [state, 1, 0] {
        native_i64(bytes, value);
    }
    for reference in links.into_iter().chain([-1, 0]) {
        native_ref(bytes, reference);
    }
    bytes.push(0x0b);
    Ok(())
}

/// The bulletin-board chain terminator and the `04 0 11` separator.
fn end_state(bytes: &mut Vec<u8>) {
    native_i64(bytes, 0);
    native_i64(bytes, 0);
    bytes.push(0x11);
}

/// `stream`, a binary ASM stream, with its header flags word and its
/// `scale` replaced (asm.md §1): the flags word at offset 39, and the f64
/// after the product, version and date strings.
pub(in crate::writer::generate) fn with_header(
    stream: &[u8],
    flags: u64,
    scale: f64,
) -> Result<Vec<u8>, CodecError> {
    const MAGIC: &[u8] = b"ASM BinaryFile8";
    const FLAGS: usize = 39;
    const STRING: u8 = 0x07;
    const DOUBLE: u8 = 0x06;
    let malformed = || CodecError::Malformed("the generated B-rep stream header".into());
    if !stream.starts_with(MAGIC) {
        return Err(malformed());
    }
    let mut at = FLAGS + 8;
    for _ in 0..3 {
        if stream.get(at) != Some(&STRING) {
            return Err(malformed());
        }
        let length = usize::from(*stream.get(at + 1).ok_or_else(malformed)?);
        at += 2 + length;
    }
    if stream.get(at) != Some(&DOUBLE) || stream.len() < at + 9 {
        return Err(malformed());
    }
    let mut out = stream.to_vec();
    out[FLAGS..FLAGS + 8].copy_from_slice(&flags.to_le_bytes());
    out[at + 1..at + 9].copy_from_slice(&scale.to_le_bytes());
    Ok(out)
}
