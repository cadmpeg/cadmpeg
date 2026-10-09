//! Leaf output receipts transfer only the actual surviving backing.

use super::{
    dissectable_child_sources, sketch_block_assembly_frame, transform_sketch_block_geometry,
    SketchBlockAssemblyFrame, SketchBlockInstancePlacement,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{SketchFontWeight, SketchGeometry, SketchGeometryDefinition};
use cadmpeg_ir::transform::Transform;

// The first u32 HashSet growth uses four buckets, 16-byte alignment and
// 16 trailing control bytes: 4 * sizeof(u32) + 15 + 4 + 16.
const CHILD_BYTES: u64 = 4 * std::mem::size_of::<u32>() as u64 + 15 + 4 + 16;
// Amortized Vec growth starts with four slots for an f64 element.
const ROTATION_BYTES: u64 = 4 * std::mem::size_of::<f64>() as u64;
const TEXT: &str = "label";
const FONT: &str = "font";
const TEXT_BYTES: u64 = (TEXT.len() + FONT.len()) as u64;
const CANDIDATE: &str = "build SLDPRT profile candidate";
const CHILD_SOURCES: &str = "collect SLDPRT dissectable child sources";

fn assert_refusal(
    ctx: &DecodeContext<'_>,
    error: CodecError,
    dimension: ResourceDimension,
    operation: &str,
    used: u64,
    additional: u64,
    limit: u64,
) {
    let CodecError::ResourceLimit(original) = error else {
        panic!("expected original storage refusal");
    };
    assert_eq!(original.dimension, dimension);
    assert_eq!(original.operation, operation);
    assert_eq!(original.used, used);
    assert_eq!(original.additional, additional);
    assert_eq!(original.limit, limit);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&original));
}

fn instance() -> SketchBlockInstancePlacement {
    SketchBlockInstancePlacement {
        feature_id: "synthetic:test:id#instance".into(),
        block_source: 7,
        transform: Transform::identity(),
    }
}

fn text_geometry() -> SketchGeometry {
    SketchGeometry::try_from(SketchGeometryDefinition::Text {
        text: TEXT.try_into().unwrap(),
        font_family: FONT.try_into().unwrap(),
        font_weight: SketchFontWeight::Regular,
        height: Length::new(2.0).unwrap(),
        width_factor: None,
        placement: None,
        horizontal_alignment: None,
        vertical_alignment: None,
    }).unwrap()
}

fn frame() -> SketchBlockAssemblyFrame {
    SketchBlockAssemblyFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    }
}

#[test]
fn profile_child_sources_transfer_actual_set_and_discard_duplicates() {
    for retained in [0, CHILD_BYTES] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = CHILD_BYTES;
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for _ in 0..2 {
            assert!(dissectable_child_sources(&ctx, "7,7").unwrap().is_none());
        }
        let storage;
        let values;
        (values, storage) = ctx.with_scoped_storage(CHILD_SOURCES, || {
            dissectable_child_sources(&ctx, "7,9")
        }).unwrap();
        let values = values.unwrap();
        assert_eq!(values.len(), 2);
        assert!(values.contains(&7) && values.contains(&9));
        drop(values);
        drop(storage);
        drop(ctx.reserve_scoped(CHILD_BYTES, "reuse child source scratch").unwrap());
        if retained != 0 {
            let values = dissectable_child_sources(&ctx, "7,9").unwrap().unwrap();
            assert_eq!(values.len(), 2);
            assert!(values.contains(&7) && values.contains(&9));
        }
    }
}

#[test]
fn profile_child_sources_refuse_exact_storage_limit() {
    for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::MaterializedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = CHILD_BYTES;
        policy.limits.max_retained_bytes = CHILD_BYTES;
        if dimension == ResourceDimension::RetainedBytes {
            policy.limits.max_retained_bytes -= 1;
        } else {
            policy.limits.max_materialized_bytes -= 1;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = dissectable_child_sources(&ctx, "7,9").unwrap_err();
        assert_refusal(&ctx, error, dimension, CHILD_SOURCES, 0, CHILD_BYTES, CHILD_BYTES - 1);
    }
}

#[test]
fn sketch_block_rotation_output_transfers_exact_storage() {
    let instances = [instance()];
    for retained in [0, ROTATION_BYTES] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = ROTATION_BYTES;
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for _ in 0..2 {
            let storage;
            let result;
            (result, storage) = ctx.with_scoped_storage(CANDIDATE, || {
                sketch_block_assembly_frame(&ctx, &instances)
            }).unwrap();
            let (actual, rotations) = result.unwrap();
            assert_eq!(actual.origin, Point3::new(0.0, 0.0, 0.0));
            assert_eq!(actual.normal, Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(actual.u_axis, Vector3::new(1.0, 0.0, 0.0));
            assert_eq!(rotations, [0.0]);
            drop(rotations);
            drop(storage);
        }
        drop(ctx.reserve_scoped(ROTATION_BYTES, "reuse rotation scratch").unwrap());
        if retained != 0 {
            assert_eq!(sketch_block_assembly_frame(&ctx, &instances).unwrap().unwrap().1, [0.0]);
        }
    }
}

#[test]
fn sketch_block_rotation_output_preserves_original_refusal() {
    let instances = [instance()];
    for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::MaterializedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = ROTATION_BYTES;
        policy.limits.max_retained_bytes = ROTATION_BYTES;
        let operation = if dimension == ResourceDimension::RetainedBytes {
            policy.limits.max_retained_bytes -= 1;
            CANDIDATE
        } else {
            policy.limits.max_materialized_bytes -= 1;
            "collect SLDPRT sketch block rotations"
        };
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = sketch_block_assembly_frame(&ctx, &instances).err().unwrap();
        assert_refusal(&ctx, error, dimension, operation, 0, ROTATION_BYTES, ROTATION_BYTES - 1);
    }
}

#[test]
fn sketch_block_text_output_transfers_exact_storage() {
    let source = text_geometry();
    for retained in [0, TEXT_BYTES] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = TEXT_BYTES;
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for _ in 0..2 {
            let storage;
            let result;
            (result, storage) = ctx.with_scoped_storage(CANDIDATE, || {
                transform_sketch_block_geometry(&ctx, &source, Transform::identity(), frame(), 0.0)
            }).unwrap();
            let result = result.unwrap();
            assert_eq!(result, source);
            drop(result);
            drop(storage);
        }
        drop(ctx.reserve_scoped(TEXT_BYTES, "reuse text geometry scratch").unwrap());
        if retained != 0 {
            assert_eq!(transform_sketch_block_geometry(&ctx, &source, Transform::identity(), frame(), 0.0).unwrap().unwrap(), source);
        }
    }
}

#[test]
fn sketch_block_text_output_preserves_original_refusal() {
    let source = text_geometry();
    for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::MaterializedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = TEXT_BYTES;
        policy.limits.max_retained_bytes = TEXT_BYTES;
        let (operation, used, additional) = if dimension == ResourceDimension::RetainedBytes {
            policy.limits.max_retained_bytes -= 1;
            (CANDIDATE, 0, TEXT_BYTES)
        } else {
            policy.limits.max_materialized_bytes -= 1;
            ("transform SLDPRT sketch block geometry", TEXT.len() as u64, FONT.len() as u64)
        };
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = transform_sketch_block_geometry(&ctx, &source, Transform::identity(), frame(), 0.0).unwrap_err();
        assert_refusal(&ctx, error, dimension, operation, used, additional, TEXT_BYTES - 1);
    }
}
