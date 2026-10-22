// SPDX-License-Identifier: Apache-2.0
//! Body selection content stays native; only proven source positions move.

use super::BodySources;
use crate::records::{
    FeatureHistory, FeatureInputBodySelection, FeatureInputLane, FeatureInputName,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::BodyRetentionMode;
use cadmpeg_ir::SourceFidelity;
use std::collections::BTreeMap;

fn fixture() -> (Vec<u8>, Vec<FeatureHistory>) {
    let feature = serde_json::from_value(serde_json::json!({
        "id":"feature", "parent":"history", "source_id":"89", "ordinal":0,
        "name":"Body-Delete/Keep 1", "kind":"DeleteBody", "input_class":"moDeleteBody_c"
    }))
    .expect("feature");
    let histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature],
    }];
    let mut lane = empty_lane();
    lane.names.push(FeatureInputName {
        id: "name".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        sequence: 0,
        feature_ref: None,
        offset: None,
        object_id: None,
        value: "Body-Delete/Keep 1".into(),
    });
    lane.body_selections.push(FeatureInputBodySelection {
        id: "selection".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        sequence: 0,
        object_name_ref: "name".into(),
        feature_ref: "feature".into(),
        local_body_ids: vec![287, 115],
        body_state_ids: vec![287],
        mode: Some(BodyRetentionMode::DeleteSelected),
    });
    let mut payload = crate::test_support::history::resolved_feature_classes_with_ids(&[(
        "moDeleteBody_c",
        "Body-Delete/Keep 1",
        89,
    )]);
    let output = super::super::native_selection_write::events(&lane, &histories)
        .expect("authored selection");
    assert!(output.omitted.is_empty());
    for event in output.events {
        payload.extend(event.bytes);
    }
    (payload, histories)
}

fn empty_lane() -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes: Vec::new(),
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

fn lane(ctx: &DecodeContext<'_>, payload: &[u8]) -> FeatureInputLane {
    let mut lane = empty_lane();
    lane.classes = super::super::names::class_declarations(payload, &lane.id).expect("classes");
    lane.names = super::super::names::object_names(ctx, payload, &lane.id).expect("names");
    lane.native_payload = payload.to_vec().into();
    lane
}

#[test]
fn body_vector_pointer_is_lane_relative_but_layout_and_reads_use_actual_source() {
    let (payload, histories) = fixture();
    let vector = payload
        .windows(4)
        .rposition(|bytes| bytes == 11000u32.to_le_bytes())
        .expect("schema");
    let mode = payload
        .windows(10)
        .position(|bytes| bytes == [0x30, 0x80, 1, 0, 0, 0, 0, 0, 0, 0])
        .expect("mode");
    let mut source = vec![0xa5; 37];
    source.extend(&payload);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).expect("source");
    let view = root.child(37, source.len()).expect("actual lane");
    let mut lane = lane(&ctx, &payload);
    let mut sources =
        BodySources::from_views([(lane.id.clone(), view)]).expect("source association");
    super::super::bindings::bind_scalar_operands(
        &ctx,
        &histories,
        std::slice::from_mut(&mut lane),
        &mut sources,
    )
    .expect("body decode");
    let [record] = lane.body_selections.as_slice() else {
        panic!("one selection")
    };
    assert_eq!(record.local_body_ids, [287, 115]);
    assert_eq!(record.body_state_ids, [287]);
    assert_eq!(record.mode, Some(BodyRetentionMode::DeleteSelected));
    assert!(record.sequence > lane.names[0].sequence);
    let wire = serde_json::to_value(record).expect("native wire");
    assert!(wire.get("offset").is_none());
    let restored: FeatureInputBodySelection =
        serde_json::from_value(wire).expect("native readback");
    assert_eq!(&restored, record);
    let mut fidelity = SourceFidelity::default();
    crate::pmi::layout::install(&ctx, sources.into_pointers(&ctx).unwrap(), &mut fidelity)
        .expect("captured layout");
    let layout = &fidelity.record_layout()[&record.id];
    assert_eq!(layout.stream(), "file");
    assert_eq!(
        layout.offset(),
        37 + lane.names[0].offset.expect("actual name position")
    );
    assert_eq!(
        layout.pointers()["offset"],
        u64::try_from(vector).expect("pointer")
    );
    let covers = |start: usize, end: usize| {
        layout.reads()["file"].iter().any(|range| {
            range.start <= u64::try_from(start).unwrap() && u64::try_from(end).unwrap() <= range.end
        })
    };
    assert!(covers(37 + vector, source.len()));
    assert!(covers(37 + mode, 37 + mode + 10));
    assert!(covers(37 + mode - 83, 37 + mode));
    let name = usize::try_from(layout.offset()).unwrap();
    assert!(
        !covers(name, name + 1),
        "object framing is not blanket semantic coverage"
    );
    let restored: SourceFidelity =
        serde_json::from_value(serde_json::to_value(&fidelity).unwrap()).unwrap();
    assert_eq!(restored.record_layout(), fidelity.record_layout());
}

