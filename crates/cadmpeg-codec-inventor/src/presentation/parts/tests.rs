// SPDX-License-Identifier: Apache-2.0

mod source_reads;

use crate::container::InventorContainer;
use crate::native::{
    PmAppDefaultStyleRecord, PmAppRenderingStyleRecord, PmGraphicsFaceRecord,
    PmGraphicsPrimaryColorStyleRecord, PmGraphicsStyleCollectionRecord, RseRecordRecord,
};
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::{PartOutput, PartState};
use cadmpeg_ir::CodecFormat;

fn default_style() -> Vec<u8> {
    let mut bytes = 77_u32.to_le_bytes().to_vec();
    bytes.extend(3_u16.to_le_bytes());
    bytes.extend([0; 36]);
    bytes.push(0);
    bytes.extend(0_u32.to_le_bytes());
    bytes
}

fn text(bytes: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    bytes.extend(u32::try_from(units.len()).unwrap().to_le_bytes());
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
}

fn rendering_style() -> Vec<u8> {
    let mut bytes = vec![0; 7 + 2 + 4 + 4 + 4 + 4];
    text(&mut bytes, "actual style");
    text(&mut bytes, "long name");
    bytes.extend(0_u16.to_le_bytes());
    for _ in 0..4 {
        text(&mut bytes, "");
    }
    bytes.extend([0; 20]);
    bytes
}

