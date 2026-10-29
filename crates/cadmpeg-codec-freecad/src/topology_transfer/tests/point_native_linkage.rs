// SPDX-License-Identifier: Apache-2.0

use super::super::{ArenaPositions, Builder, Located};
use crate::brep::{
    ShapePayloadRecord, Tables, TextOrientation, TextShapeUse, TextTShape, TextTShapeGeometry,
    TextTShapes,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::native::NativeNamespace;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::CadIr;

fn use_shape(location: usize) -> TextShapeUse {
    TextShapeUse {
        shape: 1,
        location: location.into(),
        orientation: TextOrientation::Forward,
    }
}

fn payload() -> ShapePayloadRecord {
    let shapes = TextTShapes::from(vec![TextTShape {
        geometry: TextTShapeGeometry::Vertex {
            tolerance: FiniteReal::ZERO,
            point: FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap(),
            representations: Vec::new(),
        },
        flags: [false; 7],
        children: Vec::new(),
    }]);
    serde_json::from_value(serde_json::json!({
        "id": "fcstd:native:shape#PointOwner", "property": "fcstd:native:property#PointOwner:Shape",
        "entry": "fcstd:native:entry#PointOwner", "form": "binary", "text": null,
        "binary": { "topology_version": 1,
            "locations": [{"factors": [], "transform": super::translation(5.0, 0.0, 0.0)}],
            "curve2ds": [], "curves": [], "surfaces": [], "polygons3d": [],
            "polygons_on_triangulations": [], "triangulations": [], "tshapes": shapes,
            "roots": [use_shape(0), use_shape(1)] }
    }))
    .unwrap()
}

fn owner() -> cadmpeg_core::text::NonBlankString {
    cadmpeg_core::text::NonBlankString::new("PointOwner").unwrap()
}

fn namespace(ctx: &DecodeContext<'_>, payload: &ShapePayloadRecord) -> NativeNamespace {
    let mut positions = ArenaPositions::default();
    let builder = Builder::new(
        ctx,
        payload,
        Tables::from_payload(payload).unwrap(),
        owner(),
        &mut positions,
    )
    .unwrap();
    let mut namespace = NativeNamespace::default();
    namespace.arenas_mut().insert(
        "topology_uses".to_owned(),
        builder.source_uses.into_records().unwrap().unwrap(),
    );
    namespace
        .set_arena(ctx, "shape_payloads", &[payload])
        .unwrap();
    namespace
}

fn existing(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
    payload: &ShapePayloadRecord,
) -> Vec<(usize, String)> {
    super::super::source_uses::resolve_namespace(ctx, namespace, std::slice::from_ref(payload))
        .unwrap()
        .into_iter()
        .map(|row| (row.node, row.id))
        .collect()
}

#[test]
fn projected_points_keep_the_exact_vertex_source_use_and_source_facts() {
    let payload = payload();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let native_before = namespace(&ctx, &payload);
    let mut positions = ArenaPositions::default();
    let mut builder = Builder::new(
        &ctx,
        &payload,
        Tables::from_payload(&payload).unwrap(),
        owner(),
        &mut positions,
    )
    .unwrap();
    let expected = builder
        .source_uses
        .selector(1, 1, Located::identity())
        .unwrap()
        .to_owned();
    let mut ir = CadIr::empty();
    let vertex = builder
        .ensure_vertex(&mut ir, &use_shape(0), Located::identity())
        .unwrap();
    assert_eq!(ir.model.points.len(), 1);
    assert_eq!(ir.model.vertices.len(), 1);
    assert_eq!(ir.model.vertices[0].id, vertex);
    assert_eq!(ir.model.vertices[0].point, ir.model.points[0].id);
    assert_eq!(
        ir.model.points[0].native_ref.as_deref(),
        Some(expected.as_str())
    );
    assert_eq!(
        ir.model.vertices[0].native_ref,
        ir.model.points[0].native_ref
    );
    assert_eq!(
        ir.model.points[0].position(),
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap()
    );
    let mut native_after = NativeNamespace::default();
    native_after.arenas_mut().insert(
        "topology_uses".to_owned(),
        builder.source_uses.into_records().unwrap().unwrap(),
    );
    native_after
        .set_arena(&ctx, "shape_payloads", &[&payload])
        .unwrap();
    assert_eq!(native_after, native_before);
}

#[test]
fn placed_point_roots_keep_distinct_actual_source_use_owners() {
    let payload = payload();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut positions = ArenaPositions::default();
    let mut builder = Builder::new(
        &ctx,
        &payload,
        Tables::from_payload(&payload).unwrap(),
        owner(),
        &mut positions,
    )
    .unwrap();
    let mut ir = CadIr::empty();
    for root in 1..=2 {
        builder.root_discriminator = Some(root);
        let local = builder.location(root - 1).unwrap();
        let expected = builder
            .source_uses
            .selector(root, 1, local)
            .unwrap()
            .to_owned();
        builder
            .ensure_vertex(&mut ir, &use_shape(root - 1), Located::identity())
            .unwrap();
        assert_eq!(
            ir.model.points[root - 1].native_ref.as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(
            ir.model.points[root - 1].native_ref,
            ir.model.vertices[root - 1].native_ref
        );
    }
    assert_ne!(ir.model.points[0].native_ref, ir.model.points[1].native_ref);
    assert_eq!(
        ir.model.points[0].position(),
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap()
    );
    assert_eq!(
        ir.model.points[1].position(),
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(6.0, 2.0, 3.0)).unwrap()
    );
}

#[test]
fn current_point_projection_keeps_opaque_native_ids_after_arena_reordering() {
    let payload = payload();
    let setup = cadmpeg_test_support::service_decode_context();
    let mut namespace = namespace(&setup, &payload);
    let mut rows = namespace
        .arena_as::<crate::native::source_use::SourceUseSelector>("topology_uses")
        .unwrap();
    for row in &mut rows {
        let crate::native::source_use::SourceUse::Root { root } = row.source_use else {
            panic!("fixture has root uses only")
        };
        row.id = match root {
            1 => "opaque:native:point-use#origin",
            2 => "opaque:native:point-use#translated",
            _ => panic!("fixture has two roots"),
        }
        .to_owned();
    }
    rows.reverse();
    namespace.set_arena(&setup, "topology_uses", &rows).unwrap();
    let before = namespace.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let rows = existing(&ctx, &namespace, &payload);
    let mut positions = ArenaPositions::default();
    let mut builder = Builder::new_with_identities(
        &ctx,
        &payload,
        Tables::from_payload(&payload).unwrap(),
        owner(),
        Some(&rows),
        &mut positions,
    )
    .unwrap();
    let mut ir = CadIr::empty();
    for root in 1..=2 {
        builder.root_discriminator = Some(root);
        builder
            .ensure_vertex(&mut ir, &use_shape(root - 1), Located::identity())
            .unwrap();
    }
    assert_eq!(
        ir.model.points[0].native_ref.as_deref(),
        Some("opaque:native:point-use#origin")
    );
    assert_eq!(
        ir.model.points[1].native_ref.as_deref(),
        Some("opaque:native:point-use#translated")
    );
    assert!(ir
        .model
        .points
        .iter()
        .zip(&ir.model.vertices)
        .all(|(point, vertex)| point.native_ref == vertex.native_ref));
    assert_eq!(namespace, before);
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn missing_actual_point_selector_stays_absent_without_borrowing_a_sibling() {
    let payload = payload();
    let setup = cadmpeg_test_support::service_decode_context();
    let namespace = namespace(&setup, &payload);
    let mut rows = existing(&setup, &namespace, &payload);
    let missing = rows.remove(0).1;
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut positions = ArenaPositions::default();
    let mut builder = Builder::new_with_identities(
        &ctx,
        &payload,
        Tables::from_payload(&payload).unwrap(),
        owner(),
        Some(&rows),
        &mut positions,
    )
    .unwrap();
    let mut ir = CadIr::empty();
    let absent_root = (1..=2)
        .find(|root| {
            let placed = builder.location(root - 1).unwrap();
            builder.source_uses.selector(*root, 1, placed).is_none()
        })
        .unwrap();
    builder.root_discriminator = Some(absent_root);
    builder
        .ensure_vertex(&mut ir, &use_shape(absent_root - 1), Located::identity())
        .unwrap();
    assert_eq!(ir.model.points[0].native_ref, None);
    assert_eq!(ir.model.vertices[0].native_ref, None);
    assert!(rows.iter().all(|(_, id)| id != &missing));
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn fresh_point_and_vertex_references_admit_two_owned_copies_before_append() {
    for short in [false, true] {
        let payload = payload();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut positions = ArenaPositions::default();
        let mut builder = Builder::new(
            &ctx,
            &payload,
            Tables::from_payload(&payload).unwrap(),
            owner(),
            &mut positions,
        )
        .unwrap();
        let reference = builder
            .source_uses
            .selector(1, 1, Located::identity())
            .unwrap();
        let copies = 2 * cadmpeg_core::decode::u64_from_index(reference.len());
        let headroom = copies - u64::from(short);
        ctx.retain_bytes(
            ctx.retained_bytes_available() - headroom,
            "point fixture retained headroom",
        )
        .unwrap();
        let mut ir = CadIr::empty();
        let result = builder.ensure_vertex(&mut ir, &use_shape(0), Located::identity());
        if short {
            assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
            assert!(ir.model.points.is_empty());
            assert!(ir.model.vertices.is_empty());
        } else {
            result.unwrap();
            assert_eq!(ir.model.points.len(), 1);
            assert_eq!(
                ir.model.points[0].native_ref,
                ir.model.vertices[0].native_ref
            );
            assert_eq!(ctx.retained_bytes_available(), 0);
            assert!(ctx.resource_refusal().is_none());
        }
    }
}

#[test]
fn prior_actual_context_refusal_propagates_even_when_point_selector_is_missing() {
    let payload = payload();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let rows = [];
    let mut positions = ArenaPositions::default();
    let builder = Builder::new_with_identities(
        &ctx,
        &payload,
        Tables::from_payload(&payload).unwrap(),
        owner(),
        Some(&rows),
        &mut positions,
    )
    .unwrap();
    assert_eq!(builder.source_ref(1, Located::identity()).unwrap(), None);
    let Err(CodecError::ResourceLimit(refusal)) = ctx.retain_bytes(
        ctx.retained_bytes_available() + 1,
        "point fixture prior refusal",
    ) else {
        panic!("fixture must refuse")
    };
    assert!(
        matches!(builder.source_ref(1, Located::identity()), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}
