use super::split_face_operation;
use crate::design::decode::meta::unambiguous_design_primary_frames;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::feature::split_face::{
    DesignSplitFaceDirectionReference, DesignSplitFaceMethod, DesignSplitFaceMethodControl,
};
use crate::records::identity::{Located, ReferenceRun};
use crate::test_support::split_face::metadata;

fn split_face_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::SplitFace,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.frame_length = 408;
            draft.reference_count_offset = 120;
            draft.reference_members = ReferenceRun::located(vec![Located {
                value: 9,
                offset: 125,
            }]);
            draft.kind_offset = 143;
            draft.feature_ordinal_offset = 330;
            draft.paired_byte_offset = 408;
        })
        .unwrap();
    scope
}

fn method_bytes(
    family: u32,
    projection: u8,
    extension: u8,
    carrier: Option<u64>,
    property_block: bool,
) -> Vec<u8> {
    let mut bytes = vec![0; 408];
    bytes[19] = 0;
    let mut reference_at = 21;
    if property_block {
        bytes[20] = 1;
        bytes[21..25].copy_from_slice(&0_u32.to_le_bytes());
        reference_at = 25;
    }
    let method_at = match carrier {
        None => {
            bytes[reference_at] = 0;
            reference_at + 1
        }
        Some(record_index) => {
            bytes[reference_at] = 1;
            bytes[reference_at + 1..reference_at + 9].copy_from_slice(&record_index.to_le_bytes());
            bytes[reference_at + 9] = 0;
            bytes[reference_at + 10] = 0;
            reference_at + 11
        }
    };
    bytes[method_at..method_at + 4].copy_from_slice(&family.to_le_bytes());
    bytes[method_at + 4] = projection;
    bytes[method_at + 5] = extension;
    bytes
}

fn decode(bytes: &[u8]) -> crate::records::feature::split_face::DesignSplitFaceOperation {
    let scope = split_face_scope();
    crate::test_support::with_decode_context(|ctx| split_face_operation(ctx, bytes, &scope, &[]))
        .unwrap()
        .unwrap()
}

#[test]
fn surface_intersection_preserves_each_supported_extension_value() {
    for extension in [0, 1] {
        let bytes = method_bytes(1, 0, extension, None, false);
        let operation = decode(&bytes);
        assert_eq!(
            operation.method(),
            DesignSplitFaceMethod::SurfaceIntersection {
                extend: extension != 0
            }
        );
        assert_eq!(operation.method_offset, 22);
    }
}

#[test]
fn closest_point_reads_controls_after_an_empty_property_block() {
    let bytes = method_bytes(1, 1, 0, None, true);
    let operation = decode(&bytes);
    assert_eq!(operation.method(), DesignSplitFaceMethod::ClosestPoint);
    assert_eq!(operation.method_offset, 26);
}

#[test]
fn along_vector_keeps_its_carrier_when_the_source_record_is_unavailable() {
    let bytes = method_bytes(0, 1, 0, Some(77), false);
    let operation = decode(&bytes);
    assert_eq!(operation.method(), DesignSplitFaceMethod::AlongVector);
    assert_eq!(operation.direction, None);
    assert!(operation.inputs.is_empty());
    assert!(matches!(
        operation.direction_reference,
        DesignSplitFaceDirectionReference::Local {
            target: Located { value: 77, .. },
            ..
        }
    ));
}

#[test]
fn along_vector_keeps_a_cross_segment_carrier_without_resolving_its_source() {
    let mut bytes = vec![0; 408];
    bytes[19] = 0;
    bytes[21] = 1;
    bytes[22..30].copy_from_slice(&77_u64.to_le_bytes());
    bytes[30] = 0;
    bytes[31] = 1;
    bytes[32..36].copy_from_slice(&9_u32.to_le_bytes());
    bytes[36..40].copy_from_slice(&0_u32.to_le_bytes());
    bytes[40] = 1;
    bytes[41] = 0;
    let scope = split_face_scope();
    let operation = crate::test_support::with_decode_context(|ctx| {
        split_face_operation(ctx, &bytes, &scope, &[])
    })
    .unwrap()
    .unwrap();

    assert_eq!(operation.method(), DesignSplitFaceMethod::AlongVector);
    assert_eq!(operation.direction, None);
    assert!(matches!(
        operation.direction_reference,
        DesignSplitFaceDirectionReference::CrossSegment {
            target: Located { value: 77, .. },
            segment: 9,
            same_document_asset: None,
            ..
        }
    ));
}

