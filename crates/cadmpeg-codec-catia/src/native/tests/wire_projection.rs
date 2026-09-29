// SPDX-License-Identifier: Apache-2.0
//! Resource admission for native wire views built during binary decode.

use super::super::{
    CatiaObjectGraph, CatiaObjectRecord, CatiaObjectRecordWire, CatiaValueBlock,
    CatiaValueBlockWire,
};
use crate::object_graph::{ObjectPayload, PayloadField};

#[test]
fn entity_reference_borrowed_wire_preserves_json_bytes() {
    for reference in [
        crate::native::CatiaEntityReference::Null { entity_id: 0 },
        crate::native::CatiaEntityReference::Unresolved { entity_id: 7 },
        crate::native::CatiaEntityReference::Resolved {
            entity_id: 8,
            entity: "catia:test:entity#8".to_owned(),
            class_name: Some("Example".to_owned()),
        },
    ] {
        let owned: crate::native::CatiaEntityReferenceWire = reference.clone().into();
        assert_eq!(
            serde_json::to_vec(&reference).expect("borrowed reference JSON"),
            serde_json::to_vec(&owned).expect("owned reference JSON")
        );
    }
}

#[test]
fn entity_reference_retained_limit_refuses_json_record() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        #[serde(flatten)]
        reference: &'a crate::native::CatiaEntityReference,
    }
    let reference = crate::native::CatiaEntityReference::Resolved {
        entity_id: 8,
        entity: "catia:test:entity#8".to_owned(),
        class_name: Some("Example".to_owned()),
    };

    let record = Record {
        id: "catia:test:entity-reference#0",
        reference: &reference,
    };
    let arena_name = "entity_references";
    let json_len = serde_json::to_vec(&record).expect("reference JSON").len();
    let limit = u64::try_from(json_len + arena_name.len() - 1).expect("small JSON");
    let refused = crate::test_support::with_retained_limit(limit, |ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace.set_arena(ctx, arena_name, std::slice::from_ref(&record))
    });
    let error = refused.expect_err("record exceeds retained-byte limit");
    assert!(error.to_string().contains("RetainedBytes"), "{error}");
    crate::test_support::with_service_context(|ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace
            .set_arena(ctx, arena_name, std::slice::from_ref(&record))
            .expect("service profile admits reference");
    });
}

fn signature_cohort() -> crate::native::CatiaReferenceSignatureCohort {
    crate::native::CatiaReferenceSignatureCohort {
        references: crate::entity_table::ConsecutiveReferences::new(3).expect("two references"),
        id: "catia:test:cohort#0".to_owned(),
        parent: "catia:test:graph#0".to_owned(),
        ordinal: 0,
        first_entity: crate::native::CatiaEntityReference::Resolved {
            entity_id: 3,
            entity: "catia:test:entity#3".to_owned(),
            class_name: None,
        },
        second_entity: crate::native::CatiaEntityReference::Unresolved { entity_id: 4 },
        schema_selection: None,
        members: vec!["catia:test:member#0".to_owned()],
    }
}

#[test]
fn signature_cohort_borrowed_wire_preserves_json_bytes() {
    let cohort = signature_cohort();
    let owned: crate::native::CatiaReferenceSignatureCohortWire = cohort.clone().into();
    assert_eq!(
        serde_json::to_vec(&cohort).expect("borrowed cohort JSON"),
        serde_json::to_vec(&owned).expect("owned cohort JSON")
    );
}

#[test]
fn signature_cohort_retained_limit_refuses_json_record() {
    let cohort = signature_cohort();
    let arena_name = "reference_signature_cohorts";
    let json_len = serde_json::to_vec(&cohort).expect("cohort JSON").len();
    let limit = u64::try_from(json_len + arena_name.len() - 1).expect("small JSON");
    let refused = crate::test_support::with_retained_limit(limit, |ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace.set_arena(ctx, arena_name, std::slice::from_ref(&cohort))
    });
    let error = refused.expect_err("record exceeds retained-byte limit");
    assert!(error.to_string().contains("RetainedBytes"), "{error}");
    crate::test_support::with_service_context(|ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace
            .set_arena(ctx, arena_name, std::slice::from_ref(&cohort))
            .expect("service profile admits cohort");
    });
}

fn object_record_reference() -> crate::native::CatiaObjectRecordReference {
    crate::native::CatiaObjectRecordReference::from_parts(
        9,
        12,
        crate::native::CatiaObjectRecordReferenceSource::Field,
        false,
        Some("catia:test:record#9".to_owned()),
        Some("catia:test:design#0".to_owned()),
    )
}

