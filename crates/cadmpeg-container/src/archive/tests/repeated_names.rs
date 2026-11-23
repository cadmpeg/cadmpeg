use super::*;
use crate::layout::{central_header, end_record};

fn unicode_entry(raw: &str, name: &str, payload: &[u8]) -> RawEntry {
    let mut entry = RawEntry::stored(raw, payload);
    entry.extra.extend_from_slice(&0x7075_u16.to_le_bytes());
    entry.extra.extend_from_slice(
        &u16::try_from(5 + name.len())
            .expect("Unicode path fits u16")
            .to_le_bytes(),
    );
    entry.extra.push(1);
    entry
        .extra
        .extend_from_slice(&crc32fast::hash(raw.as_bytes()).to_le_bytes());
    entry.extra.extend_from_slice(name.as_bytes());
    entry
}

#[test]
fn unicode_path_extra_names_the_manifest() {
    for raw in ["manifest-old.json", "notes/水.txt"] {
        let bytes = zip_fixture::archive(&[unicode_entry(raw, "manifest.json", b"{}")]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("root context");
        let ordinary = ArchiveSnapshot::new(&ctx, root).expect("ordinary snapshot");
        let repeated = ArchiveSnapshot::with_repeated_names(&ctx, root).expect("opt-in snapshot");
        assert_eq!(repeated.entries(), ordinary.entries());
        assert_eq!(
            repeated
                .open(&ctx, "manifest.json")
                .expect("manifest")
                .window(),
            b"{}"
        );
    }
}

#[test]
fn repeated_effective_names_keep_the_first_declaration() {
    let bytes = zip_fixture::archive(&[
        unicode_entry("old-a.txt", "notes/水.txt", b"first"),
        unicode_entry("old-b.txt", "notes/水.txt", b"second"),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("root context");
    assert!(ArchiveSnapshot::new(&ctx, root).is_err());
    let snapshot = ArchiveSnapshot::with_repeated_names(&ctx, root).expect("opt-in snapshot");
    assert_eq!(snapshot.entries().len(), 2);
    assert!(snapshot
        .entries()
        .iter()
        .all(|entry| entry.name == "notes/水.txt"));
    assert_eq!(
        snapshot
            .open(&ctx, "notes/水.txt")
            .expect("first member")
            .window(),
        b"first"
    );
    assert_eq!(snapshot.entries()[1].uncompressed_size, 6);
    assert!(snapshot.entries()[0].central_start < snapshot.entries()[1].central_start);
}

#[test]
fn repeated_name_inventory_stops_at_the_admitted_directory_count_and_extent() {
    let mut bytes = zip_fixture::archive(&[
        RawEntry::stored("a", b"first"),
        RawEntry::stored("bb", b"second"),
    ]);
    let end = bytes.len() - end_record::LEN;
    for field in [end_record::DISK_ENTRIES, end_record::ENTRIES] {
        bytes[end + field..end + field + 2].copy_from_slice(&1_u16.to_le_bytes());
    }
    bytes[end + end_record::DIRECTORY_SIZE..end + end_record::DIRECTORY_SIZE + 4].copy_from_slice(
        &u32::try_from(central_header::LEN + 1)
            .expect("size")
            .to_le_bytes(),
    );
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("root context");
    let snapshot = ArchiveSnapshot::with_repeated_names(&ctx, root).expect("admitted inventory");
    assert_eq!(snapshot.entries().len(), 1);
    assert_eq!(snapshot.entries()[0].name, "a");
    assert!(snapshot.entry("bb").is_none());
    assert_eq!(
        snapshot.open(&ctx, "a").expect("first member").window(),
        b"first"
    );
}

#[test]
fn invalid_optional_metadata_is_isolated_to_its_member() {
    for effective in ["notes/time.txt", "notes/水.txt"] {
        let mut damaged = unicode_entry("notes/time.txt", effective, b"x");
        damaged.extra.extend_from_slice(&[0x55, 0x54, 0, 0]);
        let bytes = zip_fixture::archive(&[
            RawEntry::stored("manifest.json", b"{}"),
            damaged,
            RawEntry::stored("geometry.step", b"geometry"),
        ]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("root context");
        let ordinary = ArchiveSnapshot::new(&ctx, root).expect("recovery snapshot");
        assert!(ordinary.recovery_reason().is_some());
        let snapshot = ArchiveSnapshot::with_repeated_names(&ctx, root).expect("opt-in snapshot");
        assert!(snapshot.recovery_reason().is_none());
        assert_eq!(snapshot.entries().len(), 3);
        assert!(snapshot.entry(effective).is_some());
        assert!(snapshot.open(&ctx, effective).is_err());
        assert_eq!(
            snapshot
                .open(&ctx, "manifest.json")
                .expect("readable manifest")
                .window(),
            b"{}"
        );
        assert_eq!(
            snapshot
                .open(&ctx, "geometry.step")
                .expect("readable geometry")
                .window(),
            b"geometry"
        );
    }
}

#[test]
fn repeated_names_recover_a_broken_directory() {
    let mut bytes = zip_fixture::archive(&[RawEntry::stored("manifest.json", b"{}")]);
    let central = bytes
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .expect("central header");
    bytes[central..central + 4].fill(0);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("root context");
    let snapshot = ArchiveSnapshot::with_repeated_names(&ctx, root).expect("recovery snapshot");
    assert!(snapshot.recovery_reason().is_some());
}
