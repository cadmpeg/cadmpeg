use super::*;
use crate::layout::directory_entry as directory_layout;

fn parse_record(
    bytes: &[u8],
    version: CompoundVersion,
) -> Result<Vec<DirectorySlot>, CodecError> {
    with_context(bytes, &DecodePolicy::service(), |ctx| {
        parse_directory(ctx, bytes, version)
    })
}

fn assert_malformed(bytes: &[u8], version: CompoundVersion, expected: &str) {
    let error = parse_record(bytes, version).expect_err("directory fields violate MS-CFB");
    assert!(matches!(error, CodecError::Malformed(message) if message == expected));
}

fn stream_entry() -> [u8; directory_layout::LEN] {
    let mut bytes = [0_u8; directory_layout::LEN];
    directory_entry(
        &mut bytes,
        0,
        "Stream",
        2,
        NO_STREAM,
        NO_STREAM,
        NO_STREAM,
        END_OF_CHAIN,
        0,
    );
    bytes
}

fn storage_entry() -> [u8; directory_layout::LEN] {
    let mut bytes = [0_u8; directory_layout::LEN];
    directory_entry(
        &mut bytes,
        0,
        "Storage",
        1,
        NO_STREAM,
        NO_STREAM,
        NO_STREAM,
        0,
        0,
    );
    bytes
}

fn root_entry() -> [u8; directory_layout::LEN] {
    let mut bytes = [0_u8; directory_layout::LEN];
    directory_entry(
        &mut bytes,
        0,
        "Root Entry",
        5,
        NO_STREAM,
        NO_STREAM,
        NO_STREAM,
        END_OF_CHAIN,
        0,
    );
    bytes
}

fn set_entry_size(bytes: &mut [u8; directory_layout::LEN], size: u64) {
    bytes[directory_layout::STREAM_SIZE..directory_layout::STREAM_SIZE + 8].copy_from_slice(&size.to_le_bytes());
}

fn parsed_size(bytes: &[u8; directory_layout::LEN], version: CompoundVersion) -> Result<u64, CodecError> {
    let entries = parse_record(bytes, version)?;
    Ok(entries[0].live().expect("size fixture is live").size)
}

#[test]
fn accepts_an_unallocated_entry_with_only_nostream_links() {
    let mut bytes = [0_u8; directory_layout::LEN];
    initialize_empty_directory_entries(&mut bytes);
    let entries = parse_record(&bytes, CompoundVersion::V3).expect("valid free entry");
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0], DirectorySlot::Free));
}

#[test]
fn accepts_nonzero_stream_state_bits() {
    let mut bytes = stream_entry();
    bytes[directory_layout::STATE_BITS] = 1;
    let entries = parse_record(&bytes, CompoundVersion::V3)
        .expect("stream State Bits are SHOULD-zero");
    assert!(entries[0].live().is_some());
}

#[test]
fn v3_stream_and_root_accept_the_inclusive_size_limit() {
    let mut stream = stream_entry();
    set_entry_size(&mut stream, 0x8000_0000);
    assert_eq!(
        parsed_size(&stream, CompoundVersion::V3).expect("stream size limit is inclusive"),
        0x8000_0000
    );

    let mut root = root_entry();
    set_entry_size(&mut root, 0x8000_0000);
    assert_eq!(
        parsed_size(&root, CompoundVersion::V3).expect("root mini-stream limit is inclusive"),
        0x8000_0000
    );
}

#[test]
fn v3_stream_and_root_reject_size_above_the_limit() {
    for mut bytes in [stream_entry(), root_entry()] {
        set_entry_size(&mut bytes, 0x8000_0001);
        assert_malformed(
            &bytes,
            CompoundVersion::V3,
            "CFB v3 stream size exceeds 0x80000000",
        );
    }
}