#[test]
fn object_record_reference_borrowed_wire_preserves_json_bytes() {
    let reference = object_record_reference();
    let owned: crate::native::CatiaObjectRecordReferenceWire = reference.clone().into();
    assert_eq!(
        serde_json::to_vec(&reference).expect("borrowed record reference JSON"),
        serde_json::to_vec(&owned).expect("owned record reference JSON")
    );
}

#[test]
fn object_record_reference_retained_limit_refuses_json_record() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        #[serde(flatten)]
        reference: &'a crate::native::CatiaObjectRecordReference,
    }
    let reference = object_record_reference();

    let record = Record {
        id: "catia:test:record-reference#0",
        reference: &reference,
    };
    let arena_name = "object_record_references";
    let json_len = serde_json::to_vec(&record)
        .expect("record reference JSON")
        .len();
    let limit = u64::try_from(json_len + arena_name.len() - 1).expect("small JSON");
    let refused = crate::test_support::with_retained_limit(limit, |ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace.set_arena(ctx, arena_name, std::slice::from_ref(&record))
    });
    let error = refused.expect_err("record exceeds retained-byte limit");
    assert!(error.to_string().contains("RetainedBytes"), "{error}");
    crate::test_support::with_service_context(|ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace
            .set_arena(ctx, arena_name, std::slice::from_ref(&record))
            .expect("service profile admits record reference");
    });
}

#[test]
fn native_value_block_wire_refuses_before_field_view_growth() {
    let block = CatiaValueBlock {
        id: "block".to_owned(),
        byte_offset: 0,
        object_graph: None,
        catalog: "catalog".to_owned(),
        payload: vec![0x37],
        schema_selections: Vec::new(),
    };
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        CatiaValueBlockWire::from_charged(ctx, block.clone())
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_value_fields")
    );
    let charged = crate::test_support::with_service_context(|ctx| {
        CatiaValueBlockWire::from_charged(ctx, block.clone())
    })
    .expect("service budget admits value fields");
    let original: CatiaValueBlockWire = block.into();
    assert_eq!(
        serde_json::to_value(charged).expect("charged wire"),
        serde_json::to_value(original).expect("legacy wire")
    );
}

#[test]
fn native_object_wire_refuses_before_repeated_suffix_growth() {
    let atom = |value, offset| PayloadField::Atom { value, offset };
    let reference = |value, offset| PayloadField::Reference { value, offset };
    let record = CatiaObjectRecord {
        id: "catia:test:object#0".to_owned(),
        parent: "catia:test:graph#0".to_owned(),
        design_object: None,
        entity: None,
        ordinal: 0,
        byte_offset: 0,
        byte_len: 83,
        lead: 0,
        head: Vec::new(),
        inline_body: None,
        owner: None,
        class: None,
        storage: None,
        payload: ObjectPayload {
            size: 83,
            fields: vec![
                atom(44, 0),
                PayloadField::Blob {
                    bytes: vec![0; 59],
                    offset: 1,
                },
                atom(5, 65),
                atom(46, 66),
                atom(19, 67),
                atom(48, 68),
                atom(3, 69),
                reference(60, 70),
                reference(62, 72),
                reference(49, 74),
                atom(3, 76),
                reference(60, 77),
                reference(62, 79),
                atom(129, 81),
                PayloadField::Terminator,
            ],
        },
        repeated_reference_schema_selection: None,
        references: Vec::new(),
    };
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        CatiaObjectRecordWire::from_charged(ctx, record.clone())
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_repeated_reference_suffix")
    );
    let charged = crate::test_support::with_service_context(|ctx| {
        CatiaObjectRecordWire::from_charged(ctx, record.clone())
    })
    .expect("service budget admits repeated suffix");
    let original: CatiaObjectRecordWire = record.clone().into();
    assert_eq!(
        serde_json::to_value(charged).expect("charged wire"),
        serde_json::to_value(original).expect("legacy wire")
    );

    let native = super::super::CatiaNative {
        object_graphs: vec![CatiaObjectGraph {
            id: "catia:test:graph#0".to_owned(),
            byte_offset: 0,
            byte_len: 83,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![record],
        }],
        ..Default::default()
    };
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        native
            .clone()
            .store_owned(ctx, &mut cadmpeg_ir::NativeNamespace::default())
    });
    assert!(matches!(
        refused,
        Err(cadmpeg_ir::NativeConvertError::Resource(
            cadmpeg_core::CodecError::ResourceLimit(limit)
        )) if limit.operation == "catia_native_repeated_reference_suffix"
    ));
}
