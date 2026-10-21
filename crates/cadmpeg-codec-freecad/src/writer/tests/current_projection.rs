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
fn coherent_array_reference_rename_declines_patch_and_regenerates_typed_values() {
    let source = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut current = source.ir().clone();
    let namespace = current.native.namespace_mut("fcstd");
    let mut properties = namespace.arena_as::<PropertyRecord>("properties").unwrap();
    let property = properties
        .iter_mut()
        .find(|property| property.name == "PlacementList")
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
    let after = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ProductNodeRecord>("product_nodes")
        .unwrap();
    let before = before.iter().find(|node| node.object == owner).unwrap();
    let after = after.iter().find(|node| node.object == owner).unwrap();
    assert!(!before.element_transforms().is_empty());
    assert_eq!(after.element_count(), before.element_count());
    assert_eq!(after.element_transforms(), before.element_transforms());
    assert_eq!(after.element_scales(), before.element_scales());
}
