// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use super::named_parameter_scope_tail_is_valid;
use super::{
    copy_sketch_entity_id, reference_table, sketch_entity_index, unique_sketch_entity_reference,
    utf16_has_control,
};
use crate::test_support::lp_utf16;

fn named_scope_tail(lane_value: u64) -> Vec<u8> {
    let label = "Canvas";
    let label_code_units = label.encode_utf16().count();
    let marker = 19 + label_code_units * 2;
    let mut bytes = vec![0; marker + 59];
    bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
    let mut label_bytes = Vec::new();
    lp_utf16(&mut label_bytes, label);
    bytes[8..8 + label_bytes.len()].copy_from_slice(&label_bytes);
    bytes[marker] = 1;
    bytes[marker + 1] = 0xd5;
    bytes[marker + 2..marker + 10].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 12..marker + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[marker + 16..marker + 20].copy_from_slice(&0xfcu32.to_le_bytes());
    bytes[marker + 20..marker + 28].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[marker + 28..marker + 32].copy_from_slice(&0xfcu32.to_le_bytes());
    bytes[marker + 32] = 1;
    bytes[marker + 33] = 0xd4;
    bytes[marker + 34..marker + 42].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 42..marker + 46].copy_from_slice(&[0, 1, 0, 0]);
    bytes[marker + 46] = 1;
    bytes[marker + 47] = 0xd3;
    bytes[marker + 48..marker + 56].copy_from_slice(&lane_value.to_le_bytes());
    bytes
}

#[test]
fn named_scope_tail_requires_one_repeated_binary_lane_value() {
    for lane_value in [0, 1] {
        let bytes = named_scope_tail(lane_value);
        assert_eq!(
            named_parameter_scope_tail_is_valid(
                &cadmpeg_test_support::service_decode_context(),
                &bytes,
                0,
                bytes.len(),
                bytes.len()
            )
            .unwrap(),
            Some(true)
        );
    }

    let mut mismatched = named_scope_tail(0);
    let marker = mismatched.len() - 59;
    mismatched[marker + 34..marker + 42].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(
        named_parameter_scope_tail_is_valid(
            &cadmpeg_test_support::service_decode_context(),
            &mismatched,
            0,
            mismatched.len(),
            mismatched.len()
        )
        .unwrap(),
        Some(false)
    );

    let outside_domain = named_scope_tail(2);
    assert_eq!(
        named_parameter_scope_tail_is_valid(
            &cadmpeg_test_support::service_decode_context(),
            &outside_domain,
            0,
            outside_domain.len(),
            outside_domain.len()
        )
        .unwrap(),
        Some(false)
    );
}

#[test]
fn named_scope_tail_refuses_temporary_text_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let bytes = named_scope_tail(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = named_parameter_scope_tail_is_valid(&ctx, &bytes, 0, bytes.len(), bytes.len());
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::MaterializedBytes
                && failure.operation == "f3d Design temporary UTF-16 text"
    ));
}

fn sketch_entity(id: &str, entity_id: &str) -> crate::records::entity_header::DesignEntityHeader {
    crate::records::entity_header::DesignEntityHeader {
        id: id.into(),
        byte_offset: 0,
        entity_id: crate::records::identity::DesignEntityId::try_from(entity_id.to_owned())
            .unwrap(),
        class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned()),
            None,
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    }
}

fn marked_references(suffixes: &[u32]) -> Vec<u8> {
    let mut frame = vec![0xff];
    for suffix in suffixes {
        frame.push(1);
        frame.extend_from_slice(&suffix.to_le_bytes());
        frame.extend_from_slice(&[0; 6]);
    }
    frame
}

