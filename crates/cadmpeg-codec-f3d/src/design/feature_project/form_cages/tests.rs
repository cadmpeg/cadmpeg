// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

use cadmpeg_core::decode::index_from_u32;

use super::{
    form_body_references, form_cage_serializers, form_cage_surface, form_class_325_cage_surface,
    form_class_328_envelope, form_typed_serializers, FormCageSerializers, FormRecordTypes,
};
use cadmpeg_ir::features::form::*;

mod final_boundaries;
mod reference_groups;
mod resource_associations;
mod review;
mod review1368;
mod tracking;

fn retain_ungrouped(
    old: &FormConstruction,
    new: &FormConstruction,
) -> Result<FormConstruction, cadmpeg_core::CodecError> {
    crate::design::test_support::with_test_decode_context(|ctx| {
        super::retain_knowledge(ctx, old, new, &super::ReferenceGroups::default(), None)
    })
}

fn form_cage_objects(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    record: u32,
    owner: u32,
) -> Option<Vec<Option<u64>>> {
    crate::design::test_support::with_test_decode_context(|ctx| {
        super::form_cage_objects(ctx, bytes, records, record, owner)
    })
    .unwrap()
}

fn parsed_serializers(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
) -> FormCageSerializers {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    form_cage_serializers(&ctx, bytes, records).unwrap()
}

