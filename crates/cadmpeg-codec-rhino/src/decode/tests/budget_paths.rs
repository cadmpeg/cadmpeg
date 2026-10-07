// SPDX-License-Identifier: Apache-2.0

use super::super::decode_pcurves;
use super::{source_shaped_plane_brep, with_expand_bytes, ArchiveVersion, DecodeContext,
    object_record, scan_with_objects};

#[test]
fn shared_brep_c2_slot_preserves_each_trim_geometry() {
    let (data, mut raw) = source_shaped_plane_brep();
    raw.trims[1].curve = Some(0);
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate Brep with a shared C2 slot");
    let values = with_expand_bytes(&data, |expand| {
        decode_pcurves(
            expand.ctx(), &data, ArchiveVersion::V5, brep.raw(), brep.resolved(),
            "plane", &std::collections::HashMap::new(),
        ).map(|decoded| decoded.values)
    }).expect("shared C2 slot decodes under the service profile");
    assert_eq!(values.len(), 3);
    assert_eq!(values[0].geometry, values[1].geometry);
    assert_ne!(values[0].id, values[1].id);
}

#[test]
fn brep_c2_cache_lookup_preserves_work_refusal() {
    let (data, mut raw) = source_shaped_plane_brep();
    raw.trims[1].curve = Some(0);
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    }).expect("validate Brep with a shared C2 slot");
    let refused = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "Rhino Brep decoded C2 slots", |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)?;
            decode_pcurves(&ctx, &data, ArchiveVersion::V5, brep.raw(), brep.resolved(),
                "plane", &std::collections::HashMap::new())
                .map(|decoded| decoded.values)
                .map_err(|error| match error {
                    crate::curves::GeometryError::Codec(error) => error,
                    error => panic!("unexpected geometry error: {error}"),
                })
        });
    assert!(matches!(refused, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "Rhino Brep decoded C2 slots"));
}

#[test]
fn brep_c2_cache_leaves_retention_for_output_poles() {
    let (data, raw) = source_shaped_plane_brep();
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    }).expect("validate Brep");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "Rhino Brep pcurve poles", |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)?;
            decode_pcurves(&ctx, &data, ArchiveVersion::V5, brep.raw(), brep.resolved(),
                "plane", &std::collections::HashMap::new())
                .map(|decoded| decoded.values)
                .map_err(|error| match error {
                    crate::curves::GeometryError::Codec(error) => error,
                    error => panic!("unexpected geometry error: {error}"),
                })
        });
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.used == 0));
}

#[test]
fn indexed_instance_dispatch_visits_only_selected_source() {
    let objects = (0..4).map(|_| object_record(ArchiveVersion::V5, 8, [0; 16]))
        .collect::<Vec<_>>();
    let scan = scan_with_objects(&objects);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "Rhino object dispatch", |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
            let mut transaction = DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
            transaction.instance_selection = Some(super::super::InstanceSelection::new(
                &ctx, 2, &[], crate::wire::Uuid::nil())?);
            transaction.decode_geometry()
        });
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "Rhino object dispatch" && limit.additional == 1));
}
