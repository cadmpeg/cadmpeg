use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::identity::{Located, ReferenceRun};
use crate::test_support::native_fixture::f3d_native_mut;
use cadmpeg_core::serde_erase::json;

fn document() -> cadmpeg_ir::CadIr {
    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::SplitFace,
        1,
    );
    scope.try_edit(|draft| {
        draft.byte_offset = 1000;
        draft.frame_length = 408;
        draft.reference_count_offset = 1120;
        draft.reference_members = ReferenceRun::located(vec![Located { value: 9, offset: 1125 }]);
        draft.kind_offset = 1143;
        draft.feature_ordinal_offset = 1330;
        draft.paired_byte_offset = 1408;
        let DesignScopePayload::SplitFace(value) = &mut draft.payload else { panic!("SplitFace payload") };
        *value = Some(json::from_slice(br#"{"method_offset":1032,"control":{"family":0,"projection":1,"extension":0},"direction_reference":{"kind":"local","target":{"value":55,"offset":1022},"inline_type_guid":null},"carrier_frame":{"primary_offset":120,"secondary_offset":228,"end_offset":239,"version":1},"direction":{"value":[2.4,0.0,0.0],"offset":169},"input_count":{"value":1,"offset":213},"inputs":[{"value":{"kind":"local","target":{"value":88,"offset":218},"inline_type_guid":null},"offset":217}],"inputs_complete":true}"#).unwrap());
    }).unwrap();
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_types.push(
            SegmentType::try_new(
                "f3d:Design/MetaStream.dat:design-type#0".into(),
                SegmentTypeData {
                    byte_offset: 0,
                    type_guid: "F2A7590D-6654-4674-B393-A2AEF4FEC48A"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    type_guid_offset: 4,
                    base_type_guid: BaseTypeGuid::Absent,
                    version: 1,
                    version_offset: 44,
                    module: "Fusion".into(),
                    entities: ReferenceRun::unlocated(vec![55]),
                },
            )
            .unwrap(),
        );
        for (record_index, byte_offset) in [(1, 1000), (9, 800), (55, 120), (56, 239)] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#{record_index}"),
                record_index,
                class_tag: "333".to_owned().try_into().unwrap(),
                byte_offset,
            });
        }
        native.design_parameter_scopes.push(scope);
    }
    ir
}

fn invalid_split_face_frame(ir: &cadmpeg_ir::CadIr) -> bool {
    crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, ir).unwrap()
    })
    .iter()
    .any(|finding| {
        finding.entity.as_deref() == Some("f3d:Design/BulkStream.dat:design-parameter-scope#1")
            && finding.message == "Fusion Design parameter scope has an invalid paired frame"
    })
}

#[test]
fn split_face_carrier_snapshot_requires_its_registered_source_envelope() {
    let ir = document();
    assert!(!invalid_split_face_frame(&ir));
    let mut missing = ir.clone();
    f3d_native_mut(&mut missing)
        .design_record_headers
        .retain(|header| header.record_index != 55);
    assert!(invalid_split_face_frame(&missing));
    let mut wrong_location = ir.clone();
    f3d_native_mut(&mut wrong_location)
        .design_record_headers
        .iter_mut()
        .find(|header| header.record_index == 55)
        .unwrap()
        .byte_offset = 121;
    assert!(invalid_split_face_frame(&wrong_location));
    let mut clipped = ir.clone();
    f3d_native_mut(&mut clipped)
        .design_record_headers
        .iter_mut()
        .find(|header| header.record_index == 56)
        .unwrap()
        .byte_offset = 227;
    assert!(invalid_split_face_frame(&clipped));
    let mut wrong_type = ir.clone();
    f3d_native_mut(&mut wrong_type).design_types.clear();
    assert!(invalid_split_face_frame(&wrong_type));
    let mut ambiguous = ir.clone();
    {
        let mut native = f3d_native_mut(&mut ambiguous);
        let duplicate = native.design_types[0].clone();
        native.design_types.push(duplicate);
    }
    assert!(invalid_split_face_frame(&ambiguous));
}

#[test]
fn split_face_missing_carrier_remains_admitted_without_a_claimed_snapshot() {
    let mut ir = document();
    {
        let mut native = f3d_native_mut(&mut ir);
        native
            .design_record_headers
            .retain(|header| header.record_index != 55);
        let crate::records::feature::scope::DesignScopePayloadMut::SplitFace(value) =
            native.design_parameter_scopes[0].payload_mut()
        else {
            panic!("SplitFace payload")
        };
        let operation = value.as_mut().unwrap();
        operation.carrier_frame = None;
        operation.direction = None;
        operation.input_count = None;
        operation.inputs.clear();
        operation.inputs_complete = false;
    }
    assert!(!invalid_split_face_frame(&ir));
}

#[test]
fn split_face_type_conflict_is_reported_with_valid_carrier() {
    for (guid, warning) in [
        ("F2A7590D-6654-4674-B393-A2AEF4FEC48A", false),
        ("00000000-0000-0000-0000-000000000099", true),
    ] {
        let mut ir = document();
        {
            let mut native = f3d_native_mut(&mut ir);
            native.design_parameter_scopes[0].try_edit(|draft| {
                let DesignScopePayload::SplitFace(Some(operation)) = &mut draft.payload else { panic!("SplitFace") };
                operation.method_offset = 1072;
                let crate::records::feature::split_face::DesignSplitFaceDirectionReference::Local { inline_type_guid, .. } = &mut operation.direction_reference else { panic!("local") };
                *inline_type_guid = Some(guid.to_owned());
            }).unwrap();
        }
        let findings = crate::test_support::with_decode_context(|ctx| {
            crate::validate::validate_native(ctx, &ir).unwrap()
        });
        assert!(!invalid_split_face_frame(&ir));
        assert_eq!(findings.iter().any(|finding| finding.message == "Fusion SplitFace carrier registration conflicts with its inline type qualifier"), warning);
    }
}
