// SPDX-License-Identifier: Apache-2.0
use super::{ExtrusionFormat, MillimeterScale, CHUNKS};
use crate::chunks::BoundedReader;
use crate::curves::GeometryError;
use crate::loss::Diagnostics;
use crate::mesh::{MeshBudget, MeshExpand};
use crate::objects::{ClassUserdata, UserdataDescriptor};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn format() -> ExtrusionFormat {
    ExtrusionFormat {
        archive: CHUNKS,
        writer_version: None,
        scale: MillimeterScale::IDENTITY,
    }
}
fn codec(error: GeometryError) -> CodecError {
    match error {
        GeometryError::Codec(error) => error,
        error => panic!("unexpected geometry error: {error:?}"),
    }
}

#[test]
fn extrusion_boundary_staging_is_scoped_until_consumed() {
    let bytes = super::payload(2, [false, false], None);
    for operation in [
        "Rhino extrusion boundaries",
        "Rhino extrusion transformed NURBS",
        "Rhino extrusion cap points",
        "Rhino extrusion cap knots",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        policy.limits.max_materialized_bytes = 65_536;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let (extrusion, storage) = super::super::decode(
            MeshExpand::new(&ctx, root),
            &bytes,
            0..bytes.len(),
            format(),
            &[],
            &mut MeshBudget::new(),
        )
        .unwrap();
        drop(probe);
        assert_eq!(extrusion.boundaries.len(), 1);
        drop(extrusion);
        drop(storage);
        let reclaimed = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test reclaimed extrusion staging",
            )
            .unwrap();
        drop(reclaimed);
        assert!(ctx.finish_session().is_ok());
    }
}

#[test]
fn extrusion_mesh_cache_payload_promotes_only_after_cache_admission() {
    for v5 in [false, true] {
        let mut bytes = if v5 {
            super::one_mesh_wrapper()
        } else {
            super::one_mesh_cache()
        };
        if v5 {
            bytes.extend(super::polyline_wrapper(false, true));
            bytes.extend(super::null_object_wrapper());
        } else {
            let marker = bytes.len() - 5;
            bytes[marker] = 2;
        }
        let descriptor = UserdataDescriptor::Known(ClassUserdata {
            range: 0..bytes.len(),
            version: (2, 2),
            class_uuid: super::ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            item_uuid: super::ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            save_context: None,
            payload_range: 0..bytes.len(),
        });
        let promotion = if v5 {
            "Rhino V5 extrusion mesh-cache payloads"
        } else {
            "Rhino extrusion mesh-cache payloads"
        };
        for operation in ["Rhino mesh scaled vertices", promotion] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = u64::MAX;
            policy.limits.max_materialized_bytes = 65_536;
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
            let expand = MeshExpand::new(&ctx, root);
            let mut warnings = Diagnostics::new();
            let result = if v5 {
                super::super::read_v5_mesh_cache(
                    expand,
                    &bytes,
                    format(),
                    std::slice::from_ref(&descriptor),
                    &mut MeshBudget::new(),
                    &mut warnings,
                )
            } else {
                let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
                super::super::read_mesh_cache(
                    expand,
                    &bytes,
                    &mut reader,
                    format(),
                    &mut MeshBudget::new(),
                    &mut warnings,
                )
            };
            assert!(
                matches!(result, Err(GeometryError::Malformed(_))),
                "{result:?}"
            );
            drop(result);
            drop(probe);
            let reclaimed = ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "test reclaimed rejected mesh payloads",
                )
                .unwrap();
            drop(reclaimed);
            assert!(ctx.finish_session().is_ok());
        }
    }
}

#[test]
fn accepted_extrusion_cache_payload_retained_probe_is_active() {
    for v5 in [false, true] {
        let (bytes, range, userdata) = extrusion_with_cache(v5, true, [false, false], false);
        let operation = if v5 {
            "Rhino V5 extrusion mesh-cache payloads"
        } else {
            "Rhino extrusion mesh-cache payloads"
        };
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
                let result = super::super::decode(
                    MeshExpand::new(&ctx, root),
                    &bytes,
                    range.clone(),
                    format(),
                    &userdata,
                    &mut MeshBudget::new(),
                );
                if let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = &result {
                    assert_eq!(ctx.resource_refusal(), Some(*limit));
                }
                result.map(|_| ()).map_err(codec)
            },
        );
    }
}

