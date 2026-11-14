use super::*;
use crate::test_support::shell_slots::{fixture, scalar, Prefix};

#[test]
fn counted_control_retains_slots_and_source_locations() {
    let (bytes, scope) = fixture(Prefix::Counted(1), Some((93, 0.25)), None, &[91]);
    crate::test_support::with_decode_context(|ctx| {
        let readout = read(ctx, &bytes, &IndexedRecordOffsets::build(&bytes), &scope).unwrap();
        let frames = readout
            .frames
            .iter()
            .filter(|frame| frame.aligned)
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 1);
        let frame = frames[0];
        assert!(frame.form == Form::Counted);
        assert_eq!(frame.control.as_ref().unwrap().value, 1);
        assert_eq!(frame.control.as_ref().unwrap().offset, 26);
        assert_eq!(frame.slots[0].as_ref().unwrap().record, Some(93));
        assert_eq!(frame.slots[0].as_ref().unwrap().offset, 27);
        assert_eq!(frame.slots[1].as_ref().unwrap().record, None);
        assert_eq!(frame.slots[1].as_ref().unwrap().offset, 38);
        assert_eq!(frame.auxiliary.as_ref().unwrap().count.value, 0);
        assert_eq!(frame.auxiliary.as_ref().unwrap().count.offset, 43);
        let groups = frame.groups.as_ref().unwrap();
        assert_eq!(groups.count.offset, 51);
        assert_eq!(groups.entries[0].unwrap().value, 91);
        assert_eq!(groups.entries[0].unwrap().offset, 55);
        assert_eq!(scope.reference_count_offset(), 145);
        assert_eq!(readout.scalars.len(), 1);
        assert_eq!(readout.scalars[0].owner, Some(90));
        assert_eq!(readout.scalars[0].value, 0.25);
        assert_eq!(readout.scalars[0].offset, 540);
    });
}

#[test]
fn cornered_group_count_locates_both_groups_before_the_guid() {
    let (bytes, scope) = fixture(Prefix::Cornered(true), Some((93, 0.125)), None, &[91, 92]);
    crate::test_support::with_decode_context(|ctx| {
        let readout = read(ctx, &bytes, &IndexedRecordOffsets::build(&bytes), &scope).unwrap();
        let frame = readout.frames.iter().find(|frame| frame.aligned).unwrap();
        assert!(frame.form == Form::Cornered);
        assert_eq!(frame.flag.value, 1);
        assert_eq!(frame.flag.offset, 21);
        assert_eq!(frame.rounded.as_ref().unwrap().value, 1);
        let groups = frame.groups.as_ref().unwrap();
        assert_eq!(groups.count.value, 2);
        assert_eq!(groups.count.offset, 47);
        assert_eq!(groups.entries[0].unwrap().value, 91);
        assert_eq!(groups.entries[0].unwrap().offset, 51);
        assert_eq!(groups.entries[1].unwrap().value, 92);
        assert_eq!(groups.entries[1].unwrap().offset, 62);
        assert_eq!(scope.reference_count_offset(), 152);
        assert_eq!(readout.scalars[0].value, 0.125);
    });
}

#[test]
fn oversized_group_count_keeps_the_preceding_wall() {
    let (mut bytes, scope) = fixture(Prefix::Counted(1), Some((93, 0.25)), None, &[91]);
    bytes[51..55].copy_from_slice(&u32::MAX.to_le_bytes());
    crate::test_support::with_decode_context(|ctx| {
        let readout = read(ctx, &bytes, &IndexedRecordOffsets::build(&bytes), &scope).unwrap();
        let frame = readout
            .frames
            .iter()
            .find(|frame| frame.form == Form::Counted)
            .unwrap();
        assert!(!frame.aligned && frame.groups.is_none());
        assert_eq!(frame.slots[0].as_ref().unwrap().record, Some(93));
        assert_eq!(readout.scalars[0].value, 0.25);
    });
}

#[test]
fn ambiguous_scalar_definitions_retain_bounded_fragments() {
    let (mut bytes, scope) = fixture(Prefix::Counted(1), Some((93, 0.25)), None, &[91]);
    scalar(&mut bytes, 93, 90, 0.5);
    crate::test_support::with_decode_context(|ctx| {
        let readout = read(ctx, &bytes, &IndexedRecordOffsets::build(&bytes), &scope).unwrap();
        assert!(readout.scalars.is_empty());
        let fragments = readout
            .fragments
            .iter()
            .filter(|fragment| fragment.record == 93)
            .collect::<Vec<_>>();
        assert_eq!(
            fragments
                .iter()
                .map(|fragment| fragment.offset)
                .collect::<Vec<_>>(),
            [500, 604, 615, 719]
        );
        assert!(fragments
            .iter()
            .all(|fragment| fragment.bytes.as_ref().len() <= 104));
    });
}
