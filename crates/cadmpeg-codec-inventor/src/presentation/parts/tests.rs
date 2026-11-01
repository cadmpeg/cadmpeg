// SPDX-License-Identifier: Apache-2.0

mod source_reads;

use crate::container::InventorContainer;
use crate::native::{
    PmAppDefaultStyleRecord, PmAppRenderingStyleRecord, PmGraphicsFaceRecord,
    PmGraphicsPrimaryColorStyleRecord, PmGraphicsStyleCollectionRecord, RseRecordRecord,
};
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::test_support::given_up_records;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartOutput;
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
    bytes.extend(
        u32::try_from(units.len())
            .expect("the text length fits u32")
            .to_le_bytes(),
    );
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
        &SegmentKind::PmGraphics,
        &records,
    );
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    let mut output = PartOutput::new(CodecFormat::Inventor, &ctx);
    let inventory = super::super::inventory(&ctx, &container.rse, &mut output)
        .expect("the presentation inventory decodes");
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
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(losses.is_empty());
    let ns = native
        .namespace("inventor")
        .expect("the inventor namespace exists");
    let styles = ns
        .arena_as::<PmAppDefaultStyleRecord>("pm_app_default_styles")
        .expect("the default styles arena holds default style records");
    assert_eq!(styles[0].header_value, 77);
    assert_eq!(styles[0].header_id, 3);
    let rendering = ns
        .arena_as::<PmAppRenderingStyleRecord>("pm_app_rendering_styles")
        .expect("the rendering styles arena holds rendering style records");
    let wire = serde_json::to_value(&rendering[0]).expect("a rendering style record serializes");
    assert_eq!(wire["name"], "actual style");
    assert_eq!(wire["long_name"], "long name");
    assert_eq!(wire["comment"], "");
    assert_eq!(
        ns.arena_as::<PmGraphicsFaceRecord>("pm_graphics_faces")
            .expect("the graphics faces arena holds graphics face records")[0]
            .bounds
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [1.0; 6]
    );
    assert_eq!(
        ns.arena_as::<PmGraphicsStyleCollectionRecord>("pm_graphics_style_collections")
            .expect("the style collections arena holds style collection records")
            .len(),
        1
    );
    assert_eq!(
        ns.arena_as::<PmGraphicsPrimaryColorStyleRecord>("pm_graphics_primary_color_styles")
            .expect("the primary color styles arena holds primary color style records")[0]
            .colors[1]
            .map(cadmpeg_ir::scalar::FiniteBinary32::get),
        [0.25; 4]
    );
    assert_eq!(
        ns.arena_as::<RseRecordRecord>("rse_records")
            .expect("the rse records arena holds rse records")
            .len(),
        5
    );
    assert!(!ns.arenas().contains_key("presentation_record_issues"));
    assert!(given_up_records(&fidelity).is_empty());
}

#[test]
fn malformed_presentation_body_keeps_its_framing_and_exact_readable_peer() {
    let records = [
        (super::super::DEFAULT_STYLE_TYPE, Vec::new()),
        (super::super::DEFAULT_STYLE_TYPE, default_style()),
    ];
    let bytes = crate::test_support::test_fixtures::presentation_envelope_fixture(
        &SegmentKind::PmApp,
        &records,
    );
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    let segment = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.kind == SegmentKind::PmApp)
        .expect("the container holds a pm app segment");
    let SegmentBulkState::Framed(bulk) = &segment.bulk else {
        panic!("framed source")
    };
    let RecordFrameState::Framed(table) = &bulk.records else {
        panic!("record frames")
    };
    let failed = crate::records::source_admission::body(bulk.expanded, &table.records[0])
        .expect("the first record frame holds a body");
    let token = segment.pair.token.as_str();
    let bad_id = format!("inventor:presentation:default-style#{token}-0");
    let good_id = format!("inventor:presentation:default-style#{token}-1");
    let mut output = PartOutput::new(CodecFormat::Inventor, &ctx);
    let inventory = super::super::inventory(&ctx, &container.rse, &mut output)
        .expect("the presentation inventory decodes");
    assert_eq!(inventory.default_styles.len(), 1);
    assert_eq!(inventory.default_styles[0].header_value, 77);
    let (_, native, fidelity, losses) = output.into_parts();
    let ns = native
        .namespace("inventor")
        .expect("the inventor namespace exists");
    let styles = ns
        .arena_as::<PmAppDefaultStyleRecord>("pm_app_default_styles")
        .expect("the default styles arena holds default style records");
    assert_eq!(styles.len(), 1);
    assert_eq!(styles[0].id, good_id);
    assert_eq!(styles[0].header_value, 77);
    assert_eq!(styles[0].header_id, 3);
    assert_eq!(
        ns.arena_as::<RseRecordRecord>("rse_records")
            .expect("the rse records arena holds rse records")
            .len(),
        2
    );
    assert!(!ns.arenas().contains_key("presentation_record_issues"));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(!styles.iter().any(|style| style.id == bad_id));
    let given_up = given_up_records(&fidelity);
    assert_eq!(given_up.len(), 1);
    let retained = given_up[0];
    assert_eq!(retained.data(), Some(failed.window()));
    assert_eq!(
        retained.untyped(),
        &cadmpeg_ir::ArenaClasses::from([
            cadmpeg_ir::ArenaClass::Model,
            cadmpeg_ir::ArenaClass::Design,
        ])
    );
    assert_eq!(&fidelity.unrepresented(), retained.untyped());
}

#[test]
fn held_binding_projection_moves_actual_entities_through_the_final_stage() {
    let inventory = super::super::tests::face_override_inventory(42, true);
    let face = cadmpeg_ir::ids::FaceId::mint("inventor:test:face#1").expect("the face id is valid");
    let keys = std::collections::HashMap::from([(face, 42)]);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("the empty source fits the service policy");
    let expected = super::super::project_bindings(&ctx, &inventory, &[], &[], &keys, None)
        .expect("the unstaged binding projection succeeds");
    let mut output = PartOutput::new(CodecFormat::Inventor, &ctx);
    let staged =
        super::super::project_bindings(&ctx, &inventory, &[], &[], &keys, Some(&mut output))
            .expect("the staged binding projection succeeds");
    assert!(staged.appearances.is_empty() && staged.bindings.is_empty());
    let (model, _, _, losses) = output.into_parts();
    assert_eq!(model.appearances, expected.appearances);
    assert_eq!(model.appearance_bindings, expected.bindings);
    assert!(losses.is_empty());
}