fn indexed_frame(class: &[u8; 3], record_index: u32, length: usize) -> Vec<u8> {
    let mut frame = vec![0; length];
    frame[..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(class);
    frame[7..11].copy_from_slice(&record_index.to_le_bytes());
    frame
}

#[test]
fn reads_owned_cage_objects() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"402");
    bytes.extend_from_slice(&2196u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.push(1);
    bytes.extend_from_slice(&2190u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for reference in [8300u64, 8303] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    bytes.resize(110, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"264");
    bytes.extend_from_slice(&2196u64.to_le_bytes());
    assert_eq!(
        form_cage_objects(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            2196,
            2190
        ),
        Some(vec![Some(8300), Some(8303)])
    );
    let mut alternate_pair = bytes.clone();
    alternate_pair[110 + 4..110 + 7].copy_from_slice(b"258");
    assert_eq!(
        form_cage_objects(
            &alternate_pair,
            &crate::design::test_support::indexed_record_offsets_for_test(&alternate_pair),
            2196,
            2190
        ),
        Some(vec![Some(8300), Some(8303)])
    );

    let mut empty = Vec::new();
    empty.extend_from_slice(&3u32.to_le_bytes());
    empty.extend_from_slice(b"402");
    empty.extend_from_slice(&2196u64.to_le_bytes());
    empty.extend_from_slice(&[0; 6]);
    empty.push(1);
    empty.extend_from_slice(&2190u64.to_le_bytes());
    empty.extend_from_slice(&[0; 2]);
    empty.extend_from_slice(&0u32.to_le_bytes());
    empty.resize(88, 0);
    empty.extend_from_slice(&3u32.to_le_bytes());
    empty.extend_from_slice(b"264");
    empty.extend_from_slice(&2196u64.to_le_bytes());
    assert_eq!(
        form_cage_objects(
            &empty,
            &crate::design::test_support::indexed_record_offsets_for_test(&empty),
            2196,
            2190
        ),
        Some(Vec::new())
    );
}

#[test]
fn reads_single_cage_list_with_opaque_tail() {
    let mut list = indexed_frame(b"415", 2196, 99);
    list[21] = 1;
    list[22..30].copy_from_slice(&2190u64.to_le_bytes());
    list[32..36].copy_from_slice(&1u32.to_le_bytes());
    list[36] = 1;
    list[37..45].copy_from_slice(&8300u64.to_le_bytes());
    list[47..99].fill(0xa5);
    let paired = indexed_frame(b"258", 2196, 15);
    let bytes = [list, paired].concat();

    assert_eq!(
        form_cage_objects(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            2196,
            2190
        ),
        Some(vec![Some(8300)])
    );
}

#[test]
fn resolves_cage_surface_through_owned_object_chain() {
    let mut object = indexed_frame(b"301", 8300, 200);
    object[189] = 1;
    object[190..198].copy_from_slice(&8301u64.to_le_bytes());
    let mut first_wrapper = indexed_frame(b"373", 8301, 33);
    first_wrapper[21] = 1;
    first_wrapper[22..30].copy_from_slice(&8302u64.to_le_bytes());
    let mut second_wrapper = indexed_frame(b"362", 8302, 29);
    second_wrapper[21..29].copy_from_slice(&8303u64.to_le_bytes());
    let mut carrier = indexed_frame(b"457", 8303, 665);
    carrier[317] = 1;
    carrier[318..326].copy_from_slice(&2190u64.to_le_bytes());
    carrier[339] = 1;
    carrier[340..348].copy_from_slice(&8304u64.to_le_bytes());
    let paired = indexed_frame(b"264", 8303, 15);
    let bytes = [object, first_wrapper, second_wrapper, carrier, paired].concat();
    assert_eq!(
        form_cage_surface(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            8300,
            &mut Vec::new(),
        ),
        Some((2190, 8304))
    );
}

#[test]
fn form_cage_surfaces_retain_the_actual_owner_for_membership_admission() {
    let mut object = indexed_frame(b"301", 8300, 200);
    object[189] = 1;
    object[190..198].copy_from_slice(&8301u64.to_le_bytes());
    let mut first_wrapper = indexed_frame(b"373", 8301, 33);
    first_wrapper[21] = 1;
    first_wrapper[22..30].copy_from_slice(&8302u64.to_le_bytes());
    let mut second_wrapper = indexed_frame(b"362", 8302, 29);
    second_wrapper[21..29].copy_from_slice(&8303u64.to_le_bytes());
    let mut carrier = indexed_frame(b"457", 8303, 665);
    carrier[317] = 1;
    carrier[318..326].copy_from_slice(&2190u64.to_le_bytes());
    carrier[339] = 1;
    carrier[340..348].copy_from_slice(&8304u64.to_le_bytes());
    let paired = indexed_frame(b"264", 8303, 15);
    let bytes = [object, first_wrapper, second_wrapper, carrier, paired].concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert_eq!(
        form_cage_surface(&bytes, &records, 8300, &mut Vec::new()),
        Some((2190, 8304))
    );
}

#[test]
fn serializer_joins_surface_to_exact_cage_entry_name() {
    let entry_name = "TSpline.00000000-0000-0000-0000-000000000000.tsm";
    for class in [b"315", b"349", b"360", b"431", b"446"] {
        let mut serializer = indexed_frame(class, 8305, 132);
        serializer[21..25].copy_from_slice(&48u32.to_le_bytes());
        for (ordinal, code_unit) in entry_name.encode_utf16().enumerate() {
            let at = 25 + ordinal * 2;
            serializer[at..at + 2].copy_from_slice(&code_unit.to_le_bytes());
        }
        serializer[121] = 1;
        serializer[122..130].copy_from_slice(&8304u64.to_le_bytes());
        let following = indexed_frame(b"457", 8306, 15);
        let bytes = [serializer, following].concat();
        assert_eq!(
            parsed_serializers(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            )
            .entry_name(8304),
            Some(entry_name)
        );
    }
}

#[test]
fn form_serializer_unicode_name_round_trips() {
    let entry_name = format!("TSpline.{}😀.tsm", "é".repeat(34));
    assert_eq!(entry_name.encode_utf16().count(), 48);
    let mut serializer = indexed_frame(b"315", 8305, 132);
    serializer[21..25].copy_from_slice(&48u32.to_le_bytes());
    for (ordinal, code_unit) in entry_name.encode_utf16().enumerate() {
        let at = 25 + ordinal * 2;
        serializer[at..at + 2].copy_from_slice(&code_unit.to_le_bytes());
    }
    serializer[121] = 1;
    serializer[122..130].copy_from_slice(&8304u64.to_le_bytes());
    let following = indexed_frame(b"457", 8306, 15);
    let bytes = [serializer, following].concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert_eq!(
        parsed_serializers(&bytes, &records).entry_name(8304),
        Some(entry_name.as_str())
    );
}

#[test]
fn serializer_joins_class_335_surface_with_class_331_pair() {
    let entry_name = "TSpline.00000000-0000-0000-0000-000000000000.tsm";
    let mut serializer = indexed_frame(b"335", 8305, 132);
    serializer[21..25].copy_from_slice(&48u32.to_le_bytes());
    for (ordinal, code_unit) in entry_name.encode_utf16().enumerate() {
        let at = 25 + ordinal * 2;
        serializer[at..at + 2].copy_from_slice(&code_unit.to_le_bytes());
    }
    serializer[121] = 1;
    serializer[122..130].copy_from_slice(&8304u64.to_le_bytes());
    let following = indexed_frame(b"331", 8306, 15);
    let surface = indexed_frame(b"358", 8304, 15);
    let bytes = [serializer, following, surface].concat();
    assert_eq!(
        parsed_serializers(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        )
        .entry_name(8304),
        Some(entry_name)
    );

    let mut wrong_pair = bytes.clone();
    wrong_pair[132 + 4..132 + 7].copy_from_slice(b"457");
    assert!(!parsed_serializers(
        &wrong_pair,
        &crate::design::test_support::indexed_record_offsets_for_test(&wrong_pair),
    )
    .ordered
    .contains(&8304));

    let mut nonzero_tail = bytes;
    nonzero_tail[131] = 1;
    assert!(!parsed_serializers(
        &nonzero_tail,
        &crate::design::test_support::indexed_record_offsets_for_test(&nonzero_tail),
    )
    .ordered
    .contains(&8304));
}

#[test]
fn serializers_preserve_primary_frame_order() {
    let names = [
        "TSpline.00000000-0000-0000-0000-000000000000.tsm",
        "TSpline.11111111-1111-1111-1111-111111111111.tsm",
    ];
    let mut chunks = Vec::new();
    for (ordinal, (name, surface)) in names.iter().zip([8304u64, 8307]).enumerate() {
        let record = 8305 + u32::try_from(ordinal).expect("fixture value fits u32") * 2;
        let mut serializer = indexed_frame(b"315", record, 132);
        serializer[21..25].copy_from_slice(&48u32.to_le_bytes());
        for (name_ordinal, code_unit) in name.encode_utf16().enumerate() {
            let at = 25 + name_ordinal * 2;
            serializer[at..at + 2].copy_from_slice(&code_unit.to_le_bytes());
        }
        serializer[121] = 1;
        serializer[122..130].copy_from_slice(&surface.to_le_bytes());
        chunks.push(serializer);
        chunks.push(indexed_frame(b"457", record + 1, 15));
    }
    let bytes = chunks.concat();
    let serializers = parsed_serializers(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
    );
    assert_eq!(serializers.ordered, vec![8304, 8307]);
}

fn class_328_form_fixture() -> (
    Vec<u8>,
    crate::design::decode::sketch::IndexedRecordOffsets,
    crate::records::feature::scope::DesignParameterScope,
) {
    let scope_record = 201;
    let group_record = 205;
    let metadata_record = 207;
    let scope_frame = indexed_frame(
        b"328",
        scope_record,
        crate::layout::form_class_328_scope::LEN,
    );
    let scope_paired = indexed_frame(b"267", scope_record, 15);

    let mut group = indexed_frame(
        b"417",
        group_record,
        crate::layout::form_class_328_cage_group::LEN,
    );
    group[crate::layout::form_class_328_cage_group::OWNER_MARKER] = 1;
    group[crate::layout::form_class_328_cage_group::OWNER_SCOPE_RECORD_INDEX
        ..crate::layout::form_class_328_cage_group::OWNER_SCOPE_RECORD_INDEX + 8]
        .copy_from_slice(&u64::from(scope_record).to_le_bytes());
    group[crate::layout::form_class_328_cage_group::MEMBER_COUNT
        ..crate::layout::form_class_328_cage_group::MEMBER_COUNT + 4]
        .copy_from_slice(&4u32.to_le_bytes());
    group[crate::layout::form_class_328_cage_group::TERMINAL_U32
        ..crate::layout::form_class_328_cage_group::TERMINAL_U32 + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    group[crate::layout::form_class_328_cage_group::FIRST_TAIL_MARKER] = 1;
    group[crate::layout::form_class_328_cage_group::FIRST_TAIL_RECORD_INDEX
        ..crate::layout::form_class_328_cage_group::FIRST_TAIL_RECORD_INDEX + 8]
        .copy_from_slice(&206u64.to_le_bytes());
    group[crate::layout::form_class_328_cage_group::SECOND_TAIL_MARKER] = 1;
    group[crate::layout::form_class_328_cage_group::SECOND_TAIL_RECORD_INDEX
        ..crate::layout::form_class_328_cage_group::SECOND_TAIL_RECORD_INDEX + 8]
        .copy_from_slice(&208u64.to_le_bytes());
    group[crate::layout::form_class_328_cage_group::FINAL_SCOPE_MARKER] = 1;
    group[crate::layout::form_class_328_cage_group::FINAL_SCOPE_RECORD_INDEX
        ..crate::layout::form_class_328_cage_group::FINAL_SCOPE_RECORD_INDEX + 8]
        .copy_from_slice(&u64::from(scope_record).to_le_bytes());
    let group_paired = indexed_frame(b"267", group_record, 15);

    let mut metadata = indexed_frame(
        b"341",
        metadata_record,
        crate::layout::form_class_328_metadata_group::LEN,
    );
    metadata[crate::layout::form_class_328_metadata_group::OWNER_MARKER] = 1;
    metadata[crate::layout::form_class_328_metadata_group::OWNER_SCOPE_RECORD_INDEX
        ..crate::layout::form_class_328_metadata_group::OWNER_SCOPE_RECORD_INDEX + 8]
        .copy_from_slice(&u64::from(scope_record).to_le_bytes());
    metadata[crate::layout::form_class_328_metadata_group::MEMBER_COUNT
        ..crate::layout::form_class_328_metadata_group::MEMBER_COUNT + 4]
        .copy_from_slice(&19u32.to_le_bytes());
    metadata[crate::layout::form_class_328_metadata_group::TAIL_U32
        ..crate::layout::form_class_328_metadata_group::TAIL_U32 + 4]
        .copy_from_slice(&61u32.to_le_bytes());
    metadata[crate::layout::form_class_328_metadata_group::TAIL_SCALAR
        ..crate::layout::form_class_328_metadata_group::TAIL_SCALAR + 8]
        .copy_from_slice(&1.0f64.to_le_bytes());
    metadata[crate::layout::form_class_328_metadata_group::FIRST_TAIL_MARKER] = 1;
    metadata[crate::layout::form_class_328_metadata_group::FIRST_TAIL_RECORD_INDEX
        ..crate::layout::form_class_328_metadata_group::FIRST_TAIL_RECORD_INDEX + 8]
        .copy_from_slice(&209u64.to_le_bytes());
    metadata[crate::layout::form_class_328_metadata_group::SECOND_TAIL_MARKER] = 1;
    metadata[crate::layout::form_class_328_metadata_group::SECOND_TAIL_RECORD_INDEX
        ..crate::layout::form_class_328_metadata_group::SECOND_TAIL_RECORD_INDEX + 8]
        .copy_from_slice(&210u64.to_le_bytes());
    metadata[crate::layout::form_class_328_metadata_group::FINAL_SCOPE_MARKER] = 1;
    metadata[crate::layout::form_class_328_metadata_group::FINAL_SCOPE_RECORD_INDEX
        ..crate::layout::form_class_328_metadata_group::FINAL_SCOPE_RECORD_INDEX + 8]
        .copy_from_slice(&u64::from(scope_record).to_le_bytes());
    let metadata_paired = indexed_frame(b"267", metadata_record, 15);

    let mut member_frames = Vec::new();
    for (ordinal, member_record) in (301u32..305).enumerate() {
        let mut member = indexed_frame(b"350", member_record, 40);
        member[27] = 1;
        member[28..36].copy_from_slice(&u64::from(group_record).to_le_bytes());
        member[39] = 1;
        group[crate::layout::form_class_328_cage_group::MEMBER_ENTRIES
            + ordinal * crate::layout::form_class_328_reference_entry::LEN] = 1;
        group[crate::layout::form_class_328_cage_group::MEMBER_ENTRIES
            + ordinal * crate::layout::form_class_328_reference_entry::LEN
            + crate::layout::form_class_328_reference_entry::RECORD_INDEX
            ..crate::layout::form_class_328_cage_group::MEMBER_ENTRIES
                + ordinal * crate::layout::form_class_328_reference_entry::LEN
                + crate::layout::form_class_328_reference_entry::RECORD_INDEX
                + 8]
            .copy_from_slice(&u64::from(member_record).to_le_bytes());
        member_frames.push(member);
        member_frames.push(indexed_frame(b"351", member_record, 15));
    }

    for (ordinal, member_record) in (4000u32..4019).enumerate() {
        metadata[crate::layout::form_class_328_metadata_group::MEMBER_ENTRIES
            + ordinal * crate::layout::form_class_328_reference_entry::LEN] = 1;
        metadata[crate::layout::form_class_328_metadata_group::MEMBER_ENTRIES
            + ordinal * crate::layout::form_class_328_reference_entry::LEN
            + crate::layout::form_class_328_reference_entry::RECORD_INDEX
            ..crate::layout::form_class_328_metadata_group::MEMBER_ENTRIES
                + ordinal * crate::layout::form_class_328_reference_entry::LEN
                + crate::layout::form_class_328_reference_entry::RECORD_INDEX
                + 8]
            .copy_from_slice(&u64::from(member_record).to_le_bytes());
    }

    let mut chunks = vec![
        scope_frame,
        scope_paired,
        group,
        group_paired,
        metadata,
        metadata_paired,
    ];
    chunks.extend(member_frames);
    chunks.extend([
        indexed_frame(b"272", 206, 15),
        indexed_frame(b"404", 208, 15),
        indexed_frame(b"259", 209, 15),
        indexed_frame(b"404", 210, 15),
    ]);
    chunks.extend((4000..4019).map(|record| indexed_frame(b"320", record, 15)));
    let bytes = chunks.concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Form,
        scope_record,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                group_record,
                metadata_record,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, records, scope)
}

#[test]
fn reads_class_328_form_envelope() {
    let (bytes, records, scope) = class_328_form_fixture();
    assert!(form_class_328_envelope(&bytes, &records, &scope));

    let mut wrong_pair = bytes;
    let group_start = crate::layout::form_class_328_scope::LEN + 15;
    let paired_class = group_start + crate::layout::form_class_328_cage_group::LEN + 4;
    wrong_pair[paired_class..paired_class + 3].copy_from_slice(b"266");
    assert!(!form_class_328_envelope(
        &wrong_pair,
        &crate::design::test_support::indexed_record_offsets_for_test(&wrong_pair),
        &scope,
    ));
}

#[test]
fn reads_class_325_cage_table_entries() {
    let scope_record = 309;
    let owner_record = 315;
    let mut table = indexed_frame(b"325", scope_record, 1850);
    table[20] = 1;
    table[26] = 1;
    table[27..35].copy_from_slice(&(u64::from(owner_record)).to_le_bytes());
    table[37..41].copy_from_slice(&32u32.to_le_bytes());
    let mut object_records = Vec::new();
    for ordinal in 0..32u32 {
        let object_record = 1_000 + ordinal * 2;
        let companion_record = 2_000 + ordinal * 2;
        let entry = 41 + index_from_u32(ordinal) * 30;
        table[entry] = 1;
        table[entry + 1..entry + 9].copy_from_slice(&(u64::from(object_record)).to_le_bytes());
        table[entry + 11..entry + 19].copy_from_slice(&(307u64 + u64::from(ordinal)).to_le_bytes());
        table[entry + 19] = 1;
        table[entry + 20..entry + 28].copy_from_slice(&(u64::from(companion_record)).to_le_bytes());
        object_records.extend([
            indexed_frame(b"289", object_record, 15),
            indexed_frame(b"258", object_record, 15),
            indexed_frame(b"273", companion_record, 15),
        ]);
    }
    let bytes = [
        table,
        indexed_frame(b"258", scope_record, 15),
        indexed_frame(b"407", owner_record, 15),
        object_records.concat(),
    ]
    .concat();
    let slots = super::form_class_325_cage_objects(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        scope_record,
        [owner_record].into_iter(),
    )
    .unwrap()
    .unwrap();
    assert!(slots.iter().all(|slot| slot.valid));
    assert_eq!(
        slots.iter().map(|slot| slot.object).collect::<Vec<_>>(),
        (0..32u64)
            .map(|ordinal| Some(1_000 + ordinal * 2))
            .collect::<Vec<_>>()
    );
    let mut duplicate_discriminator = bytes.clone();
    duplicate_discriminator[41 + 30 + 11..41 + 30 + 19].copy_from_slice(&307u64.to_le_bytes());
    let slots = super::form_class_325_cage_objects(
        &duplicate_discriminator,
        &crate::design::test_support::indexed_record_offsets_for_test(&duplicate_discriminator),
        scope_record,
        [owner_record].into_iter(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(slots.len(), 32);
    assert!(!slots[0].valid && !slots[1].valid);
    assert!(slots[2..].iter().all(|slot| slot.valid));
    assert_eq!(slots[0].object, Some(1_000));
    assert_eq!(slots[1].object, Some(1_002));
}

#[test]
fn resolves_class_325_cage_surface_from_unique_class_310_reference() {
    let mut object = indexed_frame(b"289", 1_000, 80);
    object[20] = 1;
    object[21..25].copy_from_slice(&700u32.to_le_bytes());
    let paired = indexed_frame(b"258", 1_000, 15);
    let surface = indexed_frame(b"310", 700, 15);
    let bytes = [object, paired, surface].concat();
    assert_eq!(
        form_class_325_cage_surface(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            1_000,
        ),
        vec![700]
    );
}

#[test]
fn form_review_reproduces_missing_legacy_companion() {
    let mut table = indexed_frame(b"325", 309, 1850);
    table[20] = 1;
    table[26] = 1;
    table[27..35].copy_from_slice(&315u64.to_le_bytes());
    table[37..41].copy_from_slice(&32u32.to_le_bytes());
    let mut records = Vec::new();
    for ordinal in 0..32u32 {
        let object = 1000 + ordinal * 2;
        let companion = 2000 + ordinal * 2;
        let entry = 41 + index_from_u32(ordinal) * 30;
        table[entry] = 1;
        table[entry + 1..entry + 9].copy_from_slice(&u64::from(object).to_le_bytes());
        table[entry + 11..entry + 19].copy_from_slice(&(307u64 + u64::from(ordinal)).to_le_bytes());
        table[entry + 19] = 1;
        table[entry + 20..entry + 28].copy_from_slice(&u64::from(companion).to_le_bytes());
        records.push(indexed_frame(b"289", object, 15));
        records.push(indexed_frame(b"258", object, 15));
        if ordinal != 1 {
            records.push(indexed_frame(b"273", companion, 15));
        }
    }
    let bytes = [
        table,
        indexed_frame(b"258", 309, 15),
        indexed_frame(b"407", 315, 15),
        records.concat(),
    ]
    .concat();
    let offsets = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let slots = super::form_class_325_cage_objects(&bytes, &offsets, 309, [315].into_iter())
        .unwrap()
        .unwrap();
    assert_eq!(slots.len(), 32);
    assert!(slots[0].valid && slots[2].valid);
    assert!(!slots[1].valid);
    assert_eq!(slots[1].object, Some(1002));
    assert_eq!(slots[1].companion, Some(2002));
}

#[test]
fn form_review_reproduces_ambiguous_legacy_surfaces() {
    let mut object = indexed_frame(b"289", 1000, 80);
    object[20] = 1;
    object[21..25].copy_from_slice(&700u32.to_le_bytes());
    object[40] = 1;
    object[41..45].copy_from_slice(&701u32.to_le_bytes());
    let bytes = [
        object,
        indexed_frame(b"258", 1000, 15),
        indexed_frame(b"310", 700, 15),
        indexed_frame(b"310", 701, 15),
    ]
    .concat();
    let offsets = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert_eq!(
        form_class_325_cage_surface(&bytes, &offsets, 1000),
        [700, 701]
    );
}

#[test]
fn legacy_catalog_keeps_siblings_and_all_ambiguous_surface_paths() {
    use std::io::{Cursor, Write};
    const THIRD: &str = "33333333-3333-4333-8333-333333333333";
    for (ambiguous, invalid_object) in [(false, false), (true, false), (false, true), (true, true)]
    {
        let mut table = indexed_frame(b"325", 309, 1850);
        table[20] = 1;
        table[26] = 1;
        table[27..35].copy_from_slice(&315u64.to_le_bytes());
        table[37..41].copy_from_slice(&32u32.to_le_bytes());
        let mut entries = Vec::new();
        for ordinal in 0..32u32 {
            let object = 1000 + ordinal * 2;
            let companion = 2000 + ordinal * 2;
            let at = 41 + index_from_u32(ordinal) * 30;
            table[at] = if invalid_object && ordinal == 1 { 2 } else { 1 };
            table[at + 1..at + 9].copy_from_slice(&u64::from(object).to_le_bytes());
            table[at + 11..at + 19].copy_from_slice(&(307u64 + u64::from(ordinal)).to_le_bytes());
            table[at + 19] = 1;
            table[at + 20..at + 28].copy_from_slice(&u64::from(companion).to_le_bytes());
            let mut frame = indexed_frame(b"289", object, 80);
            if ordinal == 0 || ordinal == 2 {
                frame[20] = 1;
                frame[21..25]
                    .copy_from_slice(&(if ordinal == 0 { 700u32 } else { 702u32 }).to_le_bytes());
            }
            if ordinal == 0 && ambiguous {
                frame[40] = 1;
                frame[41..45].copy_from_slice(&701u32.to_le_bytes());
            }
            entries.push(frame);
            entries.push(indexed_frame(b"258", object, 15));
            if ordinal != 1 {
                entries.push(indexed_frame(b"273", companion, 15));
            }
        }
        let mut serializers = Vec::new();
        for (record, surface, uuid) in [
            (800, 700, FIRST_CAGE_UUID),
            (801, 701, SECOND_CAGE_UUID),
            (802, 702, THIRD),
        ] {
            let mut serializer = form_serializer_frame(record, surface, uuid);
            serializer[4..7].copy_from_slice(b"315");
            serializers.push(serializer);
        }
        let bytes = [
            table,
            indexed_frame(b"258", 309, 15),
            indexed_frame(b"407", 315, 15),
            entries.concat(),
            indexed_frame(b"310", 700, 15),
            indexed_frame(b"310", 701, 15),
            indexed_frame(b"310", 702, 15),
            serializers.concat(),
            indexed_frame(b"258", 999, 15),
        ]
        .concat();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, options);
        for uuid in [FIRST_CAGE_UUID, SECOND_CAGE_UUID, THIRD] {
            archive
                .start_file(
                    format!("FusionAssetName[Active]/TSplines.BlobParts/TSpline.{uuid}.tsm"),
                    options,
                )
                .unwrap();
            archive.write_all(b"synthetic").unwrap();
        }
        let archive = archive.finish().unwrap().into_inner();
        let mut scope = form_scope(309, vec![315]);
        scope.class_tag = "325".to_owned().try_into().unwrap();
        let cages = [
            neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
            neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID),
            neutral_form_cage("f3d:model:subd#3", THIRD),
        ];
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            crate::design::test_support::with_test_decode_context(|ctx| {
                let mut facts = super::FormFacts {
                    inputs: super::form_inputs(&scope)?,
                    observations: Vec::new(),
                };
                let source = super::FormSource {
                    bytes: &bytes,
                    records: &records,
                    scope: &scope,
                    cages: &cages,
                    stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
                };
                let membership = super::collect_membership(
                    ctx,
                    scan,
                    source,
                    std::slice::from_ref(&scope),
                    &mut facts,
                    &mut super::ReferenceGroups::default(),
                    &mut Vec::new(),
                )?;
                let result =
                    FormConstruction::try_new(facts.inputs, facts.observations, membership)
                        .unwrap();
                assert!(!result.membership_is_complete());
                assert_eq!(result.inputs().lists[0].declared_count, 32);
                assert_eq!(
                    result.inputs().lists[0].members.len(),
                    if invalid_object { 31 } else { 32 }
                );
                let companions = result
                    .inputs()
                    .lists
                    .iter()
                    .find(|list| list.role == FormListRole::CageCatalogCompanions)
                    .unwrap();
                assert_eq!(companions.declared_count, 32);
                assert!(companions
                    .members
                    .iter()
                    .any(|member| member.ordinal == 1
                        && member.reference.as_str().ends_with("#2002")));
                assert!(result
                    .committed_cages()
                    .any(|id| id.as_str() == "f3d:model:subd#3"));
                assert_eq!(
                    result
                        .committed_cages()
                        .any(|id| id.as_str() == "f3d:model:subd#1"),
                    !ambiguous
                );
                let expected = if ambiguous {
                    vec![700u64, 701, 702]
                } else {
                    vec![700u64, 702]
                };
                for surface in expected {
                    assert!(result
                        .observations()
                        .iter()
                        .flat_map(|observation| &observation.paths)
                        .any(|path| path
                            .references
                            .iter()
                            .any(|id| id.as_str().ends_with(&format!("#{surface}")))
                            && path.references.iter().any(|id| id
                                .as_str()
                                .ends_with(&format!("#{}", 800 + surface - 700)))));
                }
                Ok::<(), cadmpeg_core::CodecError>(())
            })
        })
        .unwrap();
    }
}

