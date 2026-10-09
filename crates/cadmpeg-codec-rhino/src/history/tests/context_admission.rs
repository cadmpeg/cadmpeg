// SPDX-License-Identifier: Apache-2.0
//! Same-session history metadata and embedded geometry admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::{record, ArchiveVersion, Diagnostics, EmbeddedGeometry, MillimeterScale, Uuid, Value};
use super::super::{ProjectionContext, ProjectionError};

fn cloud_source(bytes: &[u8]) -> super::HistoryRecord {
    let mut source = record(3, 4, &[], &[]);
    source.values[0].value = Value::Geometries(vec![EmbeddedGeometry {
        class_id: Uuid::from_wire(crate::test_support::test_archive::POINT_CLOUD_CLASS),
        class_data_range: 0..bytes.len(),
        userdata: Vec::new(),
    }]);
    source
}

#[test]
fn history_geometry_metadata_and_point_slots_use_the_same_collection_limit() {
    let bytes = crate::test_support::test_archive::point_cloud_payload(
        &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    let source = cloud_source(&bytes);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two ID vector slots, one source identity, one value occurrence and
    // two property entries precede the actual two-point allocation.
    policy.limits.max_collection_items = 2 + 1 + 1 + 2;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let context = ProjectionContext::Geometry {
        expand: crate::mesh::MeshExpand::new(&ctx, root),
        archive: ArchiveVersion::V8,
        writer_version: None,
        scale: MillimeterScale::IDENTITY,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut warnings = Diagnostics::new();
    let error = super::super::project(context, &[source], &mut ir, &mut warnings)
        .expect_err("metadata and points share the original budget");
    let ProjectionError::Codec(CodecError::ResourceLimit(limit)) = error else {
        panic!("collection refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "Rhino point-cloud points");
    assert_eq!((limit.used, limit.additional), (6, 2));
    assert!(ir.model.features.is_empty());
    assert!(warnings.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn history_geometry_preserves_coordinates_and_native_identity_in_one_session() {
    let bytes = crate::test_support::test_archive::point_cloud_payload(
        &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    let source = cloud_source(&bytes);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let context = ProjectionContext::Geometry {
        expand: crate::mesh::MeshExpand::new(&ctx, root),
        archive: ArchiveVersion::V8,
        writer_version: None,
        scale: MillimeterScale::IDENTITY,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut warnings = Diagnostics::new();
    assert_eq!(super::super::project(context, &[source], &mut ir, &mut warnings)
        .expect("history projection"), (1, 0, 0, 0));
    assert_eq!(ir.model.features.len(), 1);
    let feature = &ir.model.features[0];
    assert_eq!(feature.id.as_str(), "rhino:history:feature#00000000-0000-0000-0000-000000000003");
    assert_eq!(feature.native_ref.as_deref(),
        Some("rhino:history:record#00000000-0000-0000-0000-000000000003"));
    let points: serde_json::Value = serde_json::from_str(
        &feature.source_properties["value_7.0.geometry"]).expect("point coordinates");
    assert_eq!(points, serde_json::json!([
        {"x":0.0,"y":0.0,"z":0.0}, {"x":1.0,"y":0.0,"z":0.0}
    ]));
    assert!(warnings.is_empty());
    ctx.finish_session().expect("original session remains admitted");
}

#[test]
fn history_geometry_empty_source_keeps_its_original_context_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let original = ctx.charge_work(1, "history geometry original refusal")
        .expect_err("original fuse");
    let context = ProjectionContext::Geometry {
        expand: crate::mesh::MeshExpand::new(&ctx, root),
        archive: ArchiveVersion::V8,
        writer_version: None,
        scale: MillimeterScale::IDENTITY,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let error = super::super::project(context, &[], &mut ir, &mut Diagnostics::new())
        .expect_err("empty projection uses the original geometry session");
    let ProjectionError::Codec(error) = error else { panic!("original refusal"); };
    assert_eq!(error.to_string(), original.to_string());
    assert!(ir.model.features.is_empty());
    assert_eq!(ctx.finish_session().expect_err("original fuse").to_string(), original.to_string());
}
