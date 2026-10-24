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
            native_ref: None,
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
fn metadata_digest_retains_only_the_digest() {
    let ir = metadata_document();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 64;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        crate::writer::swobjects_metadata_identity_local_sha256(&ctx, &ir)
            .unwrap()
            .len(),
        64
    );
    ctx.finish_session().unwrap();
}

#[test]
fn metadata_digest_preserves_recursion_refusal() {
    let ir = metadata_document();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) =
        crate::writer::swobjects_metadata_identity_local_sha256(&ctx, &ir).unwrap_err()
    else {
        panic!("metadata digest must preserve refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
    );
}

#[test]
fn enclosure_metadata_generation_requires_retained_opaque_prologue() {
    use cadmpeg_ir::attributes::AttributeValue;
    let mut ir = CadIr::empty();
    ir.model.attributes.push(SourceAttribute {
        id: AttributeId::mint("sldprt:metadata:transformed_reference_enclosure#0:6").unwrap(),
        target: AttributeTarget::Document,
        name: "transformed_reference_enclosure".into(),
        values: vec![
            AttributeValue::vector([0., 0., 0.]).unwrap(),
            AttributeValue::vector([100., 100., 100.]).unwrap(),
            AttributeValue::vector([-100., -100., -100.]).unwrap(),
            AttributeValue::float(200.).unwrap(),
        ],
    });
    assert!(matches!(
        crate::writer::metadata_payloads(&ir, 0.001, false),
        Err(CodecError::NotImplemented(_))
    ));
    let (objects, units) = crate::writer::metadata_payloads(&ir, 0.001, true).unwrap();
    assert!(objects.is_empty());
    assert!(units.is_none());
}
