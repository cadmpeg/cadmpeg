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
use crate::design::decode::scopes::ThreadPrefix;

#[test]
fn thread_scope_decodes_standard_size_and_face_group() {
    let mut bytes = vec![0; 148];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"335");
    bytes[7..11].copy_from_slice(&987u32.to_le_bytes());
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x67, 0]);
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M30x3.5");
    lp_utf16(&mut payload, "30.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    assert_eq!(payload.len(), 70);
    bytes[38..108].copy_from_slice(&payload);
    bytes[108..113].copy_from_slice(&[0, 1, 0, 0, 0]);
    bytes[113..121].copy_from_slice(&2.97345f64.to_le_bytes());
    bytes[121..129].copy_from_slice(&2.5732f64.to_le_bytes());
    bytes[129] = 1;
    bytes[130..138].copy_from_slice(&0.35f64.to_le_bytes());
    bytes[138..146].copy_from_slice(&2.7568f64.to_le_bytes());
    bytes[146..148].copy_from_slice(&[0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::Standard,
        designation_offset: 38,
        designation: "M30x3.5".into(),
        nominal_size: crate::records::DesignThreadNominalSize::try_from("30.0".to_owned()).expect("nominal size"),
        profile: "ISO Metric profile".into(),
        major_diameter: 2.97345,
        minor_diameter: 2.5732,
        pitch: 0.35,
        pitch_diameter: 2.7568,
        face_group_record_indices: vec![988],
    };
    assert_thread_construction(
        parse_thread_payload(&bytes, 38, ThreadPrefix::Standard, vec![988]),
        &expected,
    );
    let mut invalid_standard_pitch_marker = bytes.clone();
    invalid_standard_pitch_marker[129] = 0;
    assert_eq!(
        parse_thread_payload(
            &invalid_standard_pitch_marker,
            38,
            ThreadPrefix::Standard,
            vec![988],
        ),
        None
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#standard-thread",
        crate::records::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag = "901".into();
    scope.paired_class_tag = "902".into();
    scope.frame_length = 17;
    scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![988, 989]);
    assert_thread_construction(exact_thread_construction(&bytes, &scope), &expected);

    let mut owner_marked = bytes;
    owner_marked.splice(20..20, [1, 0, 0, 0]);
    let shifted_expected =
        parse_thread_payload(&owner_marked, 42, ThreadPrefix::Standard, vec![988])
            .expect("owner-marked standard Thread payload");
    assert_eq!(shifted_expected.designation_offset, 42);
    scope.frame_length += 4;
    assert_thread_construction(
        exact_thread_construction(&owner_marked, &scope),
        &shifted_expected,
    );
    let mut invalid_owner_marker = owner_marked.clone();
    invalid_owner_marker[20..24].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        exact_thread_construction(&invalid_owner_marker, &scope),
        None
    );
    assert_eq!(
        parse_thread_payload(&owner_marked, 42, ThreadPrefix::Compact, vec![988]),
        None
    );
}