#[test]
fn duplicate_surface_serializers_stay_ambiguous() {
    let entry_name = "TSpline.00000000-0000-0000-0000-000000000000.tsm";
    let mut chunks = Vec::new();
    for (ordinal, surface) in [8304u64, 8307, 8304, 8304].into_iter().enumerate() {
        let record = 8400 + u32::try_from(ordinal).expect("fixture value fits u32") * 2;
        let mut serializer = indexed_frame(b"315", record, 132);
        serializer[21..25].copy_from_slice(&48u32.to_le_bytes());
        for (name_ordinal, code_unit) in entry_name.encode_utf16().enumerate() {
            let at = 25 + name_ordinal * 2;
            serializer[at..at + 2].copy_from_slice(&code_unit.to_le_bytes());
        }
        serializer[121] = 1;
        serializer[122..130].copy_from_slice(&surface.to_le_bytes());
        chunks.push(serializer);
        chunks.push(indexed_frame(b"457", record + 1, 15));
    }
    let bytes = chunks.concat();
    let serializers = parsed_serializers(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
    );
    assert_eq!(serializers.ordered, vec![8304, 8307]);
    assert_eq!(serializers.entry_name(8304), None);
    assert_eq!(serializers.entry_name(8307), Some(entry_name));
}

#[test]
fn class_328_form_group_rejects_each_duplicate_member_without_heap_growth() {
    use crate::layout::{
        form_class_328_cage_group as group, form_class_328_reference_entry as entry,
        form_class_328_scope as scope_layout,
    };
    let (bytes, records, scope) = class_328_form_fixture();
    assert!(form_class_328_envelope(&bytes, &records, &scope));
    for ordinal in 1..4 {
        let mut duplicate = bytes.clone();
        let at = scope_layout::LEN
            + 15
            + group::MEMBER_ENTRIES
            + ordinal * entry::LEN
            + entry::RECORD_INDEX;
        duplicate[at..at + 8].copy_from_slice(&301u64.to_le_bytes());
        assert!(
            !form_class_328_envelope(&duplicate, &records, &scope),
            "duplicate group member {ordinal}"
        );
    }
}