#[test]
fn foreign_lane_view_is_refused_without_a_body_capture() {
    let (payload, histories) = fixture();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let foreign_arena = DecodeArena::new();
    let (_, foreign) =
        DecodeContext::from_root_bytes(&payload, &foreign_arena, &DecodePolicy::service()).unwrap();
    let mut lane = lane(&ctx, &payload);
    let mut sources = BodySources::from_views([(lane.id.clone(), foreign)]).unwrap();
    assert!(super::super::bindings::bind_scalar_operands(
        &ctx,
        &histories,
        std::slice::from_mut(&mut lane),
        &mut sources
    )
    .is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn mismatched_lane_bytes_cannot_supply_a_source_pointer() {
    let (payload, histories) = fixture();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service()).unwrap();
    let mut lane = lane(&ctx, &payload);
    lane.native_payload[0] ^= 1;
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(super::super::bindings::bind_scalar_operands(
        &ctx,
        &histories,
        std::slice::from_mut(&mut lane),
        &mut sources
    )
    .is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn duplicate_named_blocks_keep_distinct_actual_body_layout_streams() {
    let (payload, histories) = fixture();
    let mut source = crate::test_support::container::outer_header();
    for _ in 0..2 {
        source.extend(crate::test_support::container::make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &payload,
        ));
    }
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let mut sources = BodySources::default();
    let mut lanes = super::super::assembly::lanes(
        &ctx,
        &scan,
        &mut sources,
        &mut cadmpeg_ir::Annotations::default(),
    )
    .unwrap();
    assert_eq!(lanes.len(), 2);
    let streams = lanes
        .iter()
        .map(|lane| {
            let view = sources.view(lane).unwrap();
            assert_ne!(view.window().as_ptr(), lane.native_payload.as_ptr());
            ctx.capture_part_extent(view).unwrap().stream().to_owned()
        })
        .collect::<Vec<_>>();
    assert_ne!(streams[0], streams[1]);
    super::super::bindings::bind_scalar_operands(&ctx, &histories, &mut lanes, &mut sources)
        .unwrap();
    let mut fidelity = SourceFidelity::default();
    crate::pmi::layout::install(&ctx, sources.into_pointers(&ctx).unwrap(), &mut fidelity).unwrap();
    for (lane, stream) in lanes.iter().zip(streams) {
        let [record] = lane.body_selections.as_slice() else {
            panic!("one body selection")
        };
        let layout = &fidelity.record_layout()[&record.id];
        assert_eq!(layout.stream(), stream);
        assert_eq!(record.local_body_ids, [287, 115]);
        assert_eq!(record.body_state_ids, [287]);
        assert_eq!(record.mode, Some(BodyRetentionMode::DeleteSelected));
        assert!(layout.pointers().contains_key("offset"));
    }
}
