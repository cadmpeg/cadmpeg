// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

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
        super::form_cage_objects(
            &bytes,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
            2196,
            2190,
        ),
        Some(vec![8300, 8303])
    );
    let mut alternate_pair = bytes.clone();
    alternate_pair[110 + 4..110 + 7].copy_from_slice(b"258");
    assert_eq!(
        super::form_cage_objects(
            &alternate_pair,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&alternate_pair),
            2196,
            2190,
        ),
        Some(vec![8300, 8303])
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
        super::form_cage_objects(
            &empty,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&empty),
            2196,
            2190,
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
        super::form_cage_objects(
            &bytes,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
            2196,
            2190,
        ),
        Some(vec![8300])
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
        super::form_cage_surface(
            &bytes,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
            8300,
            2190,
        ),
        Some(8304)
    );
    assert_eq!(
        super::form_cage_surface(
            &bytes,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
            8300,
            2191,
        ),
        None
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
            super::form_cage_serializers(
                &bytes,
                &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
            )
            .by_surface
            .get(&8304),
            Some(&Some(entry_name.into()))
        );
    }
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
        super::form_cage_serializers(
            &bytes,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
        )
        .by_surface
        .get(&8304),
        Some(&Some(entry_name.into()))
    );

    let mut wrong_pair = bytes.clone();
    wrong_pair[132 + 4..132 + 7].copy_from_slice(b"457");
    assert!(!super::form_cage_serializers(
        &wrong_pair,
        &crate::design::decode::sketch::IndexedRecordOffsets::build(&wrong_pair),
    )
    .by_surface
    .contains_key(&8304));

    let mut nonzero_tail = bytes;
    nonzero_tail[131] = 1;
    assert!(!super::form_cage_serializers(
        &nonzero_tail,
        &crate::design::decode::sketch::IndexedRecordOffsets::build(&nonzero_tail),
    )
    .by_surface
    .contains_key(&8304));
}

