use super::*;
use crate::design::{
    decode::parameters::parse_design_parameter_record, test_support::parameter_record,
};
use crate::test_support::indexed_header;
use std::collections::HashMap;

fn reference(out: &mut Vec<u8>, index: u32, guid: Option<&str>) {
    out.push(1);
    out.extend(u64::from(index).to_le_bytes());
    if let Some(guid) = guid {
        out.extend(36_u32.to_le_bytes());
        out.extend(guid.as_bytes());
    }
    out.extend([0, 0]);
}

fn parameter() -> crate::records::parameters::DesignParameter {
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(44),
        "-15 mm",
        "AlongDistance",
        Some("mm"),
        "d1",
        -1.5,
    ))
    .unwrap();
    parameter.id = "f3d:test/Design1/BulkStream.dat:parameter#0".into();
    parameter
}

fn fixture(
    typed_references: bool,
    padding: usize,
    expanded: bool,
) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"000", 10);
    bytes.extend([0; 9]);
    bytes.push(u8::from(expanded));
    if expanded {
        bytes.extend(0_u32.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend(4_u32.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0, padding));
    for value in [0.0_f64, -1.0, 0.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.push(0);
    reference(
        &mut bytes,
        44,
        typed_references.then_some("D91D429C-1BB5-4640-B49F-69AF14F463E9"),
    );
    bytes.extend([0; 8]);
    reference(&mut bytes, 30, typed_references.then_some(SETTINGS_GUID));
    bytes.extend([0; 8]);
    bytes.extend(2_u32.to_le_bytes());
    bytes.push(0);
    reference(&mut bytes, 40, typed_references.then_some(CARRIER_GUID));
    bytes.extend(0.125_f64.to_le_bytes());
    bytes.resize(512, 0);
    let mut scope = DesignParameterScope::empty(
        "f3d:test/Design1/BulkStream.dat:scope#0",
        DesignFeatureKind::ContourFlange,
        10,
    );
    crate::design::test_support::name_scope_references(&mut scope, [44, 30, 40]);
    scope
        .try_edit(|draft| crate::design::test_support::lengthen_scope_frame(draft, 512))
        .unwrap();
    (bytes, scope)
}

fn types() -> Types<'static> {
    HashMap::from([
        (10, (CONTOUR_GUID, 4)),
        (30, (SETTINGS_GUID, 3)),
        (40, (CARRIER_GUID, 0)),
        (44, ("D91D429C-1BB5-4640-B49F-69AF14F463E9", 0)),
    ])
}

#[test]
fn contour_prefix_normalizes_variable_padding_and_reference_widths() {
    for typed_references in [false, true] {
        for expanded in [false, true] {
            for padding in [8, 13, 17] {
                let (bytes, scope) = fixture(typed_references, padding, expanded);
                let parameters = [parameter()];
                let sources = ParameterScopeSources {
                    parameters: &parameters,
                    ..Default::default()
                };
                let value = operation(
                    &bytes,
                    &IndexedRecordOffsets::build(&bytes),
                    &scope,
                    &types(),
                    &sources,
                )
                .unwrap();
                assert!(value.valid_for_scope(&scope));
                assert_eq!(value.operation.value, 4);
                assert_eq!(value.profile_control.as_ref().unwrap().value, 2);
                assert_eq!(value.inline_thickness.as_ref().unwrap().value.get(), 0.125);
                let travel = value.travel.unwrap();
                assert_eq!(travel.owner.value, 44);
                assert_eq!(travel.direction.value.get(), Vector3::new(0.0, -1.0, 0.0));
                assert_eq!(
                    travel.direction.offset,
                    u64::try_from(if expanded { 30 + padding } else { 26 + padding }).unwrap()
                );
                assert!(value.rule.is_none());
            }
        }
    }
}

#[test]
fn contour_prefix_rejects_wrong_version_and_retains_partial_ambiguous_inputs() {
    let (mut bytes, scope) = fixture(false, 8, true);
    let mut types = types();
    types.insert(10, (CONTOUR_GUID, 2));
    assert!(operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &types,
        &ParameterScopeSources::default()
    )
    .is_none());
    types.insert(10, (CONTOUR_GUID, 4));
    let mut duplicate = Vec::new();
    reference(&mut duplicate, 40, None);
    bytes[220..231].copy_from_slice(&duplicate);
    let partial = operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &types,
        &ParameterScopeSources::default(),
    )
    .unwrap();
    assert!(partial.carrier.is_none());
    assert!(partial.inline_thickness.is_none());
    assert!(partial.travel.is_some());
}

#[test]
fn missing_direction_definition_retains_independent_prefix_fields() {
    let (mut bytes, scope) = fixture(true, 13, true);
    let value = operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &types(),
        &ParameterScopeSources::default(),
    )
    .unwrap();
    assert!(value.travel.as_ref().unwrap().owner_role_assumed);
    assert_eq!(value.inline_thickness.as_ref().unwrap().value.get(), 0.125);
    let carrier_at = usize::try_from(value.carrier.as_ref().unwrap().offset).unwrap();
    bytes[carrier_at + 51..carrier_at + 59].copy_from_slice(&f64::INFINITY.to_le_bytes());
    let partial = operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &types(),
        &ParameterScopeSources::default(),
    )
    .unwrap();
    assert!(partial.inline_thickness.is_none());
    assert!(partial.travel.is_some());
}

#[test]
fn contour_scope_kind_separates_shared_guid_versions_from_planar_faces() {
    for (version, padding) in [(3, 8), (4, 13), (6, 17)] {
        let (bytes, scope) = fixture(false, padding, true);
        let mut types = types();
        types.insert(10, (CONTOUR_GUID, version));
        let value = operation(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &types,
            &ParameterScopeSources::default(),
        )
        .unwrap();
        assert!(value.travel.is_some());
        assert_eq!(value.inline_thickness.unwrap().value.get(), 0.125);
        let face = DesignParameterScope::empty(
            "f3d:test/Design1/BulkStream.dat:scope#0",
            DesignFeatureKind::Face,
            10,
        );
        assert!(operation(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &face,
            &types,
            &ParameterScopeSources::default()
        )
        .is_none());
    }
}
