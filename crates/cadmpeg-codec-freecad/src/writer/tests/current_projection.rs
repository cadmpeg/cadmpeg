// SPDX-License-Identifier: Apache-2.0
//! A coherent current reference edit cannot replay a stale source layout.

use crate::native::{EntryRecord, ProductNodeRecord, PropertyBody, PropertyRecord};
use crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, SynthesisCause};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn geometry_source_patch_uses_its_scope_and_declines_a_neutral_body_edit() {
    let options = DecodeOptions {
        depth: cadmpeg_core::decode::DecodeDepth::Geometry,
        ..DecodeOptions::default()
    };
    let source = FcstdCodec
        .decode(&mut Cursor::new(CORE_DESIGN_PRODUCT), &options)
        .unwrap();
    assert_eq!(source.ir().scope, cadmpeg_ir::ContentScope::geometry());
    assert!(source.ir().model.features.is_empty());
    assert!(source.ir().model.parameters.is_empty());
    assert!(!source.ir().model.attributes.is_empty());
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(source.ir(), Some(source.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(
        matches!(plan.report().write_path(), WritePath::Patched { .. }),
        "{:#?}",
        plan.report()
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &options)
        .unwrap();
    assert_eq!(reread.ir().scope, source.ir().scope);
    assert_eq!(reread.ir().model, source.ir().model);

    let mut edited = source.ir().clone();
    edited.model.bodies.first_mut().unwrap().name = Some("Geometry-authored body".into());
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&edited, Some(source.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(matches!(
        plan.report().synthesis_cause(),
        Some(SynthesisCause::PatchDeclined { .. })
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &options)
        .unwrap();
    assert_eq!(reread.ir().scope, edited.scope);
    assert!(reread
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.name.as_deref() == Some("Geometry-authored body")));
}

#[test]
fn coherent_array_reference_rename_declines_patch_and_regenerates_typed_values() {
    for (bytes, owner, count) in [
        (CORE_DESIGN_PRODUCT, "fcstd:native:object#Occurrence", 0),
        (
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/product_assembly.FCStd"
            ))
            .as_slice(),
            "fcstd:native:object#PinArray",
            3,
        ),
    ] {
        assert_coherent_reference_rename(bytes, owner, count);
    }
}

fn assert_coherent_reference_rename(bytes: &[u8], owner: &str, count: usize) {
    let source = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut current = source.ir().clone();
    let namespace = current.native.namespace_mut("fcstd");
    let mut properties = namespace.arena_as::<PropertyRecord>("properties").unwrap();
    let property = properties
        .iter_mut()
        .find(|property| property.name == "PlacementList" && property.owner == owner)
        .unwrap();
    let owner = property.owner.clone();
    let original = property.side_entries()[0].clone();
    let renamed = "AuthoredPlacements.bin";
    if let PropertyBody::Persisted {
        values,
        side_entries,
        ..
    } = &mut property.body
    {
        assert_eq!(side_entries.len(), 1);
        side_entries[0] = renamed.into();
        let root = values
            .iter_mut()
            .find(|value| value.tag == "PlacementList" && value.parent.is_none())
            .unwrap();
        assert_eq!(root.attributes["file"], original);
        root.attributes.insert("file".into(), renamed.into());
    } else {
        panic!("placement array has a typed property");
    }
    let mut entries = namespace.arena_as::<EntryRecord>("entries").unwrap();
    let entry = entries
        .iter_mut()
        .find(|entry| entry.name() == original)
        .unwrap();
    let mut wire = serde_json::to_value(&*entry).unwrap();
    wire["name"] = renamed.into();
    *entry = serde_json::from_value(wire).unwrap();
    assert!(entry.referenced_by().contains(&property.id));
    let ctx = cadmpeg_test_support::service_decode_context();
    namespace
        .set_arena(&ctx, "properties", &properties)
        .unwrap();
    namespace.set_arena(&ctx, "entries", &entries).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&current, Some(source.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(matches!(
        plan.report().synthesis_cause(),
        Some(SynthesisCause::PatchDeclined { .. })
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let before = current
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ProductNodeRecord>("product_nodes")
        .unwrap();
    let before = before.iter().find(|node| node.object == owner).unwrap();
    assert_eq!(before.element_transforms().len(), count);
    assert_eq!(before.element_count(), Some(u64::try_from(count).unwrap()));
    if count != 0 {
        assert!(!before.element_transforms().is_empty());
    }
    // Synthesis regenerates the neutral instances as individual scalar links.
    // Native array grouping is not the geometry's authority (Rule W2). Check
    // every resulting instance, including the explicit-zero scalar source.
    let mut expected = current
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.native_ref.as_deref() == Some(owner.as_str()))
        .collect::<Vec<_>>();
    let mut actual = output
        .ir()
        .model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.name.as_deref() == Some(owner.as_str()))
        .collect::<Vec<_>>();
    expected.sort_by_key(|occurrence| occurrence.ordinal);
    actual.sort_by_key(|occurrence| occurrence.ordinal);
    assert_eq!(expected.len(), count.max(1));
    assert_eq!(actual.len(), expected.len());
    assert_eq!(
        output.ir().model.occurrences.len(),
        current.model.occurrences.len()
    );
    for (expected, actual) in expected.into_iter().zip(actual) {
        assert_eq!(
            actual.effective_transform().unwrap(),
            expected.effective_transform().unwrap()
        );
        assert_eq!(actual.transform, expected.transform);
        assert_eq!(actual.linked_prototype, expected.linked_prototype);
        assert_eq!(actual.visible, expected.visible);
        assert_eq!(actual.link, expected.link);
    }
}
