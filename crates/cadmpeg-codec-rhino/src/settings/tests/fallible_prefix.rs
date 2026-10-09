// SPDX-License-Identifier: Apache-2.0
//! Actual metadata prefixes and temporary class-owned userdata.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::chunks::ArchiveVersion;
use crate::loss::Diagnostics;

fn assert_first_visit(ctx: DecodeContext<'_>, error: CodecError, used: u64, operation: &str) {
    let CodecError::ResourceLimit(refusal) = error else { panic!("actual metadata visit refusal"); };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, operation);
    assert_eq!((refusal.used, refusal.additional), (used, 1));
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn metadata_table_traversal_refuses_only_first_visit() {
    let tables: Vec<_> = (0..1024).map(|_| super::metadata_table(0x1234, 0, Vec::new())).collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut warnings = Diagnostics::new();
    let error = crate::settings::parse_metadata(&ctx, &[], ArchiveVersion::V8, &tables, &mut warnings).unwrap_err();
    assert!(warnings.is_empty());
    assert_first_visit(ctx, error, 0, "Rhino parse metadata traversal");
}

#[test]
fn metadata_record_traversal_refuses_only_first_visit() {
    let record = crate::container::Record::short(0xa000_0038, 0..0, 7);
    let tables = [super::metadata_table(0x1000_0015, 0, vec![record; 1024])];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The one table visit is admitted; the first record then refuses.
    policy.limits.max_work_units = 1;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut warnings = Diagnostics::new();
    let error = crate::settings::parse_metadata(&ctx, &[], ArchiveVersion::V8, &tables, &mut warnings).unwrap_err();
    assert!(warnings.is_empty());
    assert_first_visit(ctx, error, 1, "Rhino parse metadata traversal");
}

#[test]
fn layer_parent_passes_refuse_only_first_actual_visit() {
    let (bytes, tables) = super::layers::layer_fixture(&[0], None, 1, [0; 16], &[]);
    let metadata = super::parse_test_metadata(&bytes, ArchiveVersion::V8, &tables, &mut Diagnostics::new());
    assert_eq!(metadata.layers.len(), 1);
    let layers = vec![metadata.layers[0].clone(); 1024];
    assert!(layers.iter().all(|layer| layer.id.is_none_or(|id| id.is_nil())));
    for admitted_visits in [0, 1024] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = admitted_visits;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut warnings = Diagnostics::new();
        let error = crate::settings::report_layer_parent_references(&ctx, &layers, &mut warnings).unwrap_err();
        assert!(warnings.is_empty());
        assert_first_visit(ctx, error, admitted_visits, "Rhino report layer parent references traversal");
    }
}

#[test]
fn layer_userdata_descriptors_use_scoped_backing() {
    use crate::test_support::test_dump::class_userdata_with_payload;
    let userdata = class_userdata_with_payload(ArchiveVersion::V8, [0x44; 16], [0x55; 16], &[]);
    let writer_version = Some(200_912_010);
    let (bytes, tables) = super::layers::layer_fixture(&[0], writer_version, 1, [0x66; 16], &userdata);
    let record = &tables.last().unwrap().records[0];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The returned layer owns its one-byte name. Its userdata descriptor
    // is consumed during parsing and needs no retained output slot.
    policy.limits.max_retained_bytes = 1;
    // The input-proportional material allowance starts at 16 MiB. A declared
    // ceiling at that base is the effective ceiling for this fixture, so
    // reserving it after parsing proves that no scratch backing remains live.
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut warnings = Diagnostics::new();
    let mut losses = Vec::new();
    let (layer, opaque) = crate::settings::parse_layer(&ctx, &bytes, record, ArchiveVersion::V8, writer_version, &mut warnings, &mut losses)
        .expect("only the returned one-byte name needs retained backing");
    assert_eq!(layer.name, "L");
    assert!(!opaque);
    assert!(layer.per_viewport_settings.is_empty());
    assert!(warnings.is_empty());
    assert!(losses.is_empty());
    let storage = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "temporary layer userdata release probe")
        .expect("all parser scratch is released while the layer stays live");
    drop(storage);
    ctx.finish_session().unwrap();
}
