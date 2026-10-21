// SPDX-License-Identifier: Apache-2.0
//! Assertions over source-only bytes and layout retained by the codec.

pub(crate) fn bytes<'a>(fidelity: &'a cadmpeg_ir::SourceFidelity, id: &str) -> &'a [u8] {
    let record = fidelity.retained_record(id).expect("source record");
    let bytes = if let Some(bytes) = record.data() {
        bytes
    } else {
        let stream = fidelity
            .retained_records()
            .values()
            .find(|candidate| {
                candidate.stream() == record.stream()
                    && candidate.offset() == 0
                    && candidate.data().is_some()
            })
            .expect("source stream image");
        let start = usize::try_from(record.offset()).expect("source offset");
        let end = usize::try_from(record.end_offset()).expect("source end");
        &stream.data().expect("retained image")[start..end]
    };
    assert_eq!(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        record.byte_len()
    );
    assert_eq!(cadmpeg_ir::hash::sha256_hex(bytes), record.sha256());
    bytes
}

pub(crate) fn layout<T: serde::de::DeserializeOwned>(
    fidelity: &cadmpeg_ir::SourceFidelity,
    name: &str,
) -> T {
    let id = crate::native::native_id("source-layout", name);
    serde_json::from_slice(bytes(fidelity, &id)).expect("retained source layout")
}