#[test]
fn class_328_form_metadata_rejects_each_duplicate_member_without_heap_growth() {
    use crate::layout::{
        form_class_328_cage_group as group, form_class_328_metadata_group as metadata,
        form_class_328_reference_entry as entry, form_class_328_scope as scope_layout,
    };
    let (bytes, records, scope) = class_328_form_fixture();
    assert!(form_class_328_envelope(&bytes, &records, &scope));
    for ordinal in 1..19 {
        let mut duplicate = bytes.clone();
        let at = scope_layout::LEN
            + 15
            + group::LEN
            + 15
            + metadata::MEMBER_ENTRIES
            + ordinal * entry::LEN
            + entry::RECORD_INDEX;
        duplicate[at..at + 8].copy_from_slice(&4000u64.to_le_bytes());
        assert!(
            !form_class_328_envelope(&duplicate, &records, &scope),
            "duplicate metadata member {ordinal}"
        );
    }
}

fn push_lp_utf16(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(
        &u32::try_from(text.encode_utf16().count())
            .unwrap()
            .to_le_bytes(),
    );
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

fn push_marked(out: &mut Vec<u8>, record_index: u32) {
    out.push(1);
    out.extend_from_slice(&u64::from(record_index).to_le_bytes());
    out.extend_from_slice(&[0, 0]);
}

fn expected_local_run(
    values: [Option<u64>; 4],
    inline: Option<&str>,
) -> [Option<super::FormReference>; 4] {
    values.map(|target| {
        target.map(|target| crate::bytes::Reference::Local {
            target,
            inline_type_guid: inline.map(str::to_owned),
        })
    })
}

fn push_material_envelope(out: &mut Vec<u8>, token: &str) {
    push_lp_utf16(out, "00000000-0000-4000-8000-000000000001");
    push_lp_utf16(out, "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C");
    out.extend_from_slice(&0u32.to_le_bytes());
    push_lp_utf16(out, token);
}

fn form_body_frame(
    body: u32,
    prefix: usize,
    references: [u32; 4],
    second_run: Option<[u32; 4]>,
) -> Vec<u8> {
    let mut frame = indexed_frame(b"274", body, 11);
    frame.extend(std::iter::repeat_n(0xa5, prefix));
    push_material_envelope(&mut frame, "PrismMaterial-018");
    for reference in references {
        push_marked(&mut frame, reference);
    }
    push_material_envelope(
        &mut frame,
        if second_run.is_some() {
            "PrismMaterial-019"
        } else {
            "PrismMaterial-018_physmat_aspects:Prism-256"
        },
    );
    match second_run {
        Some(run) => {
            for reference in run {
                push_marked(&mut frame, reference);
            }
        }
        None => frame.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0x3f]),
    }
    frame.extend_from_slice(&indexed_frame(b"258", body, 30));
    frame
}

