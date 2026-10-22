// SPDX-License-Identifier: Apache-2.0
//! Class records keep exact independent source extents and identities.

use crate::records::FeatureInputLane;
use crate::resolved_features::body_sources::BodySources;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::SourceFidelity;

fn fixture() -> (Vec<u8>, Vec<usize>) {
    let mut bytes = vec![0xa5; 9];
    let mut positions = Vec::new();
    for name in ["sgEntHandle", "sgEntHandle"] {
        positions.push(bytes.len());
        bytes.extend(super::super::CLASS_MARKER);
        bytes.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
        bytes.extend(name.as_bytes());
        bytes.extend([0xa5; 7]);
    }
    (bytes, positions)
}

fn lane(source: View<'_>) -> FeatureInputLane {
    let id = "lane";
    FeatureInputLane {
        id: id.into(),
        configuration: None,
        native_payload: source.window().to_vec().into(),
        classes: super::super::names::class_declarations(source.window(), id).unwrap(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

#[test]
fn repeated_class_names_keep_their_actual_distinct_source_extents() {
    let (payload, positions) = fixture();
    let mut bytes = vec![0xcc; 37];
    bytes.extend(payload);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(37, bytes.len()).unwrap();
    let mut lane = lane(source);
    assert_eq!(lane.classes.len(), 2);
    let mut sources = BodySources::from_views([(lane.id.clone(), source)]).unwrap();
    sources.capture_classes(&ctx, [&lane]).unwrap();
    // Derived content order and ownership do not replace a physical source address.
    lane.classes[0].sequence = 100;
    lane.classes[0].feature_ref = Some("held-feature".into());
    sources.capture_classes(&ctx, [&lane]).unwrap();
    let mut fidelity = SourceFidelity::default();
    crate::pmi::layout::install(&ctx, sources.into_pointers(), &mut fidelity).unwrap();
    for (class, position) in lane.classes.iter().zip(positions) {
        let layout = &fidelity.record_layout()[&class.id];
        let start = u64::try_from(37 + position).unwrap();
        let len = u64::try_from(6 + class.name.len()).unwrap();
        assert_eq!(class.name, "sgEntHandle");
        assert_eq!(layout.stream(), "file");
        assert_eq!(layout.offset(), start);
        assert_eq!(layout.byte_len(), len);
        assert_eq!(
            layout.pointers()["offset"],
            u64::try_from(position).unwrap()
        );
        assert_eq!(layout.reads()["file"], [start..start + len]);
    }
    let restored: SourceFidelity =
        serde_json::from_value(serde_json::to_value(&fidelity).unwrap()).unwrap();
    assert_eq!(restored.record_layout(), fidelity.record_layout());
}

#[test]
fn independently_edited_class_name_cannot_receive_original_source_evidence() {
    let (bytes, _) = fixture();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut lane = lane(root);
    lane.classes[1].name = "sgOtherHandle".into();
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(sources.capture_classes(&ctx, [&lane]).is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn class_source_capture_rejects_a_foreign_session() {
    let (bytes, _) = fixture();
    let arena = DecodeArena::new();
    let (_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let lane = lane(root);
    let foreign_arena = DecodeArena::new();
    let (foreign_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service()).unwrap();
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(sources.capture_classes(&foreign_ctx, [&lane]).is_err());
}
