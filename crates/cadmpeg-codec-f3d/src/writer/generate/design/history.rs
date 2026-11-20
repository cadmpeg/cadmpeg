// SPDX-License-Identifier: Apache-2.0
//! The ASM history of a Base Feature that installs the current bodies
//! (f3d.md §3.1, "History of a Base Feature"; asm.md §3): a root state and one
//! head state whose one bulletin board inserts every entity slot of the solved
//! model except slot `0`.

use cadmpeg_core::CodecError;

use crate::writer::generate::native_bytes::{
    native_i64, native_ident, native_ref, native_subident,
};

/// The identity of the root `delta_state`.
pub(super) const ROOT_STATE: u32 = 2;
/// The identity of the head `delta_state`, the state the Base Feature
/// commits.
pub(super) const HEAD_STATE: u32 = 3;
/// The preamble's history entry count of a two-state history.
const HISTORY_ENTRY_COUNT: i64 = 1;

/// `smbh`, written with no history, with the insertion history in place of
/// its empty history tail.
pub(in crate::writer::generate) fn with_insertion_history(
    smbh: &[u8],
) -> Result<Vec<u8>, CodecError> {
    let slots = cadmpeg_asm::edit::AsmEditSet::frame(smbh)?.records().len();
    let slots = i64::try_from(slots)
        .map_err(|_| CodecError::NotImplemented("ASM record count exceeds i64".into()))?;
    let mut empty_tail = Vec::new();
    native_ident(&mut empty_tail, "delta_state")?;
    let Some(model) = smbh.strip_suffix(empty_tail.as_slice()) else {
        return Err(CodecError::Malformed(
            "the generated B-rep stream does not end with an empty history tail".into(),
        ));
    };
    let mut bytes = model.to_vec();
    let (head, root) = (i64::from(HEAD_STATE), i64::from(ROOT_STATE));
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
    // The head state, node 0: one board that inserts slots 1 to `slots - 1`.
    delta_state(&mut bytes, head, [-1, 1, 0])?;
    native_i64(&mut bytes, 1);
    native_ref(&mut bytes, 0);
    native_i64(&mut bytes, 2);
    for slot in 1..slots {
        native_i64(&mut bytes, 1);
        native_ref(&mut bytes, -1);
        native_ref(&mut bytes, slot);
    }
    native_i64(&mut bytes, 0);
    end_state(&mut bytes);
    // The root state, node 1: no board.
    delta_state(&mut bytes, root, [0, -1, 1])?;
    end_state(&mut bytes);
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
