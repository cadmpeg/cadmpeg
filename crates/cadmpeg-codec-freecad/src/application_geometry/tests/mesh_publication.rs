// SPDX-License-Identifier: Apache-2.0

use super::{mesh_hdr, parse_mesh, resource_test_property, DecodeArena, DecodeContext, DecodePolicy};
use crate::native::PropertyRecord;
use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::tessellation::TessellationMesh;

fn mesh_bytes(vertices: &[[f32; 3]], triangles: &[[u32; 3]], bounds: [f32; 6]) -> Vec<u8> {
    let mut bytes = mesh_hdr::MAGIC_VALUE.to_le_bytes().to_vec();
    bytes.extend_from_slice(&mesh_hdr::VERSION_VALUE.to_le_bytes());
    bytes.extend_from_slice(&[0; mesh_hdr::LEN - mesh_hdr::INFORMATION]);
    bytes.extend_from_slice(&u32::try_from(vertices.len()).expect("vertex count").to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(triangles.len()).expect("facet count").to_le_bytes());
    for vertex in vertices {
        for coordinate in vertex {
            bytes.extend_from_slice(&coordinate.to_le_bytes());
        }
    }
    for triangle in triangles {
        for index in triangle {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        for _ in 0..3 {
            bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        }
    }
    for bound in bounds {
        bytes.extend_from_slice(&bound.to_le_bytes());
    }
    bytes
}

fn assert_rejected_storage_released(bytes: &[u8], property: &PropertyRecord, expected: &str) {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("context");
    for _ in 0..2 {
        let error = parse_mesh(&ctx, property, bytes).expect_err("invalid mesh candidate");
        assert!(matches!(error, CodecError::Malformed(message) if message == expected));
        assert_eq!(ctx.resource_refusal(), None);
    }
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(u64::MAX, "rejected mesh storage probe") else {
        panic!("retained overflow probe")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.used, 0);
}

#[test]
fn invalid_later_mesh_vertex_releases_candidate_storage() {
    let bytes = mesh_bytes(&[[0.0; 3], [f32::NAN, 0.0, 0.0]], &[], [0.0; 6]);
    assert_rejected_storage_released(&bytes, &resource_test_property(),
        "mesh point contains a non-finite coordinate");
}

#[test]
fn invalid_mesh_facet_releases_vertex_and_facet_storage() {
    let bytes = mesh_bytes(&[[0.0; 3]], &[[0, 0, 1]], [0.0; 6]);
    assert_rejected_storage_released(&bytes, &resource_test_property(),
        "mesh facet point is out of bounds");
}

#[test]
fn invalid_mesh_bound_releases_completed_population_storage() {
    let bytes = mesh_bytes(&[[0.0; 3]], &[[0, 0, 0]], [0.0, 0.0, 0.0, 0.0, 0.0, f32::NAN]);
    assert_rejected_storage_released(&bytes, &resource_test_property(),
        "FCStd mesh bounding box contains a non-finite value");
}

#[test]
fn invalid_mesh_source_releases_mesh_identity_and_source_storage() {
    let bytes = mesh_bytes(&[[0.0; 3]], &[[0, 0, 0]], [0.0; 6]);
    let mut property = resource_test_property();
    property.owner = " ".into();
    assert_rejected_storage_released(&bytes, &property, "source object_id must not be empty");
}

#[test]
fn rejected_mesh_identity_keeps_escaping_diagnostic_storage() {
    let bytes = mesh_bytes(&[[0.0; 3]], &[[0, 0, 0]], [0.0; 6]);
    let mut property = resource_test_property();
    property.id = "invalid identity".into();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("context");
    let error = parse_mesh(&ctx, &property, &bytes).expect_err("invalid identity");
    let CodecError::Malformed(message) = error else {
        panic!("identity diagnostic")
    };
    assert_eq!(message, "identity is invalid: \"invalid identity:mesh\"");
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(u64::MAX, "escaping mesh diagnostic probe") else {
        panic!("retained overflow probe")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.used, u64_from_index(message.len()));
    assert_eq!(message, "identity is invalid: \"invalid identity:mesh\"");
}

#[test]
fn accepted_mesh_keeps_population_identity_and_source_storage() {
    let bytes = mesh_bytes(&[[1.0, 2.0, 3.0]], &[[0, 0, 0]], [0.0; 6]);
    let property = resource_test_property();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("context");
    let mesh = parse_mesh(&ctx, &property, &bytes).expect("mesh");
    let TessellationMesh::List { vertices, triangles } = mesh.mesh() else {
        panic!("list mesh")
    };
    assert_eq!(vertices.len(), 1);
    assert_eq!(vertices[0].get(), Point3::new(1.0, 2.0, 3.0));
    assert_eq!(triangles, &[[0, 0, 0]]);
    assert_eq!(mesh.id.as_str(), "fcstd:native:property#Geometry:mesh");
    let source = mesh.source_object.as_ref().expect("source");
    assert_eq!(source.object_id.as_str(), property.owner);
    assert_eq!(source.name.as_deref(), Some("Geometry"));
    let live = vertices.capacity() * std::mem::size_of::<FinitePoint3>()
        + triangles.capacity() * std::mem::size_of::<[u32; 3]>()
        + "fcstd:native:property#Geometry:mesh".len()
        + property.owner.len() + property.name.len();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(u64::MAX, "accepted mesh storage probe") else {
        panic!("retained overflow probe")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.used, u64_from_index(live));
    assert_eq!(vertices[0].get(), Point3::new(1.0, 2.0, 3.0));
}

#[test]
fn mesh_source_refusal_preserves_original_limit_after_candidate_rejection() {
    let bytes = mesh_bytes(&[[0.0; 3]], &[[0, 0, 0]], [0.0; 6]);
    let property = resource_test_property();
    let retained = u64_from_index(std::mem::size_of::<FinitePoint3>()
        + std::mem::size_of::<[u32; 3]>() + property.id.len() + ":mesh".len());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let Err(CodecError::ResourceLimit(original)) = parse_mesh(&ctx, &property, &bytes) else {
        panic!("source storage refusal")
    };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(original.operation, "FreeCAD geometry object identity");
    assert_eq!((original.limit, original.used, original.additional),
        (retained, retained, u64_from_index(property.owner.len())));
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.charge_retained(0, "later mesh publication"),
        Err(CodecError::ResourceLimit(repeated)) if repeated == original));
}
