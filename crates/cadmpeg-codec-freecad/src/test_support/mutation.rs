// SPDX-License-Identifier: Apache-2.0
//! Native entry metadata and source-only field controls.

use crate::native::EntryRecord;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

pub(crate) fn assert_entry_source_fields_rejected(
    ir: &mut CadIr,
    entry_name: &str,
    sources: &[&[u8]],
) -> Result<(), CodecError> {
    let namespace = ir.native.namespace_mut("fcstd");
    let mut entries = namespace.arena_as::<EntryRecord>("entries")?;
    let entry = entries
        .iter_mut()
        .find(|candidate| candidate.name() == entry_name)
        .ok_or_else(|| CodecError::malformed(format_args!("missing FCStd entry {entry_name}")))?;
    let wire = serde_json::to_value(&*entry).map_err(CodecError::malformed)?;
    for source in sources {
        for (field, value) in [
            (
                "data",
                serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::from(
                    source.to_vec(),
                ))
                .unwrap(),
            ),
            (
                "byte_len",
                serde_json::json!(cadmpeg_core::decode::u64_from_index(source.len())),
            ),
            (
                "sha256",
                serde_json::json!(cadmpeg_ir::hash::sha256_hex(source)),
            ),
        ] {
            let mut legacy = wire.clone();
            legacy[field] = value;
            assert!(
                serde_json::from_value::<EntryRecord>(legacy).is_err(),
                "source-only entry field {field}"
            );
        }
    }
    *entry = EntryRecord::new(
        entry.id().to_owned(),
        entry.name().to_owned(),
        entry.role,
        entry.referenced_by().to_vec(),
    )?;
    namespace.set_arena(
        &cadmpeg_test_support::service_decode_context(),
        "entries",
        &entries,
    )?;
    Ok(())
}