fn carrier_bytes(fake_header: bool) -> Vec<u8> {
    let mut bytes = method_bytes(0, 1, 0, Some(55), false);
    bytes.truncate(120);
    let mut scope_header = Vec::new();
    crate::test_support::indexed_header(&mut scope_header, *b"257", 1);
    bytes[..11].copy_from_slice(&scope_header);
    crate::test_support::indexed_header(&mut bytes, *b"256", 55);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    for value in [1.25_f64, -2.5, 3.75, 2.4, 0.0, 0.0, 0.125, -0.25] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&19u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&88u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    crate::test_support::indexed_header(&mut bytes, *b"257", 55);
    if fake_header {
        let mut fake = Vec::new();
        crate::test_support::indexed_header(&mut fake, *b"256", 55);
        bytes[145..156].copy_from_slice(&fake);
    }
    bytes
}

fn carrier_metadata(guid: &str, version: u32, secondary: &[(u64, u64)]) -> Vec<u8> {
    metadata(
        &[
            (guid, version, &[55]),
            ("00000000-0000-0000-0000-000000000002", 1, &[1]),
        ],
        &[(1, 0), (55, 120)],
        secondary,
    )
}

fn decode_registered_carrier(
    bytes: &[u8],
    metadata: &[u8],
) -> crate::records::feature::split_face::DesignSplitFaceOperation {
    let meta = crate::metastream::parse(metadata, "synthetic/MetaStream.dat").ok();
    let frames = meta
        .as_ref()
        .and_then(|meta| unambiguous_design_primary_frames(bytes, meta).ok())
        .unwrap_or_default();
    let operation = crate::test_support::with_decode_context(|ctx| {
        split_face_operation(ctx, bytes, &split_face_scope(), &frames)
    })
    .unwrap()
    .unwrap();
    assert!(operation.scope_offsets_valid(0, 120));
    operation
}

#[test]
fn split_face_generic_prefix_boundaries_retain_admitted_scope_and_serde() {
    let bytes = [0_u8; 408];
    for (count_offset, expected) in [(18, None), (19, Some(19)), (20, Some(19)), (21, Some(21))] {
        let mut scope = split_face_scope();
        scope
            .try_edit(|draft| {
                draft.reference_count_offset = count_offset;
                draft.layout_fixture_references();
            })
            .unwrap();
        let operation = crate::test_support::with_decode_context(|ctx| {
            split_face_operation(ctx, &bytes, &scope, &[])
        })
        .unwrap();
        assert_eq!(
            operation.as_ref().map(|value| value.method_offset),
            expected
        );
        scope
            .try_edit(|draft| {
                let crate::records::feature::scope::DesignScopePayload::SplitFace(slot) =
                    &mut draft.payload
                else {
                    panic!("SplitFace")
                };
                *slot = operation;
            })
            .unwrap();
        let wire = serde_json::to_vec(&scope).unwrap();
        let decoded: DesignParameterScope = serde_json::from_slice(&wire).unwrap();
        assert_eq!(decoded.split_face_operation(), scope.split_face_operation());
        crate::test_support::split_face::assert_native_scope(&decoded);
    }
}

#[test]
fn split_face_typed_local_carrier_keeps_qualifier_vector_and_width() {
    let meta = carrier_metadata(
        crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
        1,
        &[(55, 228)],
    );
    for (guid, assumed) in [
        ("F2A7590D-6654-4674-B393-A2AEF4FEC48A", false),
        ("00000000-0000-0000-0000-000000000099", true),
    ] {
        let mut bytes = carrier_bytes(false);
        bytes[30..34].copy_from_slice(&36_u32.to_le_bytes());
        bytes[34..70].copy_from_slice(guid.as_bytes());
        bytes[70..72].copy_from_slice(&[0, 0]);
        bytes[72..78].copy_from_slice(&[0, 0, 0, 0, 1, 0]);
        let operation = decode_registered_carrier(&bytes, &meta);
        assert_eq!(operation.method_offset, 72);
        assert_eq!(
            operation
                .direction
                .as_ref()
                .unwrap()
                .value
                .unwrap()
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            [2.4, 0.0, 0.0]
        );
        assert!(
            matches!(&operation.direction_reference, DesignSplitFaceDirectionReference::Local { target: Located { value: 55, offset: 22 }, inline_type_guid: Some(value) } if value == guid)
        );
        assert_eq!(operation.carrier_type_assumed(), assumed);
        let wire = serde_json::to_vec(&operation).unwrap();
        let decoded: crate::records::feature::split_face::DesignSplitFaceOperation =
            serde_json::from_slice(&wire).unwrap();
        assert_eq!(decoded, operation);
    }
}

