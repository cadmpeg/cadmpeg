// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::align::{align_envelope_links_hold, decode_align_envelope};
use crate::records::feature::align::DesignAlignForm;
use crate::test_support::align::align_prefix_fixture;

#[test]
fn align_validator_checks_both_forms_references_and_locations() {
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        let (bytes, scope) = align_prefix_fixture(form);
        let envelope = decode_align_envelope(&bytes, &scope).unwrap();
        assert!(align_envelope_links_hold(&scope, &envelope));
        for field in 0..6 {
            let mut changed = envelope.clone();
            match field {
                0 => changed.first_reference.value += 1,
                1 => changed.first_reference.offset += 1,
                2 => changed.second_reference.value += 1,
                3 => changed.second_reference.offset += 1,
                4 => changed.source_code.offset += 1,
                _ => {
                    changed.form = match form {
                        DesignAlignForm::Compact => DesignAlignForm::Extended,
                        DesignAlignForm::Extended => DesignAlignForm::Compact,
                    }
                }
            }
            assert!(!align_envelope_links_hold(&scope, &changed));
        }
    }
}

#[test]
fn align_validator_checks_complete_constructor_length_owners_and_field_locations() {
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        let (prefix, scope) = align_prefix_fixture(form);
        let mut envelope = decode_align_envelope(&prefix, &scope).unwrap();
        envelope.construction =
            Some(crate::test_support::align::align_construction_value_fixture(&scope));
        assert!(envelope.construction.is_some());
        assert!(align_envelope_links_hold(&scope, &envelope));
        for field in 0..20 {
            let mut changed = envelope.clone();
            let construction = changed.construction.as_mut().unwrap();
            match field {
                0 => construction.record_index += 1,
                1 => construction.byte_offset += 1,
                2 => construction.paired_byte_offset += 1,
                3 => construction.matrix_scalars.offset += 1,
                4 => construction.scalar.offset += 1,
                5 => construction.marked_words[4].value += 1,
                6 => construction.marked_words[7].value += 1,
                7..=14 => construction.marked_words[field - 7].offset += 1,
                _ => construction.codes[field - 15].offset += 1,
            }
            assert!(
                !align_envelope_links_hold(&scope, &changed),
                "field {field}"
            );
        }
    }
}

#[test]
fn align_motion_validator_checks_form_links_leading_fields_and_omitted_identity() {
    use crate::design::decode::scopes::direct_face::move_operation_links_hold;
    use crate::records::feature::direct_face::{
        DesignMoveForm, DesignMoveLeading, DesignMoveOperation, DesignMovePrefix,
        DesignMoveReference,
    };
    use crate::records::feature::scope::DesignScopePayloadMut;

    let (bytes, mut scope) = align_prefix_fixture(DesignAlignForm::Extended);
    let mut envelope = decode_align_envelope(&bytes, &scope).unwrap();
    envelope.motion = Some(DesignMoveOperation {
        transform: crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
        transform_offset: 568,
        transform_record_index: 806,
        form: DesignMoveForm::Form4,
        form_offset: 563,
        transform_reference_offset: envelope.first_reference.offset.checked_sub(1),
        prefix: Some(DesignMovePrefix {
            leading: Some(DesignMoveLeading {
                scalar: cadmpeg_ir::scalar::FiniteReal::new(0.625).unwrap(),
                references: [
                    DesignMoveReference::Local { record_index: 810 },
                    DesignMoveReference::Local { record_index: 812 },
                ],
            }),
            references: std::array::from_fn(|_| DesignMoveReference::Null),
            codes: [2, 0],
            matrix_omitted: true,
        }),
    });
    let header = crate::records::decal::DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:design-record#806".into(),
        record_index: 806,
        byte_offset: 500,
        class_tag: "500".to_owned().try_into().unwrap(),
    };
    let DesignScopePayloadMut::Align(slot) = scope.payload_mut() else {
        panic!("Align");
    };
    *slot = Some(envelope);
    let valid = |scope: &crate::records::feature::scope::DesignParameterScope| {
        align_envelope_links_hold(scope, scope.align_envelope().unwrap())
            && move_operation_links_hold(scope, Some(&header))
    };
    assert!(valid(&scope));
    assert!(!move_operation_links_hold(&scope, None));
    for field in 0..8 {
        let mut changed = scope.clone();
        let DesignScopePayloadMut::Align(Some(envelope)) = changed.payload_mut() else {
            panic!("Align");
        };
        let motion = envelope.motion.as_mut().unwrap();
        match field {
            0 => motion.form = DesignMoveForm::Form5,
            1 => motion.transform_record_index += 1,
            2 => motion.transform_reference_offset = Some(26),
            3 => motion.form_offset += 1,
            4 => motion.transform_offset += 1,
            5 => {
                motion
                    .prefix
                    .as_mut()
                    .unwrap()
                    .leading
                    .as_mut()
                    .unwrap()
                    .references[0] = DesignMoveReference::Typed {
                    record_index: 810,
                    type_guid: "broken".into(),
                }
            }
            6 => {
                let mut matrix = motion.transform.rows();
                matrix[0][3] = 1.;
                motion.transform = matrix.try_into().unwrap();
            }
            _ => motion.prefix.as_mut().unwrap().leading = None,
        }
        assert!(!valid(&changed), "field {field}");
    }
    let envelope = scope.align_envelope().unwrap();
    let scope_wire = serde_json::to_value(&scope).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::records::feature::scope::DesignParameterScope>(
            scope_wire.clone()
        )
        .unwrap(),
        scope
    );
    assert!(scope_wire.get("move_operation").is_none());
    let wire = serde_json::to_value(envelope).unwrap();
    assert_eq!(wire["motion"]["form"], 4);
    assert_eq!(
        serde_json::from_value::<crate::records::feature::align::DesignAlignEnvelope>(wire.clone())
            .unwrap(),
        *envelope
    );
    for field in ["motion", "leading", "scalar"] {
        let mut changed = wire.clone();
        match field {
            "motion" => changed["motion"] = serde_json::Value::Null,
            "leading" => changed["motion"]["prefix"]["leading"] = serde_json::Value::Null,
            _ => changed["motion"]["prefix"]["leading"]["scalar"] = serde_json::Value::Null,
        }
        assert!(
            serde_json::from_value::<crate::records::feature::align::DesignAlignEnvelope>(changed)
                .is_err(),
            "field {field}"
        );
    }
    let mut legacy = wire;
    legacy.as_object_mut().unwrap().remove("motion");
    assert!(
        serde_json::from_value::<crate::records::feature::align::DesignAlignEnvelope>(legacy)
            .unwrap()
            .motion
            .is_none()
    );
}
