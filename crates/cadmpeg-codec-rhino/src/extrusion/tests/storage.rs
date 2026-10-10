// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use crate::chunks::BoundedReader;
use crate::curves::GeometryError;
use crate::loss::Diagnostics;
use crate::mesh::{MeshBudget, MeshExpand};
use crate::objects::{ClassUserdata, UserdataDescriptor};
use super::{ExtrusionFormat, MillimeterScale, CHUNKS};

fn format() -> ExtrusionFormat {
    ExtrusionFormat { archive: CHUNKS, writer_version: None, scale: MillimeterScale::IDENTITY }
}
fn codec(error: GeometryError) -> CodecError {
    match error { GeometryError::Codec(error) => error, error => panic!("unexpected geometry error: {error:?}") }
}

#[test]
fn extrusion_boundary_staging_is_scoped_until_consumed() {
    let bytes = super::payload(2, [false, false], None);
    for operation in ["Rhino extrusion boundaries", "Rhino extrusion transformed NURBS",
        "Rhino extrusion cap points", "Rhino extrusion cap knots"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        policy.limits.max_materialized_bytes = 65_536;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let (extrusion, storage) = super::super::decode(MeshExpand::new(&ctx, root), &bytes,
            0..bytes.len(), format(), &[], &mut MeshBudget::new()).unwrap();
        drop(probe);
        assert_eq!(extrusion.boundaries.len(), 1);
        drop(extrusion);
        drop(storage);
        let reclaimed = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
            "test reclaimed extrusion staging").unwrap();
        drop(reclaimed);
        assert!(ctx.finish_session().is_ok());
    }
}

#[test]
fn extrusion_mesh_cache_payload_promotes_only_after_cache_admission() {
    for v5 in [false, true] {
        let mut bytes = if v5 { super::one_mesh_wrapper() } else { super::one_mesh_cache() };
        if v5 { bytes.extend(super::polyline_wrapper(false, true)); bytes.extend(super::null_object_wrapper()); }
        else { let marker = bytes.len() - 5; bytes[marker] = 2; }
        let descriptor = UserdataDescriptor::Known(ClassUserdata {
            range: 0..bytes.len(), version: (2,2),
            class_uuid: super::ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            item_uuid: super::ON_V5_EXTRUSION_DISPLAY_MESH_CACHE, copy_count: 1,
            transform_range: 0..0, application_uuid: None, save_context: None,
            payload_range: 0..bytes.len(),
        });
        let promotion = if v5 { "Rhino V5 extrusion mesh-cache payloads" } else { "Rhino extrusion mesh-cache payloads" };
        for operation in ["Rhino mesh scaled vertices", promotion] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = u64::MAX;
            policy.limits.max_materialized_bytes = 65_536;
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
            let expand = MeshExpand::new(&ctx, root);
            let mut warnings = Diagnostics::new();
            let result = if v5 { super::super::read_v5_mesh_cache(expand, &bytes, format(),
                std::slice::from_ref(&descriptor), &mut MeshBudget::new(), &mut warnings) }
            else { let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
                super::super::read_mesh_cache(expand, &bytes, &mut reader, format(),
                    &mut MeshBudget::new(), &mut warnings) };
            assert!(matches!(result, Err(GeometryError::Malformed(_))), "{result:?}");
            drop(result);
            drop(probe);
            let reclaimed = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
                "test reclaimed rejected mesh payloads").unwrap();
            drop(reclaimed);
            assert!(ctx.finish_session().is_ok());
        }
    }
}

#[test]
fn accepted_extrusion_cache_payload_retained_probe_is_active() {
    for v5 in [false, true] {
        let mut bytes = if v5 { super::one_mesh_wrapper() } else { super::one_mesh_cache() };
        if v5 { bytes.extend(super::null_object_wrapper()); bytes.extend(super::null_object_wrapper()); }
        let descriptor = UserdataDescriptor::Known(ClassUserdata {
            range: 0..bytes.len(), version: (2,2),
            class_uuid: super::ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
            item_uuid: super::ON_V5_EXTRUSION_DISPLAY_MESH_CACHE, copy_count: 1,
            transform_range: 0..0, application_uuid: None, save_context: None,
            payload_range: 0..bytes.len(),
        });
        let operation = if v5 { "Rhino V5 extrusion mesh-cache payloads" } else { "Rhino extrusion mesh-cache payloads" };
        cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = cap;
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let expand = MeshExpand::new(&ctx, root);
            let result = if v5 { super::super::read_v5_mesh_cache(expand, &bytes, format(),
                std::slice::from_ref(&descriptor), &mut MeshBudget::new(), &mut Diagnostics::new()) }
            else { let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
                super::super::read_mesh_cache(expand, &bytes, &mut reader, format(), &mut MeshBudget::new(), &mut Diagnostics::new()) };
            if let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = &result { assert_eq!(ctx.resource_refusal(), Some(*limit)); }
            result.map(|_| ()).map_err(codec)
        });
    }
}