#[test]
fn thread_scope_decodes_class_334_legacy_standard_tail() {
    let mut bytes = vec![0; 148];
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x67, 0]);
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M7x1");
    lp_utf16(&mut payload, "7.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    assert_eq!(payload.len(), 62);
    bytes[38..100].copy_from_slice(&payload);
    bytes[100..105].copy_from_slice(&[1, 1, 0, 0, 0]);
    bytes[105..113].copy_from_slice(&0.71472f64.to_le_bytes());
    bytes[113..121].copy_from_slice(&0.60355f64.to_le_bytes());
    bytes[121] = 0;
    bytes[122..130].copy_from_slice(&0.1f64.to_le_bytes());
    bytes[130..138].copy_from_slice(&0.64255f64.to_le_bytes());
    bytes[138..142].copy_from_slice(&[0, 0, 0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::StandardLegacy,
        designation_offset: 38,
        designation: "M7x1".into(),
        nominal_size: crate::records::DesignThreadNominalSize::try_from("7.0".to_owned()).expect("nominal size"),
        profile: "ISO Metric profile".into(),
        major_diameter: 0.71472,
        minor_diameter: 0.60355,
        pitch: 0.1,
        pitch_diameter: 0.64255,
        face_group_record_indices: vec![988],
    };
    assert_thread_construction(
        parse_thread_payload(&bytes, 38, ThreadPrefix::Standard, vec![988]),
        &expected,
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#legacy-thread",
        crate::records::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag = "334".into();
    scope.paired_class_tag = "262".into();
    scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![988, 991]);
    assert_thread_construction(exact_thread_construction(&bytes, &scope), &expected);

    scope.class_tag = "335".into();
    scope.paired_class_tag = "258".into();
    assert_eq!(exact_thread_construction(&bytes, &scope), None);
}

fn assert_thread_construction(
    actual: Option<DesignThreadConstruction>,
    expected: &DesignThreadConstruction,
) {
    let actual = actual.expect("typed Thread construction");
    assert_eq!(actual.form, expected.form);
    assert_eq!(actual.designation_offset, expected.designation_offset);
    assert_eq!(actual.designation, expected.designation);
    assert_eq!(actual.nominal_size.text(), expected.nominal_size.text());
    assert_eq!(actual.profile, expected.profile);
    assert_eq!(
        actual.face_group_record_indices,
        expected.face_group_record_indices
    );
    for (actual, expected) in [
        (actual.nominal_size.value().expect("actual nominal size"), expected.nominal_size.value().expect("expected nominal size")),
        (actual.major_diameter, expected.major_diameter),
        (actual.minor_diameter, expected.minor_diameter),
        (actual.pitch, expected.pitch),
        (actual.pitch_diameter, expected.pitch_diameter),
    ] {
        assert!((actual - expected).abs() < 1.0e-12);
    }
}

#[test]
fn thread_scope_decodes_compact_preamble_and_localized_profile() {
    let mut bytes = vec![0; 160];
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M3.5x0.6");
    lp_utf16(&mut payload, "3.5");
    lp_utf16(&mut payload, "GB Metric profile");
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[0, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x48, 0]);
    bytes[38..38 + payload.len()].copy_from_slice(&payload);
    let after_profile = 38 + payload.len();
    assert_eq!(after_profile, 106);
    bytes[after_profile..after_profile + 5].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[after_profile + 5..after_profile + 13].copy_from_slice(&0.35995f64.to_le_bytes());
    bytes[after_profile + 13..after_profile + 21].copy_from_slice(&0.293f64.to_le_bytes());
    bytes[after_profile + 21] = 0;
    bytes[after_profile + 22..after_profile + 30].copy_from_slice(&0.06f64.to_le_bytes());
    bytes[after_profile + 30..after_profile + 38].copy_from_slice(&0.3166f64.to_le_bytes());
    bytes[after_profile + 38..after_profile + 42].copy_from_slice(&[0, 0, 0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::Compact(None),
        designation_offset: 38,
        designation: "M3.5x0.6".into(),
        nominal_size: crate::records::DesignThreadNominalSize::try_from("3.5".to_owned()).expect("nominal size"),
        profile: "GB Metric profile".into(),
        major_diameter: 0.35995,
        minor_diameter: 0.293,
        pitch: 0.06,
        pitch_diameter: 0.3166,
        face_group_record_indices: vec![988],
    };
    assert_thread_construction(
        parse_thread_payload(&bytes, 38, ThreadPrefix::Compact, vec![988]),
        &expected,
    );
    let mut referenced = bytes.clone();
    referenced[after_profile + 38] = 1;
    referenced[after_profile + 39..after_profile + 43].copy_from_slice(&2075u32.to_le_bytes());
    referenced[after_profile + 43..after_profile + 49].fill(0);
    let mut referenced_expected = expected.clone();
    referenced_expected.form = DesignThreadForm::Compact(Some(crate::records::Located { value: std::num::NonZeroU32::new(2075).expect("reference"), offset: (after_profile + 39) as u64 }));
    assert_thread_construction(
        parse_thread_payload(&referenced, 38, ThreadPrefix::Compact, vec![988]),
        &referenced_expected,
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#compact-thread",
        crate::records::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag = "903".into();
    scope.frame_length = 19;
    scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![988, 989, 992, 993]);
    scope.paired_class_tag = "904".into();
    let mut plural_expected = expected.clone();
    plural_expected.face_group_record_indices.push(992);
    assert_thread_construction(exact_thread_construction(&bytes, &scope), &plural_expected);

    let mut owner_marked = bytes;
    owner_marked.splice(20..20, [1, 0, 0, 0]);
    plural_expected.designation_offset += 4;
    scope.frame_length += 4;
    assert_thread_construction(
        exact_thread_construction(&owner_marked, &scope),
        &plural_expected,
    );
    let mut invalid_owner_separator = owner_marked.clone();
    invalid_owner_separator[24] = 1;
    assert_eq!(
        exact_thread_construction(&invalid_owner_separator, &scope),
        None
    );

    scope.reference_members = { let mut values: Vec<u32> = scope.reference_members.values().copied().collect(); values.push(994); crate::records::ReferenceRun::Unlocated(values) };
    assert_eq!(exact_thread_construction(&owner_marked, &scope), None);
}

#[test]
fn thread_scope_decodes_class_414_legacy_compact_tail() {
    let mut bytes = vec![0; 160];
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M190x8");
    lp_utf16(&mut payload, "190.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[0, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x48, 0]);
    bytes[38..38 + payload.len()].copy_from_slice(&payload);
    let after_profile = 38 + payload.len();
    bytes[after_profile..after_profile + 5].copy_from_slice(&[1, 1, 0, 0, 0]);
    bytes[after_profile + 5..after_profile + 13].copy_from_slice(&19.08149f64.to_le_bytes());
    bytes[after_profile + 13..after_profile + 21].copy_from_slice(&18.18397f64.to_le_bytes());
    bytes[after_profile + 21] = 0;
    bytes[after_profile + 22..after_profile + 30].copy_from_slice(&0.8f64.to_le_bytes());
    bytes[after_profile + 30..after_profile + 38].copy_from_slice(&18.50413f64.to_le_bytes());
    bytes[after_profile + 38..after_profile + 42].copy_from_slice(&[0, 0, 0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::CompactLegacy,
        designation_offset: 38,
        designation: "M190x8".into(),
        nominal_size: crate::records::DesignThreadNominalSize::try_from("190.0".to_owned()).expect("nominal size"),
        profile: "ISO Metric profile".into(),
        major_diameter: 19.08149,
        minor_diameter: 18.18397,
        pitch: 0.8,
        pitch_diameter: 18.50413,
        face_group_record_indices: vec![988],
    };
    assert_thread_construction(
        parse_thread_payload(&bytes, 38, ThreadPrefix::Compact, vec![988]),
        &expected,
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#legacy-compact-thread",
        crate::records::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag = "414".into();
    scope.paired_class_tag = "263".into();
    scope.frame_length = 19;
    scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![988, 989]);
    assert_thread_construction(exact_thread_construction(&bytes, &scope), &expected);

    scope.class_tag = "334".into();
    scope.paired_class_tag = "262".into();
    assert_eq!(exact_thread_construction(&bytes, &scope), None);
}

#[test]
fn localized_sketch_scope_retains_its_generic_reference_table() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for record_index in [55u32, 56] {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Esquisse");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "301".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("localized Sketch scope");
    assert_eq!(scope.kind(), crate::records::DesignFeatureKind::Esquisse);
    assert_eq!(scope.reference_members.values().copied().collect::<Vec<_>>(), [55, 56]);
    assert!(scope.sketch_entity().is_none());
}
