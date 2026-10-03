// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn topology_finding_construction_preserves_storage_work_and_sticky_refusals() {
    for geometry in [false, true] {
        for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("test dimension"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            let result = if geometry {
                super::super::geometry_error(&ctx, &mut findings, "test:model:feature#owner", "invalid geometry")
            } else {
                super::super::ref_error(&ctx, &mut findings, "test:model:feature#owner", format_args!("historical {}", "face"), "test:model:face#missing")
            };
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("finding construction must preserve a resource refusal"); };
            assert_eq!(original.dimension, dimension);
            assert!(findings.is_empty());
            assert!(matches!(super::super::geometry_error(&ctx, &mut findings, "other", "repeated"), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        }
    }
}

#[test]
fn topology_findings_keep_message_entity_check_and_order() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::super::ref_error(&ctx, &mut findings, "test:model:feature#owner", format_args!("historical {}", "face"), "test:model:face#missing").unwrap();
    super::super::geometry_error(&ctx, &mut findings, "test:model:feature#owner", "invalid geometry").unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].check, crate::report::check::Check::ReferentialIntegrity);
    assert_eq!(findings[0].message, "references missing historical face `test:model:face#missing`");
    assert_eq!(findings[1].check, crate::report::check::Check::GeometricConsistency);
    assert_eq!(findings[1].message, "invalid geometry");
    for finding in findings {
        assert_eq!(finding.entity.as_deref(), Some("test:model:feature#owner"));
        assert_eq!(finding.severity, crate::report::Severity::Error);
    }
    ctx.finish_session().unwrap();
}
