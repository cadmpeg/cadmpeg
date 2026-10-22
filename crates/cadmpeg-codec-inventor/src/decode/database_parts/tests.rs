// SPDX-License-Identifier: Apache-2.0

use crate::native::DatabaseRecord;
use crate::rse::{DatabaseState, RseInventory};
use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::{PartOutput, PartState};
use cadmpeg_ir::CodecFormat;

#[test]
fn actual_database_values_and_record_reads_share_the_source_stage() {
    let bytes = crate::test_support::test_fixtures::primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let snapshot = CompoundSnapshot::new(&ctx, root).unwrap();
    let inventory = RseInventory::build(&ctx, &snapshot).unwrap();
    let descriptor = &inventory.databases[0];
    let expected = ctx.capture_part_extent(descriptor.source.unwrap()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    assert!(super::stage(&ctx, &mut output, descriptor).unwrap());
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let records = native
        .namespace("inventor")
        .unwrap()
        .arena_as::<DatabaseRecord>("databases")
        .unwrap();
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
    assert_eq!(fidelity.parts().len(), 2);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), records[0].id);
    assert_eq!(captures[0].stream(), expected.stream());
    assert_eq!(captures[0].offset(), expected.offset());
    assert_eq!(captures[0].byte_len(), expected.byte_len());
    assert!(!captures[0].reads().is_empty());
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
        .unwrap();
    good[band + 2] = b'2';
    let split = bad.len();
    bad.extend_from_slice(&good);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bad, &arena, &DecodePolicy::service()).unwrap();
    let first = CompoundSnapshot::new(&ctx, root.child(0, split).unwrap()).unwrap();
    let second = CompoundSnapshot::new(&ctx, root.child(split, bad.len()).unwrap()).unwrap();
    let failed = RseInventory::build(&ctx, &first).unwrap();
    let peer = RseInventory::build(&ctx, &second).unwrap();
    assert!(matches!(
        failed.databases[0].state,
        DatabaseState::Unframed { .. }
    ));
    assert_eq!(peer.databases[0].band.value(), 2);
    let mut output = PartOutput::new(CodecFormat::Inventor);
    assert!(!super::stage(&ctx, &mut output, &failed.databases[0]).unwrap());
    assert!(super::stage(&ctx, &mut output, &peer.databases[0]).unwrap());
    let (_, native, mut fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let records = native
        .namespace("inventor")
        .unwrap()
        .arena_as::<DatabaseRecord>("databases")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "inventor:rse:database#v2");
    assert_eq!(records[0].schema, 31);
    assert_eq!(records[0].note, "synthetic primary document");
    assert_eq!(records[0].created_filetime, 17);
    assert_eq!(records[0].saved_filetime, 18);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert!(losses[0].message.contains("inventor:rse:database#v1"));
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp {}))
            .count(),
        1
    );
    fidelity.install_mapped_record_captures(&ctx, Ok).unwrap();
    assert!(!fidelity
        .record_layout()
        .contains_key("inventor:rse:database#v1"));
    assert!(fidelity
        .record_layout()
        .contains_key("inventor:rse:database#v2"));
}

#[test]
fn cached_unreadable_state_cannot_replace_actual_database_fields() {
    let bytes = crate::test_support::test_fixtures::primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let snapshot = CompoundSnapshot::new(&ctx, root).unwrap();
    let mut inventory = RseInventory::build(&ctx, &snapshot).unwrap();
    inventory.databases[0].state = DatabaseState::Unreadable("stale failure cue".into());
    let mut output = PartOutput::new(CodecFormat::Inventor);
    assert!(super::stage(&ctx, &mut output, &inventory.databases[0]).unwrap());
    let (_, native, _, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert_eq!(
        native
            .namespace("inventor")
            .unwrap()
            .arena_as::<DatabaseRecord>("databases")
            .unwrap()[0]
            .note,
        "synthetic primary document"
    );
    assert!(losses.is_empty());
}