fn form_serializer_frame(serializer: u32, surface: u32, uuid: &str) -> Vec<u8> {
    let mut frame = indexed_frame(b"377", serializer, 21);
    push_lp_utf16(&mut frame, &format!("TSpline.{uuid}.tsm"));
    push_marked(&mut frame, surface);
    frame
}

fn form_selection_group_frame(group: u32, owner: u32, member_count: u32) -> Vec<u8> {
    let mut frame = indexed_frame(b"429", group, 89);
    frame[21] = 1;
    frame[22..30].copy_from_slice(&u64::from(owner).to_le_bytes());
    frame[32..36].copy_from_slice(&member_count.to_le_bytes());
    frame.extend_from_slice(&indexed_frame(b"264", group, 30));
    frame
}

fn form_meta(rows: &[(&str, u32, &[u32])]) -> crate::metastream::MetaStream {
    crate::metastream::MetaStream {
        types: rows
            .iter()
            .map(
                |&(type_guid, version, entities)| crate::records::entity_header::SegmentTypeData {
                    byte_offset: 0,
                    type_guid: type_guid.to_owned().try_into().unwrap(),
                    type_guid_offset: 0,
                    base_type_guid: if type_guid == "FA1F0081-C9A1-4E14-8F25-D12DD868ED90" {
                        crate::records::entity_header::BaseTypeGuid::Guid {
                            value: "CB844AB6-240D-4FC9-9C9F-3679DC896D6F"
                                .to_owned()
                                .try_into()
                                .unwrap(),
                            offset: 0,
                        }
                    } else {
                        crate::records::entity_header::BaseTypeGuid::Absent
                    },
                    version,
                    version_offset: 0,
                    module: "Fusion".to_owned(),
                    entities: crate::records::identity::ReferenceRun::unlocated(
                        entities.iter().map(|entity| u64::from(*entity)).collect(),
                    ),
                },
            )
            .collect(),
        records: Vec::new(),
        secondary_records: Vec::new(),
    }
}

fn form_scope(
    record_index: u32,
    references: Vec<u32>,
) -> crate::records::feature::scope::DesignParameterScope {
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("f3d:FusionAssetName[Active]/Design1/BulkStream.dat:scope#{record_index}"),
        crate::records::feature::scope::DesignFeatureKind::Form,
        record_index,
    );
    if references.is_empty() {
        return scope;
    }
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn neutral_form_cage(id: &str, uuid: &str) -> cadmpeg_ir::SubdSurface {
    cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId::mint(id).expect("identity grammar"),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        source_object: Some(cadmpeg_ir::SourceObjectAssociation {
            geometry_role: None,
            format: cadmpeg_ir::codec_id!("f3d"),
            object_id: cadmpeg_core::text::NonBlankString::new(format!(
                "FusionAssetName[Active]/TSplines.BlobParts/TSpline.{uuid}.tsm"
            ))
            .unwrap(),
            instance_path: Vec::new(),
        }),
        cage: cadmpeg_ir::subd::SubdCage::default(),
        native_ref: None,
    }
}

const FIRST_CAGE_UUID: &str = "11111111-1111-4111-8111-111111111111";
const SECOND_CAGE_UUID: &str = "22222222-2222-4222-8222-222222222222";

fn two_body_form_meta() -> crate::metastream::MetaStream {
    form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 11, &[500, 300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299, 499]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296, 496]),
        ("E079B079-016A-43C2-ACC7-9627B32A8960", 0, &[207]),
        ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
        ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &[301, 501]),
    ])
}

fn two_body_form_stream() -> Vec<u8> {
    [
        form_selection_group_frame(207, 201, 1),
        form_serializer_frame(299, 296, FIRST_CAGE_UUID),
        form_body_frame(300, 7, [201, 22, 296, 301], None),
        form_serializer_frame(499, 496, SECOND_CAGE_UUID),
        form_body_frame(500, 23, [201, 22, 496, 501], None),
    ]
    .concat()
}

#[test]
fn form_body_record_reference_run_follows_its_first_material_envelope() {
    for prefix in [0, 1, 18] {
        let bytes = form_body_frame(1300, prefix, [1200, 23, 1297, 1301], None);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert_eq!(
            crate::design::test_support::with_test_decode_context(|ctx| form_body_references(
                ctx, &bytes, &records, 1300
            ))
            .unwrap()
            .into_iter()
            .map(|run| run.references)
            .collect::<Vec<_>>(),
            vec![expected_local_run(
                [Some(1200), Some(23), Some(1297), Some(1301)],
                None
            )],
            "prefix {prefix}"
        );
    }
}

