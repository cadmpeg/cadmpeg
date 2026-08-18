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
    for class in [b"315", b"446"] {
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
            .get(&8304),
            Some(&Some(entry_name.into()))
        );
    }
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