#[test]
fn along_vector_reads_the_registered_non_unit_vector_despite_fake_same_id_header() {
    for fake_header in [false, true] {
        let bytes = carrier_bytes(fake_header);
        let meta = carrier_metadata(
            crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
            1,
            &[(55, 228)],
        );
        let operation = decode_registered_carrier(&bytes, &meta);
        assert_eq!(operation.method(), DesignSplitFaceMethod::AlongVector);
        let direction = operation.direction.unwrap();
        assert_eq!(
            direction
                .value
                .unwrap()
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            [2.4, 0.0, 0.0]
        );
        assert_eq!(direction.offset, 169);
        assert_eq!(
            operation.inputs,
            [Located {
                value: DesignSplitFaceDirectionReference::Local {
                    target: Located {
                        value: 88,
                        offset: 218
                    },
                    inline_type_guid: None
                },
                offset: 217
            }]
        );
        assert!(operation.inputs_complete);
    }
}

#[test]
fn along_vector_retains_its_reference_for_missing_or_conflicting_carrier_metadata() {
    let bytes = carrier_bytes(true);
    let guid = crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID;
    let cases = [
        Vec::new(),
        carrier_metadata(guid, 1, &[]),
        carrier_metadata(guid, 1, &[(55, 145)]),
        carrier_metadata(guid, 1, &[(55, 235)]),
        carrier_metadata(guid, 9, &[(55, 228)]),
        carrier_metadata("00000000-0000-0000-0000-000000000001", 1, &[(55, 228)]),
        metadata(
            &[
                (guid, 1, &[55, 55]),
                ("00000000-0000-0000-0000-000000000002", 1, &[1]),
            ],
            &[(1, 0), (55, 120)],
            &[(55, 228)],
        ),
        metadata(
            &[
                (guid, 1, &[55]),
                ("00000000-0000-0000-0000-000000000002", 1, &[1, 55]),
            ],
            &[(1, 0), (55, 120)],
            &[(55, 228)],
        ),
    ];
    for meta in cases {
        let operation = decode_registered_carrier(&bytes, &meta);
        assert_eq!(operation.method(), DesignSplitFaceMethod::AlongVector);
        assert!(operation.direction.is_none());
        assert!(operation.inputs.is_empty());
        assert!(matches!(
            operation.direction_reference,
            DesignSplitFaceDirectionReference::Local {
                target: Located { value: 55, .. },
                ..
            }
        ));
    }
}

#[test]
fn unsupported_method_controls_remain_unresolved_with_their_raw_values() {
    let bytes = method_bytes(1, 1, 1, None, false);
    let operation = decode(&bytes);
    assert_eq!(operation.method(), DesignSplitFaceMethod::Unresolved);
    assert_eq!(
        operation.control,
        Some(DesignSplitFaceMethodControl {
            family: 1,
            projection: 1,
            extension: 1,
        })
    );
}

#[test]
fn along_vector_survives_malformed_or_truncated_input_references() {
    let guid = crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID;
    let meta = carrier_metadata(guid, 1, &[(55, 228)]);
    for malformed in [false, true] {
        let mut bytes = carrier_bytes(false);
        if malformed {
            bytes[217] = 2;
        } else {
            bytes[213..217].copy_from_slice(&2_u32.to_le_bytes());
        }
        let operation = decode_registered_carrier(&bytes, &meta);
        let direction = operation.direction.expect("readable vector");
        assert_eq!(
            direction
                .value
                .unwrap()
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            [2.4, 0.0, 0.0]
        );
        assert_eq!(direction.offset, 169);
        assert!(!operation.inputs_complete);
    }
}

fn with_carrier_input(reference: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut bytes = carrier_bytes(false);
    bytes.splice(217..228, reference.iter().copied());
    let paired = u64::try_from(217 + reference.len()).unwrap();
    let meta = carrier_metadata(
        crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
        1,
        &[(55, paired)],
    );
    (bytes, meta)
}

