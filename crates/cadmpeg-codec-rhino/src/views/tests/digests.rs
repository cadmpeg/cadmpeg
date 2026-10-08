// SPDX-License-Identifier: Apache-2.0
use super::*;
#[test]
fn view_child_fingerprints_admit_hashing_work_through_the_parser() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let archive = ArchiveVersion::V5;
    let (bytes, record) = one_end_marker_view(archive);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino view child SHA-256",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let result = { let context = &ctx;
        let mut staging = context.reserve_scoped(0, "Rhino test view staging").unwrap();
        parse_list(context, &bytes, &record, archive, crate::settings::MillimeterScale::IDENTITY, ViewListKind::Named, &mut staging) };
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    );
}
