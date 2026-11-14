use super::*;
use crate::test_support::{indexed_header, lp_utf16, with_decode_context};

fn reference(out: &mut Vec<u8>, index: u32, guid: Option<&str>) {
    out.push(1);
    out.extend(u64::from(index).to_le_bytes());
    if let Some(guid) = guid {
        out.extend(36_u32.to_le_bytes());
        out.extend(guid.as_bytes());
    }
    out.extend([0, 0]);
}

#[test]
fn unfold_prefix_reads_both_reference_forms_and_extension_widths() {
    for typed_references in [false, true] {
        for expanded in [false, true] {
            for flag in [0, 1, 7] {
                let mut scope = DesignParameterScope::empty(
                    "f3d:test/Design1/BulkStream.dat:scope#10",
                    DesignFeatureKind::Unfold,
                    10,
                );
                crate::design::test_support::name_scope_references(&mut scope, [20, 30, 40]);
                scope
                    .try_edit(|draft| crate::design::test_support::lengthen_scope_frame(draft, 512))
                    .unwrap();
                let mut bytes = Vec::new();
                indexed_header(&mut bytes, *b"000", 10);
                bytes.extend([0; 9]);
                bytes.push(u8::from(expanded));
                if expanded {
                    bytes.extend([0; 4]);
                }
                bytes.push(flag);
                for (index, guid) in [
                    (20, SELECTION_GUID),
                    (30, SETTINGS_GUID),
                    (35, WRAPPER_GUID),
                    (40, SELECTION_GUID),
                ] {
                    reference(&mut bytes, index, typed_references.then_some(guid));
                }
                bytes.resize(512, 0);
                let records = IndexedRecordOffsets::build(&bytes);
                let types = HashMap::from([(10, (UNFOLD_GUID, 0))]);
                let value = unfold(&bytes, &records, &scope, &types).unwrap();
                assert!(value.valid_for_scope(&scope));
                let mut invalid = value.clone();
                invalid.prefix_end += 1;
                assert!(!invalid.valid_for_scope(&scope));
                assert_eq!(value.control.value, flag);
                assert_eq!(value.control.offset, if expanded { 25 } else { 21 });
                assert_eq!(value.bend_group.value, 20);
                assert_eq!(value.settings.value, 30);
                assert_eq!(value.wrapper.value, 35);
                assert_eq!(value.stationary_group.value, 40);
                assert!(value.rule_material_scalar.is_none());
                bytes[11] = 1;
                assert!(unfold(&bytes, &records, &scope, &types).is_none());
            }
        }
    }
}

fn group_bytes(typed_references: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"000", 90);
    bytes.extend([0; 10]);
    lp_utf16(&mut bytes, "UnfoldRefoldGroupMode");
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend(4_u32.to_le_bytes());
    for (index, guid) in [
        (80, UNFOLD_GUID),
        (200, WRAPPER_GUID),
        (150, WRAPPER_GUID),
        (20, REFOLD_GUID),
    ] {
        reference(&mut bytes, index, typed_references.then_some(guid));
    }
    bytes.extend([0; 5]);
    bytes
}

#[test]
fn group_keeps_nonmonotonic_authored_order_and_checks_boundaries() {
    with_decode_context(|ctx| {
        for typed_references in [false, true] {
            let mut bytes = group_bytes(typed_references);
            assert_eq!(bytes.len(), if typed_references { 285 } else { 125 });
            let value = group(ctx, &bytes, 0, bytes.len(), &HashMap::new(), 90)
                .unwrap()
                .unwrap();
            assert!(value
                .members
                .iter()
                .map(|member| member.value)
                .eq([80, 200, 150, 20]));
            let scope = DesignParameterScope::empty(
                "f3d:test/Design1/BulkStream.dat:scope#20",
                DesignFeatureKind::Refold,
                20,
            );
            assert!(value.valid_for_scope(&scope));
            let mut invalid = value.clone();
            invalid.group_paired_byte_offset += 1;
            assert!(!invalid.valid_for_scope(&scope));
            assert_eq!(
                value.members.first().map(|member| &member.offset),
                Some(&76)
            );
            assert!(group(ctx, &bytes, 0, bytes.len() - 1, &HashMap::new(), 90)
                .unwrap()
                .is_none());
            bytes[71] = 0;
            assert!(group(ctx, &bytes, 0, bytes.len(), &HashMap::new(), 90)
                .unwrap()
                .is_none());
            bytes[71] = 1;
            bytes[72..76].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(group(ctx, &bytes, 0, bytes.len(), &HashMap::new(), 90)
                .unwrap()
                .is_none());
        }
    });
}

#[test]
fn inline_reference_type_must_agree_with_registered_target() {
    let mut bytes = Vec::new();
    reference(&mut bytes, 10, Some(SELECTION_GUID));
    let types = HashMap::from([(10, (SETTINGS_GUID, 1))]);
    assert!(local(&mut View::over_retained(&bytes), &types, None).is_none());
    assert!(local(
        &mut View::over_retained(&bytes),
        &HashMap::new(),
        Some(SETTINGS_GUID)
    )
    .is_none());
}

