// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::test_object_graph::{
    catalog_stream, entity_table_record_with_value, inline_object_graph_record,
    object_graph_from_records, object_graph_record,
};

fn fixture(self_ref: u8, value: &[u8], repeated_outer: u32) -> Vec<u8> {
    let mut bytes = entity_table_record_with_value(1, &[0xfe]);
    let mut body = vec![3, 0x32];
    body.extend(5_u32.to_le_bytes());
    body.push(0x32);
    body.extend(6_u32.to_le_bytes());
    body.push(0xea);
    body.extend(2_u32.to_le_bytes());
    body.extend([0x82, 0x81, 0, 0x82, 0x32]);
    body.extend(repeated_outer.to_le_bytes());
    body.push(0x32);
    body.extend(5_u32.to_le_bytes());
    bytes.extend([0x7c, 5]);
    bytes.extend(
        u32::try_from(body.len() + 6)
            .expect("synthetic entity length")
            .to_le_bytes(),
    );
    bytes.extend(body);
    bytes.extend(entity_table_record_with_value(3, value));
    bytes.push(0xde);
    let mut inline = vec![0x10, 0xfe, self_ref, 0x82, 0x32];
    inline.extend(7_u32.to_le_bytes());
    inline.push(0x32);
    inline.extend(5_u32.to_le_bytes());
    inline.extend([0x82, 0x81, 0x82, 0x83, 0x06]);
    bytes.extend(object_graph_from_records(&[
        object_graph_record(&[0x12, 0x81, 0x84], &[0xfe]),
        inline_object_graph_record(&inline),
        object_graph_record(&[0x12, 0x81, 0x84], &[0x85, 0xfe]),
    ]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "paramin",
        "CATEarlySpecBrpLocation",
        "data",
        "Projected3DGeometrySet",
    ]));
    bytes
}

#[test]
fn binds_field_incidences_and_source_closed_schema_path() {
    let native = super::super::CatiaNative::decode(&fixture(0x82, &[0x85, 0xfe], 7));
    let [field] = native.inline_fields.as_slice() else {
        panic!("one strict inline field");
    };
    assert_eq!(field.self_field.reference.entity_id(), 2);
    assert_eq!(field.anchor_field.reference.entity_id(), 1);
    assert_eq!(
        field
            .value_field
            .as_ref()
            .expect("extended value incidence")
            .reference
            .entity_id(),
        3
    );
    assert_eq!(field.self_field.body_offset, 2);
    assert_eq!(field.anchor_field.body_offset, 15);
    assert_eq!(
        field
            .value_field
            .as_ref()
            .expect("extended value incidence")
            .body_offset,
        17
    );
    assert!(matches!(
        field.schema,
        InlineSchema::Words { path: Some(_), .. }
    ));
    let source = &native.object_graphs[0].records[1];
    assert!(
        source.owner.is_none()
            && source.class.is_none()
            && source.storage.is_none()
            && source.design_object.is_none()
    );
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store inline bindings");
    let loaded = super::super::CatiaNative::load(&namespace).expect("reload inline bindings");
    assert_eq!(loaded.inline_fields, native.inline_fields);
}

#[test]
fn contradictory_self_or_value_carrier_leaves_body_unassigned() {
    for bytes in [
        fixture(0x83, &[0x85, 0xfe], 7),
        fixture(0x82, &[0xfe], 7),
        fixture(0x82, &[0xfe, 0xfe], 7),
    ] {
        let native = super::super::CatiaNative::decode(&bytes);
        assert!(native.inline_fields.is_empty());
        assert!(native.object_graphs[0].records[1].inline_body.is_some());
    }
    let native = super::super::CatiaNative::decode(&fixture(0x82, &[0x85, 0xfe], 6));
    assert_eq!(native.inline_fields.len(), 1);
    assert!(matches!(
        native.inline_fields[0].schema,
        InlineSchema::Words {
            path: None,
            outer: 7,
            field: 5
        }
    ));
}

#[test]
fn inline_binding_obeys_collection_and_work_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    let native = super::super::CatiaNative::decode(&fixture(0x82, &[0x85, 0xfe], 7));
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!(),
        }
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty source context");
        let error = derive(
            &ctx,
            &native.object_graphs,
            &native.entity_records,
            &native.catalogs,
        )
        .expect_err("inline binding exceeds explicit budget");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
    }
}

#[test]
fn field_identity_does_not_resolve_in_another_graph() {
    let mut bytes = fixture(0x82, &[0x85, 0xfe], 7);
    let at = bytes
        .windows(5)
        .position(|w| w == [0x10, 0xfe, 0x82, 0x82, 0x32])
        .expect("constructed inline body");
    bytes[at + 15] = 0x88;
    bytes.extend(entity_table_record_with_value(8, &[0xfe]));
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&[object_graph_record(
        &[0x12, 0x88, 0x84],
        &[0xfe],
    )]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "paramin",
    ]));
    let native = super::super::CatiaNative::decode(&bytes);
    assert_eq!(native.object_graphs.len(), 2);
    assert_eq!(native.inline_fields.len(), 1);
    assert!(matches!(
        native.inline_fields[0].anchor_field.reference,
        CatiaEntityReference::Unresolved { entity_id: 8 }
    ));
}

#[test]
fn data_member_selection_is_independent_of_field_ordinal() {
    let mut bytes = fixture(0x82, &[0x85, 0xfe], 7);
    let at = bytes
        .windows(7)
        .position(|w| w == [3, 0x32, 5, 0, 0, 0, 0x32])
        .expect("constructed paired schema prefix");
    bytes[at + 7..at + 11].copy_from_slice(&8_u32.to_le_bytes());
    let catalog = bytes
        .windows(2)
        .rposition(|w| w == [0x7c, 2])
        .expect("constructed final catalog");
    bytes.truncate(catalog);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "paramin",
        "CATEarlySpecBrpLocation",
        "OtherMember",
        "Projected3DGeometrySet",
        "data",
    ]));
    let native = super::super::CatiaNative::decode(&bytes);
    let InlineSchema::Words {
        path: Some(path), ..
    } = &native.inline_fields[0].schema
    else {
        panic!("complete nonadjacent member path");
    };
    assert_eq!(path.member, native.catalogs[0].entries[8].id);
    assert_ne!(path.member, native.catalogs[0].entries[6].id);
}
