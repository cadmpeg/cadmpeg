// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::insert_source_property;
use crate::native::attach::insert_source_property_reference;
use crate::native::attach::segment_binding_body_indexes;

#[test]
fn source_property_reference_preserves_block_and_numeric_fallback() {
    crate::test_support::with_decode_context(|ctx| {
        let mut properties = std::collections::BTreeMap::new();
        insert_source_property_reference(
            ctx,
            &mut properties,
            format_args!("reference.{}", 0),
            Some("source-block"),
            17u32,
        )
        .unwrap();
        insert_source_property_reference(
            ctx,
            &mut properties,
            format_args!("reference.{}", 1),
            None,
            17u32,
        )
        .unwrap();
        assert_eq!(properties["reference.0"], "source-block");
        assert_eq!(properties["reference.1"], "17");
    });
}

#[test]
fn operation_body_operand_source_keys_follow_reference_ordinal() {
    crate::test_support::with_decode_context(|ctx| {
        let mut properties = std::collections::BTreeMap::new();
        for reference_ordinal in [0, 1] {
            insert_source_property_reference(
                ctx,
                &mut properties,
                format_args!("operation_body_operand.{}.{}", reference_ordinal, 0),
                None,
                20u32,
            )
            .unwrap();
        }
        assert_eq!(properties["operation_body_operand.0.0"], "20");
        assert_eq!(properties["operation_body_operand.1.0"], "20");
    });
}

fn source_property_with_limit(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<(), cadmpeg_core::CodecError> {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            let mut properties = std::collections::BTreeMap::new();
            insert_source_property(
                ctx,
                &mut properties,
                format_args!("body_write.{}", 7),
                format_args!("{}", "write"),
            )?;
            assert_eq!(properties["body_write.7"], "write");
            Ok(())
        },
    )
}

#[test]
fn source_property_formats_key_and_value() {
    source_property_with_limit(|_| {}).unwrap();
}

#[test]
fn source_property_refuses_collection_limit() {
    let error =
        source_property_with_limit(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn source_property_refuses_retained_limit() {
    let error =
        source_property_with_limit(|policy| policy.limits.max_retained_bytes = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn source_property_refuses_work_limit() {
    let error = source_property_with_limit(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn segment_body_index_with_limit(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<(), cadmpeg_core::CodecError> {
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

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            let bindings = [binding];
            let indexes = segment_binding_body_indexes(ctx, &ir, &bindings)?;
            assert_eq!(
                indexes.by_object[&10].as_slice(),
                std::slice::from_ref(&body)
            );
            assert_eq!(
                indexes.by_object[&11].as_slice(),
                std::slice::from_ref(&body)
            );
            assert_eq!(indexes.by_binding["binding"], [body]);
            Ok(())
        },
    )
}

#[test]
fn segment_body_index_refuses_collection_limit() {
    let error =
        segment_body_index_with_limit(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn segment_body_index_refuses_scoped_limit() {
    let error = segment_body_index_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn segment_body_index_refuses_work_limit() {
    let error =
        segment_body_index_with_limit(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