#[test]
fn carrier_preserves_null_typed_local_and_external_inputs() {
    let (bytes, meta) = with_carrier_input(&[0]);
    let operation = decode_registered_carrier(&bytes, &meta);
    assert!(operation.inputs_complete);
    assert_eq!(
        operation.inputs,
        [Located {
            offset: 217,
            value: DesignSplitFaceDirectionReference::Null
        }]
    );

    let guid = "00000000-0000-0000-0000-000000000099";
    let mut reference = vec![1];
    reference.extend_from_slice(&4_294_967_384_u64.to_le_bytes());
    reference.extend_from_slice(&36_u32.to_le_bytes());
    reference.extend_from_slice(guid.as_bytes());
    reference.extend_from_slice(&[0, 0]);
    let (bytes, meta) = with_carrier_input(&reference);
    let operation = decode_registered_carrier(&bytes, &meta);
    assert!(operation.inputs_complete);
    assert_eq!(
        operation.inputs[0].value,
        DesignSplitFaceDirectionReference::Local {
            target: Located {
                value: 4_294_967_384,
                offset: 218
            },
            inline_type_guid: Some(guid.into()),
        }
    );
    assert_eq!(
        operation
            .direction
            .unwrap()
            .value
            .unwrap()
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [2.4, 0.0, 0.0]
    );

    let mut reference = vec![1];
    reference.extend_from_slice(&88_u64.to_le_bytes());
    reference.extend_from_slice(&[0, 1]);
    reference.extend_from_slice(&7_u32.to_le_bytes());
    let (bytes, meta) = with_carrier_input(&reference);
    let operation = decode_registered_carrier(&bytes, &meta);
    assert!(operation.inputs_complete);
    assert_eq!(
        operation.inputs[0].value,
        DesignSplitFaceDirectionReference::CrossSegment {
            target: Located {
                value: 88,
                offset: 218
            },
            inline_type_guid: None,
            segment: 7,
            same_document_asset: None,
        }
    );
    assert_eq!(operation.inputs[0].value.local_record_index(), None);
}

#[test]
fn carrier_preserves_document_type_link_and_version_qualifiers() {
    let guid = "00000000-0000-0000-0000-000000000099";
    let asset = "00000000-0000-0000-0000-000000000088";
    let key = "00000000-0000-0000-0000-000000000077";
    let mut reference = vec![1];
    reference.extend_from_slice(&88_u64.to_le_bytes());
    reference.extend_from_slice(&36_u32.to_le_bytes());
    reference.extend_from_slice(guid.as_bytes());
    reference.push(1);
    reference.extend_from_slice(&7_u32.to_le_bytes());
    crate::test_support::lp_utf16(&mut reference, asset);
    reference.push(0);
    reference.extend_from_slice(&36_u32.to_le_bytes());
    reference.extend_from_slice(guid.as_bytes());
    crate::test_support::lp_utf16(&mut reference, "pi-π");
    reference.push(1);
    crate::test_support::lp_utf16(&mut reference, key);
    crate::test_support::lp_utf16(&mut reference, "urn:test");
    let (bytes, meta) = with_carrier_input(&reference);
    let operation = decode_registered_carrier(&bytes, &meta);
    assert!(operation.inputs_complete);
    assert_eq!(
        operation.inputs[0].value,
        DesignSplitFaceDirectionReference::CrossDocument {
            target: Located {
                value: 88,
                offset: 218
            },
            inline_type_guid: Some(guid.into()),
            segment: 7,
            asset_guid: asset.into(),
            target_type_guid: guid.into(),
            external_type_guid_present: true,
            link_name: "pi-π".into(),
            version: Some((key.into(), "urn:test".into())),
        }
    );
}

#[test]
fn carrier_preserves_the_asset_prefixed_same_document_form() {
    let mut reference = vec![1];
    reference.extend_from_slice(&88_u64.to_le_bytes());
    reference.push(1);
    reference.extend_from_slice(&7_u32.to_le_bytes());
    crate::test_support::lp_utf16(&mut reference, "asset-π");
    reference.push(1);
    let (bytes, meta) = with_carrier_input(&reference);
    let operation = decode_registered_carrier(&bytes, &meta);
    assert!(operation.inputs_complete);
    assert!(operation.scope_offsets_valid(0, 120));
    assert_eq!(
        operation.inputs[0].value,
        DesignSplitFaceDirectionReference::CrossSegment {
            target: Located {
                value: 88,
                offset: 218
            },
            inline_type_guid: None,
            segment: 7,
            same_document_asset: Some("asset-π".into()),
        }
    );
}

