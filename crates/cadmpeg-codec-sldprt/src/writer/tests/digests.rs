// SPDX-License-Identifier: Apache-2.0
//! Admission and canonical ordering of borrowed metadata digest views.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::AttributeId;

fn metadata_document() -> CadIr {
    let mut ir = CadIr::empty();
    for (id, name) in [
        ("sldprt:test:attribute#20:4", "later"),
        ("sldprt:test:attribute#10:2", "earlier"),
        ("sldprt:test:attribute#10:3", "source_linear_unit_code"),
    ] {
        ir.model.attributes.push(SourceAttribute {
            id: AttributeId::mint(id).unwrap(),
            target: AttributeTarget::Document,
            name: name.to_owned(),
            values: Vec::new(),
        });
    }
    ir
}

#[test]
fn metadata_digest_preserves_source_order_and_excludes_unit_code() {
    let ir = metadata_document();
    let expected = [
        (
            &ir.model.attributes[1].id,
            ir.model.attributes[1].name.as_str(),
        ),
        (
            &ir.model.attributes[0].id,
            ir.model.attributes[0].name.as_str(),
        ),
    ];
    let expected = cadmpeg_ir::hash::sha256_hex(&serde_json::to_vec_pretty(&expected).unwrap());
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        crate::writer::swobjects_metadata_identity_local_sha256(&ctx, &ir).unwrap(),
        expected
    );
    ctx.finish_session().unwrap();
}

#[test]
fn metadata_digest_views_use_scoped_storage_and_retain_only_the_digest() {
    let ir = metadata_document();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 64;
    policy.limits.max_materialized_bytes = 16384;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        crate::writer::swobjects_metadata_identity_local_sha256(&ctx, &ir)
            .unwrap()
            .len(),
        64
    );
    let storage = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "metadata digest views released",
        )
        .unwrap();
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn metadata_digest_preserves_each_resource_refusal() {
    let ir = metadata_document();
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("metadata digest refusal dimensions"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(limit) =
            crate::writer::swobjects_metadata_identity_local_sha256(&ctx, &ir).unwrap_err()
        else {
            panic!("metadata digest must preserve refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
        );
    }
}
