// SPDX-License-Identifier: Apache-2.0
//! Reference layouts come from exact compact and full scalar operand cells.

use super::capture;
use crate::records::FeatureInputLane;
use crate::resolved_features::body_sources::BodySources;
use crate::resolved_features::class_sources::ClassSources;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::SourceFidelity;

fn fixture(compact: bool) -> Vec<u8> {
    let mut bytes = super::super::NAME_MARKER.to_vec();
    bytes.push(2);
    for unit in "D1".encode_utf16() {
        bytes.extend(unit.to_le_bytes());
    }
    bytes.extend(if compact {
        super::super::COMPACT_SCALAR_HEADER
    } else {
        super::super::SCALAR_HEADER
    });
    bytes.extend(0.025_f64.to_le_bytes());
    let trailer = bytes.len();
    bytes.resize(trailer + if compact { 51 } else { 59 }, 0);
    bytes[trailer + 3..trailer + 7].copy_from_slice(&42_u32.to_le_bytes());
    if compact {
        bytes[trailer + 21..trailer + 27].copy_from_slice(&[1, 0, 0, 0, 2, 0]);
    } else {
        bytes[trailer + 24..trailer + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
    }
    let second = if compact { 43 } else { 47 };
    let tag = if compact { [0x52, 0x81] } else { [0xd6, 0x80] };
    for (relative, index) in [(35, 7_u16), (second, 9)] {
        bytes[trailer + relative..trailer + relative + 2].copy_from_slice(&tag);
        bytes[trailer + relative + 2..trailer + relative + 4].copy_from_slice(&index.to_le_bytes());
        bytes[trailer + relative + 4..trailer + relative + 8].fill(0xff);
    }
    bytes
}

fn lane(ctx: &DecodeContext<'_>, source: View<'_>) -> FeatureInputLane {
    let id = "lane";
    let names = super::super::names::object_names(ctx, source.window(), id).unwrap();
    let classes = super::super::names::class_declarations(source.window(), id).unwrap();
    let class_sources = ClassSources::from_source(ctx, source, &classes).unwrap();
    let scalars = super::super::scalars::named_scalars(source.window(), id, &names).unwrap();
    let references =
        super::super::markers::reference_cells(&class_sources, &scalars.records, &classes).unwrap();
    FeatureInputLane {
        id: id.into(),
        configuration: None,
        native_payload: source.window().to_vec().into(),
        names,
        classes,
        scalars: scalars.records,
        references: references.records,
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn assert_actual_cell_layout(compact: bool) {
    let payload = fixture(compact);
    let mut bytes = vec![0xa5; 37];
    bytes.extend(&payload);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(37, bytes.len()).unwrap();
    let mut lane = lane(&ctx, source);
    assert_eq!(lane.references.len(), 2);
    let scalar = &lane.scalars[0];
    assert_eq!(scalar.value.get(), 0.025);
    let old_positions = scalar
        .operands
        .iter()
        .map(|operand| operand.offset.unwrap())
        .collect::<Vec<_>>();
    let mut sources = BodySources::from_views([(lane.id.clone(), source)]).unwrap();
    super::super::bindings::bind_scalar_operands(
        &ctx,
        &[],
        std::slice::from_mut(&mut lane),
        &mut sources,
    )
    .unwrap();
    let mut fidelity = SourceFidelity::default();
    crate::pmi::layout::install(
        &ctx,
        crate::test_support::native::layout_pointers(&ctx, sources),
        &mut fidelity,
    )
    .unwrap();
    for (ordinal, (reference, position)) in lane.references.iter().zip(old_positions).enumerate() {
        assert_eq!(reference.object_index, [7, 9][ordinal]);
        let layout = &fidelity.record_layout()[&reference.id];
        let len = if compact { 8 } else { 12 };
        assert_eq!(layout.stream(), "file");
        assert_eq!((layout.offset(), layout.byte_len()), (37 + position, len));
        assert_eq!(layout.pointers()["offset"], position);
        assert_eq!(layout.reads()["file"], [37 + position..37 + position + len]);
        assert!(lane.names[0].sequence < reference.sequence);
        let wire = serde_json::to_value(reference).unwrap();
        assert!(wire.get("offset").is_none());
        let restored: crate::records::FeatureInputReference = serde_json::from_value(wire).unwrap();
        assert_eq!(&restored, reference);
    }
    assert!(lane.references[0].sequence < lane.references[1].sequence);
    let restored: SourceFidelity =
        serde_json::from_value(serde_json::to_value(&fidelity).unwrap()).unwrap();
    assert_eq!(restored.record_layout(), fidelity.record_layout());
}

#[test]
fn compact_reference_cells_have_exact_eight_byte_source_owners() {
    assert_actual_cell_layout(true);
}

#[test]
fn full_reference_cells_have_exact_twelve_byte_source_owners() {
    assert_actual_cell_layout(false);
}

#[test]
fn independently_edited_reference_index_cannot_receive_original_source_reads() {
    let bytes = fixture(false);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut lane = lane(&ctx, root);
    lane.references[0].object_index = 17;
    let mut sources = BodySources::from_views([(lane.id.clone(), root)]).unwrap();
    assert!(capture(&ctx, &lane, &mut sources).is_err());
    assert!(sources.pointers().is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn reference_capture_rejects_a_foreign_source_view() {
    let bytes = fixture(true);
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