#[test]
fn carrier_admits_both_inline_document_type_spellings_at_their_exact_ends() {
    let guid = "00000000-0000-0000-0000-000000000099";
    for external in [
        None,
        Some(guid),
        Some("00000000-0000-0000-0000-000000000098"),
    ] {
        let mut reference = vec![1];
        reference.extend_from_slice(&88_u64.to_le_bytes());
        reference.extend_from_slice(&36_u32.to_le_bytes());
        reference.extend_from_slice(guid.as_bytes());
        reference.push(1);
        reference.extend_from_slice(&7_u32.to_le_bytes());
        crate::test_support::lp_utf16(&mut reference, "asset");
        reference.push(0);
        if let Some(external) = external {
            reference.extend_from_slice(&36_u32.to_le_bytes());
            reference.extend_from_slice(external.as_bytes());
        }
        crate::test_support::lp_utf16(&mut reference, "link");
        reference.push(0);
        reference.push(0);
        let (mut bytes, meta) = with_carrier_input(&reference);
        bytes[213..217].copy_from_slice(&2_u32.to_le_bytes());
        let operation = decode_registered_carrier(&bytes, &meta);
        assert!(operation.inputs_complete);
        assert_eq!(operation.inputs.len(), 2);
        assert_eq!(
            operation.inputs[1].value,
            DesignSplitFaceDirectionReference::Null
        );
        assert!(operation.scope_offsets_valid(0, 120));
        let DesignSplitFaceDirectionReference::CrossDocument {
            external_type_guid_present,
            inline_type_guid,
            target_type_guid,
            ..
        } = &operation.inputs[0].value
        else {
            panic!("document reference")
        };
        assert_eq!(*external_type_guid_present, external.is_some());
        assert_eq!(inline_type_guid.as_deref(), Some(guid));
        assert_eq!(target_type_guid, external.unwrap_or(guid));
        assert_eq!(
            operation
                .direction
                .as_ref()
                .unwrap()
                .value
                .unwrap()
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            [2.4, 0.0, 0.0]
        );
        let indexed = crate::test_support::with_decode_context(|ctx| {
            super::super::point_direction::point_direction_carrier_at(
                ctx,
                &bytes,
                120,
                super::super::point_direction::PointDirectionBoundary::Indexed(
                    usize::try_from(operation.carrier_frame.unwrap().secondary_offset).unwrap(),
                ),
                55,
                1,
            )
        })
        .unwrap()
        .unwrap();
        let matching = external.is_none_or(|external| external == guid);
        assert_eq!(indexed.inputs_complete, matching);
        assert_eq!(indexed.inputs.len(), if matching { 2 } else { 0 });
        for shift in [1, 11] {
            let mut invalid = operation.clone();
            invalid.carrier_frame.as_mut().unwrap().secondary_offset += shift;
            assert!(!invalid.scope_offsets_valid(0, 120));
        }
    }
}

#[test]
fn carrier_keeps_vector_when_the_input_count_is_absent() {
    let mut bytes = carrier_bytes(false);
    bytes.truncate(213);
    crate::test_support::indexed_header(&mut bytes, *b"257", 55);
    let meta = carrier_metadata(
        crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
        1,
        &[(55, 213)],
    );
    let operation = decode_registered_carrier(&bytes, &meta);
    assert_eq!(
        operation
            .direction
            .unwrap()
            .value
            .unwrap()
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [2.4, 0.0, 0.0]
    );
    assert_eq!(operation.input_count, None);
    assert!(!operation.inputs_complete);
}