#[test]
fn form_body_record_preserves_both_reference_run_alternatives() {
    let bytes = form_body_frame(
        1300,
        0,
        [1200, 23, 1297, 1301],
        Some([1200, 23, 1297, 1301]),
    );
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert_eq!(
        crate::design::test_support::with_test_decode_context(|ctx| form_body_references(
            ctx, &bytes, &records, 1300
        ))
        .unwrap()
        .into_iter()
        .map(|run| run.references)
        .collect::<Vec<_>>(),
        vec![expected_local_run([Some(1200), Some(23), Some(1297), Some(1301)], None); 2]
    );
}

fn body_construction(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    scope: &crate::records::feature::scope::DesignParameterScope,
    cages: &[cadmpeg_ir::SubdSurface],
) -> FormConstruction {
    use std::io::{Cursor, Write};
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
    for uuid in [FIRST_CAGE_UUID, SECOND_CAGE_UUID] {
        archive
            .start_file(
                format!("FusionAssetName[Active]/TSplines.BlobParts/TSpline.{uuid}.tsm"),
                stored,
            )
            .unwrap();
        archive.write_all(b"synthetic resource").unwrap();
    }
    let archive = archive.finish().unwrap().into_inner();
    let mut result = None;
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
            let mut facts = super::FormFacts {
                inputs: super::form_inputs(scope)?,
                observations: Vec::new(),
            };
            let source = super::FormSource {
                bytes,
                records: &records,
                scope,
                cages,
                stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
            };
            let types = FormRecordTypes::of(meta);
            facts.tracking(ctx, source, &types, &mut Vec::new())?;
            let mut serializers = super::form_serializers(ctx, bytes, &records, Some(&types))?;
            let mut groups = super::ReferenceGroups::default();
            super::bind_serializers(ctx, source, &mut serializers, &mut groups)?;
            super::retain_unbound_serializers(
                ctx,
                scan,
                source,
                &serializers,
                &mut facts,
                &mut Vec::new(),
            )?;
            let membership = super::body_membership(
                ctx,
                scan,
                source,
                (&types, &serializers),
                (&mut facts, &mut groups),
                None,
                &mut Vec::new(),
            )?;
            result = Some(
                FormConstruction::try_new(facts.inputs, facts.observations, membership).unwrap(),
            );
            Ok::<(), cadmpeg_core::CodecError>(())
        })
    })
    .unwrap();
    result.unwrap()
}

#[test]
fn form_body_surfaces_retain_owned_facts_and_separate_foreign_scopes() {
    let bytes = [
        form_body_frame(300, 0, [201, 22, 296, 301], None),
        form_body_frame(500, 0, [999, 22, 496, 501], None),
    ]
    .concat();
    let cages = [neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)];
    let construction = body_construction(
        &bytes,
        &two_body_form_meta(),
        &form_scope(201, vec![207]),
        &cages,
    );
    assert_eq!(construction.observations().len(), 1);
    assert!(matches!(
        construction.membership(),
        FormMembership::Partial {
            cardinality: Some(1),
            ..
        }
    ));
    assert_eq!(construction.observations()[0].paths.len(), 4);
}

#[test]
fn form_body_records_of_other_versions_withhold_closure() {
    let meta = form_meta(&[("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 8, &[300])]);
    let types = FormRecordTypes::of(&meta);
    assert!(types.bodies.is_empty());
    assert!(types.unsupported_bodies);
    for version in [9, 10, 11] {
        let meta = form_meta(&[("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", version, &[300])]);
        assert_eq!(FormRecordTypes::of(&meta).bodies, [300]);
    }
}

#[test]
fn typed_form_serializer_joins_its_surface_under_any_class_tag() {
    let bytes = two_body_form_stream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    let typed = form_typed_serializers(
        &ctx,
        &bytes,
        &records,
        &FormRecordTypes::of(&two_body_form_meta()),
    )
    .unwrap();
    assert_eq!(
        typed.entry_name(296),
        Some(format!("TSpline.{FIRST_CAGE_UUID}.tsm").as_str())
    );
    assert_eq!(
        typed.entry_name(496),
        Some(format!("TSpline.{SECOND_CAGE_UUID}.tsm").as_str())
    );
    assert_eq!(parsed_serializers(&bytes, &records).entry_name(296), None);
}

#[test]
fn form_typed_serializers_consume_inline_surface_references_and_preserve_siblings() {
    let plain = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
    let type_at = plain.len() - 2;
    let mut inline = plain[..type_at].to_vec();
    inline.extend_from_slice(&36u32.to_le_bytes());
    inline.extend_from_slice(b"22709494-9149-4A70-A829-7D748A7B5DB1");
    inline.extend_from_slice(&[0, 0]);
    let cages = [
        neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
        neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID),
    ];
    let construction = |serializer| {
        let bytes = [
            serializer,
            form_body_frame(300, 0, [201, 22, 296, 301], None),
            form_serializer_frame(499, 496, SECOND_CAGE_UUID),
            form_body_frame(500, 0, [201, 22, 496, 501], None),
        ]
        .concat();
        body_construction(
            &bytes,
            &two_body_form_meta(),
            &form_scope(201, vec![205]),
            &cages,
        )
    };
    for serializer in [plain, inline.clone()] {
        let result = construction(serializer);
        assert!(result.membership_is_complete());
        assert_eq!(
            result.committed_cages().collect::<Vec<_>>(),
            [&cages[0].id, &cages[1].id]
        );
    }
    let mut bad_guid = inline.clone();
    bad_guid[type_at + 4] = b'z';
    let mut bad_flags = inline.clone();
    *bad_flags.last_mut().unwrap() = 1;
    let mut extra_tail = inline.clone();
    extra_tail.push(0);
    let mut null = inline.clone();
    null[type_at - 9] = 0;
    for serializer in [
        inline[..inline.len() - 5].to_vec(),
        bad_guid,
        bad_flags,
        extra_tail,
        null,
    ] {
        let result = construction(serializer);
        assert!(!result.membership_is_complete());
        assert_eq!(result.committed_cages().collect::<Vec<_>>(), [&cages[1].id]);
        assert!(result
            .observations()
            .iter()
            .any(|observation| matches!(observation.binding, FormCageBinding::Unresolved {})));
    }
}

#[test]
fn form_review_reproduces_contradictory_serializer_inline_type() {
    let plain = form_serializer_frame(299, 296, FIRST_CAGE_UUID);
    let type_at = plain.len() - 2;
    let mut serializer = plain[..type_at].to_vec();
    serializer.extend_from_slice(&36u32.to_le_bytes());
    serializer.extend_from_slice(b"00000000-0000-4000-8000-000000000002");
    serializer.extend_from_slice(&[0, 0]);
    let bytes = [
        serializer,
        form_body_frame(300, 0, [201, 22, 296, 301], None),
    ]
    .concat();
    let meta = form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296]),
        ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
        ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &[301]),
    ]);
    let result = body_construction(
        &bytes,
        &meta,
        &form_scope(201, vec![]),
        &[neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)],
    );
    assert!(!result.membership_is_complete());
}

