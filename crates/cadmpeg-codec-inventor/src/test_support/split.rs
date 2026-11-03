// SPDX-License-Identifier: Apache-2.0
//! Bounded Split property fixtures.

use crate::{
    feature::{
        split::{
            self,
            tests::solid::{plane_split_at, properties_at},
        },
        tests::{reference, reference_list, test_property},
        PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
        PmDcFeatureReferenceFamily,
    },
    pmdc::{PmDcListMetadata, PmDcReferenceList, PmDcU32List},
    record_identity::{Located, RecordTypeId},
};

pub(crate) fn face_fixture(side: bool) -> (PmDcFeature, Vec<PmDcFeatureProperty>) {
    let mut source = plane_split_at(30, 60, 42);
    set_version(&mut source, 19);
    let mut payload = (*source).clone();
    let mut slots = payload.properties.references().to_vec();
    slots[2] = reference(73);
    slots[9] = reference(0);
    payload.properties =
        PmDcReferenceList::new(2, Some(PmDcListMetadata::U32([10, 0])), slots).expect("ten slots");
    let source = Located::new(
        payload,
        source.identity.type_id.clone(),
        source.identity.segment_token.clone(),
        source.identity.record_ordinal,
    );
    let mut properties = properties_at(60, 2);
    set_kind(
        &mut properties,
        0,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Auxiliary,
            type_value: 3,
            value: 2,
        },
    );
    set_kind(
        &mut properties,
        5,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BodySelection,
            items: reference_list(&[69]),
        },
    );
    let mut bytes = crate::test_support::test_fixtures::content_at(9, 19, &[]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(u8::from(side));
    let parser = split::property_parser(split::SPLIT_BOOLEAN_TYPE).expect("Boolean parser");
    let payload =
        crate::test_support::test_fixtures::parse(&bytes, |ctx, view| parser(ctx, view, 19))
            .expect("decoded side control");
    properties[4] = Located::new(
        payload,
        RecordTypeId::from_bytes(split::SPLIT_BOOLEAN_TYPE),
        properties[4].identity.segment_token.clone(),
        properties[4].identity.record_ordinal,
    );
    properties.push(test_property(
        72,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FaceCollection,
            items: reference_list(&[74]),
        },
    ));
    properties.push(test_property(73, face_item(&[7])));
    for property in &mut properties {
        let mut payload = (**property).clone();
        payload.save_version_major = 19;
        *property = Located::new(
            payload,
            property.identity.type_id.clone(),
            property.identity.segment_token.clone(),
            property.identity.record_ordinal,
        );
    }
    for (index, type_id) in [
        (0, split::SPLIT_ENUM_TYPE),
        (1, crate::feature::BOOLEAN_TYPE),
        (2, split::TOOL_ENUM_TYPE),
        (3, crate::feature::BOUNDARY_PATCH_TYPE),
        (4, split::SPLIT_BOOLEAN_TYPE),
        (5, crate::feature::motion::BODY_SELECTION_TYPE),
        (6, crate::feature::OBJECT_COLLECTION_TYPE),
        (7, crate::feature::SURFACE_BODY_TYPE),
        (8, crate::feature::SURFACE_BODY_TYPE),
        (9, crate::feature::SURFACE_BODY_TYPE),
        (10, crate::feature::SURFACE_BODY_TYPE),
        (11, crate::feature::FACE_COLLECTION_TYPE),
        (12, crate::feature::FACE_ITEM_TYPE),
    ] {
        properties[index].identity.type_id = RecordTypeId::from_bytes(type_id);
    }
    (source, properties)
}

pub(crate) fn boolean(value: bool) -> PmDcFeaturePropertyKind {
    PmDcFeaturePropertyKind::Boolean {
        name: String::new(),
        name_value: 0,
        value,
    }
}

pub(crate) fn face_item(values: &[u32]) -> PmDcFeaturePropertyKind {
    PmDcFeaturePropertyKind::FaceItem {
        index_references: PmDcU32List::new(
            2,
            (!values.is_empty()).then_some(PmDcListMetadata::U32([1, 0])),
            values.to_vec(),
        )
        .expect("index list"),
    }
}

pub(crate) fn set_kind(
    properties: &mut [PmDcFeatureProperty],
    index: usize,
    kind: PmDcFeaturePropertyKind,
) {
    let source = &properties[index];
    let mut payload = (**source).clone();
    payload.kind = kind;
    properties[index] = Located::new(
        payload,
        source.identity.type_id.clone(),
        source.identity.segment_token.clone(),
        source.identity.record_ordinal,
    );
}

pub(crate) fn set_slot(source: &mut PmDcFeature, slot: usize, value: crate::pmdc::PmDcReference) {
    let mut payload = (**source).clone();
    let mut slots = payload.properties.references().to_vec();
    slots[slot] = value;
    payload.properties =
        PmDcReferenceList::new(2, Some(PmDcListMetadata::U32([10, 0])), slots).expect("ten slots");
    *source = Located::new(
        payload,
        source.identity.type_id.clone(),
        source.identity.segment_token.clone(),
        source.identity.record_ordinal,
    );
}

pub(crate) fn set_marker(properties: &mut [PmDcFeatureProperty], position: usize) {
    let kind = match &properties[position].kind {
        PmDcFeaturePropertyKind::References { family, items } => {
            PmDcFeaturePropertyKind::References {
                family: *family,
                items: PmDcReferenceList::new(
                    3,
                    items.metadata().cloned(),
                    items.references().to_vec(),
                )
                .expect("alternate marker"),
            }
        }
        PmDcFeaturePropertyKind::FaceItem { index_references } => {
            PmDcFeaturePropertyKind::FaceItem {
                index_references: PmDcU32List::new(
                    3,
                    Some(PmDcListMetadata::U32([1, 0])),
                    index_references.values().to_vec(),
                )
                .expect("alternate marker"),
            }
        }
        _ => panic!("list property"),
    };
    set_kind(properties, position, kind);
}

pub(crate) fn set_version(source: &mut PmDcFeature, version: u8) {
    let mut value = serde_json::to_value(&*source).expect("serialized feature");
    value["save_version_major"] = serde_json::Value::from(version);
    *source = serde_json::from_value(value).expect("feature version");
}

pub(crate) fn trim_fixture() -> (PmDcFeature, Vec<PmDcFeatureProperty>) {
    let (mut source, mut properties) = face_fixture(false);
    set_kind(
        &mut properties,
        0,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Auxiliary,
            type_value: 3,
            value: 1,
        },
    );
    set_kind(
        &mut properties,
        2,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Auxiliary,
            type_value: 4,
            value: 3,
        },
    );
    for (position, index) in [(7, 1), (10, 2)] {
        set_kind(
            &mut properties,
            position,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: crate::pmdc::PmDcReference::new(index, false).expect("metadata reference"),
            },
        );
    }
    set_slot(&mut source, 2, reference(0));
    set_slot(&mut source, 5, reference(0));
    set_slot(&mut source, 8, reference(72));
    (source, properties)
}
