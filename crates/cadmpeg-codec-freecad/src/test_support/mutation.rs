// SPDX-License-Identifier: Apache-2.0
//! Synthetic native-entry mutations shared by crate tests.

use crate::native::EntryRecord;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

pub(crate) fn replace_entry(
    ir: &mut CadIr,
    entry_name: &str,
    bytes: Vec<u8>,
) -> Result<(), CodecError> {
    if entry_name == "Document.xml" {
        return Err(CodecError::NotImplemented(
            "Document.xml must be edited through typed property records".into(),
        ));
    }
    let namespace = ir.native.namespace_mut("fcstd");
    let mut entries = namespace.arena_as::<EntryRecord>("entries")?;
    let entry = entries
        .iter_mut()
        .find(|candidate| candidate.name() == entry_name)
        .ok_or_else(|| CodecError::malformed(format_args!("missing FCStd entry {entry_name}")))?;
    entry.replace_data(bytes);
    namespace.set_arena(
        &cadmpeg_test_support::service_decode_context(),
        "entries",
        &entries,
    )?;
    Ok(())
}
