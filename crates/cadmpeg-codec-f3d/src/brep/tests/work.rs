// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use serde_value::Value;

#[test]
fn brep_value_walks_preserve_work_refusals() {
    for operation in [
        "walk F3D BREP owned IDs",
        "identity rewrite scalar",
        "walk F3D BREP references",
    ] {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 1;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let value = Value::Seq(vec![Value::U32(1), Value::U32(2)]);
            let error = match operation {
                "walk F3D BREP owned IDs" => {
                    super::super::graph_ops::collect_owned_ids(ctx, &value, &mut Vec::new())
                }
                "identity rewrite scalar" => {
                    let mut map = IdentityMap::new(ctx, operation, |source: &str| {
                        ctx.copy_retained_text(source, operation)
                    })
                    .unwrap();
                    vec![1_u32, 2_u32]
                        .rewrite_identities(ctx, &mut map)
                        .map(|_| ())
                }
                _ => super::super::graph_ops::collect_brep_references(
                    ctx,
                    &value,
                    &[],
                    &mut Vec::new(),
                ),
            }
            .unwrap_err();
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("walk must refuse");
            };
            assert_eq!(limit.operation, operation);
            assert_eq!(Some(limit), ctx.resource_refusal());
        });
    }
}

#[test]
fn body_selector_scans_preserve_work_refusals() {
    let key = cadmpeg_asm::brep::records::BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: cadmpeg_ir::ids::BodyId::mint("f3d:brep:body#1").unwrap(),
        record_index: 1,
        body_ordinal: 0,
        source_brep: None,
        asm_body_key: Some(7),
    };
    for (work, selector, operation) in [
        (0, 7, "match F3D native body selector"),
        (1, 0, "match F3D ordinal body selector"),
    ] {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = work;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let error =
                super::super::resolve_body_selector(ctx, [&key].into_iter(), selector).unwrap_err();
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("selector must refuse");
            };
            assert_eq!(limit.operation, operation);
            assert_eq!(Some(limit), ctx.resource_refusal());
        });
    }
}
