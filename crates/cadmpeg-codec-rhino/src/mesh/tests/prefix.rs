// SPDX-License-Identifier: Apache-2.0
use super::{
    correspondence_userdata_payload, v5_double_userdata_descriptor,
    v5_double_userdata_payload, with_expand, with_expand_policy,
};
use crate::chunks::ArchiveVersion;
use crate::curves::GeometryError;
use crate::mesh::{
    decode, MeshBudget, MeshDecodeOptions, MeshExpand, MeshId,
    TT_MAPPING_MESH_INFO_USERDATA, TT_RENDER_MESH_INFO_USERDATA,
};
use crate::objects::{ClassUserdata, UserdataDescriptor};
use crate::settings::MillimeterScale;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn raw_mesh(points: &[[f32; 3]], normals: bool) -> Vec<u8> {
    let count = i32::try_from(points.len()).expect("fixture count");
    let mut payload = vec![0x10];
    payload.extend(count.to_le_bytes());
    payload.extend(1_i32.to_le_bytes());
    for _ in 0..4 {
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
    }
    payload.extend([0; 16]);
    payload.extend([0; 64]);
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0; 5]);
    payload.extend(1_i32.to_le_bytes());
    payload.extend([0, 1, 2, 2]);
    payload.extend(count.to_le_bytes());
    for point in points {
        for coordinate in point {
            payload.extend(coordinate.to_le_bytes());
        }
    }
    payload.extend(if normals { count } else { 0 }.to_le_bytes());
    if normals {
        for _ in points {
            for coordinate in [0.0_f32, 0.0, 1.0] {
                payload.extend(coordinate.to_le_bytes());
            }
        }
    }
    for _ in 0..3 {
        payload.extend(0_i32.to_le_bytes());
    }
    payload
}

fn decode_mesh(
    expand: MeshExpand<'_>,
    bytes: &[u8],
    end: usize,
    scale: MillimeterScale,
    userdata: &[UserdataDescriptor],
) -> Result<crate::mesh::DecodedMesh, GeometryError> {
    decode(
        expand,
        bytes,
        0..end,
        ArchiveVersion::V5,
        MeshDecodeOptions {
            writer_version: None,
            association: None,
            id: MeshId::Ready(
                cadmpeg_ir::tessellation::TessellationId::mint(
                    "synthetic:test:tessellation#prefix",
                )
                .expect("fixture identity"),
            ),
            scale,
            userdata,
        },
        &mut MeshBudget::new(),
    )
}

fn large_first_point() -> [[f32; 3]; 32] {
    let mut points = [[0.0; 3]; 32];
    points[0][0] = f32::MAX;
    points[2][1] = 1.0;
    points
}

#[test]
fn float_mesh_scaling_stops_work_at_first_invalid_point() {
    let points = large_first_point();
    let bytes = raw_mesh(&points, false);
    // 32 point reads, one face, three empty userdata end probes, then one scale visit.
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 32 + 1 + 3 + 1;
    with_expand_policy(&bytes, policy, |expand| {
        let error = decode_mesh(
            expand,
            &bytes,
            bytes.len(),
            crate::test_support::millimeter_scale(f64::MAX),
            &[],
        )
        .expect_err("first scale overflows");
        assert!(matches!(error, GeometryError::Malformed(_)));
        assert!(error.to_string().contains("scaled mesh vertex is invalid"));
        assert!(expand.ctx().resource_refusal().is_none());
    });
    with_expand(&bytes, |expand| {
        let mesh = decode_mesh(expand, &bytes, bytes.len(), MillimeterScale::IDENTITY, &[])
            .expect("finite scale control");
        assert_eq!(mesh.tessellation.vertices().len(), 32);
        assert_eq!(mesh.tessellation.vertices()[0].get().x, f64::from(f32::MAX));
    });
}

