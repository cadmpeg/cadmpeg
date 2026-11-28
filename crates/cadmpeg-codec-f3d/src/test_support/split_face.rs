use crate::test_support::{lp_ascii, lp_utf16};

pub(crate) fn assert_native_scope(scope: &crate::records::feature::scope::DesignParameterScope) {
    use crate::records::decal::DesignRecordHeader;
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let mut document = cadmpeg_ir::examples::unit_cube().unwrap();
    {
        let mut native = crate::test_support::native_fixture::f3d_native_mut(&mut document);
        native.design_parameter_scopes.push(scope.clone());
        native.design_record_headers.push(DesignRecordHeader {
            id: format!("{stream}:design-record-header#{}", scope.byte_offset()),
            record_index: scope.record_index,
            class_tag: scope.class_tag.clone(),
            byte_offset: scope.byte_offset(),
        });
        let carrier_index = scope
            .split_face_operation()
            .filter(|operation| operation.carrier_frame.is_some())
            .and_then(|operation| operation.direction_reference.local_record_index());
        for (ordinal, record_index) in scope.reference_members().values().enumerate() {
            if Some(*record_index) == carrier_index {
                continue;
            }
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#{}", 300 + ordinal * 11),
                record_index: *record_index,
                class_tag: "333".to_owned().try_into().unwrap(),
                byte_offset: 300 + u64::try_from(ordinal).unwrap() * 11,
            });
        }
        if let Some(operation) = scope.split_face_operation() {
            if let Some(frame) = operation.carrier_frame {
                let start = frame.primary_offset;
                let end = frame.end_offset;
                let record_index = operation.direction_reference.local_record_index().unwrap();
                assert!(end < scope.byte_offset() || start > scope.paired_byte_offset());
                let meta_stream = stream.strip_suffix("BulkStream.dat").unwrap();
                native.design_types.push(
                    SegmentType::try_new(
                        format!("{meta_stream}MetaStream.dat:design-type#0"),
                        SegmentTypeData {
                            byte_offset: 0,
                            type_guid: "F2A7590D-6654-4674-B393-A2AEF4FEC48A"
                                .to_owned()
                                .try_into()
                                .unwrap(),
                            type_guid_offset: 4,
                            base_type_guid: BaseTypeGuid::Absent,
                            version: frame.version,
                            version_offset: 44,
                            module: "Fusion".into(),
                            entities: ReferenceRun::unlocated(vec![u64::from(record_index)]),
                        },
                    )
                    .unwrap(),
                );
                native.design_record_headers.push(DesignRecordHeader {
                    id: format!("{stream}:design-record-header#{start}"),
                    record_index,
                    class_tag: "333".to_owned().try_into().unwrap(),
                    byte_offset: start,
                });
                native.design_record_headers.push(DesignRecordHeader {
                    id: format!("{stream}:design-record-header#900"),
                    record_index: 999,
                    class_tag: "334".to_owned().try_into().unwrap(),
                    byte_offset: 900,
                });
            }
        }
    }
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &document)
    })
    .unwrap();
    assert!(
        findings
            .iter()
            .all(|finding| finding.entity.as_deref() != Some(scope.id.as_str())
                || finding.message == "Fusion SplitFace carrier registration conflicts with its inline type qualifier"),
        "{findings:?}"
    );
}

pub(crate) fn metadata(
    types: &[(&str, u32, &[u64])],
    primary: &[(u64, u64)],
    secondary: &[(u64, u64)],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "Design");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    for value in [1234u32, 0, 0, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    lp_ascii(&mut bytes, "FusionDesignSegmentType");
    lp_ascii(&mut bytes, "Fusion");
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(types.len()).unwrap().to_le_bytes());
    for (guid, version, entities) in types {
        lp_ascii(&mut bytes, guid);
        lp_ascii(&mut bytes, "");
        bytes.extend_from_slice(&version.to_le_bytes());
        lp_ascii(&mut bytes, "Fusion");
        bytes.extend_from_slice(&u32::try_from(entities.len()).unwrap().to_le_bytes());
        for entity in *entities {
            bytes.extend_from_slice(&entity.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for index in [primary, secondary] {
        bytes.extend_from_slice(&u32::try_from(index.len()).unwrap().to_le_bytes());
        for (entity, offset) in index {
            bytes.extend_from_slice(&entity.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
    }
    bytes
}
