// SPDX-License-Identifier: Apache-2.0
//! Fallible presentation projections admit each executed source visit.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::wire::Uuid;

fn with_one_visit<T, R>(f: impl FnOnce(DecodeContext<'_>, u64) -> R) -> R {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One source visit plus 36 UUID bytes in formatted_length; storage refuses before formatting the output.
    policy.limits.max_work_units = 1 + 36;
    // collection_vec uses exact growth from an empty vector before projection.
    let backing = u64::try_from(1024 * std::mem::size_of::<T>()).unwrap();
    policy.limits.max_retained_bytes = backing;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    f(ctx, backing)
}

fn assert_first_text_refusal(ctx: DecodeContext<'_>, error: CodecError, operation: &'static str, backing: u64) {
    let CodecError::ResourceLimit(refusal) = error else { panic!("expected text refusal"); };
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.operation, operation);
    assert_eq!(refusal.used, backing);
    assert_eq!(refusal.additional, 36);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn displacement_projection_admits_only_the_first_sub_item_before_text_refusal() {
    let mut value = super::displacement_with_sub_item();
    value.sub_items[0].texture = Some(Uuid::from_canonical([1; 16]));
    value.sub_items = vec![value.sub_items[0].clone(); 1024];
    with_one_visit::<crate::presentation::DisplacementSubItemRecord, _>(|ctx, backing| {
        let error = crate::presentation::displacement_record(&ctx, &value).unwrap_err();
        assert_first_text_refusal(ctx, error, "Rhino projected displacement sub-item texture UUID", backing);
    });
}

#[test]
fn shut_lining_projection_admits_only_the_first_curve_before_text_refusal() {
    let value = crate::mesh_modifiers::ShutLiningModifier {
        xml_version: 2,
        on: true,
        options: crate::mesh_modifiers::ShutLiningOptions {
            faceted: false,
            auto_update: false,
            force_update: false,
        },
        curves: vec![crate::mesh_modifiers::ShutLiningCurve {
            uuid: Some(Uuid::from_canonical([1; 16])),
            radius: crate::test_support::finite(1.0),
            profile: 0,
            enabled: true,
            pull: false,
            is_bump: false,
        }; 1024],
    };
    with_one_visit::<crate::presentation::ShutLiningCurveRecord, _>(|ctx, backing| {
        let error = crate::presentation::shut_lining_record(&ctx, &value).unwrap_err();
        assert_first_text_refusal(ctx, error, "Rhino projected shut-lining curve UUID", backing);
    });
}

fn projected_attributes_error(ctx: &DecodeContext<'_>, attributes: &crate::objects::ObjectAttributes) -> CodecError {
    crate::presentation::object_attributes_presentation(
        ctx, &[], attributes, &[], &[],
        crate::presentation::ObjectPresentationSource {
            archive: crate::chunks::ArchiveVersion::V8,
            offset: 0,
            uuid: attributes.object_id,
        },
        &mut Vec::new(),
    ).unwrap_err()
}

#[test]
fn display_material_projection_admits_only_the_first_pair_before_text_refusal() {
    let mut attributes = super::presentation_attributes();
    attributes.display_materials = vec![(Uuid::nil(), Uuid::nil()); 1024];
    with_one_visit::<[String; 2], _>(|ctx, backing| {
        let error = projected_attributes_error(&ctx, &attributes);
        assert_first_text_refusal(ctx, error, "Rhino projected display viewport UUID", backing);
    });
}

#[test]
fn clipping_plane_projection_admits_only_the_first_uuid_before_text_refusal() {
    let mut attributes = super::presentation_attributes();
    attributes.clipping_plane_ids = vec![Uuid::nil(); 1024];
    with_one_visit::<String, _>(|ctx, backing| {
        let error = projected_attributes_error(&ctx, &attributes);
        assert_first_text_refusal(ctx, error, "Rhino projected clipping plane UUID text", backing);
    });
}
