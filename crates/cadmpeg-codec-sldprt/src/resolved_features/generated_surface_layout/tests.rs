// SPDX-License-Identifier: Apache-2.0
//! Real generated-surface frames and content-ordered ownership.

use super::capture;
use crate::records::FeatureInputLane;
use crate::resolved_features::body_sources::BodySources;
use crate::resolved_features::class_sources::ClassSources;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::SourceFidelity;

fn append_identity(payload: &mut Vec<u8>, owner: u32, terminal: Option<u32>) -> usize {
    let name = "moWzdHoleSurfIdRep_c";
    payload.extend(super::super::CLASS_MARKER);
    payload.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
    payload.extend(name.as_bytes());
    payload.extend([0, 0]);
    let offset = payload.len();
    let prefix = [0xc3, 0x80, 0xc5, 0];
    payload.extend(prefix);
    payload.extend(owner.to_le_bytes());
    payload.extend(1u32.to_le_bytes());
    if let Some(terminal) = terminal {
        payload.extend(0x85b5u16.to_le_bytes());
        payload.extend([0, 0]);
        payload.extend(prefix);
        payload.extend(terminal.to_le_bytes());
        payload.extend(2u32.to_le_bytes());
    }
    payload.extend(3u32.to_le_bytes());
    offset
}

fn lane(ctx: &DecodeContext<'_>, source: View<'_>) -> FeatureInputLane {
    let id = "lane";
    let classes = super::super::names::class_declarations(source.window(), id).unwrap();
    let class_sources = ClassSources::from_source(ctx, source, &classes).unwrap();
    let names = super::super::names::object_names(ctx, source.window(), id).unwrap();
    let parsed = super::super::selections::generated_surface_identities(
        &class_sources,
        id,
        &classes,
        source.window(),
    )
    .unwrap();
    FeatureInputLane {
        id: id.into(),
        configuration: None,
        native_payload: source.window().to_vec().into(),
        classes,
        names,
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: parsed.records,
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

#[test]
fn full_path_extent_keeps_the_earlier_producer_when_terminal_source_is_reserved() {
    let mut payload = Vec::new();
    let offset = append_identity(&mut payload, 89, Some(u32::MAX));
    let mut bytes = vec![0xa5; 37];
    bytes.extend(&payload);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(37, bytes.len()).unwrap();
    let lane = lane(&ctx, source);
    let [record] = lane.generated_surface_identities.as_slice() else {
        panic!("one identity")
    };
    assert_eq!(record.feature_source_id.value(), 89);
    assert_eq!(record.local_identity, 0x85b5);
    assert_eq!(record.components.len(), 2);
    assert_eq!(
        &record.components[1].type_signature[4..8],
        &u32::MAX.to_le_bytes()
    );
    assert_eq!(record.components[1].local_id, Some(3));
    let mut sources = BodySources::from_views([(lane.id.clone(), source)]).unwrap();
    capture(&ctx, &lane, &mut sources).unwrap();
    let mut fidelity = SourceFidelity::default();
    crate::pmi::layout::install(
        &ctx,
        crate::test_support::native::layout_pointers(&ctx, sources),
        &mut fidelity,
    )
    .unwrap();
    let layout = &fidelity.record_layout()[&record.id];
    assert_eq!(layout.stream(), "file");
    assert_eq!(layout.offset(), u64::try_from(37 + offset).unwrap());
    assert_eq!(layout.byte_len(), 32);
    assert_eq!(layout.pointers()["offset"], u64::try_from(offset).unwrap());
    let start = u64::try_from(37 + offset).unwrap();
    assert!(layout.reads()["file"]
        .iter()
        .any(|range| range.start <= start && start + 32 <= range.end));
    assert!(layout.reads()["file"].iter().all(|range| range.start >= 37));
    let wire = serde_json::to_value(record).unwrap();
    assert!(wire.get("offset").is_none());
    let restored: crate::records::FeatureInputGeneratedSurfaceIdentity =
        serde_json::from_value(wire).unwrap();
    assert_eq!(&restored, record);
    let restored: SourceFidelity =
        serde_json::from_value(serde_json::to_value(&fidelity).unwrap()).unwrap();
    assert_eq!(restored.record_layout(), fidelity.record_layout());
}

#[test]
fn content_sequence_matches_source_order_across_repeated_declaration_scopes() {
    let mut payload = crate::test_support::history::resolved_feature_classes_with_ids(&[(
        "moCirPattern_c",
        "Pattern1",
        228,
    )]);
    let first = append_identity(&mut payload, 224, None);
    payload.extend(
        crate::test_support::history::resolved_feature_classes_with_ids(&[(
            "moCirPattern_c",
            "Pattern2",
            229,
        )]),
    );
    let second = append_identity(&mut payload, 225, None);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let mut lane = lane(&ctx, root);
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    crate::resolved_features::bindings::finalize_lane_bindings(&ctx, &[], &mut lane, &mut sources)
        .unwrap();
    assert_eq!(lane.generated_surface_identities.len(), 2);
    assert_eq!(lane.names.len(), 2);
    let identities = &lane.generated_surface_identities;
    assert!(lane.names[0].sequence < identities[0].sequence);
    assert!(identities[0].sequence < lane.names[1].sequence);
    assert!(lane.names[1].sequence < identities[1].sequence);
    let mut actual = lane
        .names
        .iter()
        .map(|name| (name.offset.unwrap(), name.sequence))
        .collect::<Vec<_>>();
    actual.extend(
        identities
            .iter()
            .map(|record| (sources.pointers()[&record.id]["offset"], record.sequence)),
    );
    for left in &actual {
        for right in &actual {
            assert_eq!(left.0.cmp(&right.0), left.1.cmp(&right.1));
        }
    }
    assert_eq!(
        sources.pointers()[&identities[0].id]["offset"],
        u64::try_from(first).unwrap()
    );
    assert_eq!(
        sources.pointers()[&identities[1].id]["offset"],
        u64::try_from(second).unwrap()
    );
}

#[test]
fn independently_edited_native_path_cannot_receive_original_source_reads() {
    let mut bytes = Vec::new();
    append_identity(&mut bytes, 89, None);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut lane = lane(&ctx, root);
    lane.generated_surface_identities[0].components[0].local_id = Some(17);
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(capture(&ctx, &lane, &mut sources).is_err());
    assert!(sources.pointers().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn foreign_source_path_is_rejected_before_capture() {
    let mut bytes = Vec::new();
    append_identity(&mut bytes, 89, None);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let lane = lane(&ctx, root);
    let foreign_arena = DecodeArena::new();
    let (_foreign_ctx, foreign) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service()).unwrap();
    let mut sources = BodySources::from_views([(lane.id.clone(), foreign)]).unwrap();
    assert!(capture(&ctx, &lane, &mut sources).is_err());
    assert!(sources.pointers().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
