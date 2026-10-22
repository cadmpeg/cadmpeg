// SPDX-License-Identifier: Apache-2.0
//! Compact edge vectors retain exact source ownership and typed contents.

use super::capture;
use crate::records::{Feature, FeatureHistory, FeatureInputLane};
use crate::resolved_features::body_sources::BodySources;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::SourceFidelity;
use std::collections::BTreeMap;

fn fixture(reference_list: bool) -> (Vec<u8>, usize, usize) {
    let mut bytes = super::super::NAME_MARKER.to_vec();
    bytes.push(7);
    for unit in "Chamfer".encode_utf16() {
        bytes.extend(unit.to_le_bytes());
    }
    bytes.resize(64, 0);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([0, 2, 0, 0]);
    bytes.extend([0; 4]);
    let marker = bytes.len();
    bytes.extend(super::super::selections::COMPACT_EDGE_VECTOR_MARKER);
    bytes.extend([0; 2]);
    bytes.extend(0x8130_u16.to_le_bytes());
    bytes.extend([0; 2]);
    bytes.extend([0x2a, 0x81, 0x2c, 1]);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([0x24, 1, 0xd3, 0x48]);
    bytes.extend(7_u32.to_le_bytes());
    if reference_list {
        bytes.extend([0xff; 4]);
    }
    let end = bytes.len();
    bytes.extend([0xa5; 11]);
    (bytes, marker, end)
}

fn histories() -> Vec<FeatureHistory> {
    vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "chamfer".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: None,
            ordinal: 0,
            name: "Chamfer".into(),
            kind: "Chamfer".into(),
            input_class: Some("Chamfer_c".into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }]
}

fn lane(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    histories: &[FeatureHistory],
) -> FeatureInputLane {
    let id = "lane";
    let mut lane = FeatureInputLane {
        id: id.into(),
        configuration: None,
        native_payload: source.window().to_vec().into(),
        classes: super::super::names::class_declarations(source.window(), id).unwrap(),
        names: super::super::names::object_names(ctx, source.window(), id).unwrap(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    lane.edge_selections = super::super::selections::compact_edge_selections(ctx, histories, &lane)
        .unwrap()
        .records;
    lane
}

fn assert_source_layout(reference_list: bool) {
    let (payload, marker, end) = fixture(reference_list);
    let mut bytes = vec![0xcc; 37];
    bytes.extend(&payload);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(37, bytes.len()).unwrap();
    let histories = histories();
    let mut lane = lane(&ctx, source, &histories);
    assert_eq!(lane.edge_selections.len(), 1);
    assert_eq!(lane.edge_selections[0].local_edge_ids, [7]);
    let mut sources = BodySources::from_views([(lane.id.clone(), source)]).unwrap();
    super::super::bindings::bind_scalar_operands(
        &ctx,
        &histories,
        std::slice::from_mut(&mut lane),
        &mut sources,
    )
    .unwrap();
    capture(&ctx, &histories, &lane, &mut sources).unwrap();
    let mut fidelity = SourceFidelity::default();
    crate::pmi::layout::install(&ctx, sources.into_pointers(), &mut fidelity).unwrap();
    let record = &lane.edge_selections[0];
    let layout = &fidelity.record_layout()[&record.id];
    assert_eq!(layout.stream(), "file");
    assert_eq!(layout.offset(), u64::try_from(37 + marker - 12).unwrap());
    assert_eq!(
        layout.byte_len(),
        u64::try_from(end - (marker - 12)).unwrap()
    );
    assert_eq!(layout.pointers()["offset"], u64::try_from(marker).unwrap());
    assert_eq!(
        layout.reads()["file"],
        [u64::try_from(37 + marker - 12).unwrap()..u64::try_from(37 + end).unwrap()]
    );
    assert!(lane.names[0].sequence < record.sequence);
    assert_eq!(record.references.len(), usize::from(reference_list));
    let wire = serde_json::to_value(record).unwrap();
    assert!(wire.get("offset").is_none());
    let restored: crate::records::FeatureInputEdgeSelection = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, *record);
    let restored: SourceFidelity =
        serde_json::from_value(serde_json::to_value(&fidelity).unwrap()).unwrap();
    assert_eq!(restored.record_layout(), fidelity.record_layout());
}

#[test]
fn homogeneous_edge_vector_has_exact_source_extent_and_marker_pointer() {
    assert_source_layout(false);
}

#[test]
fn reference_list_edge_vector_owns_its_actual_reference_sentinel() {
    assert_source_layout(true);
}

#[test]
fn changed_edge_id_cannot_receive_original_source_reads() {
    let (bytes, _, _) = fixture(false);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let histories = histories();
    let mut lane = lane(&ctx, root, &histories);
    lane.edge_selections[0].local_edge_ids[0] = 287;
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(capture(&ctx, &histories, &lane, &mut sources).is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn edge_vector_rejects_a_foreign_source_session() {
    let (bytes, _, _) = fixture(false);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let histories = histories();
    let lane = lane(&ctx, root, &histories);
    let foreign_arena = DecodeArena::new();
    let (foreign_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service()).unwrap();
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(capture(&foreign_ctx, &histories, &lane, &mut sources).is_err());
}