#[test]
fn carrier_input_budget_refusal_propagates() {
    let bytes = carrier_bytes(false);
    let metadata = carrier_metadata(
        crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
        1,
        &[(55, 228)],
    );
    let meta = crate::metastream::parse(&metadata, "synthetic/MetaStream.dat").unwrap();
    let frames = unambiguous_design_primary_frames(&bytes, &meta).unwrap();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let result = crate::test_support::with_decode_policy(&policy, |ctx| {
        split_face_operation(ctx, &bytes, &split_face_scope(), &frames)
    });
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

#[test]
fn registered_carrier_rejects_an_unsupported_leading_block() {
    let mut bytes = carrier_bytes(false);
    bytes[139] = 255;
    let meta = carrier_metadata(
        crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
        1,
        &[(55, 228)],
    );
    let operation = decode_registered_carrier(&bytes, &meta);
    assert_eq!(operation.direction, None);
    assert!(matches!(
        operation.direction_reference,
        DesignSplitFaceDirectionReference::Local {
            target: Located { value: 55, .. },
            ..
        }
    ));
}

#[test]
fn reference_ending_at_scope_boundary_survives_serde() {
    let mut bytes = vec![0; 408];
    bytes[20] = 1;
    bytes[21..25].copy_from_slice(&2_u32.to_le_bytes());
    let mut cursor = 25;
    for _ in 0..2 {
        let key = b"abcdefghijklmnopqrstuvwxyza";
        bytes[cursor..cursor + 4].copy_from_slice(&27_u32.to_le_bytes());
        cursor += 4;
        bytes[cursor..cursor + key.len()].copy_from_slice(key);
        cursor += key.len();
        let name = b"IntrinsicMetaTypeuint64";
        bytes[cursor..cursor + 4]
            .copy_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
        cursor += 4;
        bytes[cursor..cursor + name.len()].copy_from_slice(name);
        cursor += name.len() + 8;
    }
    assert_eq!(cursor, 157);
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&55_u64.to_le_bytes());
    let mut scope = split_face_scope();
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = 168;
            draft.layout_fixture_references();
        })
        .unwrap();
    let operation = crate::test_support::with_decode_context(|ctx| {
        split_face_operation(ctx, &bytes, &scope, &[])
    })
    .unwrap()
    .unwrap();
    assert_eq!(operation.method_offset, 168);
    assert_eq!(operation.method(), DesignSplitFaceMethod::Unresolved);
    assert!(matches!(
        operation.direction_reference,
        DesignSplitFaceDirectionReference::Local {
            target: Located {
                value: 55,
                offset: 158
            },
            ..
        }
    ));
    scope
        .try_edit(|draft| {
            let crate::records::feature::scope::DesignScopePayload::SplitFace(payload) =
                &mut draft.payload
            else {
                panic!("SplitFace")
            };
            *payload = Some(operation);
        })
        .unwrap();
    let encoded = serde_json::to_vec(&scope).unwrap();
    let decoded: DesignParameterScope = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.split_face_operation(), scope.split_face_operation());
}

#[test]
fn unavailable_reference_at_scope_boundary_survives_serde() {
    let bytes = [0_u8; 408];
    let mut scope = split_face_scope();
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = 21;
            draft.layout_fixture_references();
        })
        .unwrap();
    let operation = crate::test_support::with_decode_context(|ctx| {
        split_face_operation(ctx, &bytes, &scope, &[])
    })
    .unwrap()
    .unwrap();
    assert_eq!(operation.method_offset, 21);
    assert_eq!(
        operation.direction_reference,
        DesignSplitFaceDirectionReference::Unavailable
    );
    assert_eq!(operation.method(), DesignSplitFaceMethod::Unresolved);
    scope
        .try_edit(|draft| {
            let crate::records::feature::scope::DesignScopePayload::SplitFace(payload) =
                &mut draft.payload
            else {
                panic!("SplitFace")
            };
            *payload = Some(operation);
        })
        .unwrap();
    let encoded = serde_json::to_vec(&scope).unwrap();
    let decoded: DesignParameterScope = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.split_face_operation(), scope.split_face_operation());
}