fn extrusion_with_cache(
    v5: bool,
    closed: bool,
    caps: [bool; 2],
    degenerate_path: bool,
) -> (Vec<u8>, std::ops::Range<usize>, Vec<UserdataDescriptor>) {
    let cache = (!v5).then(super::one_mesh_cache);
    let mut bytes = super::payload_with_profile(
        if v5 { 2 } else { 3 },
        caps,
        cache,
        super::polyline_wrapper(false, closed),
    );
    if degenerate_path {
        let outer = crate::chunks::chunk_at(&bytes, 0, bytes.len(), CHUNKS, false).unwrap();
        let mut body = bytes[outer.body()].to_vec();
        let profile = crate::chunks::chunk_at(
            &body,
            crate::layout::anonymous_version_prefix::LEN,
            body.len(),
            CHUNKS,
            false,
        )
        .unwrap();
        let path_start = profile.next_offset();
        let point_bytes = std::mem::size_of::<[f64; 3]>();
        let from = body[path_start..path_start + point_bytes].to_vec();
        body[path_start + point_bytes..path_start + 2 * point_bytes].copy_from_slice(&from);
        let mut children = vec![profile.range()];
        if !v5 {
            children.push(body.len() - super::one_mesh_cache().len()..body.len());
        }
        bytes = crate::test_support::test_dump::crc_chunk_excluding(
            CHUNKS,
            super::ANONYMOUS,
            &body,
            &children,
        );
    }
    let range = 0..bytes.len();
    let mut userdata = Vec::new();
    if v5 {
        let cache_start = bytes.len();
        bytes.extend(super::one_mesh_wrapper());
        bytes.extend(super::null_object_wrapper());
        bytes.extend(super::null_object_wrapper());
        userdata.push(UserdataDescriptor::Known(ClassUserdata {
            range: cache_start..bytes.len(),
            version: (2, 2),
            class_uuid: super::ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            item_uuid: super::ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            save_context: None,
            payload_range: cache_start..bytes.len(),
        }));
    }
    (bytes, range, userdata)
}

fn assert_rejection_reclaims_storage(
    bytes: &[u8],
    range: std::ops::Range<usize>,
    userdata: &[UserdataDescriptor],
    message: &str,
    buffer_bytes: usize,
) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65_536;
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let mut mesh_budget = MeshBudget::new();
    for attempt in 1..=16 {
        let error = super::super::decode(
            MeshExpand::new(&ctx, root),
            bytes,
            range.clone(),
            format(),
            userdata,
            &mut mesh_budget,
        )
        .expect_err("extrusion fails after its speculative output is built");
        assert!(matches!(&error, GeometryError::Malformed(_)), "{error:?}");
        assert!(error.to_string().contains(message), "{error}");
        drop(error);
        assert_eq!(mesh_budget.used(), attempt * buffer_bytes);
        let reclaimed = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test reclaimed rejected extrusion storage",
            )
            .unwrap();
        drop(reclaimed);
    }
    ctx.finish_session().unwrap();
}

fn damage_cache_uuid_crc(
    bytes: &mut [u8],
    range: &std::ops::Range<usize>,
    userdata: &[UserdataDescriptor],
    v5: bool,
) {
    let (wrapper_start, wrapper_end) = if v5 {
        let payload = &userdata[0].known().unwrap().payload_range;
        (payload.start, payload.end)
    } else {
        let outer = crate::chunks::chunk_at(bytes, range.start, range.end, CHUNKS, false).unwrap();
        let cache = crate::chunks::chunk_at(
            bytes,
            outer.body().end - super::one_mesh_cache().len(),
            outer.body().end,
            CHUNKS,
            false,
        )
        .unwrap();
        let item = crate::chunks::chunk_at(
            bytes,
            cache.body().start + crate::layout::anonymous_version_prefix::LEN + 1,
            cache.body().end,
            CHUNKS,
            false,
        )
        .unwrap();
        (
            item.body().start
                + crate::layout::anonymous_version_prefix::LEN
                + crate::layout::uuid_wire_form::LEN,
            item.body().end,
        )
    };
    let wrapper =
        crate::chunks::chunk_at(bytes, wrapper_start, wrapper_end, CHUNKS, false).unwrap();
    let uuid = crate::chunks::chunk_at(
        bytes,
        wrapper.body().start,
        wrapper.body().end,
        CHUNKS,
        true,
    )
    .unwrap();
    assert_eq!(uuid.typecode, 0x0002_fffb);
    bytes[uuid.body().end] ^= 1;
}