#[test]
fn sketch_scope_binds_the_only_entity_its_frame_names() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let entities = [
        sketch_entity("f3d:Design/A.dat:entity#1", "0_42"),
        sketch_entity("f3d:Design/A.dat:entity#2", "0_43"),
        sketch_entity("f3d:Design/B.dat:entity#3", "0_44"),
        sketch_entity("f3d:Design/A.dat:entity#4", "0_45"),
        sketch_entity("f3d:Design/A.dat:entity#5", "0_45"),
    ];
    let index = sketch_entity_index(&ctx, &entities).unwrap();
    let stream = "f3d:Design/A.dat";

    // An unknown suffix and a repeated reference to the matched suffix leave
    // one match at its first reference.
    let frame = marked_references(&[7, 42, 42]);
    let (entity, suffix_at) = unique_sketch_entity_reference(&ctx, &frame, stream, &index)
        .unwrap()
        .expect("unique entity");
    assert_eq!(entity.entity_id.as_str(), "0_42");
    assert_eq!(suffix_at, 13);

    // Entities of another stream do not match.
    let frame = marked_references(&[44]);
    assert!(unique_sketch_entity_reference(&ctx, &frame, stream, &index)
        .unwrap()
        .is_none());

    // Two named entities, or two entities with one suffix, are ambiguous.
    for suffixes in [&[42, 43][..], &[45]] {
        let frame = marked_references(suffixes);
        assert!(unique_sketch_entity_reference(&ctx, &frame, stream, &index)
            .unwrap()
            .is_none());
    }
}

#[test]
fn sketch_scope_reference_scan_stops_at_the_second_match() {
    let entities = [
        sketch_entity("f3d:Design/A.dat:entity#1", "0_42"),
        sketch_entity("f3d:Design/A.dat:entity#2", "0_43"),
    ];
    let mut frame = marked_references(&[42, 43]);
    frame.extend_from_slice(&[0; 64]);
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D sketch scope marked references",
        0,
        |ctx| {
            let index = sketch_entity_index(ctx, &entities)?;
            unique_sketch_entity_reference(ctx, &frame, "f3d:Design/A.dat", &index)
        },
    );
    // The scan admits the marker byte of the second reference and nothing after it.
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    assert_eq!(limit.additional, 1);
    let ctx = cadmpeg_test_support::service_decode_context();
    let index = sketch_entity_index(&ctx, &entities).unwrap();
    assert!(
        unique_sketch_entity_reference(&ctx, &frame, "f3d:Design/A.dat", &index)
            .unwrap()
            .is_none()
    );
}

fn reference_run(count_at: usize, count: u32, members: &[u32]) -> Vec<u8> {
    let mut bytes = vec![0; count_at];
    bytes.extend_from_slice(&count.to_le_bytes());
    for member in members {
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes
}

#[test]
fn reference_table_is_the_only_counted_marked_run_before_the_kind() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let bytes = reference_run(40, 3, &[10, 11, 12]);
    let table = reference_table(&ctx, &bytes, 0, bytes.len())
        .unwrap()
        .expect("table");
    assert_eq!(table.count_at, 40);
    assert_eq!(table.count, 3);
    assert_eq!(
        (0..3)
            .map(|ordinal| table.member(&bytes, ordinal).unwrap())
            .collect::<Vec<_>>(),
        [10, 11, 12]
    );
    assert_eq!(table.member_offset(0), 45);

    // A count that reaches before `start + 11` opens no table.
    assert!(reference_table(&ctx, &bytes, 30, bytes.len())
        .unwrap()
        .is_none());

    // The count of a shorter table lies in the zero padding of a slot of the
    // longer one, so at most one complete table ends at the kind field.
    let mut nested = reference_run(40, 3, &[10, 11, 12]);
    nested[51..55].copy_from_slice(&2u32.to_le_bytes());
    let table = reference_table(&ctx, &nested, 0, nested.len())
        .unwrap()
        .expect("shorter table");
    assert_eq!((table.count_at, table.count), (51, 2));

    // A slot that is not a marked reference rules out every table that holds it.
    let mut broken = reference_run(40, 3, &[10, 11, 12]);
    broken[44] = 0;
    assert!(reference_table(&ctx, &broken, 0, broken.len())
        .unwrap()
        .is_none());
}

#[test]
fn reference_table_scan_admits_each_slot_once() {
    let bytes = reference_run(40, 3, &[10, 11, 12]);
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D parameter-scope reference slots",
        0,
        |ctx| reference_table(ctx, &bytes, 0, bytes.len()),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    // Slots lie on the 11-byte lattice that ends at the table end.
    assert_eq!(limit.additional, u64_from_index((bytes.len() - 15) / 11));
}

