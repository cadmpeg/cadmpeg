// SPDX-License-Identifier: Apache-2.0
//! A native record the generator cannot write as it stands is dropped alone,
//! with a loss naming it, and the other native records are written
//! (cadir-v11.md §3.4, W1).

use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::export::ExportReport;

use crate::records::sketch_links::{SketchCurveLink, SketchLinkSense};
use crate::test_support::assert_written_with_loss;
use crate::test_support::native_test::{f3d_native, f3d_native_mut};

const OMITTED: &str = "export/native.records-omitted";

/// The messages of the `export:native.records-omitted` losses of `report`.
fn omitted(report: &ExportReport) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == OMITTED)
        .map(|loss| loss.message.as_str())
        .collect()
}

/// `report` drops no native record set whole: no Design stream and no
/// namespace.
fn assert_no_whole_drop(report: &ExportReport) {
    for message in omitted(report) {
        assert!(
            !message.contains("of the Design stream")
                && !message.contains("cannot write the document with them"),
            "{message}"
        );
    }
}

/// Write `ir` with no source file and decode the written bytes.
fn write_and_decode(ir: &CadIr) -> CadIr {
    let mut bytes = Vec::new();
    cadmpeg_ir::codec::write::Encoder::plan(
        &crate::F3dCodec,
        cadmpeg_ir::codec::write::EncodeInput::new(ir, None),
        cadmpeg_ir::codec::write::target::TargetRequest::Unnamed,
    )
    .and_then(|plan| plan.write_to(&mut bytes))
    .expect("the writer writes the document");
    let decoded = cadmpeg_ir::codec::Codec::decode(
        &crate::F3dCodec,
        &mut std::io::Cursor::new(bytes),
        &cadmpeg_ir::codec::DecodeOptions::default(),
    )
    .expect("the written file decodes");
    decoded.into_parts().0
}

#[test]
fn sketch_curve_links_that_share_a_coedge_are_dropped_alone() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let coedges = [0, 0, 1].map(|index| ir.geometry.coedges[index].id.clone());
    f3d_native_mut(&mut ir).sketch_curve_links = coedges
        .into_iter()
        .enumerate()
        .map(|(ordinal, coedge)| SketchCurveLink {
            id: format!("f3d:generated:sketch-curve-link#{ordinal}"),
            target: AttributeTarget::Coedge(coedge),
            sketch_curve_id: 113 + i64::try_from(ordinal).expect("small ordinal"),
            ref_b: 0,
            sense: Some(SketchLinkSense::try_from(1).expect("link sense")),
            role: 2,
            closure: 3,
        })
        .collect();

    let report = assert_written_with_loss(
        &ir,
        None,
        OMITTED,
        "of sketch-curve links that share their coedge with another link",
    );
    assert_no_whole_drop(&report);
    let messages = omitted(&report).join("\n");
    assert!(
        messages.contains("f3d:generated:sketch-curve-link#0, f3d:generated:sketch-curve-link#1"),
        "{messages}"
    );
    assert!(
        !messages.contains("f3d:generated:sketch-curve-link#2"),
        "{messages}"
    );
}

#[test]
fn a_body_visibility_under_another_asm_key_is_dropped_and_the_body_keeps_its_visibility() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].visible = Some(false);
    let body = ir.geometry.bodies[0].id.clone();
    f3d_native_mut(&mut ir).body_visibilities =
        vec![crate::records::bodies::BodyVisibility::try_from(
            crate::records::bodies::BodyVisibilityWire {
                id: "f3d:generated:body-visibility#4242".into(),
                body,
                stream: "generated/Design1/BulkStream.dat".into(),
                byte_offset: 0,
                asm_body_key_offset: 0,
                asm_body_key: 4242,
                entity_suffix: 985,
                visible: false,
            },
        )
        .expect("a body visibility")];

    let report = assert_written_with_loss(&ir, None, OMITTED, "of body visibilities whose body");
    assert_no_whole_drop(&report);
    let decoded = write_and_decode(&ir);
    assert_eq!(decoded.geometry.bodies[0].visible, Some(false));
    assert_eq!(f3d_native(&decoded).body_visibilities.len(), 1);
}