#[test]
fn all_presentation_grammars_form_native_records_and_actual_source_captures() {
    let mut collection = 2_u16.to_le_bytes().to_vec();
    collection.extend(0x3000_u16.to_le_bytes());
    collection.extend(0_u32.to_le_bytes());
    let mut face = super::super::tests::graphics_face_fixture([1.0; 6]);
    face[10..14].copy_from_slice(&0x8000_0004_u32.to_le_bytes());
    let records = [
        (super::super::DEFAULT_STYLE_TYPE, default_style()),
        (super::super::RENDERING_STYLE_TYPE, rendering_style()),
        (super::super::GRAPHICS_FACE_TYPE, face),
        (super::super::GRAPHICS_STYLE_COLLECTION_TYPE, collection),
        (
            super::super::GRAPHICS_PRIMARY_COLOR_STYLE_TYPE,
            super::super::tests::primary_color_fixture([0.25; 4]),
        ),
    ];
    let bytes = crate::test_support::test_fixtures::presentation_envelope_fixture(
        SegmentKind::PmGraphics,
        &records,
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let inventory = super::super::inventory(&ctx, &container.rse, &mut output).unwrap();
    assert_eq!(
        [
            inventory.default_styles.len(),
            inventory.rendering_styles.len(),
            inventory.graphics_faces.len(),
            inventory.graphics_style_collections.len(),
            inventory.graphics_primary_color_styles.len()
        ],
        [1; 5]
    );
    let (_, native, mut fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert!(losses.is_empty());
    let ns = native.namespace("inventor").unwrap();
    let styles = ns
        .arena_as::<PmAppDefaultStyleRecord>("pm_app_default_styles")
        .unwrap();
    assert_eq!(styles[0].header_value, 77);
    assert_eq!(styles[0].header_id, 3);
    let rendering = ns
        .arena_as::<PmAppRenderingStyleRecord>("pm_app_rendering_styles")
        .unwrap();
    let wire = serde_json::to_value(&rendering[0]).unwrap();
    assert_eq!(wire["name"], "actual style");
    assert_eq!(wire["long_name"], "long name");
    assert_eq!(wire["comment"], "");
    assert_eq!(
        ns.arena_as::<PmGraphicsFaceRecord>("pm_graphics_faces")
            .unwrap()[0]
            .bounds
            .map(|value| value.get()),
        [1.0; 6]
    );
    assert_eq!(
        ns.arena_as::<PmGraphicsStyleCollectionRecord>("pm_graphics_style_collections")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ns.arena_as::<PmGraphicsPrimaryColorStyleRecord>("pm_graphics_primary_color_styles")
            .unwrap()[0]
            .colors[1]
            .map(|value| value.get()),
        [0.25; 4]
    );
    assert_eq!(
        ns.arena_as::<RseRecordRecord>("rse_records").unwrap().len(),
        5
    );
    assert!(!ns.arenas().contains_key("presentation_record_issues"));
    fidelity.install_mapped_record_captures(&ctx, Ok).unwrap();
    // Five presentation records (one per grammar) and their five `rse_records`
    // wrappers. The open's metadata type descriptors (Appendix C
    // `inventor.type_descriptor`) and segment pair records are not asserted here.
    assert_eq!(
        fidelity
            .record_layout()
            .keys()
            .filter(|id| !id.starts_with("inventor:rse:meta-type#")
                && !id.starts_with("inventor:rse:segment#"))
            .count(),
        10
    );
    for (arena, records) in ns.arenas() {
        for record in records {
            let layout = &fidelity.record_layout()[record.id()];
            assert!(!layout.reads().is_empty(), "{arena}: {}", record.id());
        }
    }
}

#[test]
fn malformed_presentation_body_keeps_its_framing_and_exact_readable_peer() {
    let records = [
        (super::super::DEFAULT_STYLE_TYPE, Vec::new()),
        (super::super::DEFAULT_STYLE_TYPE, default_style()),
    ];
    let bytes = crate::test_support::test_fixtures::presentation_envelope_fixture(
        SegmentKind::PmApp,
        &records,
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let segment = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.kind == SegmentKind::PmApp)
        .unwrap();
    let SegmentBulkState::Framed(bulk) = &segment.bulk else {
        panic!("framed source")
    };
    let RecordFrameState::Framed(table) = &bulk.records else {
        panic!("record frames")
    };
    let failed =
        crate::records::source_admission::body(&ctx, bulk.expanded, &table.records[0]).unwrap();
    let expected = ctx.capture_part_extent(failed).unwrap();
    let token = segment.pair.token.as_str();
    let bad_id = format!("inventor:presentation:default-style#{token}-0");
    let good_id = format!("inventor:presentation:default-style#{token}-1");
    let wrapper_id = format!("inventor:rse:record#{token}-0");
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let inventory = super::super::inventory(&ctx, &container.rse, &mut output).unwrap();
    assert_eq!(inventory.default_styles.len(), 1);
    assert_eq!(inventory.default_styles[0].header_value, 77);
    let (model, native, mut fidelity, mut losses) = output.into_parts(&ctx).unwrap().into_parts();
    let ns = native.namespace("inventor").unwrap();
    let styles = ns
        .arena_as::<PmAppDefaultStyleRecord>("pm_app_default_styles")
        .unwrap();
    assert_eq!(styles.len(), 1);
    assert_eq!(styles[0].id, good_id);
    assert_eq!(styles[0].header_value, 77);
    assert_eq!(styles[0].header_id, 3);
    assert_eq!(
        ns.arena_as::<RseRecordRecord>("rse_records").unwrap().len(),
        2
    );
    assert!(!ns.arenas().contains_key("presentation_record_issues"));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(losses[0].message.contains(&bad_id));
    let failed = fidelity
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp {}))
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].stream(), expected.stream());
    assert_eq!(failed[0].offset(), expected.offset());
    assert_eq!(failed[0].byte_len(), expected.byte_len());
    let failed_id = fidelity
        .parts()
        .iter()
        .find(|(_, part)| matches!(part.state(), PartState::GivenUp {}))
        .map(|(id, _)| id)
        .expect("one actual failed part");
    let retained = fidelity.retained_record(failed_id.as_str()).unwrap();
    assert_eq!(
        retained.untyped(),
        &cadmpeg_ir::ArenaClasses::from([
            cadmpeg_ir::ArenaClass::Model,
            cadmpeg_ir::ArenaClass::Design,
        ])
    );
    assert_eq!(&fidelity.unrepresented(), retained.untyped());
    let streams = ctx.source_streams().unwrap();
    fidelity.install_mapped_record_captures(&ctx, Ok).unwrap();
    assert!(!fidelity.record_layout().contains_key(&bad_id));
    assert!(fidelity.record_layout().contains_key(&good_id));
    assert!(fidelity.record_layout().contains_key(&wrapper_id));
    let wrapper = &fidelity.record_layout()[&wrapper_id];
    assert!(
        wrapper.stream() != expected.stream()
            || wrapper.offset() < expected.offset()
            || wrapper.offset() >= expected.offset() + expected.byte_len()
    );
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model = model;
    ir.native = native;
    let index = cadmpeg_ir::source_fidelity::b1::classification::ClassificationIndex::new_with_owner_resolver(
        &ctx, &ir, &DefaultStyleOwners).unwrap();
    let partitions = streams
        .iter()
        .map(|stream| {
            let parts = fidelity
                .parts()
                .values()
                .filter(|part| part.stream() == stream.name());
            let framing = parts
                .clone()
                .flat_map(|part| part.framing_bytes().iter().cloned())
                .collect::<Vec<_>>();
            let given_up = parts
                .filter(|part| matches!(part.state(), PartState::GivenUp {}))
                .map(|part| part.offset()..part.offset() + part.byte_len())
                .collect::<Vec<_>>();
            cadmpeg_ir::source_fidelity::b1::partition_stream(
                &ctx,
                stream.name(),
                stream.byte_len(),
                fidelity.record_layout(),
                &framing,
                &[],
                &given_up,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    fidelity
        .finalize_b1(&ctx, &index, "inventor", &partitions, &mut losses)
        .unwrap();
    assert!(fidelity
        .unrepresented()
        .contains(&cadmpeg_ir::ArenaClass::Model));
    assert!(fidelity.b1_spans()[expected.stream()]
        .spans()
        .iter()
        .any(|span| span.range().start <= expected.offset()
            && span.range().end >= expected.offset() + expected.byte_len()
            && span.classes().contains(&cadmpeg_ir::ArenaClass::Model)));
    fidelity
        .check_b1_closure(&ctx, &index, "inventor", &partitions, &losses)
        .unwrap();
}

#[test]
fn held_binding_projection_moves_actual_entities_through_the_final_stage() {
    let inventory = super::super::tests::face_override_inventory(42, true);
    let face = cadmpeg_ir::ids::FaceId::mint("inventor:test:face#1").unwrap();
    let keys = std::collections::HashMap::from([(face, 42)]);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let expected = super::super::project_bindings(&ctx, &inventory, &[], &[], &keys, None).unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let staged =
        super::super::project_bindings(&ctx, &inventory, &[], &[], &keys, Some(&mut output))
            .unwrap();
    assert!(staged.appearances.is_empty() && staged.bindings.is_empty());
    let (model, _, _, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert_eq!(model.appearances, expected.appearances);
    assert_eq!(model.appearance_bindings, expected.bindings);
    assert!(losses.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

struct DefaultStyleOwners;

impl cadmpeg_ir::source_fidelity::b1::provenance::NativeOwnerResolver for DefaultStyleOwners {
    fn visit_owners(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &cadmpeg_ir::CadIr,
        visitor: &mut dyn FnMut(
            cadmpeg_ir::source_fidelity::b1::provenance::NativeOwnership,
        ) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        use cadmpeg_ir::source_fidelity::b1::provenance::NativeOwnership;
        let ns = ir.native.namespace("inventor").unwrap();
        for record in ns
            .arena_as_for_decode::<PmAppDefaultStyleRecord>(ctx, "pm_app_default_styles")
            .unwrap()
        {
            visitor(NativeOwnership::new(
                cadmpeg_ir::ids::Identity::new(record.id).unwrap(),
                None,
            ))?;
        }
        for record in ns
            .arena_as_for_decode::<RseRecordRecord>(ctx, "rse_records")
            .unwrap()
        {
            visitor(NativeOwnership::new(
                cadmpeg_ir::ids::Identity::new(record.id).unwrap(),
                None,
            ))?;
        }
        Ok(())
    }
}
