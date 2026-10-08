// SPDX-License-Identifier: Apache-2.0
//! Connectivity keys expire before the component output does.

use crate::brep::{
    ShapePayload, ShapePayloadRecord, Tables, TextOrientation, TextShapeKind,
    TextShapeUse, TextTShape, TextTShapes,
};
use crate::native::element_map::ScopedData;
use crate::topology_transfer::{Builder, GeometryIndexes, ScopedVec};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::transform::Transform;

const MATERIALIZED_CAP: u64 = 4096;
// reserve_vec uses four slots for both usize and Vec<usize> at first growth.
const COMPONENT_BYTES: u64 = cadmpeg_core::decode::u64_from_index(
    4 * std::mem::size_of::<usize>() + 4 * std::mem::size_of::<Vec<usize>>(),
);

fn shape_use(shape: usize) -> TextShapeUse {
    TextShapeUse {
        shape,
        orientation: TextOrientation::Forward,
        location: 0.into(),
    }
}

fn connectivity_shapes() -> TextTShapes {
    TextTShapes::from(
        [TextShapeKind::Face, TextShapeKind::Wire, TextShapeKind::Edge, TextShapeKind::Vertex]
            .into_iter()
            .enumerate()
            .map(|(index, kind)| TextTShape {
                geometry: super::geometry_for_kind(kind),
                flags: [false; 7],
                children: if index < 3 { vec![shape_use(index + 2)] } else { Vec::new() },
            })
            .collect::<Vec<_>>(),
    )
}

fn builder<'a, 'c, 'r, 'occ>(
    ctx: &'c DecodeContext<'r>,
    payload: &'a ShapePayloadRecord,
    shapes: &'a TextTShapes,
) -> Builder<'a, 'c, 'r, 'occ> {
    let tables = Tables {
        locations: &[], curve2ds: &[], curves: &[], surfaces: &[],
        polygons3d: &[], polygons_on_triangulations: &[], triangulations: &[],
        tshapes: shapes, roots: &[],
    };
    Builder::new(
        ctx, payload, tables,
        ScopedData {
            data: cadmpeg_core::text::NonBlankString::try_from("Object").unwrap(),
            _storage: ctx.reserve_scoped(0, "test source object").unwrap(),
        },
        GeometryIndexes::new(ctx).unwrap(), None,
    ).unwrap()
}

fn payload() -> ShapePayloadRecord {
    ShapePayloadRecord {
        id: "fcstd:native:entry#Connectivity".into(),
        property: "fcstd:native:property#Connectivity".into(),
        entry: "Shape.brp".into(), payload: ShapePayload::Empty,
    }
}

#[test]
fn face_connectivity_storage_releases_keys_before_components() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = MATERIALIZED_CAP;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let shapes = connectivity_shapes();
    let payload = payload();
    let builder = builder(&ctx, &payload, &shapes);
    let face = shape_use(1);
    let (values, storage) = ctx.with_scoped_storage("test component output", || {
        builder.face_components(&ctx, &[&face], Transform::identity())
    }).unwrap();
    let result = ScopedVec { values, _storage: storage };
    assert_eq!(result.values, vec![vec![0]]);
    assert_eq!(result.values.capacity(), 4);
    assert_eq!(result.values[0].capacity(), 4);
    let remaining = ctx.reserve_scoped(MATERIALIZED_CAP - COMPONENT_BYTES,
        "test remaining component capacity").expect("only components survive");
    drop(remaining);
    drop(result);
    let entire = ctx.reserve_scoped(MATERIALIZED_CAP,
        "test released component capacity").expect("all component backing released");
    drop(entire);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn face_connectivity_storage_refusal_preserves_surviving_bytes_and_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = MATERIALIZED_CAP;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let shapes = connectivity_shapes();
    let payload = payload();
    let builder = builder(&ctx, &payload, &shapes);
    let face = shape_use(1);
    let (values, storage) = ctx.with_scoped_storage("test component output", || {
        builder.face_components(&ctx, &[&face], Transform::identity())
    }).unwrap();
    let result = ScopedVec { values, _storage: storage };
    assert_eq!(result.values, vec![vec![0]]);
    let Err(CodecError::ResourceLimit(original)) = ctx.reserve_scoped(
        MATERIALIZED_CAP - COMPONENT_BYTES + 1, "test excess component capacity",
    ) else { panic!("one byte above surviving capacity must refuse") };
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((original.used, original.additional, original.limit),
        (COMPONENT_BYTES, MATERIALIZED_CAP - COMPONENT_BYTES + 1, MATERIALIZED_CAP));
    for uses in [&[&face][..], &[][..]] {
        assert!(matches!(builder.face_components(&ctx, uses, Transform::identity()),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    drop(result);
    assert_eq!(ctx.resource_refusal(), Some(original));
}
