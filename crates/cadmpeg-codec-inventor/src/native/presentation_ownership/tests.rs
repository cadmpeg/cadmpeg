// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::schema::rewrite::typed::{
    prune::{self, DependencyPosition, ReferencePruneEffect},
    RewriteIdentities,
};
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

use super::super::{
    PmAppDefaultStyleRecord, PmGraphicsFaceRecord, PmGraphicsStyleCollectionRecord,
};
use crate::pmdc::{PmDcPairedReferenceList, PmDcReference};

fn default_style() -> PmAppDefaultStyleRecord {
    PmAppDefaultStyleRecord {
        id: "inventor:presentation:default-style#segment-4".into(),
        segment_token: "segment".into(),
        record_ordinal: 4,
        segment_version_major: 26,
        header_value: 77,
        header_id: 3,
        material_reference: 2,
        rendering_style_reference: 2,
        related_references: [2; 7],
        state: 1,
        terminal_reference: 2,
        suffix_len: 0,
        suffix_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
    }
}

fn face() -> PmGraphicsFaceRecord {
    serde_json::from_value(serde_json::json!({
        "id":"inventor:presentation:graphics-face#segment-7", "segment_token":"segment", "record_ordinal":7, "segment_version_major":26, "header_value":4, "header_id":5, "flags":0,
        "styles_reference":4,"styles_reference_qualified":true,"surface_reference":4,"surface_reference_qualified":false,"parent_reference":4,"parent_reference_qualified":true,"state":0,"edge_references":[4],"edge_reference_qualifiers":[true],"edge_list_metadata":[11,12],"visibility_state":0,"bounds":[1.0,2.0,3.0,4.0,5.0,6.0],"key":42,"values":[7,8]
    })).unwrap()
}

