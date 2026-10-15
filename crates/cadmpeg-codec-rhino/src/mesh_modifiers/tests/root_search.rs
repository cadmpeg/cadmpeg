// SPDX-License-Identifier: Apache-2.0
use crate::chunks::{ArchiveVersion, FramingError};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn modifier_xml_root_search_admits_leading_document_children() {
    for kind in 0..5 {
        let tag = match kind {
            0 => super::super::DISPLACEMENT_ROOT,
            1 => super::super::EDGE_SOFTENING_ROOT,
            2 => super::super::THICKENING_ROOT,
            3 => super::super::CURVE_PIPING_ROOT,
            _ => super::super::SHUT_LINING_ROOT,
        };
        let xml = format!("<!--first--><?skip second?><XmL><{tag}/></XmL>");
        let parse = |ctx: &DecodeContext<'_>| match kind {
            0 => super::super::parse_xml(ctx, &xml, 2, ArchiveVersion::V6).map(drop),
            1 => super::super::parse_edge_softening_xml(ctx, &xml, 2).map(drop),
            2 => super::super::parse_thickening_xml(ctx, &xml, 2).map(drop),
            3 => super::super::parse_curve_piping_xml(ctx, &xml, 2).map(drop),
            _ => super::super::parse_shut_lining_xml(ctx, &xml, 2).map(drop),
        };
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "Rhino XML root element search",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
                let result = parse(&ctx);
                if let Err(FramingError::Resource(limit)) = &result {
                    assert_eq!(limit.additional, 1);
                    assert_eq!(ctx.resource_refusal(), Some(*limit));
                }
                result.map_err(|error| match error {
                    FramingError::Resource(limit) => CodecError::ResourceLimit(limit),
                    error => panic!("valid XML: {error:?}"),
                })
            },
        );
        parse(&cadmpeg_test_support::service_decode_context()).unwrap();
    }
}