#[test]
fn form_body_records_bind_the_owning_scope_cages_in_body_record_order() {
    let bytes = two_body_form_stream();
    let cages = [
        neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID),
        neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
    ];
    let construction = body_construction(
        &bytes,
        &two_body_form_meta(),
        &form_scope(201, vec![205, 207]),
        &cages,
    );
    assert!(construction.membership_is_complete());
    assert_eq!(
        construction
            .committed_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#1", "f3d:model:subd#2"]
    );
    let partial = body_construction(
        &bytes,
        &two_body_form_meta(),
        &form_scope(201, vec![205, 207]),
        &cages[..1],
    );
    assert!(!partial.membership_is_complete());
    assert_eq!(
        partial
            .committed_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#2"]
    );
    assert!(partial.observations().iter().any(|observation| matches!(
        observation.binding,
        FormCageBinding::Undecoded {
            availability: FormResourceAvailability::ActiveUndecoded,
            ..
        }
    )));
}

#[test]
fn form_registry_aliases_do_not_duplicate_bodies_or_serializer_alternatives() {
    let bytes = two_body_form_stream();
    let mut meta = two_body_form_meta();
    meta.types[0].entities =
        crate::records::identity::ReferenceRun::unlocated(vec![500u64, 300, 300, 500]);
    meta.types[1].entities =
        crate::records::identity::ReferenceRun::unlocated(vec![499u64, 299, 499, 299]);
    let cages = [
        neutral_form_cage("f3d:model:subd#2", SECOND_CAGE_UUID),
        neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
    ];
    let construction = body_construction(&bytes, &meta, &form_scope(201, vec![205, 207]), &cages);
    assert!(construction.membership_is_complete());
    assert_eq!(construction.observations().len(), 2);
    assert_eq!(
        construction
            .committed_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#1", "f3d:model:subd#2"]
    );
}

#[test]
fn form_body_envelopes_accept_empty_library_labels_and_inline_typed_references() {
    for (library, label, token) in [
        ("", "", "MaterialInv_001"),
        (
            "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
            "Metal",
            "PrismMaterial-001",
        ),
    ] {
        let mut bytes = indexed_frame(b"274", 300, 11);
        push_lp_utf16(&mut bytes, "00000000-0000-4000-8000-000000000001");
        push_lp_utf16(&mut bytes, library);
        push_lp_utf16(&mut bytes, label);
        push_lp_utf16(&mut bytes, token);
        for target in [201u64, 22, 296, 301] {
            bytes.push(1);
            bytes.extend_from_slice(&target.to_le_bytes());
            bytes.extend_from_slice(&36u32.to_le_bytes());
            bytes.extend_from_slice(b"00000000-0000-4000-8000-000000000002");
            bytes.extend_from_slice(&[0, 0]);
        }
        bytes.extend_from_slice(&indexed_frame(b"258", 300, 15));
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert_eq!(
            crate::design::test_support::with_test_decode_context(|ctx| form_body_references(
                ctx, &bytes, &records, 300
            ))
            .unwrap()
            .into_iter()
            .map(|run| run.references)
            .collect::<Vec<_>>(),
            vec![expected_local_run(
                [Some(201), Some(22), Some(296), Some(301)],
                Some("00000000-0000-4000-8000-000000000002")
            )]
        );
    }
}

#[test]
fn form_missing_sources_preserve_native_parameters_metadata_and_repeated_binding() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
    use std::io::Cursor;
    let scope = form_scope(201, vec![205, 207]);
    let row: Feature = serde_json::from_value(serde_json::json!({
        "id":"f3d:model:feature#form", "ordinal":7, "name":"Form", "suppressed":true,
        "dependencies":["f3d:model:feature#previous"], "source_properties":{"reference:0":"205"},
        "outputs":["f3d:model:body#result"], "native_ref":scope.id,
        "definition":{"kind":"operation","value":{"kind":"native","value":{"kind":"Form","parameters":{"width":"width / 2","extra":"sin(angle)"}}}},
        "source_content": []
    })).unwrap();
    let mut rows = vec![row.clone()];
    let mut losses = Vec::new();
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(
        &mut archive,
        crate::zip_write::file_options(zip::CompressionMethod::Stored),
    );
    let archive = archive.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            let mut projection = super::FormProjection::new(scan);
            projection.project(
                ctx,
                std::slice::from_ref(&scope),
                &mut rows,
                &[],
                &mut losses,
            )?;
            let once = rows.clone();
            let notes = losses.clone();
            projection.project(
                ctx,
                std::slice::from_ref(&scope),
                &mut rows,
                &[],
                &mut losses,
            )?;
            assert_eq!(rows, once);
            assert_eq!(losses, notes);
            Ok::<(), cadmpeg_core::CodecError>(())
        })
    })
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Form(construction)) =
        rows[0].evaluation.definition()
    else {
        panic!("Form");
    };
    assert_eq!(construction.inputs().parameters["extra"], "sin(angle)");
    assert_eq!(
        construction
            .inputs()
            .references
            .iter()
            .map(cadmpeg_ir::ids::NativeId::as_str)
            .collect::<Vec<_>>(),
        [
            "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#205",
            "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#207"
        ]
    );
    assert!(!construction.membership_is_complete());
    assert_eq!(losses.len(), 1);
    rows[0]
        .evaluation
        .set_definition(row.evaluation.definition().clone());
    assert_eq!(rows[0], row);
}

#[test]
fn form_native_source_reference_takes_precedence_over_derived_feature_identity() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
    use std::io::Cursor;
    let scopes = [
        form_scope(201, vec![205, 207]),
        form_scope(202, vec![208, 209]),
    ];
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(
        &mut archive,
        crate::zip_write::file_options(zip::CompressionMethod::Stored),
    );
    let archive = archive.finish().unwrap().into_inner();
    for (native_ref, expected) in [
        (Some(scopes[0].id.clone()), vec![205u64, 207]),
        (None, vec![205, 207]),
        (Some("invalid".to_owned()), vec![]),
        (
            Some("f3d:FusionAssetName[Active]/Design1/BulkStream.dat:scope#999".to_owned()),
            vec![],
        ),
        (Some(scopes[1].id.clone()), vec![208, 209]),
    ] {
        let mut feature: Feature = serde_json::from_value(serde_json::json!({
            "id":crate::ids::neutral_feature_id(&scopes[0]), "ordinal":7, "suppressed":true,
            "outputs":["f3d:model:body#result"],
            "definition":{"kind":"operation","value":{"kind":"native","value":{"kind":"Form","parameters":{"extra":"sin(angle)"}}}},
            "dependencies": [], "source_properties": {}, "source_content": []
        }))
        .unwrap();
        feature.native_ref = native_ref;
        let original = feature.clone();
        let mut features = [feature];
        let mut losses = Vec::new();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            crate::design::test_support::with_test_decode_context(|ctx| {
                super::FormProjection::new(scan).project(
                    ctx,
                    &scopes,
                    &mut features,
                    &[],
                    &mut losses,
                )
            })
        })
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Form(construction)) =
            features[0].evaluation.definition()
        else {
            panic!("Form");
        };
        assert_eq!(construction.inputs().parameters["extra"], "sin(angle)");
        assert_eq!(
            construction
                .inputs()
                .references
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|id| format!(
                    "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record#{id}"
                ))
                .collect::<Vec<_>>()
        );
        assert!(!construction.membership_is_complete());
        assert!(
            losses
                .iter()
                .any(|loss| loss.code
                    == crate::loss::F3dLossCode::FeatureFormSourceUnavailable.kind())
        );
        features[0]
            .evaluation
            .set_definition(original.evaluation.definition().clone());
        assert_eq!(features[0], original);
    }
}