fn rule_bytes(version: u32, scalar: f64, duplicate: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"000", 60);
    bytes.extend([0; 10]);
    if version == 0 {
        bytes.extend(1_u32.to_le_bytes());
        reference(&mut bytes, 30, None);
    }
    bytes.extend(3_u32.to_le_bytes());
    lp_utf16(&mut bytes, "Thickness");
    bytes.extend(0.125_f64.to_le_bytes());
    bytes.extend([0; 12]);
    for _ in 0..if duplicate { 2 } else { 1 } {
        bytes.extend(scalar.to_le_bytes());
        lp_utf16(&mut bytes, "Alloy");
        lp_utf16(&mut bytes, "Thickness");
        bytes.extend(0.125_f64.to_le_bytes());
    }
    indexed_header(&mut bytes, *b"001", 60);
    bytes
}

#[test]
fn rule_material_scalar_requires_one_finite_trailer_and_bounded_consumers() {
    for version in [0, 1] {
        for scalar in [0.44, 1.0] {
            let bytes = rule_bytes(version, scalar, false);
            let records = IndexedRecordOffsets::build(&bytes);
            let types = HashMap::from([(60, (RULE_GUID, version))]);
            assert_eq!(
                material_scalar(&bytes, &records, &types, 60)
                    .unwrap()
                    .value
                    .get(),
                scalar
            );
            let duplicated = rule_bytes(version, scalar, true);
            assert!(material_scalar(
                &duplicated,
                &IndexedRecordOffsets::build(&duplicated),
                &types,
                60
            )
            .is_none());
            let invalid = rule_bytes(version, f64::INFINITY, false);
            assert!(
                material_scalar(&invalid, &IndexedRecordOffsets::build(&invalid), &types, 60)
                    .is_none()
            );
            if version == 0 {
                let mut invalid = bytes.clone();
                invalid[21..25].copy_from_slice(&u32::MAX.to_le_bytes());
                assert!(material_scalar(
                    &invalid,
                    &IndexedRecordOffsets::build(&invalid),
                    &types,
                    60
                )
                .is_none());
            }
        }
    }
}

#[test]
fn group_source_uses_registered_type_and_order_before_refold() {
    with_decode_context(|ctx| {
        let mut bytes = group_bytes(true);
        indexed_header(&mut bytes, *b"001", 90);
        let types = HashMap::from([
            (90, (GROUP_GUID, 0)),
            (80, (UNFOLD_GUID, 0)),
            (20, (REFOLD_GUID, 1)),
        ]);
        let mut scopes = [DesignParameterScope::empty(
            "f3d:test/Design1/BulkStream.dat:scope#20",
            DesignFeatureKind::Refold,
            20,
        )];
        bind(
            ctx,
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &types,
            &mut scopes,
        )
        .unwrap();
        assert_eq!(
            scopes[0].refold_operation().unwrap().preceding_unfolds,
            vec![80]
        );
    });
}

#[test]
fn equivalent_group_carriers_keep_one_source_and_incompatible_groups_refuse_it() {
    with_decode_context(|ctx| {
        let mut bytes = group_bytes(false);
        indexed_header(&mut bytes, *b"001", 90);
        let mut second = group_bytes(false);
        second[7..11].copy_from_slice(&91_u32.to_le_bytes());
        indexed_header(&mut second, *b"001", 91);
        let second_at = bytes.len();
        bytes.extend(second);
        let types = HashMap::from([
            (90, (GROUP_GUID, 0)),
            (91, (GROUP_GUID, 0)),
            (80, (UNFOLD_GUID, 0)),
            (100, (UNFOLD_GUID, 0)),
            (20, (REFOLD_GUID, 1)),
        ]);
        let mut scopes = [DesignParameterScope::empty(
            "f3d:test/Design1/BulkStream.dat:scope#20",
            DesignFeatureKind::Refold,
            20,
        )];
        bind(
            ctx,
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &types,
            &mut scopes,
        )
        .unwrap();
        assert_eq!(
            scopes[0].refold_operation().unwrap().preceding_unfolds,
            vec![80]
        );
        bytes[second_at + 77..second_at + 85].copy_from_slice(&100_u64.to_le_bytes());
        bind(
            ctx,
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &types,
            &mut scopes,
        )
        .unwrap();
        assert!(scopes[0].refold_operation().is_none());
    });
}

#[test]
fn settings_rule_checks_versioned_optional_slots_and_owner_boundary() {
    for (version, slots) in [(1, 6), (2, 7), (3, 9)] {
        for typed_references in [false, true] {
            let mut bytes = Vec::new();
            indexed_header(&mut bytes, *b"000", 30);
            bytes.extend([0; 10]);
            bytes.extend(0_u32.to_le_bytes());
            bytes.extend(std::iter::repeat_n(0, slots));
            reference(&mut bytes, 60, typed_references.then_some(RULE_GUID));
            bytes.extend(if version == 3 { vec![0; 2] } else { vec![0; 4] });
            reference(
                &mut bytes,
                31,
                typed_references.then_some(PRESENTATION_GUID),
            );
            bytes.push(0);
            reference(&mut bytes, 80, typed_references.then_some(UNFOLD_GUID));
            indexed_header(&mut bytes, *b"001", 30);
            let types = HashMap::from([(30, (SETTINGS_GUID, version))]);
            let records = IndexedRecordOffsets::build(&bytes);
            assert_eq!(settings_rule(&bytes, &records, &types, 30, 80), Some(60));
            assert_eq!(settings_rule(&bytes, &records, &types, 30, 81), None);
            bytes[11] = 1;
            assert_eq!(settings_rule(&bytes, &records, &types, 30, 80), None);
        }
    }
}
