// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

use crate::records::identity::DesignEntityId;

#[test]
fn rewritten_sketch_entity_id_keeps_admitted_malformed_diagnostics() {
    for (target, message) in [
        ("invalid", "entity_id requires a decimal suffix"),
        ("entity_bad", "entity_id requires a decimal u64 suffix"),
        ("entity_18446744073709551616", "entity_id requires a decimal u64 suffix"),
    ] {
        let source = crate::test_support::with_decode_context(|ctx| {
            let replacements = [("source_7".to_owned(), target.to_owned())];
            let mut map = IdentityMap::new(ctx, "rewrite entity-ID test text", |text| {
                ctx.copy_retained_text(text, "rewrite entity-ID test text")
            })?
            .with_text_replacements(replacements.iter().map(|(key, value)| (key, value)))?;
            DesignEntityId::try_from("source_7".to_owned())
                .unwrap()
                .rewrite_identities(ctx, &mut map)
        })
        .unwrap();
        assert_eq!(source.as_str(), target);
        assert_eq!(source.suffix(), 7);
        let copied = u64_from_index(target.len());
        let diagnostic = u64_from_index(message.len());
        let total = copied + diagnostic;
        for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::WorkUnits] {
            for ceiling in [total, total - 1] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                match dimension {
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = ceiling,
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = ceiling,
                    _ => unreachable!(),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = crate::design::decode::sketch::clone_sketch_entity_id_charged(&ctx, &source);
                if ceiling == total {
                    let Err(CodecError::Malformed(held)) = result else {
                        panic!("the original diagnostic variant must escape");
                    };
                    assert_eq!(held, message);
                    assert_eq!(held.capacity(), message.len());
                    let CodecError::ResourceLimit(limit) = ctx
                        .copy_retained_text("a", "after entity-ID diagnostic")
                        .unwrap_err()
                    else {
                        panic!("the diagnostic's account must remain live");
                    };
                    assert_eq!(limit.dimension, dimension);
                    assert_eq!(limit.operation, "after entity-ID diagnostic");
                    assert_eq!(limit.used, total);
                    assert_eq!(limit.additional, 1);
                    assert_eq!(limit.limit, ceiling);
                    assert_eq!(held, message);
                    let CodecError::ResourceLimit(later) =
                        ctx.charge_work(0, "after held diagnostic refusal").unwrap_err()
                    else {
                        panic!("the original refusal must remain fused");
                    };
                    assert_eq!(later, limit);
                } else {
                    let Err(CodecError::ResourceLimit(limit)) = result else {
                        panic!("the actual diagnostic copy must refuse");
                    };
                    assert_eq!(limit.dimension, dimension);
                    assert_eq!(limit.operation, "f3d sketch placement entity ID");
                    assert_eq!(limit.used, copied);
                    assert_eq!(limit.additional, diagnostic);
                    assert_eq!(limit.limit, ceiling);
                    let CodecError::ResourceLimit(later) =
                        ctx.charge_work(0, "after entity-ID refusal").unwrap_err()
                    else {
                        panic!("the original refusal must remain fused");
                    };
                    assert_eq!(later, limit);
                }
            }
        }
    }
}