fn collection(references: &[(u32, bool)]) -> PmGraphicsStyleCollectionRecord {
    PmGraphicsStyleCollectionRecord::new(
        "inventor:presentation:graphics-style-collection#segment-3".into(),
        cadmpeg_ir::ids::IdentityKey::try_new("segment".to_owned()).unwrap(),
        3,
        26,
        PmDcPairedReferenceList::new(
            Some([77, 88]),
            references
                .iter()
                .map(|(index, qualifier)| PmDcReference::new(*index, *qualifier).unwrap())
                .collect(),
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn only_proved_same_segment_scalar_roles_are_required_dependencies() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let value = default_style();
    let mut edges = Vec::new();
    value
        .visit_dependency_references(&ctx, &mut |edge| {
            edges.push((edge.target.to_owned(), edge.field, edge.position));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        edges,
        vec![(
            "inventor:presentation:rendering-style#segment-1".to_owned(),
            Some("rendering_style_reference"),
            DependencyPosition::Required
        )]
    );
    let deleted = BTreeSet::from([edges[0].0.clone()]);
    assert_eq!(
        prune::analyze(&value, &ctx, &deleted).unwrap(),
        ReferencePruneEffect::DropRequired
    );
    assert_eq!(
        prune::analyze(
            &value,
            &ctx,
            &BTreeSet::from(["inventor:presentation:rendering-style#other-1".into()])
        )
        .unwrap(),
        ReferencePruneEffect::Unchanged
    );
    let mut value = value;
    value.rendering_style_reference = 0;
    assert_eq!(
        prune::analyze(&value, &ctx, &deleted).unwrap(),
        ReferencePruneEffect::Unchanged
    );
    let value = face();
    let mut edges = Vec::new();
    value
        .visit_dependency_references(&ctx, &mut |edge| {
            edges.push((edge.target.to_owned(), edge.field, edge.position));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        edges,
        vec![(
            "inventor:presentation:graphics-style-collection#segment-3".to_owned(),
            Some("styles_reference"),
            DependencyPosition::Required
        )]
    );
    assert_eq!(
        prune::analyze(&value, &ctx, &BTreeSet::from([edges[0].0.clone()])).unwrap(),
        ReferencePruneEffect::DropRequired
    );
}

#[test]
fn style_list_pruning_keeps_order_nulls_qualifiers_and_exact_paired_metadata() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut value = collection(&[(3, true), (1, false), (3, false), (0, true)]);
    let mut edges = Vec::new();
    value
        .visit_dependency_references(&ctx, &mut |edge| {
            edges.push((edge.target.to_owned(), edge.position));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        edges,
        vec![
            (
                "inventor:presentation:graphics-primary-color#segment-2".into(),
                DependencyPosition::List {
                    length: 4,
                    index: 0
                }
            ),
            (
                "inventor:presentation:graphics-primary-color#segment-0".into(),
                DependencyPosition::List {
                    length: 4,
                    index: 1
                }
            ),
            (
                "inventor:presentation:graphics-primary-color#segment-2".into(),
                DependencyPosition::List {
                    length: 4,
                    index: 2
                }
            ),
        ]
    );
    let deleted =
        BTreeSet::from(["inventor:presentation:graphics-primary-color#segment-2".to_owned()]);
    assert_eq!(
        value.prune_reference_lists(&ctx, &deleted).unwrap(),
        ReferencePruneEffect::ListsChanged
    );
    assert_eq!(value.style_references.metadata(), Some(&[77, 88]));
    assert_eq!(
        value
            .style_references
            .references()
            .iter()
            .map(|r| (r.index(), r.qualified()))
            .collect::<Vec<_>>(),
        vec![(1, false), (0, true)]
    );
    assert_eq!(
        prune::analyze(&value, &ctx, &deleted).unwrap(),
        ReferencePruneEffect::Unchanged
    );
    let wire = serde_json::to_value(&value).unwrap();
    let admitted: PmGraphicsStyleCollectionRecord = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    assert_eq!(
        wire["id"],
        "inventor:presentation:graphics-style-collection#segment-3"
    );
}

#[test]
fn deleting_last_style_does_not_discard_its_nonempty_list_metadata() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut value = collection(&[(3, true)]);
    let original = serde_json::to_value(&value).unwrap();
    let deleted =
        BTreeSet::from(["inventor:presentation:graphics-primary-color#segment-2".to_owned()]);
    assert_eq!(
        value.prune_reference_lists(&ctx, &deleted).unwrap(),
        ReferencePruneEffect::ListsChanged
    );
    assert!(value.style_references.references().is_empty());
    assert_eq!(value.style_references.metadata(), Some(&[77, 88]));
    let mut expected = original;
    expected["style_references"] = serde_json::json!([]);
    expected["style_reference_qualifiers"] = serde_json::json!([]);
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(wire, expected);
    let admitted: PmGraphicsStyleCollectionRecord = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    assert!(PmDcPairedReferenceList::new(Some([77, 88]), Vec::new()).is_none());
    assert!(PmDcPairedReferenceList::<[u32; 2]>::from_document(
        None,
        vec![PmDcReference::new(3, true).unwrap()]
    )
    .is_none());
}

#[test]
fn failed_color_prunes_only_the_list_entry_and_keeps_collection_metadata_face_and_peer() {
    let bytes = [0u8, 0, 0, 0, 1, 2, 2, 0, 0, 0];
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Inventor);
    let mut original = default_style();
    original.rendering_style_reference = 0;
    let expected = serde_json::to_value(&original).unwrap();
    output
        .insert_native(&ctx, "pm_app_default_styles", original)
        .unwrap();
    output
        .insert_native(
            &ctx,
            "pm_graphics_style_collections",
            collection(&[(3, true)]),
        )
        .unwrap();
    output
        .insert_native(&ctx, "pm_graphics_faces", face())
        .unwrap();
    let failed = "inventor:presentation:graphics-primary-color#segment-2";
    let result = ctx
        .decode_part(
            &mut output,
            root,
            PartSpec::new(
                PartLevel::parse("inventor.rse_record_body").unwrap(),
                Vec::new(),
            ),
            |_| {
                ctx.record(failed, root.child(4, 6).unwrap(), || {
                    let mut payload = root.child(4, 6).unwrap();
                    crate::reader::u32(&mut payload, "graphics primary-color header")?;
                    Ok(())
                })
            },
        )
        .unwrap();
    assert!(result.is_none());
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let namespace = native.namespace("inventor").unwrap();
    let collections = namespace
        .arena_as::<PmGraphicsStyleCollectionRecord>("pm_graphics_style_collections")
        .unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(
        collections[0].id(),
        "inventor:presentation:graphics-style-collection#segment-3"
    );
    assert_eq!(collections[0].style_references.metadata(), Some(&[77, 88]));
    assert!(collections[0].style_references.references().is_empty());
    let wire = serde_json::to_value(&collections[0]).unwrap();
    let admitted: PmGraphicsStyleCollectionRecord = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    let faces = namespace
        .arena_as::<PmGraphicsFaceRecord>("pm_graphics_faces")
        .unwrap();
    assert_eq!(faces.len(), 1);
    assert_eq!(
        serde_json::to_value(&faces[0]).unwrap(),
        serde_json::to_value(face()).unwrap()
    );
    let kept = namespace
        .arena_as::<PmAppDefaultStyleRecord>("pm_app_default_styles")
        .unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(serde_json::to_value(&kept[0]).unwrap(), expected);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert!(losses[0].message.contains(failed));
    assert!(!losses[0]
        .message
        .contains("inventor:presentation:graphics-style-collection#segment-3"));
    assert!(!losses[0]
        .message
        .contains("inventor:presentation:graphics-face#segment-7"));
    let given_up = fidelity
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].offset(), 0);
    assert_eq!(given_up[0].byte_len(), 10);
}

#[test]
fn failed_rendering_target_removes_its_scalar_dependent_and_keeps_exact_null_reference_peer() {
    let bytes = [0u8, 0, 0, 0, 1, 2, 2, 0, 0, 0];
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Inventor);
    output
        .insert_native(&ctx, "pm_app_default_styles", default_style())
        .unwrap();
    let mut peer = default_style();
    peer.id = "inventor:presentation:default-style#segment-8".into();
    peer.record_ordinal = 8;
    peer.rendering_style_reference = 0;
    let expected = serde_json::to_value(&peer).unwrap();
    output
        .insert_native(&ctx, "pm_app_default_styles", peer)
        .unwrap();
    let failed = "inventor:presentation:rendering-style#segment-1";
    assert!(ctx
        .decode_part(
            &mut output,
            root,
            PartSpec::new(
                PartLevel::parse("inventor.rse_record_body").unwrap(),
                Vec::new()
            ),
            |_| {
                ctx.record(failed, root.child(4, 6).unwrap(), || {
                    let mut payload = root.child(4, 6).unwrap();
                    crate::reader::u32(&mut payload, "rendering style header")?;
                    Ok(())
                })
            }
        )
        .unwrap()
        .is_none());
    let (_, native, _, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let kept = native
        .namespace("inventor")
        .unwrap()
        .arena_as::<PmAppDefaultStyleRecord>("pm_app_default_styles")
        .unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(serde_json::to_value(&kept[0]).unwrap(), expected);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains(failed));
    assert!(losses[0]
        .message
        .contains("inventor:presentation:default-style#segment-4"));
    assert!(!losses[0]
        .message
        .contains("inventor:presentation:default-style#segment-8"));
}