#[test]
fn extrusion_rejected_path_reclaims_cache_warning_storage() {
    for v5 in [false, true] {
        let (mut bytes, range, userdata) = extrusion_with_cache(v5, true, [false, false], true);
        damage_cache_uuid_crc(&mut bytes, &range, &userdata, v5);
        assert_rejection_reclaims_storage(
            &bytes,
            range,
            &userdata,
            "extrusion path is not finite and distinct",
            std::mem::size_of::<[f32; 3]>(),
        );
    }
}

#[test]
fn accepted_extrusion_cache_warning_copies_promote_retained_storage() {
    for v5 in [false, true] {
        let (mut bytes, range, userdata) = extrusion_with_cache(v5, true, [false, false], false);
        damage_cache_uuid_crc(&mut bytes, &range, &userdata, v5);
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "Rhino diagnostic copy text",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
                let result = super::super::decode(
                    MeshExpand::new(&ctx, root),
                    &bytes,
                    range.clone(),
                    format(),
                    &userdata,
                    &mut MeshBudget::new(),
                );
                match &result {
                    Ok((extrusion, _storage)) => {
                        assert_eq!(extrusion.warnings.len(), 1);
                        assert_eq!(
                            extrusion.warnings[0].code,
                            Some(crate::loss::RhinoLossCode::IntegrityFailure),
                        );
                        assert!(extrusion.warnings[0]
                            .message
                            .starts_with("CRC mismatch at offset "));
                        assert!(extrusion.warnings[0].message.contains("typecode 0x2fffb:"));
                        assert_eq!(extrusion.meshes.len(), 1);
                    }
                    Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) => {
                        assert_eq!(ctx.resource_refusal(), Some(*limit));
                        assert!(limit.additional > 0);
                    }
                    Err(error) => panic!("recoverable cache CRC: {error:?}"),
                }
                result.map(|_| ()).map_err(codec)
            },
        );
    }
}

#[test]
fn extrusion_rejected_path_reclaims_successful_cache_payloads() {
    for v5 in [false, true] {
        let (bytes, range, userdata) = extrusion_with_cache(v5, true, [false, false], true);
        assert_rejection_reclaims_storage(
            &bytes,
            range,
            &userdata,
            "extrusion path is not finite and distinct",
            std::mem::size_of::<[f32; 3]>(),
        );
    }
}

#[test]
fn extrusion_rejected_caps_reclaims_successful_cache_and_start_curves() {
    for v5 in [false, true] {
        let (bytes, range, userdata) = extrusion_with_cache(v5, false, [true, false], false);
        assert_rejection_reclaims_storage(
            &bytes,
            range,
            &userdata,
            "capped extrusion requires closed profile boundaries",
            std::mem::size_of::<[f32; 3]>(),
        );
    }
}

#[test]
fn extrusion_rejected_caps_reclaims_start_curve_warning_copies() {
    let mut profile = super::polyline_wrapper(false, false);
    let wrapper = crate::chunks::chunk_at(&profile, 0, profile.len(), CHUNKS, false).unwrap();
    let uuid = crate::chunks::chunk_at(
        &profile,
        wrapper.body().start,
        wrapper.body().end,
        CHUNKS,
        true,
    )
    .unwrap();
    profile[uuid.body().end] ^= 1;
    let bytes = super::payload_with_profile(2, [true, false], None, profile);
    assert_rejection_reclaims_storage(
        &bytes,
        0..bytes.len(),
        &[],
        "capped extrusion requires closed profile boundaries",
        0,
    );
}

#[test]
fn accepted_extrusion_start_curves_promote_retained_storage() {
    let bytes = super::payload(2, [false, false], None);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "Rhino extrusion start curves",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let result = super::super::decode(
                MeshExpand::new(&ctx, root),
                &bytes,
                0..bytes.len(),
                format(),
                &[],
                &mut MeshBudget::new(),
            );
            if let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = &result {
                assert_eq!(ctx.resource_refusal(), Some(*limit));
                assert!(limit.additional > 0);
            }
            result.map(|_| ()).map_err(codec)
        },
    );
}