#[test]
fn utf16_control_test_reads_code_units_until_the_first_control() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let encode = |text: &str| {
        text.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    assert!(!utf16_has_control(&ctx, &encode("Fillet 😀")).unwrap());
    assert!(utf16_has_control(&ctx, &encode("Fil\u{7}let")).unwrap());
    assert!(utf16_has_control(&ctx, &encode("Fil\u{85}let")).unwrap());
    let bytes = encode("A\u{1}BCDEF");
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "validate F3D scope text characters",
        1,
        |ctx| utf16_has_control(ctx, &bytes),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    assert_eq!(limit.additional, 1);
}

#[test]
fn sketch_scope_entity_id_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let source =
        crate::records::identity::DesignEntityId::try_from("entity_42".to_owned()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64_from_index(source.as_str().len()) - 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = copy_sketch_entity_id(&ctx, &source);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d Sketch scope entity ID"
    ));
    let copied =
        copy_sketch_entity_id(&cadmpeg_test_support::service_decode_context(), &source).unwrap();
    assert_eq!(copied, source);
}

#[test]
fn scope_payload_length_counts_supplementary_utf16_units_and_refuses_work() {
    let kind =
        crate::records::feature::scope::DesignFeatureKind::try_from("A😀".to_owned()).unwrap();
    let scope = crate::records::feature::scope::DesignParameterScope::empty("scope", kind, 1);
    assert_eq!(
        super::parameter_scope_payload_length(
            &cadmpeg_test_support::service_decode_context(),
            &scope
        )
        .unwrap(),
        Some(122),
    );
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "count F3D scope kind UTF-16 units",
        0,
        |ctx| super::parameter_scope_payload_length(ctx, &scope),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "count F3D scope kind UTF-16 units"
            && limit.additional == 5)
    );
}

#[test]
fn payload_prologue_skips_the_property_block() {
    let mut bytes = vec![0, 1];
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(b'x');
    bytes.extend_from_slice(&23_u32.to_le_bytes());
    bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
    bytes.extend_from_slice(&9_u64.to_le_bytes());
    assert_eq!(super::payload_prologue(&bytes, 0, bytes.len()), Some(46));
    assert_eq!(super::payload_prologue(&bytes, 0, bytes.len() - 1), None);
    let mut unprintable = bytes.clone();
    unprintable[10] = b' ';
    assert_eq!(super::payload_prologue(&unprintable, 0, bytes.len()), None);
    assert_eq!(super::payload_prologue(&[0, 0], 0, 2), Some(2));
}

#[test]
fn named_scope_label_scan_stops_at_the_first_control() {
    let mut bytes = named_scope_tail(0);
    // "Canvas" opens at offset 12; make its second unit a control character.
    bytes[14] = 0x07;
    assert_eq!(
        named_parameter_scope_tail_is_valid(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            0,
            bytes.len(),
            bytes.len()
        )
        .unwrap(),
        Some(false)
    );
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "validate F3D scope text characters",
        1,
        |ctx| named_parameter_scope_tail_is_valid(ctx, &bytes, 0, bytes.len(), bytes.len()),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    assert_eq!(limit.additional, 1);
}

/// A scope frame with record index 12, kind `kind`, one reference and a named
/// tail with an empty label.
fn named_scope_frame(kind: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, kind);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.push(1);
    bytes.push(0x0f);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.push(1);
    bytes.push(0x0e);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(0x0d);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes
}

fn parse_named_scope_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<crate::records::feature::scope::DesignParameterScope>, cadmpeg_core::CodecError>
{
    super::parse_parameter_scope(
        ctx,
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        12,
        &crate::records::references::DesignClassTag::try_from("378".to_owned()).unwrap(),
        0,
    )
}

#[test]
fn native_scope_kind_retains_its_name_and_known_kinds_retain_nothing() {
    let bytes = named_scope_frame("WidgetFeature");
    let scope = parse_named_scope_frame(&cadmpeg_test_support::service_decode_context(), &bytes)
        .unwrap()
        .expect("native scope");
    assert_eq!(scope.kind_name(), "WidgetFeature");
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "f3d Design scope kind storage",
        0,
        |ctx| parse_named_scope_frame(ctx, &bytes),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("retained refusal");
    };
    assert_eq!(limit.additional, u64_from_index("WidgetFeature".len()));

    let bytes = named_scope_frame("CylinderPrimitive");
    let ctx = cadmpeg_test_support::service_decode_context();
    let scope = parse_named_scope_frame(&ctx, &bytes)
        .unwrap()
        .expect("known scope");
    assert_eq!(scope.kind_name(), "CylinderPrimitive");
}