#[test]
fn vector_survives_truncated_or_nonfinite_unrelated_fields() {
    let mut truncated = carrier_bytes(false);
    truncated.truncate(193);
    crate::test_support::indexed_header(&mut truncated, *b"257", 55);
    let meta = carrier_metadata(
        crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
        1,
        &[(55, 193)],
    );
    let operation = decode_registered_carrier(&truncated, &meta);
    assert_eq!(
        operation
            .direction
            .unwrap()
            .value
            .unwrap()
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [2.4, 0.0, 0.0]
    );
    assert_eq!(operation.input_count, None);
    for at in [145, 193] {
        let mut bytes = carrier_bytes(false);
        bytes[at..at + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        let meta = carrier_metadata(
            crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
            1,
            &[(55, 228)],
        );
        let operation = decode_registered_carrier(&bytes, &meta);
        assert_eq!(
            operation
                .direction
                .unwrap()
                .value
                .unwrap()
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            [2.4, 0.0, 0.0]
        );
        assert_eq!(operation.input_count.unwrap().value, 1);
        assert!(operation.inputs_complete);
    }
}

#[test]
fn registered_nonfinite_tangent_preserves_direction_and_input_run() {
    for (version, tangent_at, count_at, secondary_at) in [(2, 213, 237, 253), (4, 214, 238, 254)] {
        let mut bytes = carrier_bytes(false);
        let mut tangent = Vec::new();
        if version == 4 {
            tangent.push(7);
        }
        tangent.extend_from_slice(&f64::NAN.to_le_bytes());
        tangent.extend_from_slice(&1.0_f64.to_le_bytes());
        tangent.extend_from_slice(&2.0_f64.to_le_bytes());
        bytes.splice(213..213, tangent);
        bytes[count_at..count_at + 4].copy_from_slice(&2_u32.to_le_bytes());
        bytes.insert(count_at + 4, 0);
        let meta = carrier_metadata(
            crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
            version,
            &[(55, secondary_at)],
        );
        let operation = decode_registered_carrier(&bytes, &meta);
        assert_eq!(
            operation
                .direction
                .unwrap()
                .value
                .unwrap()
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            [2.4, 0.0, 0.0]
        );
        assert_eq!(
            operation.input_count,
            Some(Located {
                value: 2,
                offset: cadmpeg_core::decode::u64_from_index(count_at)
            })
        );
        assert_eq!(
            operation.inputs,
            [
                Located {
                    value: DesignSplitFaceDirectionReference::Null,
                    offset: cadmpeg_core::decode::u64_from_index(count_at + 4)
                },
                Located {
                    value: DesignSplitFaceDirectionReference::Local {
                        target: Located {
                            value: 88,
                            offset: cadmpeg_core::decode::u64_from_index(count_at + 6)
                        },
                        inline_type_guid: None
                    },
                    offset: cadmpeg_core::decode::u64_from_index(count_at + 5)
                },
            ]
        );
        assert!(operation.inputs_complete);
        let indexed = crate::test_support::with_decode_context(|ctx| {
            super::super::point_direction::point_direction_carrier_at(
                ctx,
                &bytes,
                120,
                super::super::point_direction::PointDirectionBoundary::Indexed(
                    usize::try_from(secondary_at).unwrap(),
                ),
                55,
                version,
            )
        })
        .unwrap()
        .unwrap();
        assert!(indexed.tangent_point_data.is_none());
        assert!(indexed.input_count.is_none());
        let truncated = crate::test_support::with_decode_context(|ctx| {
            super::super::point_direction::point_direction_carrier_at(
                ctx,
                &bytes,
                120,
                super::super::point_direction::PointDirectionBoundary::Registered(tangent_at + 23),
                55,
                version,
            )
        })
        .unwrap()
        .unwrap();
        assert!(truncated.input_count.is_none());
        assert!(!truncated.inputs_complete);
    }
}

#[test]
fn unrelated_duplicate_registration_preserves_the_registered_carrier() {
    let bytes = carrier_bytes(false);
    let meta = metadata(
        &[
            (
                crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
                1,
                &[55],
            ),
            ("00000000-0000-0000-0000-000000000002", 1, &[1, 999, 999]),
        ],
        &[(1, 0), (55, 120)],
        &[(55, 228)],
    );
    let operation = decode_registered_carrier(&bytes, &meta);
    assert_eq!(
        operation
            .direction
            .unwrap()
            .value
            .unwrap()
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [2.4, 0.0, 0.0]
    );
    assert_eq!(operation.input_count.unwrap().value, 1);
    assert_eq!(operation.inputs[0].value.local_record_index(), Some(88));
    assert!(operation.inputs_complete);
}

#[test]
fn nonfinite_direction_retains_registered_input_slots_and_location() {
    let meta = metadata(
        &[
            (
                crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID,
                1,
                &[55],
            ),
            ("00000000-0000-0000-0000-000000000002", 1, &[1]),
        ],
        &[(1, 0), (55, 500)],
        &[(55, 608)],
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut bytes = carrier_bytes(false);
        let carrier = bytes.split_off(120);
        bytes.resize(500, 0);
        bytes.extend_from_slice(&carrier);
        bytes[549..557].copy_from_slice(&value.to_le_bytes());
        let operation = decode_registered_carrier(&bytes, &meta);
        let direction = operation.direction.as_ref().unwrap();
        assert_eq!(direction.offset, 549);
        assert_eq!(direction.value, None);
        assert_eq!(operation.input_count.unwrap().value, 1);
        assert_eq!(operation.inputs[0].value.local_record_index(), Some(88));
        assert!(operation.inputs_complete);
        let mut scope = split_face_scope();
        scope
            .try_edit(|draft| {
                let crate::records::feature::scope::DesignScopePayload::SplitFace(slot) =
                    &mut draft.payload
                else {
                    panic!("SplitFace")
                };
                *slot = Some(operation.clone());
            })
            .unwrap();
        let wire = serde_json::to_vec(&scope).unwrap();
        let decoded: DesignParameterScope = serde_json::from_slice(&wire).unwrap();
        assert_eq!(decoded.split_face_operation(), Some(&operation));
        crate::test_support::split_face::assert_native_scope(&decoded);
    }
}

#[test]
fn scope_document_type_disagreement_retains_the_method_controls() {
    let mut bytes = method_bytes(0, 1, 0, None, false);
    bytes.resize(488, 0);
    let mut reference = vec![1];
    reference.extend_from_slice(&88_u64.to_le_bytes());
    reference.extend_from_slice(&36_u32.to_le_bytes());
    reference.extend_from_slice(b"00000000-0000-0000-0000-000000000099");
    reference.push(1);
    reference.extend_from_slice(&7_u32.to_le_bytes());
    crate::test_support::lp_utf16(&mut reference, "");
    reference.push(0);
    reference.extend_from_slice(&36_u32.to_le_bytes());
    reference.extend_from_slice(b"00000000-0000-0000-0000-000000000098");
    crate::test_support::lp_utf16(&mut reference, "");
    reference.push(0);
    assert_eq!(reference.len(), 104);
    bytes[21..125].copy_from_slice(&reference);
    bytes[125..131].copy_from_slice(&[0, 0, 0, 0, 1, 0]);
    let mut scope = split_face_scope();
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = 200;
            draft.kind_offset = 223;
            draft.feature_ordinal_offset = 410;
            draft.paired_byte_offset = 488;
            draft.frame_length = 488;
            draft.reference_members = ReferenceRun::located(vec![Located {
                value: 9,
                offset: 205,
            }]);
        })
        .unwrap();
    let operation = crate::test_support::with_decode_context(|ctx| {
        split_face_operation(ctx, &bytes, &scope, &[])
    })
    .unwrap()
    .unwrap();
    assert_eq!(operation.method(), DesignSplitFaceMethod::AlongVector);
    assert_eq!(operation.method_offset, 125);
    assert!(operation.scope_offsets_valid(0, 200));
    let DesignSplitFaceDirectionReference::CrossDocument {
        inline_type_guid,
        target_type_guid,
        external_type_guid_present,
        ..
    } = operation.direction_reference
    else {
        panic!("document")
    };
    assert_eq!(
        inline_type_guid.as_deref(),
        Some("00000000-0000-0000-0000-000000000099")
    );
    assert_eq!(target_type_guid, "00000000-0000-0000-0000-000000000098");
    assert!(external_type_guid_present);
}

#[test]
fn unreadable_temporary_input_slots_do_not_consume_retained_output_budget() {
    let mut bytes = carrier_bytes(false);
    bytes.truncate(217);
    bytes[213..217].copy_from_slice(&128_u32.to_le_bytes());
    bytes.extend_from_slice(&[255; 128]);
    crate::test_support::indexed_header(&mut bytes, *b"257", 55);
    crate::test_support::with_decode_context(|ctx| {
        let before = ctx.retained_bytes_available();
        let carrier = super::super::point_direction::point_direction_carrier_at(
            ctx,
            &bytes,
            120,
            super::super::point_direction::PointDirectionBoundary::Registered(345),
            55,
            1,
        )
        .unwrap()
        .unwrap();
        assert_eq!(carrier.input_count.unwrap().value, 128);
        assert!(carrier.inputs.is_empty());
        assert!(!carrier.inputs_complete);
        assert_eq!(ctx.retained_bytes_available(), before);
    });
}
