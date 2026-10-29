// SPDX-License-Identifier: Apache-2.0
use super::project;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn inherited_caller_refusal_precedes_projection_scope_shortcuts() {
    let ir = CadIr::empty();
    let before = serde_json::to_value(&ir).unwrap();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
    let mut working = Vec::<u64>::new();
    let CodecError::ResourceLimit(refusal) = ctx
        .reserve_working(&mut working, usize::MAX, "native projection prior refusal")
        .unwrap_err()
    else {
        panic!("working capacity overflow must latch a resource refusal")
    };
    assert!(matches!(
        project(&ctx, &ir),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    assert!(working.is_empty());
    assert_eq!(serde_json::to_value(&ir).unwrap(), before);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
}

#[test]
fn typed_native_geometry_projects_under_the_actual_source_free_encode_context() {
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
    ));
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.ir().model.bodies.is_empty());
    let expected = decoded.ir().model.bodies.len();
    let mut wire = serde_json::to_value(decoded.ir()).unwrap();
    for arena in cadmpeg_ir::document::ArenaName::ALL {
        wire["model"][arena.as_str()] = serde_json::json!([]);
    }
    wire["scope"] = serde_json::to_value(cadmpeg_ir::ContentScope::container()).unwrap();
    let ir = serde_json::from_value(wire).unwrap();
    let before = serde_json::to_value(&ir).unwrap();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
    assert!(ctx.is_encoding());
    let (projected, _) = project(&ctx, &ir).unwrap().unwrap();
    assert_eq!(projected.model.bodies.len(), expected);
    assert_eq!(serde_json::to_value(&ir).unwrap(), before);
    assert!(ctx.is_encoding());
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn source_free_reprojection_preserves_existing_opaque_source_use_identities() {
    use crate::native::source_use::SourceUseSelector;
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
    ));
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let mut rows = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<SourceUseSelector>("topology_uses")
        .unwrap();
    assert!(!rows.is_empty());
    use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    let original = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (row.id.clone(), format!("opaque:native:use#{index}")))
        .collect::<std::collections::HashMap<_, _>>();
    let mut identities =
        IdentityMap::new(|id: &str| Ok(original.get(id).cloned().unwrap_or_else(|| id.to_owned())));
    rows = rows
        .into_iter()
        .map(|row| {
            row.rewrite_identities(
                &cadmpeg_test_support::service_decode_context(),
                &mut identities,
            )
            .unwrap()
        })
        .collect();
    identities.finish().unwrap();
    let expected = rows
        .iter()
        .map(|row| row.id.clone())
        .collect::<std::collections::HashSet<_>>();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "topology_uses",
            &rows,
        )
        .unwrap();
    let mut wire = serde_json::to_value(&ir).unwrap();
    for arena in cadmpeg_ir::document::ArenaName::ALL {
        wire["model"][arena.as_str()] = serde_json::json!([]);
    }
    wire["scope"] = serde_json::to_value(cadmpeg_ir::ContentScope::container()).unwrap();
    let ir: CadIr = serde_json::from_value(wire).unwrap();
    let before = serde_json::to_value(&ir).unwrap();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
    let (projected, _) = project(&ctx, &ir).unwrap().unwrap();
    let actual = projected
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<SourceUseSelector>("topology_uses")
        .unwrap();
    assert_eq!(
        actual
            .iter()
            .map(|row| row.id.clone())
            .collect::<std::collections::HashSet<_>>(),
        expected
    );
    let references = projected
        .model
        .bodies
        .iter()
        .map(|row| row.native_ref.as_deref())
        .chain(
            projected
                .model
                .regions
                .iter()
                .map(|row| row.native_ref.as_deref()),
        )
        .chain(
            projected
                .model
                .shells
                .iter()
                .map(|row| row.native_ref.as_deref()),
        )
        .chain(
            projected
                .model
                .faces
                .iter()
                .map(|row| row.native_ref.as_deref()),
        )
        .chain(
            projected
                .model
                .loops
                .iter()
                .map(|row| row.native_ref.as_deref()),
        )
        .chain(
            projected
                .model
                .edges
                .iter()
                .map(|row| row.native_ref.as_deref()),
        )
        .chain(
            projected
                .model
                .vertices
                .iter()
                .map(|row| row.native_ref.as_deref()),
        );
    assert!(references
        .into_iter()
        .all(|native| native.is_some_and(|id| expected.contains(id))));
    assert_eq!(serde_json::to_value(&ir).unwrap(), before);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn temporary_projection_preserves_missing_selector_subsets_and_absent_arena() {
    use crate::native::source_use::{SourceUse, SourceUseSelector};
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
    ));
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    for absent in [false, true] {
        let mut ir = decoded.ir().clone();
        let namespace = ir.native.namespace_mut("fcstd");
        let rows = namespace
            .arena_as::<SourceUseSelector>("topology_uses")
            .unwrap();
        assert!(rows
            .iter()
            .any(|row| matches!(row.source_use, SourceUse::Child { .. })));
        if absent {
            namespace.arenas_mut().remove("topology_uses");
        } else {
            let roots = rows
                .into_iter()
                .filter(|row| matches!(row.source_use, SourceUse::Root { .. }))
                .collect::<Vec<_>>();
            assert!(!roots.is_empty());
            namespace
                .set_arena(
                    &cadmpeg_test_support::service_decode_context(),
                    "topology_uses",
                    &roots,
                )
                .unwrap();
        }
        let expected = serde_json::to_value(ir.native.namespace("fcstd").unwrap()).unwrap();
        let mut wire = serde_json::to_value(&ir).unwrap();
        for arena in cadmpeg_ir::document::ArenaName::ALL {
            wire["model"][arena.as_str()] = serde_json::json!([]);
        }
        wire["scope"] = serde_json::to_value(cadmpeg_ir::ContentScope::container()).unwrap();
        let ir: CadIr = serde_json::from_value(wire).unwrap();
        let arena = DecodeArena::new();
        let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
        let (projected, _) = project(&ctx, &ir).unwrap().unwrap();
        assert!(!projected.model.bodies.is_empty());
        assert_eq!(
            serde_json::to_value(projected.native.namespace("fcstd").unwrap()).unwrap(),
            expected
        );
        assert!(projected
            .model
            .vertices
            .iter()
            .any(|row| row.native_ref.is_none()));
        if absent {
            assert!(projected
                .model
                .bodies
                .iter()
                .all(|row| row.native_ref.is_none()));
        }
        assert_eq!(ctx.resource_refusal(), None);
        ctx.finish_session().unwrap();
    }
}