#[test]
fn a_record_header_outside_the_sketch_graph_is_dropped_alone() {
    use crate::records::decal::DesignRecordHeader;
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, SegmentType, SegmentTypeData,
        SketchHeaderReferences, DESIGN_MODULE_SKETCH,
    };
    use crate::records::identity::{DesignEntityId, Located, ReferenceRun};
    use crate::records::references::DesignClassTag;

    let header = |ordinal: u32, record_index: u32, class_tag: &str| DesignRecordHeader {
        id: format!("f3d:generated:record-header#{ordinal}"),
        record_index,
        class_tag: DesignClassTag::try_from(class_tag.to_owned()).expect("class tag"),
        byte_offset: 0,
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut ir);
    native.design_types = vec![SegmentType::try_new(
        "f3d:generated/MetaStream.dat:design-type#0".into(),
        SegmentTypeData {
            byte_offset: 0,
            module: DESIGN_MODULE_SKETCH.to_owned(),
            entities: ReferenceRun::unlocated(vec![277]),
            type_guid: "22222222-3333-4444-5555-666666666666"
                .to_owned()
                .try_into()
                .expect("type GUID"),
            type_guid_offset: 0,
            base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
            version: 4,
            version_offset: 0,
        },
    )
    .expect("a sketch type")];
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "f3d:generated:entity-header#0".into(),
        byte_offset: 0,
        entity_id: DesignEntityId::try_from("0_277".to_owned()).expect("entity ID"),
        class_tag: DesignClassTag::try_from("256".to_owned()).expect("class tag"),
        optional_slot_present: true,
        registration: DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            Some(SketchHeaderReferences {
                record_reference: Some(584),
                record_reference_offset: 0,
                references: vec![Located {
                    value: 33,
                    offset: 0,
                }],
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .expect("a sketch registration"),
    }];
    native.design_record_headers = vec![header(0, 33, "350"), header(1, 55, "351")];
    drop(native);

    let report = assert_written_with_loss(
        &ir,
        None,
        OMITTED,
        "of Design record headers that no sketch header or sketch relation names",
    );
    assert_no_whole_drop(&report);
    assert!(
        omitted(&report)
            .iter()
            .any(|message| message.ends_with(": f3d:generated:record-header#1")),
        "{:#?}",
        report.losses
    );
    let decoded = write_and_decode(&ir);
    let indices = f3d_native(&decoded)
        .design_record_headers
        .iter()
        .map(|header| header.record_index)
        .collect::<Vec<_>>();
    assert_eq!(indices, [33]);
}

#[test]
fn several_asm_histories_are_dropped_and_the_design_stream_is_written() {
    use crate::history_records::AsmHistory;

    let source = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_with_history_smbh(),
    );
    let decoded = cadmpeg_ir::codec::Codec::decode(
        &crate::F3dCodec,
        &mut std::io::Cursor::new(source),
        &cadmpeg_ir::codec::DecodeOptions::default(),
    )
    .expect("the history fixture decodes");
    let (mut ir, _, _) = decoded.into_parts();
    ir.source = None;
    ir.set_native_unknowns(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        "f3d",
        &[],
    )
    .expect("the unknown records clear");
    let first = f3d_native(&ir).asm_histories[0].id.clone();
    let second = "f3d:generated:asm-history#1".to_owned();
    f3d_native_mut(&mut ir).asm_histories.push(AsmHistory::new(
        second.clone(),
        0,
        None,
        false,
        Vec::new(),
    ));

    let report = assert_written_with_loss(&ir, None, OMITTED, "of 2 ASM history streams");
    assert_no_whole_drop(&report);
    let messages = omitted(&report).join("\n");
    assert!(
        messages.contains(&first) && messages.contains(&second),
        "{messages}"
    );
    let decoded = write_and_decode(&ir);
    assert_eq!(decoded.geometry.bodies.len(), ir.geometry.bodies.len());
}
