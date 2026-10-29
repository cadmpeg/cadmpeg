// SPDX-License-Identifier: Apache-2.0

use crate::native::DatabaseRecord;
use crate::rse::{DatabaseState, RseInventory};
use crate::test_support::given_up_records;
use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::CodecFormat;

#[test]
fn actual_database_values_and_record_reads_share_the_source_stage() {
    let bytes = crate::test_support::test_fixtures::primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("the fixture is a compound file");
    let inventory = RseInventory::build(&ctx, &snapshot).expect("the rse inventory builds");
    let descriptor = &inventory.databases[0];
    let mut output = PartOutput::new(CodecFormat::Inventor);
    assert!(super::stage(&ctx, &mut output, descriptor).expect("the database stage succeeds"));
    let (_, native, fidelity, losses) = output.into_parts();
    let records = native
        .namespace("inventor")
        .expect("the inventor namespace exists")
        .arena_as::<DatabaseRecord>("databases")
        .expect("the databases arena holds database records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "inventor:rse:database#v1");
    assert_eq!(records[0].database_id, "21212121212121212121212121212121");
    assert_eq!(records[0].schema, 31);
    assert_eq!(records[0].created_by.major, 24);
    assert_eq!(records[0].saved_by.major, 25);
    assert_eq!(records[0].created_filetime, 17);
    assert_eq!(records[0].saved_filetime, 18);
    assert_eq!(records[0].note, "synthetic primary document");
    assert!(losses.is_empty());
    assert!(given_up_records(&fidelity).is_empty());
}

#[test]
fn unreadable_database_note_drops_its_actual_record_and_keeps_the_exact_peer() {
    let mut bad = crate::test_support::test_fixtures::primary_envelope_fixture();
    let needle = "synthetic primary document"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let locations = bad
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == needle).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(locations.len(), 1);
    bad[locations[0]..locations[0] + 2].copy_from_slice(&0xd800_u16.to_le_bytes());
    let mut good = crate::test_support::test_fixtures::primary_envelope_fixture();
    let band = good
        .windows(6)
        .position(|bytes| bytes == [b'V', 0, b'1', 0, 0, 0])
        .expect("the fixture holds the v1 band");
    good[band + 2] = b'2';
    let split = bad.len();
    bad.extend_from_slice(&good);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bad, &arena, &DecodePolicy::service())
        .expect("the joined fixtures fit the service policy");
    let first = CompoundSnapshot::new(
        &ctx,
        root.child(0, split)
            .expect("the first fixture lies in the source"),
    )
    .expect("the first fixture is a compound file");
    let second = CompoundSnapshot::new(
        &ctx,
        root.child(split, bad.len())
            .expect("the second fixture lies in the source"),
    )
    .expect("the second fixture is a compound file");
    let failed = RseInventory::build(&ctx, &first).expect("the first rse inventory builds");
    let peer = RseInventory::build(&ctx, &second).expect("the peer rse inventory builds");
    assert!(matches!(
        failed.databases[0].state,
        DatabaseState::Unframed { .. }
    ));
    assert_eq!(peer.databases[0].band.value(), 2);
    let mut output = PartOutput::new(CodecFormat::Inventor);
    assert!(!super::stage(&ctx, &mut output, &failed.databases[0])
        .expect("the failed database stage succeeds"));
    assert!(super::stage(&ctx, &mut output, &peer.databases[0])
        .expect("the peer database stage succeeds"));
    let (_, native, fidelity, losses) = output.into_parts();
    let records = native
        .namespace("inventor")
        .expect("the inventor namespace exists")
        .arena_as::<DatabaseRecord>("databases")
        .expect("the databases arena holds database records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "inventor:rse:database#v2");
    assert_eq!(records[0].schema, 31);
    assert_eq!(records[0].note, "synthetic primary document");
    assert_eq!(records[0].created_filetime, 17);
    assert_eq!(records[0].saved_filetime, 18);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    let given_up = given_up_records(&fidelity);
    assert_eq!(given_up.len(), 1);
    assert_eq!(
        given_up[0].data(),
        Some(
            failed.databases[0]
                .source
                .expect("the failed database keeps its source")
                .window()
        )
    );
}

#[test]
fn cached_unreadable_state_cannot_replace_actual_database_fields() {
    let bytes = crate::test_support::test_fixtures::primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("the fixture is a compound file");
    let mut inventory = RseInventory::build(&ctx, &snapshot).expect("the rse inventory builds");
    inventory.databases[0].state = DatabaseState::Unreadable("stale failure cue".into());
    let mut output = PartOutput::new(CodecFormat::Inventor);
    assert!(super::stage(&ctx, &mut output, &inventory.databases[0])
        .expect("the database stage succeeds"));
    let (_, native, _, losses) = output.into_parts();
    assert_eq!(
        native
            .namespace("inventor")
            .expect("the inventor namespace exists")
            .arena_as::<DatabaseRecord>("databases")
            .expect("the databases arena holds database records")[0]
            .note,
        "synthetic primary document"
    );
    assert!(losses.is_empty());
}