#[test]
fn v3_stream_and_root_ignore_the_high_size_dword_before_the_limit_check() {
    for mut bytes in [stream_entry(), root_entry()] {
        set_entry_size(&mut bytes, 0xdead_beef_8000_0000);
        assert_eq!(
            parsed_size(&bytes, CompoundVersion::V3)
                .expect("v3 ignores the uninitialized high size DWORD"),
            0x8000_0000
        );
    }
}

#[test]
fn v4_stream_and_root_keep_full_sizes_above_the_v3_limit() {
    let size = 0x1_0000_0001;
    for mut bytes in [stream_entry(), root_entry()] {
        set_entry_size(&mut bytes, size);
        assert_eq!(
            parsed_size(&bytes, CompoundVersion::V4).expect("v4 uses the full 64-bit size"),
            size
        );
    }
}

#[test]
fn accepts_meaningful_storage_metadata() {
    let mut bytes = storage_entry();
    bytes[directory_layout::CLSID] = 1;
    bytes[directory_layout::STATE_BITS] = 1;
    bytes[directory_layout::CREATION_TIME] = 1;
    bytes[directory_layout::MODIFIED_TIME] = 1;
    let entries = parse_record(&bytes, CompoundVersion::V3)
        .expect("storage metadata fields are meaningful");
    assert!(entries[0].live().is_some());
}

#[test]
fn accepts_stale_stream_child_clsid_and_timestamps() {
    for version in [CompoundVersion::V3, CompoundVersion::V4] {
        let mut bytes = stream_entry();
        put_u32(&mut bytes, directory_layout::CHILD, 0);
        bytes[directory_layout::CLSID] = 1;
        bytes[directory_layout::CREATION_TIME] = 1;
        bytes[directory_layout::MODIFIED_TIME] = 1;
        let entries = parse_record(&bytes, version).expect("stale stream metadata is accepted");
        let entry = entries[0].live().expect("live stream");
        assert_eq!(entry.child, 0);
        assert_eq!(entry.size, 0);
        assert_eq!(entry.start_sector, END_OF_CHAIN);
    }
}

#[test]
fn accepts_nonzero_storage_start_and_effective_size() {
    let mut bytes = storage_entry();
    put_u32(&mut bytes, directory_layout::START_SECTOR, END_OF_CHAIN);
    set_entry_size(&mut bytes, 0xdead_beef_0000_0001);
    for (version, size) in [(CompoundVersion::V3, 1), (CompoundVersion::V4, 0xdead_beef_0000_0001)] {
        let entries = parse_record(&bytes, version).expect("storage allocation fields are accepted");
        let entry = entries[0].live().expect("live storage");
        assert_eq!(entry.start_sector, END_OF_CHAIN);
        assert_eq!(entry.size, size);
    }
}

#[test]
fn accepts_nonzero_root_creation_time() {
    let mut bytes = root_entry();
    bytes[directory_layout::CREATION_TIME] = 1;
    for version in [CompoundVersion::V3, CompoundVersion::V4] {
        let entries = parse_record(&bytes, version).expect("root creation time is accepted");
        assert_eq!(entries[0].live().expect("live root").name.as_str(), "Root Entry");
    }
}

#[test]
fn accepts_root_clsid_state_bits_and_modified_time() {
    let mut bytes = root_entry();
    bytes[directory_layout::CLSID] = 1;
    bytes[directory_layout::STATE_BITS] = 1;
    bytes[directory_layout::MODIFIED_TIME] = 1;
    let entries = parse_record(&bytes, CompoundVersion::V3)
        .expect("root CLSID, State Bits and Modified Time are meaningful");
    assert!(entries[0].live().is_some());
}

#[test]
fn v3_storage_ignores_the_high_size_dword() {
    let mut v3_storage = storage_entry();
    v3_storage[directory_layout::STREAM_SIZE + 4] = 1;
    let entries = parse_record(&v3_storage, CompoundVersion::V3)
        .expect("v3 ignores the uninitialized high size DWORD");
    assert_eq!(entries[0].live().expect("live storage").size, 0);
}
