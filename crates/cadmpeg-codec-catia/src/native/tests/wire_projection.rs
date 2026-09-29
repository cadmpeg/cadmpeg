// SPDX-License-Identifier: Apache-2.0
//! Resource admission for native wire views built during binary decode.

use super::super::{
    CatiaObjectGraph, CatiaObjectRecord, CatiaObjectRecordWire, CatiaValueBlock,
    CatiaValueBlockWire,
};
use crate::object_graph::{ObjectPayload, PayloadField};

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