#[test]
fn form_preserves_sibling_bindings_around_an_unresolved_middle_member() {
    let bytes = [
        form_serializer_frame(299, 296, FIRST_CAGE_UUID),
        form_body_frame(300, 0, [201, 22, 296, 301], None),
        form_body_frame(400, 0, [201, 22, 396, 401], None),
        form_serializer_frame(499, 496, SECOND_CAGE_UUID),
        form_body_frame(500, 0, [201, 22, 496, 501], None),
    ]
    .concat();
    let meta = form_meta(&[
        ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300, 400, 500]),
        ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299, 499]),
        ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296, 396, 496]),
        ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
        ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &[301, 401, 501]),
    ]);
    let cages = [
        neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID),
        neutral_form_cage("f3d:model:subd#3", SECOND_CAGE_UUID),
    ];
    let construction = body_construction(&bytes, &meta, &form_scope(201, vec![]), &cages);
    assert!(!construction.membership_is_complete());
    assert_eq!(
        construction
            .committed_cages()
            .map(cadmpeg_ir::ids::SubdId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:model:subd#1", "f3d:model:subd#3"]
    );
    let FormMembership::Partial {
        cardinality,
        members,
        ..
    } = construction.membership()
    else {
        panic!("partial");
    };
    assert_eq!(*cardinality, Some(3));
    assert_eq!(
        members
            .iter()
            .map(|member| member.ordinal)
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(2)]
    );
    assert!(matches!(
        construction.observations()[members[1].observation.0].binding,
        FormCageBinding::Unresolved {}
    ));
}

#[test]
fn form_resource_activity_uses_the_exact_active_asset_without_a_parse_diagnosis() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::io::{Cursor, Write};
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
    archive
        .start_file(
            format!("FusionAssetName[Active]/TSplines.BlobParts/TSpline.{FIRST_CAGE_UUID}.tsm"),
            stored,
        )
        .unwrap();
    archive.write_all(b"undecoded synthetic program").unwrap();
    let archive = archive.finish().unwrap().into_inner();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&archive, &arena, &DecodePolicy::default()).unwrap();
    let mut scan = crate::container::scan(&ctx, root).unwrap();
    let scope = form_scope(201, vec![]);
    let bytes = [
        form_serializer_frame(299, 296, FIRST_CAGE_UUID),
        form_serializer_frame(499, 496, SECOND_CAGE_UUID),
        indexed_frame(b"258", 499, 15),
    ]
    .concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut serializers = form_typed_serializers(
        &ctx,
        &bytes,
        &records,
        &FormRecordTypes::of(&two_body_form_meta()),
    )
    .unwrap();
    let mut facts = super::FormFacts {
        inputs: FormInputs::default(),
        observations: Vec::new(),
    };
    let mut wrong_asset = neutral_form_cage("f3d:model:subd#other", FIRST_CAGE_UUID);
    wrong_asset.source_object.as_mut().unwrap().object_id =
        cadmpeg_core::text::NonBlankString::new(format!(
            "OtherAsset/TSplines.BlobParts/TSpline.{FIRST_CAGE_UUID}.tsm"
        ))
        .unwrap();
    let cages = [wrong_asset];
    let source = super::FormSource {
        bytes: &bytes,
        records: &records,
        scope: &scope,
        cages: &cages,
        stream: "FusionAssetName[Active]/Design1/BulkStream.dat",
    };
    super::bind_serializers(
        &ctx,
        source,
        &mut serializers,
        &mut super::ReferenceGroups::default(),
    )
    .unwrap();
    facts
        .surface(
            &ctx,
            &scan,
            source,
            super::FormSurface {
                path: super::input_path(&scope, &[296]).unwrap(),
                record: Some(296),
            },
            &serializers,
            &mut Vec::new(),
        )
        .unwrap();
    facts
        .surface(
            &ctx,
            &scan,
            source,
            super::FormSurface {
                path: super::input_path(&scope, &[496]).unwrap(),
                record: Some(496),
            },
            &serializers,
            &mut Vec::new(),
        )
        .unwrap();
    assert!(matches!(
        facts.observations[0].binding,
        FormCageBinding::Undecoded {
            availability: FormResourceAvailability::ActiveUndecoded,
            ..
        }
    ));
    assert!(matches!(
        facts.observations[1].binding,
        FormCageBinding::Undecoded {
            availability: FormResourceAvailability::Inactive,
            ..
        }
    ));
    scan.kind = crate::container::F3dContainerKind::MultiDocument {
        matched: scan.kind.dialect().clone(),
    };
    facts
        .surface(
            &ctx,
            &scan,
            source,
            super::FormSurface {
                path: super::input_path(&scope, &[296]).unwrap(),
                record: Some(296),
            },
            &serializers,
            &mut Vec::new(),
        )
        .unwrap();
    assert!(matches!(
        facts.observations[2].binding,
        FormCageBinding::Undecoded {
            availability: FormResourceAvailability::Unknown,
            ..
        }
    ));
}

#[test]
fn form_source_recovery_propagates_a_resource_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::io::Cursor;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(
        &mut archive,
        crate::zip_write::file_options(zip::CompressionMethod::Stored),
    );
    let archive = archive.finish().unwrap().into_inner();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        ctx.retain_bytes(1, "Form test fuse"),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    let mut rows = vec![serde_json::from_value(serde_json::json!({"id":"f3d:model:feature#form","ordinal":0,"definition":{"kind":"operation","value":{"kind":"native","value":{"kind":"Form","parameters":{}}}},"dependencies":[],"source_properties":{},"source_content":[],"outputs":[]})).unwrap()];
    let original = rows.clone();
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        assert!(matches!(
            super::FormProjection::new(scan).project(&ctx, &[], &mut rows, &[], &mut losses),
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
    });
    assert_eq!(rows, original);
    assert!(losses.is_empty());
}

#[test]
fn form_body_target_failures_preserve_decoded_cage_candidates() {
    for (references, scenes) in [([201, 23, 296, 301], [301]), ([201, 22, 296, 301], [999])] {
        let bytes = [
            form_serializer_frame(299, 296, FIRST_CAGE_UUID),
            form_body_frame(300, 0, references, None),
        ]
        .concat();
        let meta = form_meta(&[
            ("FA1F0081-C9A1-4E14-8F25-D12DD868ED90", 10, &[300]),
            ("830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571", 0, &[299]),
            ("22709494-9149-4A70-A829-7D748A7B5DB1", 0, &[296]),
            ("CD57BC48-50EC-47DC-975A-FB6DEA72F4DA", 4, &[22]),
            ("702b9cd2-537c-429e-8cc4-22beeeb98c37", 1, &scenes),
        ]);
        let cages = [neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)];
        let construction = body_construction(&bytes, &meta, &form_scope(201, vec![]), &cages);
        assert!(!construction.membership_is_complete());
        assert_eq!(construction.committed_cages().count(), 1);
        assert_eq!(
            construction
                .referenced_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            ["f3d:model:subd#1"]
        );
        assert_eq!(construction.observations()[0].paths.len(), 4);
    }
}

#[test]
fn form_body_failure_keeps_owned_siblings_and_foreign_targets_do_not_poison_ownership() {
    let cages = [neutral_form_cage("f3d:model:subd#1", FIRST_CAGE_UUID)];
    for foreign in [false, true] {
        let other = if foreign {
            form_body_frame(500, 0, [999, 999, 999, 999], None)
        } else {
            [
                indexed_frame(b"274", 500, 15),
                indexed_frame(b"258", 500, 15),
            ]
            .concat()
        };
        let bytes = [
            form_serializer_frame(299, 296, FIRST_CAGE_UUID),
            form_body_frame(300, 0, [201, 22, 296, 301], None),
            other,
        ]
        .concat();
        let construction = body_construction(
            &bytes,
            &two_body_form_meta(),
            &form_scope(201, vec![]),
            &cages,
        );
        assert_eq!(construction.membership_is_complete(), foreign);
        if !foreign {
            let FormMembership::Partial {
                cardinality,
                order,
                members,
            } = construction.membership()
            else {
                panic!("partial");
            };
            assert_eq!(*cardinality, None);
            assert_eq!(*order, FormOrder::Unknown);
            assert_eq!(members[0].ordinal, None);
        }
        assert_eq!(
            construction
                .committed_cages()
                .map(cadmpeg_ir::ids::SubdId::as_str)
                .collect::<Vec<_>>(),
            ["f3d:model:subd#1"]
        );
    }
}