#[test]
fn double_mesh_scaling_stops_work_at_first_invalid_point() {
    let points = large_first_point();
    let mut bytes = raw_mesh(&points, false);
    let end = bytes.len();
    let doubles = points.map(|point| point.map(f64::from));
    bytes.extend(v5_double_userdata_payload(&doubles));
    let userdata = [v5_double_userdata_descriptor(end..bytes.len())];
    // Float reads + face + ngon scan/end + double lookup + synchronization/end
    // + two correspondence visits + proxy scan/end + first double scale visit.
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 32 + 1 + 2 + 1 + 33 + 2 + 2 + 1;
    with_expand_policy(&bytes, policy, |expand| {
        let error = decode_mesh(
            expand,
            &bytes,
            end,
            crate::test_support::millimeter_scale(f64::MAX),
            &userdata,
        )
        .expect_err("first double scale overflows");
        assert!(matches!(error, GeometryError::Malformed(_)));
        assert!(error.to_string().contains("scaled mesh vertex is invalid"));
        assert!(expand.ctx().resource_refusal().is_none());
    });
    with_expand(&bytes, |expand| {
        let mesh = decode_mesh(expand, &bytes, end, MillimeterScale::IDENTITY, &userdata)
            .expect("finite double scale control");
        assert_eq!(mesh.tessellation.vertices().len(), 32);
        assert!(mesh.warnings.is_empty());
    });
}

#[test]
fn mesh_scaling_refusal_admits_one_point_and_stays_sticky() {
    let points = large_first_point();
    let bytes = raw_mesh(&points, false);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 32 + 1 + 3;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let expand = MeshExpand::new(&ctx, root);
    let GeometryError::Codec(CodecError::ResourceLimit(limit)) =
        decode_mesh(expand, &bytes, bytes.len(), MillimeterScale::IDENTITY, &[])
            .expect_err("first scale visit refuses")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "Rhino float mesh vertex scaling");
    assert_eq!(limit.additional, 1);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

fn correspondence_descriptor(
    range: std::ops::Range<usize>,
    class: crate::wire::Uuid,
) -> UserdataDescriptor {
    UserdataDescriptor::Known(ClassUserdata {
        range: range.clone(),
        version: (2, 2),
        class_uuid: class,
        item_uuid: class,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: range,
    })
}

#[test]
fn correspondence_warning_refusal_leaves_descriptor_suffix_unvisited() {
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut bytes = raw_mesh(&points, false);
    let end = bytes.len();
    bytes.extend(correspondence_userdata_payload(2, true));
    let descriptor = correspondence_descriptor(end..bytes.len(), TT_MAPPING_MESH_INFO_USERDATA);
    let userdata = std::iter::repeat_n(descriptor, 32).collect::<Vec<_>>();
    let mut policy = DecodePolicy::service();
    // Four source slots, two full 32-descriptor searches/end probes, one correspondence visit.
    policy.limits.max_collection_items = 4;
    policy.limits.max_work_units = 3 + 1 + 33 + 33 + 1;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let expand = MeshExpand::new(&ctx, root);
    let GeometryError::Codec(CodecError::ResourceLimit(limit)) =
        decode_mesh(expand, &bytes, end, MillimeterScale::IDENTITY, &userdata)
            .expect_err("first diagnostic slot refuses")
    else {
        panic!("diagnostic refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "Rhino diagnostics");
    assert_eq!((limit.used, limit.additional), (4, 1));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn correspondence_warning_order_stays_mapping_then_render() {
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let mut bytes = raw_mesh(&points, false);
    let end = bytes.len();
    bytes.extend(correspondence_userdata_payload(2, true));
    let range = end..bytes.len();
    let userdata = [
        correspondence_descriptor(range.clone(), TT_RENDER_MESH_INFO_USERDATA),
        correspondence_descriptor(range, TT_MAPPING_MESH_INFO_USERDATA),
    ];
    with_expand(&bytes, |expand| {
        let mesh = decode_mesh(expand, &bytes, end, MillimeterScale::IDENTITY, &userdata)
            .expect("optional carrier warnings");
        assert_eq!(mesh.warnings.len(), 2);
        assert!(mesh.warnings[0]
            .message
            .contains("CTtMappingMeshInfoUserData"));
        assert!(mesh.warnings[1]
            .message
            .contains("CTtRenderMeshInfoUserData"));
    });
}

#[test]
fn raw_mesh_normal_promotion_preserves_retained_refusal() {
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let bytes = raw_mesh(&points, true);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let expand = MeshExpand::new(&ctx, root);
    let GeometryError::Codec(CodecError::ResourceLimit(limit)) =
        decode_mesh(expand, &bytes, bytes.len(), MillimeterScale::IDENTITY, &[])
            .expect_err("normal promotion refuses")
    else {
        panic!("retained refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "Rhino mesh normal scratch");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
    with_expand(&bytes, |expand| {
        let mesh = decode_mesh(expand, &bytes, bytes.len(), MillimeterScale::IDENTITY, &[])
            .expect("normal promotion control");
        assert_eq!(mesh.tessellation.vertex_normals().len(), 3);
    });
}
