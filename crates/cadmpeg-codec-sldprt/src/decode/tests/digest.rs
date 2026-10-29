// SPDX-License-Identifier: Apache-2.0
//! Decoder digest state-restoration tests.

use crate::decode::brep_local_sha256_in_place;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, BodyId, FaceId};
use cadmpeg_ir::topology::{Body, BodyKind};
use std::collections::BTreeMap;

fn named_body_document() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        native_ref: None,
        id: BodyId::mint("synthetic:test:id#digest-body").unwrap(),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: Some("digest body".to_owned()),
        color: None,
        visible: None,
    });
    ir
}

fn appearance(id: &str) -> Appearance {
    Appearance {
        native_ref: None,
        id: AppearanceId::mint(id).unwrap(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: BTreeMap::new(),
        textures: Vec::new(),
    }
}

fn binding(id: &str, target: AppearanceTarget, appearance: &str) -> AppearanceBinding {
    AppearanceBinding {
        native_ref: None,
        id: AppearanceBindingId::mint(id).unwrap(),
        target,
        appearance: AppearanceId::mint(appearance).unwrap(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: BTreeMap::new(),
    }
}

#[test]
fn digest_restores_body_display_fields() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"digest", &arena, &DecodePolicy::service()).unwrap();
    let mut ir = named_body_document();
    let original = ir.model.bodies[0].clone();
    brep_local_sha256_in_place(&ctx, &mut ir).unwrap();
    assert_eq!(ir.model.bodies[0], original);
}

#[test]
fn digest_hash_refusal_restores_the_moved_model() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"digest", &arena, &policy).unwrap();
    let mut ir = named_body_document();
    ir.model
        .appearances
        .push(appearance("synthetic:test:id#face-appearance"));
    ir.model.appearance_bindings.push(binding(
        "synthetic:test:id#face-binding",
        AppearanceTarget::Face(FaceId::mint("synthetic:test:id#digest-face").unwrap()),
        "synthetic:test:id#face-appearance",
    ));
    let original = ir.clone();
    let CodecError::ResourceLimit(limit) = brep_local_sha256_in_place(&ctx, &mut ir).unwrap_err()
    else {
        panic!("digest must refuse recursion");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RecursionDepth
    );
    assert_eq!(ir, original);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
    );
}

#[test]
fn digest_restores_appearance_order() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"digest", &arena, &DecodePolicy::service()).unwrap();
    let mut ir = CadIr::empty();
    ir.model
        .appearances
        .push(appearance("synthetic:test:id#body-appearance"));
    ir.model
        .appearances
        .push(appearance("synthetic:test:id#face-appearance"));
    ir.model.appearance_bindings.push(binding(
        "synthetic:test:id#body-binding",
        AppearanceTarget::Body(BodyId::mint("synthetic:test:id#digest-body").unwrap()),
        "synthetic:test:id#body-appearance",
    ));
    ir.model.appearance_bindings.push(binding(
        "synthetic:test:id#face-binding",
        AppearanceTarget::Face(FaceId::mint("synthetic:test:id#digest-face").unwrap()),
        "synthetic:test:id#face-appearance",
    ));
    let appearances = ir.model.appearances.clone();
    let bindings = ir.model.appearance_bindings.clone();
    let expected_hash = crate::decode::brep_local_sha256(&ir).unwrap();
    let actual_hash = brep_local_sha256_in_place(&ctx, &mut ir).unwrap();
    assert_eq!(actual_hash, expected_hash);
    assert_eq!(ir.model.appearances, appearances);
    assert_eq!(ir.model.appearance_bindings, bindings);
}
