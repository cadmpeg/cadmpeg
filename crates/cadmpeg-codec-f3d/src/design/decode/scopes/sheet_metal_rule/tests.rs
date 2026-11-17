use super::*;
use crate::test_support::{indexed_header, lp_utf16};

fn material_scalar(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &Types<'_>,
    index: u32,
) -> Option<Located<FiniteReal>> {
    rule_values(bytes, records, types, index).map(|values| values.material_scalar)
}

fn reference(out: &mut Vec<u8>, index: u32, guid: Option<&str>) {
    out.push(1);
    out.extend(u64::from(index).to_le_bytes());
    if let Some(guid) = guid {
        out.extend(36_u32.to_le_bytes());
        out.extend(guid.as_bytes());
    }
    out.extend([0, 0]);
}

const SELECTION_GUID: &str = "2CA5A1CD-C99B-4C9B-91AF-57989148E841";
const WRAPPER_GUID: &str = "2E3AC990-483D-4A55-8662-C064B976A957";

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
            assert_eq!(
                settings_rule(&bytes, &records, &types, 30, 80, UNFOLD_GUID),
                Some((60, false))
            );
            assert_eq!(
                settings_rule(&bytes, &records, &types, 30, 81, UNFOLD_GUID),
                None
            );
            bytes[11] = 1;
            assert_eq!(
                settings_rule(&bytes, &records, &types, 30, 80, UNFOLD_GUID),
                None
            );
        }
    }
}

#[test]
fn settings_rule_recovers_unique_typed_target_with_present_inputs() {
    for typed_references in [false, true] {
        for ambiguous in [false, true] {
            let mut bytes = Vec::new();
            indexed_header(&mut bytes, *b"000", 30);
            bytes.extend([0; 10]);
            reference(&mut bytes, 70, typed_references.then_some(WRAPPER_GUID));
            bytes.extend([0; 12]);
            reference(&mut bytes, 60, typed_references.then_some(RULE_GUID));
            if ambiguous {
                reference(&mut bytes, 61, typed_references.then_some(RULE_GUID));
            }
            bytes.extend([1, 0, 0]);
            reference(
                &mut bytes,
                31,
                typed_references.then_some(PRESENTATION_GUID),
            );
            bytes.push(0);
            reference(&mut bytes, 80, typed_references.then_some(UNFOLD_GUID));
            indexed_header(&mut bytes, *b"001", 30);
            let types = HashMap::from([
                (30, (SETTINGS_GUID, 3)),
                (60, (RULE_GUID, 1)),
                (61, (RULE_GUID, 0)),
                (70, (WRAPPER_GUID, 0)),
                (80, (UNFOLD_GUID, 0)),
            ]);
            let records = IndexedRecordOffsets::build(&bytes);
            assert_eq!(
                settings_rule(&bytes, &records, &types, 30, 80, UNFOLD_GUID),
                if ambiguous { None } else { Some((60, true)) }
            );
            assert_eq!(
                settings_rule(&bytes, &records, &types, 30, 81, UNFOLD_GUID),
                None
            );
            bytes[11] = 1;
            assert_eq!(
                settings_rule(&bytes, &records, &types, 30, 80, UNFOLD_GUID),
                None
            );
        }
    }
}

#[test]
fn independent_rule_entries_preserve_radius_and_explicit_thickness_separately() {
    let mut bytes = rule_bytes(1, 0.44, false);
    bytes[47..55].copy_from_slice(&0.2_f64.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let types = HashMap::from([(60, (RULE_GUID, 1))]);
    assert_eq!(
        material_scalar(&bytes, &records, &types, 60)
            .expect("independent scalar")
            .value
            .get(),
        0.44
    );
    let value = rule_values(&bytes, &records, &types, 60).unwrap();
    assert_eq!(value.first_value.value.get(), 0.2);
    assert_eq!(value.thickness.value.get(), 0.125);
    assert_eq!(value.material_scalar.value.get(), 0.44);
}