#[test]
fn serializers_preserve_primary_frame_order() {
    let names = [
        "TSpline.00000000-0000-0000-0000-000000000000.tsm",
        "TSpline.11111111-1111-1111-1111-111111111111.tsm",
    ];
    let mut chunks = Vec::new();
    for (ordinal, (name, surface)) in names.iter().zip([8304u64, 8307]).enumerate() {
        let record = 8305 + ordinal as u32 * 2;
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
    let serializers = super::form_cage_serializers(
        &bytes,
        &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
    );
    assert_eq!(
        serializers
            .ordered
            .iter()
            .map(|(surface, _)| *surface)
            .collect::<Vec<_>>(),
        vec![8304, 8307]
    );
}

#[test]
fn reads_class_328_form_envelope() {
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
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let mut scope = crate::records::DesignParameterScope::empty("scope", "Form", scope_record);
    scope.reference_members = vec![group_record, metadata_record];
    assert!(super::form_class_328_envelope(&bytes, &records, &scope));

    let mut wrong_pair = bytes;
    let group_start = crate::layout::form_class_328_scope::LEN + 15;
    let paired_class = group_start + crate::layout::form_class_328_cage_group::LEN + 4;
    wrong_pair[paired_class..paired_class + 3].copy_from_slice(b"266");
    assert!(!super::form_class_328_envelope(
        &wrong_pair,
        &crate::design::decode::sketch::IndexedRecordOffsets::build(&wrong_pair),
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
    table[27..35].copy_from_slice(&(owner_record as u64).to_le_bytes());
    table[37..41].copy_from_slice(&32u32.to_le_bytes());
    let mut object_records = Vec::new();
    for ordinal in 0..32u32 {
        let object_record = 1_000 + ordinal * 2;
        let companion_record = 2_000 + ordinal * 2;
        let entry = 41 + ordinal as usize * 30;
        table[entry] = 1;
        table[entry + 1..entry + 9].copy_from_slice(&(object_record as u64).to_le_bytes());
        table[entry + 11..entry + 19].copy_from_slice(&(307u64 + ordinal as u64).to_le_bytes());
        table[entry + 19] = 1;
        table[entry + 20..entry + 28].copy_from_slice(&(companion_record as u64).to_le_bytes());
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
    assert_eq!(
        super::form_class_325_cage_objects(
            &bytes,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
            scope_record,
            &[owner_record],
        ),
        Some((0..32u32).map(|ordinal| 1_000 + ordinal * 2).collect())
    );
    let mut duplicate_discriminator = bytes.clone();
    duplicate_discriminator[41 + 30 + 11..41 + 30 + 19].copy_from_slice(&307u64.to_le_bytes());
    assert_eq!(
        super::form_class_325_cage_objects(
            &duplicate_discriminator,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&duplicate_discriminator),
            scope_record,
            &[owner_record],
        ),
        None
    );
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
        super::form_class_325_cage_surface(
            &bytes,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
            1_000,
        ),
        Some(700)
    );
}

#[test]
fn reads_compact_form_one_cage_envelope() {
    let mut list = indexed_frame(b"355", 205, 100);
    list[21] = 1;
    list[22..30].copy_from_slice(&201u64.to_le_bytes());
    list[32..36].copy_from_slice(&1u32.to_le_bytes());
    list[36] = 1;
    list[37..45].copy_from_slice(&971u64.to_le_bytes());
    list[47..49].copy_from_slice(&[0xfc, 0]);
    let paired = indexed_frame(b"262", 205, 15);
    let object = indexed_frame(b"325", 971, 15);
    let bytes = [list, paired, object].concat();
    assert_eq!(
        super::legacy_form_cage_count(
            &bytes,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
            205,
            201,
        ),
        Some(1)
    );
}

#[test]
fn reads_legacy_form_one_cage_owner_envelopes() {
    for (owner_class, paired_class, nested_class) in [
        (b"335", b"262", b"328"),
        (b"395", b"264", b"329"),
        (b"448", b"258", b"276"),
        (b"295", b"258", b"274"),
    ] {
        let mut owner = indexed_frame(owner_class, 205, 81);
        owner[25] = 1;
        owner[26..34].copy_from_slice(&201u64.to_le_bytes());
        owner[58] = 1;
        owner[59..67].copy_from_slice(&211u64.to_le_bytes());
        owner[70] = 1;
        owner[71..79].copy_from_slice(&201u64.to_le_bytes());
        let paired = indexed_frame(paired_class, 205, 15);
        let nested = indexed_frame(nested_class, 211, 15);
        let bytes = [owner, paired, nested].concat();
        assert_eq!(
            super::legacy_form_cage_count(
                &bytes,
                &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
                205,
                201,
            ),
            Some(1),
            "owner class {owner_class:?}"
        );
    }
}

#[test]
fn rejects_legacy_form_owner_with_wrong_nested_class() {
    let mut owner = indexed_frame(b"335", 205, 81);
    owner[25] = 1;
    owner[26..34].copy_from_slice(&201u64.to_le_bytes());
    owner[58] = 1;
    owner[59..67].copy_from_slice(&211u64.to_le_bytes());
    owner[70] = 1;
    owner[71..79].copy_from_slice(&201u64.to_le_bytes());
    let paired = indexed_frame(b"262", 205, 15);
    let nested = indexed_frame(b"329", 211, 15);
    let bytes = [owner, paired, nested].concat();
    assert_eq!(
        super::legacy_form_cage_count(
            &bytes,
            &crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes),
            205,
            201,
        ),
        None
    );
}

#[test]
fn retains_parameter_when_owner_frame_has_no_scope_binding() {
    let parameter = crate::records::DesignParameter {
        id: "f3d:Design/BulkStream.dat:design-parameter#7".into(),
        byte_offset: 0,
        class_tag: "301".into(),
        record_index: 7,
        family_discriminator: None,
        family_discriminator_offset: None,
        source_ordinal: 0,
        owner_record_index: Some(8),
        expression: "12.5 mm".into(),
        expression_offset: 0,
        source_kind: "AlongDistance".into(),
        source_kind_offset: 0,
        kind: crate::records::DesignParameterKind::Feature,
        unit: Some("mm".into()),
        unit_offset: Some(0),
        name: "distance".into(),
        name_offset: 0,
        evaluated_value: 1.25,
        evaluated_value_offset: 0,
    };
    let scope = crate::records::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#9",
        "Unsupported",
        9,
    );

    let (_, parameters) =
        super::project_parameter_design(&[parameter], &[], &[scope], &[], &[], &[], &[], &[]);

    let [parameter] = parameters.as_slice() else {
        panic!("expected one retained parameter");
    };
    assert_eq!(parameter.owner, None);
    assert_eq!(
        parameter
            .properties
            .get("owner_record_index")
            .map(String::as_str),
        Some("8")
    );
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.5)
        ))
    );
}
