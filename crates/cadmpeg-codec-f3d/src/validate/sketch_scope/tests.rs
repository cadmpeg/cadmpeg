// SPDX-License-Identifier: Apache-2.0

use crate::native::F3dNative;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData},
    feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut, DesignSketchEntityBinding,
    },
    identity::{DesignEntityId, ReferenceRun},
};

fn fixture() -> (F3dNative, DesignParameterScope) {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"256", 10);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(0);
    bytes.extend_from_slice(&[0; 24]);
    bytes.extend_from_slice(&[0; 12]);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 24]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&5u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let placement = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::placement_construction::decode_frame(
            ctx,
            "f3d:Design/BulkStream.dat:design-placement-construction#0".into(),
            cadmpeg_core::decode::View::over_retained(&bytes),
            16,
        )
        .unwrap()
        .unwrap()
    });
    assert_eq!(
        placement.base_level.offset,
        u64::try_from(bytes.len()).unwrap()
    );
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::Sketch,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![10]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::Sketch(binding) = scope.payload_mut() {
        *binding = Some(DesignSketchEntityBinding {
            entity_id: DesignEntityId::from_parts("sketch", 20),
            entity_reference_offset: 21,
        });
    }
    let mut native = F3dNative::default();
    native.design_record_headers.push(DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:header#10".into(),
        record_index: 10,
        class_tag: placement.class_tag.clone(),
        byte_offset: placement.byte_offset,
    });
    native.design_types.push(
        SegmentType::try_new(
            "f3d:Design/MetaStream.dat:design-type#0".into(),
            SegmentTypeData {
                byte_offset: 0,
                type_guid: "7B6C3C2D-4096-4E6D-B225-699E8C2D9355"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                type_guid_offset: 0,
                base_type_guid: BaseTypeGuid::Guid {
                    value: "2CA5A1CD-C99B-4C9B-91AF-57989148E841"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 0,
                },
                version: 16,
                version_offset: 0,
                module: "Fusion".into(),
                entities: ReferenceRun::unlocated(vec![10]),
            },
        )
        .unwrap(),
    );
    native.design_placement_constructions.push(placement);
    (native, scope)
}

fn linked(native: &F3dNative, scope: &DesignParameterScope) -> bool {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    crate::test_support::with_decode_context(|ctx| {
        super::placement_link(&crate::validate::Ctx::new(&ir, native, ctx), scope)
    })
}

#[test]
fn sketch_scope_accepts_its_registered_versioned_placement() {
    let (native, scope) = fixture();
    assert!(linked(&native, &scope));
    assert!(native.design_sketch_placements.is_empty());
}

#[test]
fn sketch_scope_requires_unique_referenced_placement_and_matching_header() {
    let (native, scope) = fixture();
    for mutation in 0..6 {
        let mut changed = native.clone();
        match mutation {
            0 => changed.design_placement_constructions.clear(),
            1 => {
                changed.design_placement_constructions[0].id =
                    "f3d:Other/BulkStream.dat:placement#0".into();
            }
            2 => changed
                .design_placement_constructions
                .push(changed.design_placement_constructions[0].clone()),
            3 => changed.design_placement_constructions[0].record_index = 11,
            4 => changed.design_record_headers[0].byte_offset += 1,
            5 => changed.design_record_headers[0].class_tag = "257".to_owned().try_into().unwrap(),
            _ => unreachable!(),
        }
        assert!(!linked(&changed, &scope), "mutation {mutation}");
    }
}

#[test]
fn sketch_scope_requires_the_placement_type_version_and_stream() {
    let (native, scope) = fixture();
    for mutation in 0..7 {
        let mut changed = native.clone();
        let kind = &mut changed.design_types[0];
        match mutation {
            0 => changed.design_types.clear(),
            1 => kind.set_version(15),
            2 => kind.set_type_guid(
                "11111111-2222-3333-4444-555555555555"
                    .to_owned()
                    .try_into()
                    .unwrap(),
            ),
            3 => kind.set_module("MSketch".into()),
            4 => kind.set_base_type_guid(BaseTypeGuid::Absent),
            5 => kind.set_entities(ReferenceRun::unlocated(vec![11])),
            6 => {
                *kind = SegmentType::try_new(
                    "f3d:Other/MetaStream.dat:design-type#0".into(),
                    (**kind).clone(),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(!linked(&changed, &scope), "mutation {mutation}");
    }
}
