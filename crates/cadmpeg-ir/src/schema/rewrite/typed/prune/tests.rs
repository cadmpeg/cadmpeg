// SPDX-License-Identifier: Apache-2.0
use super::{analyze, DependencyPosition, ReferencePruneEffect};
use crate::ids::{BodyId, Identity};
use crate::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Object {
    target: Identity,
    label: String,
}
rewrite_record!(Object, []; {target, label});

fn deleted() -> BTreeSet<String> {
    ["test:native:record#gone".into()].into()
}
fn id(text: &str) -> Identity {
    text.try_into().expect("admitted identity")
}

#[test]
fn direct_reference_vec_prunes_entries_and_preserves_exact_order_duplicates_and_labels() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"test", &arena, &DecodePolicy::service()).expect("context");
    let mut values = vec![
        id("test:native:record#kept-b"),
        id("test:native:record#gone"),
        id("test:native:record#kept-a"),
        id("test:native:record#kept-b"),
    ];
    assert_eq!(
        analyze(&values, &ctx, &deleted()).expect("analysis"),
        ReferencePruneEffect::ListsChanged
    );
    assert_eq!(
        values
            .prune_reference_lists(&ctx, &deleted())
            .expect("typed edit"),
        ReferencePruneEffect::ListsChanged
    );
    assert_eq!(
        values,
        [
            id("test:native:record#kept-b"),
            id("test:native:record#kept-a"),
            id("test:native:record#kept-b")
        ]
    );
    let mut labels = vec!["test:native:record#gone".to_owned()];
    assert_eq!(
        labels
            .prune_reference_lists(&ctx, &deleted())
            .expect("text is not a reference"),
        ReferencePruneEffect::Unchanged
    );
    assert_eq!(labels, ["test:native:record#gone"]);
}

#[test]
fn option_reference_is_required_and_never_cleared_to_none() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"test", &arena, &DecodePolicy::service()).expect("context");
    let mut value = Some(id("test:native:record#gone"));
    assert_eq!(
        value
            .prune_reference_lists(&ctx, &deleted())
            .expect("required result"),
        ReferencePruneEffect::DropRequired
    );
    assert_eq!(value, Some(id("test:native:record#gone")));
    let mut values = vec![None, value.clone(), Some(id("test:native:record#kept"))];
    assert_eq!(
        values
            .prune_reference_lists(&ctx, &deleted())
            .expect("optional reference list"),
        ReferencePruneEffect::ListsChanged
    );
    assert_eq!(values, [None, Some(id("test:native:record#kept"))]);
}

#[test]
fn object_fields_remain_required_inside_or_outside_a_vec() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"test", &arena, &DecodePolicy::service()).expect("context");
    let object = Object {
        target: id("test:native:record#gone"),
        label: "test:native:record#gone".into(),
    };
    assert_eq!(
        analyze(&object, &ctx, &deleted()).expect("object"),
        ReferencePruneEffect::DropRequired
    );
    let mut values = vec![
        object.clone(),
        Object {
            target: id("test:native:record#kept"),
            label: "keep me".into(),
        },
    ];
    let original = values.clone();
    assert_eq!(
        values
            .prune_reference_lists(&ctx, &deleted())
            .expect("nested object is required"),
        ReferencePruneEffect::DropRequired
    );
    assert_eq!(values, original);
    let mut fields = Vec::new();
    object
        .visit_dependency_references(&ctx, &mut |reference| {
            fields.push((
                reference.target.to_owned(),
                reference.position,
                reference.field,
            ));
            Ok(())
        })
        .expect("typed fields");
    assert_eq!(
        fields,
        [(
            "test:native:record#gone".into(),
            DependencyPosition::Required,
            Some("target")
        )]
    );
}

#[test]
fn closed_reference_enum_is_transparent_to_direct_array_position() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"test", &arena, &DecodePolicy::service()).expect("context");
    let gone = BodyId::try_from("test:native:record#gone").expect("body ID");
    let kept = BodyId::try_from("test:native:record#kept").expect("body ID");
    let mut values = vec![
        crate::attributes::AttributeTarget::Body(gone),
        crate::attributes::AttributeTarget::Body(kept.clone()),
    ];
    assert_eq!(
        values
            .prune_reference_lists(&ctx, &deleted())
            .expect("reference enum list"),
        ReferencePruneEffect::ListsChanged
    );
    assert_eq!(values, [crate::attributes::AttributeTarget::Body(kept)]);
    let local = crate::references::ReferenceTarget::Local("test:native:record#gone".into());
    assert_eq!(
        analyze(&local, &ctx, &deleted()).expect("declared local string reference"),
        ReferencePruneEffect::DropRequired
    );
}

#[test]
fn fixed_reference_array_defers_without_retaining_or_dropping_a_record_as_a_fallback() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"test", &arena, &DecodePolicy::service()).expect("context");
    let mut values = [id("test:native:record#gone"), id("test:native:record#kept")];
    let before = values.clone();
    let effect = values
        .prune_reference_lists(&ctx, &deleted())
        .expect("typed deferred result");
    assert!(matches!(
        effect,
        ReferencePruneEffect::UnrepresentablePrunedList { .. }
    ));
    assert_eq!(values, before);
}

#[test]
fn declared_native_selection_reference_is_required_but_an_ordinary_selection_is_not_guessed() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::service())
        .expect("context");
    let reference = crate::features::SelectionReference::new("test:native:record#gone".into())
        .expect("nonblank reference");
    let placement = crate::features::CoilPlacement::Native {
        native_ref: reference.clone(),
    };
    assert_eq!(
        analyze(&placement, &ctx, &deleted()).expect("declared native field"),
        ReferencePruneEffect::DropRequired
    );
    assert_eq!(
        analyze(&reference, &ctx, &deleted()).expect("ordinary selection owner"),
        ReferencePruneEffect::Unchanged
    );
}
