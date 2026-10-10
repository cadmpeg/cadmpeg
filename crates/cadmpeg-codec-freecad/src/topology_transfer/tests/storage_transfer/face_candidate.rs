// SPDX-License-Identifier: Apache-2.0
//! A face identity has no retained consumer without a surface or triangulation.

use super::archive;
use crate::brep::{
    ShapePayload, ShapePayloadRecord, Tables, TextOrientation, TextShapeKind, TextShapeUse,
    TextTShape, TextTShapes,
};
use crate::native::element_map::ScopedData;
use crate::topology_transfer::{Builder, GeometryIndexes};
use crate::FcstdCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::ShellId;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

// Both strings reserve exact bytes: topology label1 and the model identity.
const MATERIALIZED_CAP: u64 =
    cadmpeg_core::decode::u64_from_index("1".len() + "fcstd:model:face#EmptyFace:1".len());

fn with_face<T>(
    materialized_cap: u64,
    use_face: impl FnOnce(
        &DecodeContext<'_>,
        &mut Builder<'_, '_, '_, '_>,
        &ShellId,
        &TextShapeUse,
    ) -> T,
) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = materialized_cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let payload = ShapePayloadRecord {
        id: "fcstd:native:entry#EmptyFace".into(),
        property: "fcstd:native:property#EmptyFace".into(),
        entry: "Shape.brp".into(),
        payload: ShapePayload::Empty,
    };
    let shapes = TextTShapes::from(vec![TextTShape {
        geometry: super::super::geometry_for_kind(TextShapeKind::Face),
        flags: [false; 7],
        children: Vec::new(),
    }]);
    let tables = Tables {
        locations: &[],
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        triangulations: &[],
        tshapes: &shapes,
        roots: &[],
    };
    let mut builder = Builder::new(
        &ctx,
        &payload,
        tables,
        ScopedData {
            data: cadmpeg_core::text::NonBlankString::try_from("Object").unwrap(),
            _storage: ctx.reserve_scoped(0, "test source object").unwrap(),
        },
        GeometryIndexes::new(&ctx).unwrap(),
        None,
    )
    .unwrap();
    let face = TextShapeUse {
        shape: 1,
        orientation: TextOrientation::Forward,
        location: 0.into(),
    };
    let shell = ShellId::mint("fcstd:test:shell#1").unwrap();
    use_face(&ctx, &mut builder, &shell, &face)
}

#[test]
fn face_without_geometry_releases_identity_candidate_and_preserves_fuse() {
    with_face(MATERIALIZED_CAP, |ctx, builder, shell, face| {
        let mut ir = CadIr::empty();
        assert_eq!(
            builder
                .append_face(&mut ir, shell, face, Transform::identity(), false)
                .unwrap(),
            None
        );
        assert!(ir.model.faces.is_empty());
        assert!(ir.model.surfaces.is_empty());
        assert!(ir.model.tessellations.is_empty());
        assert!(ir.model.loops.is_empty());
        assert!(ir.model.coedges.is_empty());
        let scratch = ctx
            .reserve_scoped(MATERIALIZED_CAP, "test released face candidate")
            .expect("all candidate backing released");
        drop(scratch);
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) =
            ctx.retained_string(1, "test original face fuse")
        else {
            panic!("zero retained cap must refuse")
        };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(
            (original.used, original.additional, original.limit),
            (0, 1, 0)
        );
        assert!(matches!(builder.append_face(&mut ir, shell, face,
            Transform::identity(), false), Err(CodecError::ResourceLimit(actual))
            if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn face_without_geometry_identity_candidate_refuses_at_exact_scratch_boundary() {
    with_face(MATERIALIZED_CAP - 1, |ctx, builder, shell, face| {
        let mut ir = CadIr::empty();
        let Err(CodecError::ResourceLimit(original)) =
            builder.append_face(&mut ir, shell, face, Transform::identity(), false)
        else {
            panic!("candidate backing must refuse one byte below its peak")
        };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(original.operation, "FreeCAD face identity");
        assert_eq!(
            (original.used, original.additional, original.limit),
            (1, MATERIALIZED_CAP - 1, MATERIALIZED_CAP - 1)
        );
        assert!(ir.model.faces.is_empty());
        assert!(matches!(builder.append_face(&mut ir, shell, face,
            Transform::identity(), false), Err(CodecError::ResourceLimit(actual))
            if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn unrooted_native_face_without_geometry_preserves_facts_without_neutral_face() {
    let bytes = archive(
        b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 1
Fa 0 0 0 0 1001000 *
*",
    );
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.faces.is_empty());
    assert!(result.ir().model.surfaces.is_empty());
    assert!(result.ir().model.tessellations.is_empty());
    let payloads = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ShapePayloadRecord>("shape_payloads")
        .unwrap();
    assert_eq!(payloads.len(), 1);
    let tables = Tables::from_payload(&payloads[0]).unwrap();
    assert_eq!(tables.tshapes.len(), 1);
    assert_eq!(
        tables.tshapes[0].geometry,
        super::super::geometry_for_kind(TextShapeKind::Face)
    );
    assert!(tables.roots.is_empty());
    assert!(result.ir().model.bodies.is_empty());
}
#[test]
fn rooted_face_without_geometry_preserves_empty_shell_rejection() {
    let bytes = archive(
        b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 1
Fa 0 0 0 0 1001000 *
+1 0 *",
    );
    let result = FcstdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default());
    assert!(matches!(result,
        Err(cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(message)))
            if message == cadmpeg_ir::features::BodySelectionError::Empty.to_string()));
}
