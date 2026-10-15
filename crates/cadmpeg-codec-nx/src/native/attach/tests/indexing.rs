// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::insert_source_property;
use crate::native::attach::insert_source_property_reference;
use crate::native::attach::segment_binding_body_indexes;

#[test]
fn source_property_reference_preserves_block_and_numeric_fallback() {
    let mut properties = std::collections::BTreeMap::new();
    insert_source_property_reference(
        &mut properties,
        format_args!("reference.{}", 0),
        Some("source-block"),
        17u32,
    );
    insert_source_property_reference(
        &mut properties,
        format_args!("reference.{}", 1),
        None,
        17u32,
    );
    assert_eq!(properties["reference.0"], "source-block");
    assert_eq!(properties["reference.1"], "17");
}

#[test]
fn operation_body_operand_source_keys_follow_reference_ordinal() {
    let mut properties = std::collections::BTreeMap::new();
    for reference_ordinal in [0, 1] {
        insert_source_property_reference(
            &mut properties,
            format_args!("operation_body_operand.{}.{}", reference_ordinal, 0),
            None,
            20u32,
        );
    }
    assert_eq!(properties["operation_body_operand.0.0"], "20");
    assert_eq!(properties["operation_body_operand.1.0"], "20");
}

#[test]
fn source_property_formats_key_and_value() {
    let mut properties = std::collections::BTreeMap::new();
    insert_source_property(
        &mut properties,
        format_args!("body_write.{}", 7),
        format_args!("{}", "write"),
    );
    assert_eq!(properties["body_write.7"], "write");
}

#[test]
fn segment_body_index_groups_bodies_by_object_and_binding() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let body = cadmpeg_ir::ids::BodyId::mint("nx:s2:body#3").unwrap();
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        id: body.clone(),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let binding = crate::native::segments::SegmentBodyBinding {
        id: "binding".into(),
        stream_link: "stream".into(),
        stream_ordinal: 2,
        stream_kind: crate::parasolid::StreamKind::Partition,
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 0,
        source_offset: 0,
    };

    crate::test_support::with_decode_context(|ctx| {
        let bindings = [binding];
        let indexes = segment_binding_body_indexes(ctx, &ir, &bindings).unwrap();
        assert_eq!(
            indexes.by_object[&10].as_slice(),
            std::slice::from_ref(&body)
        );
        assert_eq!(
            indexes.by_object[&11].as_slice(),
            std::slice::from_ref(&body)
        );
        assert_eq!(indexes.by_binding["binding"], [body]);
    });
}
