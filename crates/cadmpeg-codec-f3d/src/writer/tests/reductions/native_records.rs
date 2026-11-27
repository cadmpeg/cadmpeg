// SPDX-License-Identifier: Apache-2.0
//! A native record the generator cannot write as it stands is dropped alone,
//! with a loss naming it, and the other native records are written
//! (cadir-v11.md §3.4, W1).

use cadmpeg_asm::brep::records::identity::NativeRecordNamespace;
use cadmpeg_asm::brep::records::EdgeContinuity;
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

/// Write `ir` with no source file; the write's report, the written bytes
/// and their decode.
fn write_with_bytes(ir: &CadIr) -> (ExportReport, Vec<u8>, CadIr) {
    let mut bytes = Vec::new();
    let report = cadmpeg_ir::codec::write::Encoder::plan(
        &crate::F3dCodec,
        cadmpeg_ir::codec::write::EncodeInput::new(ir, None),
        cadmpeg_ir::codec::write::target::TargetRequest::Unnamed,
    )
    .and_then(|plan| plan.write_to(&mut bytes))
    .expect("the writer writes the document");
    let decoded = cadmpeg_ir::codec::Codec::decode(
        &crate::F3dCodec,
        &mut std::io::Cursor::new(&bytes),
        &cadmpeg_ir::codec::DecodeOptions::default(),
    )
    .expect("the written file decodes");
    (report, bytes, decoded.into_parts().0)
}

/// Write `ir` with no source file; the write's report and the decoded
/// written bytes.
fn write_with_report(ir: &CadIr) -> (ExportReport, CadIr) {
    let (report, _, decoded) = write_with_bytes(ir);
    (report, decoded)
}

/// Write `ir` with no source file and decode the written bytes.
fn write_and_decode(ir: &CadIr) -> CadIr {
    write_with_report(ir).1
}

/// `report` charges no loss whose message holds `fragment`.
fn assert_no_loss_naming(report: &ExportReport, fragment: &str) {
    assert!(
        report
            .losses
            .iter()
            .all(|loss| !loss.message.contains(fragment)),
        "{:#?}",
        report.losses
    );
}

fn continuity(ir: &CadIr, edge: usize, record_index: u32, token: &str) -> EdgeContinuity {
    EdgeContinuity {
        source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
        edge: ir.geometry.edges[edge].id.clone(),
        record_index,
        sense: cadmpeg_ir::topology::Sense::Forward,
        continuity: token.into(),
    }
}

#[test]
fn an_edge_continuity_token_of_any_text_is_written() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let rows = vec![
        continuity(&ir, 0, 0, "convex"),
        continuity(&ir, 1, 1, "tangent"),
    ];
    let convex = rows[0].id().clone();
    f3d_native_mut(&mut ir).edge_continuities = rows;

    let (report, decoded) = write_with_report(&ir);
    assert_no_loss_naming(&report, &convex);
    let tokens = f3d_native(&decoded)
        .edge_continuities
        .iter()
        .filter(|row| row.continuity == "convex")
        .count();
    assert_eq!(tokens, 1);
}

#[test]
fn a_tangent_not_g2_edge_continuity_is_written() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let rows = vec![
        continuity(&ir, 0, 0, "tangent_not_g2"),
        continuity(&ir, 1, 1, "tangent"),
    ];
    f3d_native_mut(&mut ir).edge_continuities = rows;

    let decoded = write_and_decode(&ir);
    let tokens = f3d_native(&decoded)
        .edge_continuities
        .iter()
        .filter(|row| row.continuity == "tangent_not_g2")
        .count();
    assert_eq!(tokens, 1);
}

#[test]
fn sketch_curve_links_that_share_a_coedge_are_written_in_order() {
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

    let decoded = write_and_decode(&ir);
    let links = &f3d_native(&decoded).sketch_curve_links;
    let curves = links
        .iter()
        .map(|link| link.sketch_curve_id)
        .collect::<Vec<_>>();
    assert_eq!(curves, [113, 114, 115]);
    assert_eq!(links[0].target, links[1].target);
    assert_ne!(links[1].target, links[2].target);
}

/// A body visibility record of the unit cube's body under ASM body key
/// `asm_body_key`, which states `visible`.
fn body_visibility(
    ir: &CadIr,
    asm_body_key: u64,
    visible: bool,
) -> crate::records::bodies::BodyVisibility {
    crate::records::bodies::BodyVisibility::try_from(crate::records::bodies::BodyVisibilityWire {
        id: format!("f3d:generated:body-visibility#{asm_body_key}"),
        body: ir.geometry.bodies[0].id.clone(),
        stream: "generated/Design1/BulkStream.dat".into(),
        byte_offset: 0,
        asm_body_key_offset: 0,
        asm_body_key,
        entity_suffix: 985,
        browser_node_entity: 0,
        visible,
    })
    .expect("a body visibility")
}

#[test]
fn a_body_visibility_that_disagrees_with_its_body_is_written_from_the_body() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].visible = Some(false);
    let row = body_visibility(&ir, 1, true);
    f3d_native_mut(&mut ir).body_visibilities = vec![row];

    let (report, decoded) = write_with_report(&ir);
    assert_no_loss_naming(&report, "body visibilit");
    assert_eq!(decoded.geometry.bodies[0].visible, Some(false));
    let rows = &f3d_native(&decoded).body_visibilities;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].visible);
    assert_eq!(rows[0].entity_suffix, 985);
}

/// A `BodiesRoot` member that names Design entity `entity_suffix`.
fn body_member(ordinal: u64, entity_suffix: u64) -> crate::records::bodies::DesignBodyMember {
    crate::records::bodies::DesignBodyMember::try_from(
        crate::records::bodies::DesignBodyMemberWire {
            id: format!("f3d:generated/Design1/BulkStream.dat:design-body-member#{ordinal}"),
            byte_offset: ordinal,
            entity_suffix,
            flags: 0,
        },
    )
    .expect("a BodiesRoot member")
}

/// The Design entities that the `BodiesRoot` members of `ir` name.
fn body_member_entities(ir: &CadIr) -> Vec<u64> {
    f3d_native(ir)
        .design_body_members
        .iter()
        .map(|member| member.entity_suffix)
        .collect()
}

#[test]
fn a_body_visibility_under_another_asm_key_keeps_its_entity_and_takes_the_body_key() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].visible = Some(false);
    let row = body_visibility(&ir, 4242, false);
    let mut native = f3d_native_mut(&mut ir);
    native.body_visibilities = vec![row];
    native.design_body_members = vec![body_member(0, 985)];
    drop(native);

    let (report, decoded) = write_with_report(&ir);
    assert_no_loss_naming(&report, "body visibilit");
    assert_no_loss_naming(&report, "BodiesRoot");
    assert_eq!(decoded.geometry.bodies[0].visible, Some(false));
    let native = f3d_native(&decoded);
    let [row] = native.body_visibilities.as_slice() else {
        panic!("one body visibility: {:#?}", native.body_visibilities)
    };
    assert_eq!(row.entity_suffix, 985);
    assert_ne!(row.asm_body_key(), 4242);
    assert_eq!(
        native.body_native_keys[0].asm_body_key,
        Some(row.asm_body_key())
    );
    assert_eq!(body_member_entities(&decoded), [985]);
}

#[test]
fn a_second_visibility_of_a_body_is_dropped_with_the_bodies_root_member_of_its_entity() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].visible = Some(true);
    let first = body_visibility(&ir, 1, true);
    let mut wire = crate::records::bodies::BodyVisibilityWire::from(body_visibility(&ir, 7, true));
    wire.entity_suffix = 986;
    let second = crate::records::bodies::BodyVisibility::try_from(wire).expect("a body visibility");
    let mut native = f3d_native_mut(&mut ir);
    native.body_visibilities = vec![first, second];
    native.design_body_members = vec![body_member(0, 985), body_member(1, 986)];
    drop(native);

    let (report, decoded) = write_with_report(&ir);
    assert_no_whole_drop(&report);
    let messages = omitted(&report);
    assert!(
        messages.iter().any(|message| {
            message.contains("of body visibilities that name the body or the Design entity")
                && message.ends_with(": f3d:generated:body-visibility#7")
        }),
        "{:#?}",
        report.losses
    );
    assert!(
        messages.iter().any(|message| {
            message.contains("of BodiesRoot members that name the Design entity")
                && message.ends_with(": f3d:generated/Design1/BulkStream.dat:design-body-member#1")
        }),
        "{:#?}",
        report.losses
    );
    let rows = &f3d_native(&decoded).body_visibilities;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].entity_suffix, 985);
    assert_eq!(body_member_entities(&decoded), [985]);
}

#[test]
fn a_visibility_that_shares_a_design_entity_is_dropped_and_the_entity_stays_with_the_first() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let second_body = append_cube(&mut ir, "synthetic:cube_two:", 30.0);
    for body in &mut ir.geometry.bodies {
        body.visible = Some(true);
    }
    let first = body_visibility(&ir, 1, true);
    let mut wire = crate::records::bodies::BodyVisibilityWire::from(body_visibility(&ir, 2, true));
    wire.body = second_body.id.clone();
    let second = crate::records::bodies::BodyVisibility::try_from(wire).expect("a body visibility");
    let mut native = f3d_native_mut(&mut ir);
    native.body_visibilities = vec![first, second];
    native.design_body_members = vec![body_member(0, 985)];
    drop(native);

    let (report, decoded) = write_with_report(&ir);
    assert_no_whole_drop(&report);
    assert!(
        omitted(&report).iter().any(|message| {
            message.contains("of body visibilities that name the body or the Design entity")
                && message.ends_with(": f3d:generated:body-visibility#2")
        }),
        "{:#?}",
        report.losses
    );
    assert_no_loss_naming(&report, "BodiesRoot");
    let rows = &f3d_native(&decoded).body_visibilities;
    assert_eq!(rows.len(), 2);
    let entities = rows.iter().filter(|row| row.entity_suffix == 985).count();
    assert_eq!(entities, 1);
    assert_eq!(body_member_entities(&decoded), [985]);
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
fn sketch_records_whose_owner_is_absent_are_dropped_alone() {
    use crate::records::decal::DesignRecordHeader;
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, SegmentType, SegmentTypeData,
        SketchHeaderReferences, DESIGN_MODULE_SKETCH,
    };
    use crate::records::identity::{DesignEntityId, Located, ReferenceRun};
    use crate::records::references::DesignClassTag;
    use crate::records::sketch_geometry::{
        SketchPoint, SketchPointCompanion, SketchPointDraft, SketchPointRecordForm,
    };
    use crate::records::sketch_relations::{
        SketchRelation, SketchRelationDefinition, SketchRelationDraft, SketchRelationMember,
        SketchRelationReturnMember,
    };

    let header = |ordinal: u32, record_index: u32| DesignRecordHeader {
        id: format!("f3d:generated:record-header#{ordinal}"),
        record_index,
        class_tag: DesignClassTag::try_from("350".to_owned()).expect("class tag"),
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
    native.design_record_headers = vec![header(0, 33), header(1, 55)];
    native.sketch_points = vec![SketchPoint::try_from(SketchPointDraft {
        id: "f3d:generated:sketch-point#0".into(),
        record_index: 700,
        owner_reference: None,
        geometry_group: None,
        class_tag: DesignClassTag::try_from("258".to_owned()).expect("class tag"),
        byte_offset: 0,
        coordinate_offset: 33,
        companion: SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: SketchPointRecordForm::Version0 { flag: true },
        paired_reference: 701,
        coordinates: cadmpeg_ir::math::Point2::new(4.0, -7.5),
    })
    .expect("a sketch point")];
    native.sketch_relations = vec![SketchRelation::try_new(SketchRelationDraft {
        id: "f3d:generated:sketch-relation#0".into(),
        record_index: 40,
        class_tag: DesignClassTag::try_from("257".to_owned()).expect("class tag"),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 999,
        owner_entity_id: None,
        owner_reference_offset: 0,
        auxiliary_references: ReferenceRun::located(Vec::new()),
        rectangular_counted_reference_count: None,
        members: vec![
            SketchRelationMember::from_index(700),
            SketchRelationMember::from_index(55),
        ]
        .try_into()
        .expect("uniform member resolution"),
        definition: SketchRelationDefinition::new(0x11, None).expect("a relation definition"),
        entity_genesis: None,
        return_members: vec![
            SketchRelationReturnMember::from_index(55),
            SketchRelationReturnMember::from_index(700),
        ]
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .expect("a sketch relation")];
    drop(native);

    let report = assert_written_with_loss(
        &ir,
        None,
        OMITTED,
        "of sketch records whose sketch owner the document does not hold",
    );
    assert_no_whole_drop(&report);
    let messages = omitted(&report);
    for id in [
        "f3d:generated:sketch-point#0",
        "f3d:generated:sketch-relation#0",
        "f3d:generated:record-header#1",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(id)),
            "{messages:#?}"
        );
    }
    let decoded = write_and_decode(&ir);
    let native = f3d_native(&decoded);
    assert!(native.sketch_points.is_empty());
    assert!(native.sketch_relations.is_empty());
    let indices = native
        .design_record_headers
        .iter()
        .map(|header| header.record_index)
        .collect::<Vec<_>>();
    assert_eq!(indices, [33]);
}

/// Give `native` sketch container `0_277`, whose header names the records
/// `references`, and record header `33`.
fn sketch_container(native: &mut crate::native::F3dNative, references: &[u32]) {
    use crate::records::decal::DesignRecordHeader;
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, SegmentType, SegmentTypeData,
        SketchHeaderReferences, DESIGN_MODULE_SKETCH,
    };
    use crate::records::identity::{DesignEntityId, Located, ReferenceRun};
    use crate::records::references::DesignClassTag;

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
                references: references
                    .iter()
                    .map(|&value| Located { value, offset: 0 })
                    .collect(),
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .expect("a sketch registration"),
    }];
    native.design_record_headers = vec![DesignRecordHeader {
        id: "f3d:generated:record-header#0".into(),
        record_index: 33,
        class_tag: DesignClassTag::try_from("350".to_owned()).expect("class tag"),
        byte_offset: 0,
    }];
}

/// Sketch point `700` with sketch owner `owner`, whose companion names the
/// curves `incident_curves`.
fn sketch_point(
    owner: Option<u32>,
    incident_curves: Vec<u32>,
) -> crate::records::sketch_geometry::SketchPoint {
    use crate::records::sketch_geometry::{
        SketchPoint, SketchPointClosure, SketchPointCompanion, SketchPointDraft,
        SketchPointRecordForm,
    };

    SketchPoint::try_from(SketchPointDraft {
        id: "f3d:generated:sketch-point#0".into(),
        record_index: 700,
        owner_reference: owner,
        geometry_group: None,
        class_tag: crate::records::references::DesignClassTag::try_from("258".to_owned())
            .expect("class tag"),
        byte_offset: 0,
        coordinate_offset: 89,
        companion: SketchPointCompanion { incident_curves },
        record_form: SketchPointRecordForm::version11(
            500,
            SketchPointClosure::Selector0State1,
            Some(900),
            0.0,
        ),
        paired_reference: 701,
        coordinates: cadmpeg_ir::math::Point2::new(4.0, -7.5),
    })
    .expect("a sketch point")
}

/// The ids of the records `report` drops because they name a dropped
/// sketch record.
fn dropped_for_naming_a_dropped_record(report: &ExportReport) -> Vec<&str> {
    omitted(report)
        .into_iter()
        .filter(|message| message.contains("of sketch records that name a dropped sketch record"))
        .collect()
}

#[test]
fn a_relation_that_names_a_dropped_point_is_dropped_with_it() {
    use crate::records::sketch_relations::{
        SketchRelation, SketchRelationDefinition, SketchRelationDraft, SketchRelationMember,
        SketchRelationReturnMember,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut ir);
    sketch_container(&mut native, &[33, 40]);
    native.sketch_points = vec![sketch_point(None, Vec::new())];
    native.sketch_relations = vec![SketchRelation::try_new(SketchRelationDraft {
        id: "f3d:generated:sketch-relation#0".into(),
        record_index: 40,
        class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
            .expect("class tag"),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 277,
        owner_entity_id: None,
        owner_reference_offset: 0,
        auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
        rectangular_counted_reference_count: None,
        members: vec![SketchRelationMember {
            relation_ordinal: Some(0),
            ..SketchRelationMember::from_index(700)
        }]
        .try_into()
        .expect("uniform member resolution"),
        definition: SketchRelationDefinition::new(0x11, None).expect("a relation definition"),
        entity_genesis: None,
        return_members: vec![SketchRelationReturnMember::from_index(700)]
            .try_into()
            .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .expect("a sketch relation")];
    drop(native);

    let (report, decoded) = write_with_report(&ir);
    assert_no_whole_drop(&report);
    let cascade = dropped_for_naming_a_dropped_record(&report);
    assert!(
        cascade
            .iter()
            .any(|message| message.ends_with(": f3d:generated:sketch-relation#0")),
        "{:#?}",
        report.losses
    );
    let native = f3d_native(&decoded);
    assert!(native.sketch_points.is_empty());
    assert!(native.sketch_relations.is_empty());
    let references = native
        .design_entity_headers
        .iter()
        .flat_map(|header| header.reference_values().copied())
        .collect::<Vec<_>>();
    assert_eq!(references, [33]);
}

#[test]
fn a_point_whose_companion_names_a_dropped_curve_is_dropped_with_it() {
    use crate::records::sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut ir);
    sketch_container(&mut native, &[33]);
    native.sketch_points = vec![sketch_point(Some(277), vec![600])];
    native.sketch_curve_identities = vec![SketchCurveIdentity {
        id: "f3d:generated:sketch-curve#0".into(),
        record_index: 600,
        owner_reference: None,
        geometry_group: None,
        class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .expect("class tag"),
        byte_offset: 0,
        geometry_offset: 133,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(700).expect("a nonzero identity"),
        secondary_id: 701,
        geometry: Some(
            SketchCurveGeometry::line(
                Point3::new(10.0, 20.0, 0.0),
                Point3::new(40.0, 20.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("a sketch line"),
        ),
        planar_z_noise: None,
        tail_marker: None,
    }];
    drop(native);

    let (report, decoded) = write_with_report(&ir);
    assert_no_whole_drop(&report);
    let cascade = dropped_for_naming_a_dropped_record(&report);
    assert!(
        cascade
            .iter()
            .any(|message| message.ends_with(": f3d:generated:sketch-point#0")),
        "{:#?}",
        report.losses
    );
    let native = f3d_native(&decoded);
    assert!(native.sketch_points.is_empty());
    assert!(native.sketch_curve_identities.is_empty());
    assert_eq!(native.design_entity_headers.len(), 1);
}

#[test]
fn a_record_header_named_by_the_base_record_slot_is_written_with_its_sketch() {
    use crate::records::decal::DesignRecordHeader;
    use crate::records::references::DesignClassTag;

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut ir);
    sketch_container(&mut native, &[33]);
    native.design_record_headers.push(DesignRecordHeader {
        id: "f3d:generated:record-header#1".into(),
        record_index: 584,
        class_tag: DesignClassTag::try_from("269".to_owned()).expect("class tag"),
        byte_offset: 0,
    });
    drop(native);

    let (report, bytes, decoded) = write_with_bytes(&ir);
    assert_no_whole_drop(&report);
    assert_no_loss_naming(&report, "f3d:generated:record-header#1");
    let native = f3d_native(&decoded);
    let [header] = native.design_entity_headers.as_slice() else {
        panic!("one sketch header")
    };
    let base = header
        .sketch_references()
        .and_then(|list| list.record_reference);
    assert_eq!(base, Some(584));
    let scope = crate::ids::native_stream(&header.id)
        .expect("a stream-scoped header id")
        .to_owned();
    let written = crate::test_support::zip_test::with_scan(&bytes, |scan| {
        crate::design::decode::sketch::decode_related_record_headers(scan, &[(scope, 584)])
    })
    .expect("the written Design stream scans");
    let classes = written
        .iter()
        .map(|header| (header.record_index, header.class_tag.as_str().to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(classes, [(584, "269".to_owned())]);
}

#[test]
fn a_sketch_curve_link_that_names_a_dropped_curve_is_dropped_with_it() {
    use crate::records::sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let coedges = [0, 1].map(|index| ir.geometry.coedges[index].id.clone());
    let mut native = f3d_native_mut(&mut ir);
    sketch_container(&mut native, &[33]);
    native.sketch_curve_identities = vec![SketchCurveIdentity {
        id: "f3d:generated:sketch-curve#0".into(),
        record_index: 600,
        owner_reference: None,
        geometry_group: None,
        class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .expect("class tag"),
        byte_offset: 0,
        geometry_offset: 133,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(700).expect("a nonzero identity"),
        secondary_id: 701,
        geometry: Some(
            SketchCurveGeometry::line(
                Point3::new(10.0, 20.0, 0.0),
                Point3::new(40.0, 20.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("a sketch line"),
        ),
        planar_z_noise: None,
        tail_marker: None,
    }];
    native.sketch_curve_links = [(700, 701), (113, 0)]
        .into_iter()
        .zip(coedges)
        .enumerate()
        .map(
            |(ordinal, ((sketch_curve_id, ref_b), coedge))| SketchCurveLink {
                id: format!("f3d:generated:sketch-curve-link#{ordinal}"),
                target: AttributeTarget::Coedge(coedge),
                sketch_curve_id,
                ref_b,
                sense: Some(SketchLinkSense::try_from(1).expect("link sense")),
                role: 2,
                closure: 3,
            },
        )
        .collect();
    drop(native);

    let (report, decoded) = write_with_report(&ir);
    assert_no_whole_drop(&report);
    assert!(
        omitted(&report).iter().any(|message| {
            message.contains("of sketch-curve links that name a dropped sketch curve")
                && message.ends_with(": f3d:generated:sketch-curve-link#0")
        }),
        "{:#?}",
        report.losses
    );
    let native = f3d_native(&decoded);
    assert!(native.sketch_curve_identities.is_empty());
    let links = native
        .sketch_curve_links
        .iter()
        .map(|link| (link.sketch_curve_id, link.ref_b))
        .collect::<Vec<_>>();
    assert_eq!(links, [(113, 0)]);
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

/// The unit cube with a NURBS use curve on knots `[-2, 3]` on its first
/// coedge and an embedded-curve tolerant-coedge record whose range is
/// `record_range`.
fn tolerant_coedge_cube(record_range: [f64; 2]) -> (CadIr, String) {
    use cadmpeg_asm::brep::records::{TolerantCoedgeExtension, TolerantCoedgeParameters};
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let curve = cadmpeg_ir::ids::CurveId::mint("generated:test:tolerant-use-curve#0")
        .expect("identity grammar");
    let nurbs = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        1,
        vec![-2.0, -2.0, 3.0, 3.0],
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("a straight spline");
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
        source_object: None,
        native_ref: None,
    });
    ir.geometry.coedges[0].use_curve = Some(cadmpeg_ir::topology::CoedgeUseCurve {
        curve,
        parameter_range: cadmpeg_ir::topology::ParameterInterval::new([-2.0, 3.0])
            .expect("an interval"),
    });
    let record = TolerantCoedgeParameters {
        source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
        coedge: ir.geometry.coedges[0].id.clone(),
        record_index: 0,
        parameter_range: cadmpeg_ir::units::FiniteVector::new([0.0, 1.0]).expect("finite interval"),
        extension: TolerantCoedgeExtension::EmbeddedCurve {
            target: None,
            curve_reversed: false,
            payload_token_count: 0,
            parameter_range: Some(
                cadmpeg_ir::units::FiniteVector::new(record_range).expect("finite interval"),
            ),
        },
    };
    let id = record.id();
    f3d_native_mut(&mut ir).tolerant_coedge_parameters = vec![record];
    (ir, id)
}

#[test]
fn a_tolerant_coedge_range_past_the_knot_domain_by_round_off_is_written() {
    let (ir, _) = tolerant_coedge_cube([-2.0 - 1.0e-13, 3.0]);
    let decoded = write_and_decode(&ir);
    let use_curve = decoded
        .geometry
        .coedges
        .iter()
        .find_map(|coedge| coedge.use_curve.as_ref())
        .expect("the use curve reads back");
    assert_eq!(use_curve.parameter_range.endpoints(), [-2.0, 3.0]);
    assert_eq!(f3d_native(&decoded).tolerant_coedge_parameters.len(), 1);
}

#[test]
fn a_tolerant_coedge_record_with_another_range_is_written_with_the_use_curve_range() {
    let (ir, id) = tolerant_coedge_cube([-1.0, 3.0]);
    let (report, decoded) = write_with_report(&ir);
    assert_no_loss_naming(&report, &id);
    assert_no_loss_naming(&report, "use curve");
    let use_curve = decoded
        .geometry
        .coedges
        .iter()
        .find_map(|coedge| coedge.use_curve.as_ref())
        .expect("the use curve reads back");
    assert_eq!(use_curve.parameter_range.endpoints(), [-2.0, 3.0]);
    assert_eq!(f3d_native(&decoded).tolerant_coedge_parameters.len(), 1);
    assert_eq!(decoded.geometry.bodies.len(), 1);
}

#[test]
fn an_unset_tolerant_vertex_tail_on_a_vertex_with_a_tolerance_is_dropped_alone() {
    use cadmpeg_asm::brep::records::{EvaluatedToleranceSlot, TolerantVertexTail};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let tolerance = cadmpeg_ir::scalar::PositiveLength::new(0.025).expect("a length");
    ir.geometry.vertices[0].tolerance = Some(tolerance);
    let tail = TolerantVertexTail {
        source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
        record_index: 0,
        vertex: ir.geometry.vertices[0].id.clone(),
        leading_tolerances: [-1.0, -1.0]
            .map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite")),
        evaluated_slot: EvaluatedToleranceSlot::Unset { trailing: Some(0) },
    };
    let id = tail.id();
    f3d_native_mut(&mut ir).tolerant_vertex_tails = vec![tail];

    let report = assert_written_with_loss(
        &ir,
        None,
        OMITTED,
        "of tolerant-vertex records whose evaluated slot does not agree with the tolerance of their vertex",
    );
    assert_no_whole_drop(&report);
    assert!(
        omitted(&report)
            .iter()
            .any(|message| message.ends_with(&format!(": {id}"))),
        "{:#?}",
        report.losses
    );
    let decoded = write_and_decode(&ir);
    let vertex = decoded
        .geometry
        .vertices
        .iter()
        .find_map(|vertex| vertex.tolerance)
        .expect("the vertex tolerance reads back");
    assert!((vertex.get() - tolerance.get()).abs() <= 1.0e-12);
    assert_eq!(f3d_native(&decoded).tolerant_vertex_tails.len(), 1);
}

#[test]
fn a_tolerant_edge_tail_on_an_edge_with_no_tolerance_is_dropped_alone() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let tail = cadmpeg_asm::brep::records::TolerantEdgeTail {
        source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
        edge: ir.geometry.edges[0].id.clone(),
        record_index: 0,
        entity_revision: 22800,
        trailing_field: Some(1),
    };
    let id = tail.id();
    f3d_native_mut(&mut ir).tolerant_edge_tails = vec![tail];

    let report = assert_written_with_loss(
        &ir,
        None,
        OMITTED,
        "of tolerant-edge records on an edge with no tolerance",
    );
    assert_no_whole_drop(&report);
    assert!(
        omitted(&report)
            .iter()
            .any(|message| message.ends_with(&format!(": {id}"))),
        "{:#?}",
        report.losses
    );
    let decoded = write_and_decode(&ir);
    assert_eq!(decoded.geometry.bodies.len(), 1);
    assert!(f3d_native(&decoded).tolerant_edge_tails.is_empty());
}

/// Append to `ir` a copy of the unit cube under `prefix`, moved along x, and
/// return the copy's body.
fn append_cube(ir: &mut CadIr, prefix: &str, x: f64) -> cadmpeg_ir::topology::Body {
    let json = cadmpeg_ir::examples::unit_cube()
        .expect("unit cube fixture is admitted")
        .to_canonical_json()
        .expect("canonical cube JSON")
        .replace("synthetic:cube:", prefix);
    let mut cube = CadIr::from_json(&json).expect("renamed cube IR");
    cube.geometry.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("affine transform"),
    );
    let body = cube.geometry.bodies[0].clone();
    let geometry = &mut ir.geometry;
    let added = &mut cube.geometry;
    geometry.bodies.append(&mut added.bodies);
    geometry.regions.append(&mut added.regions);
    geometry.shells.append(&mut added.shells);
    geometry.faces.append(&mut added.faces);
    geometry.loops.append(&mut added.loops);
    geometry.coedges.append(&mut added.coedges);
    geometry.edges.append(&mut added.edges);
    geometry.vertices.append(&mut added.vertices);
    geometry.points.append(&mut added.points);
    geometry.surfaces.append(&mut added.surfaces);
    geometry.curves.append(&mut added.curves);
    body
}

/// A body F3D has no record form for is omitted alone. Each other body is
/// tested on its own without the native records of the document, with the
/// carrier metadata only those records hold cleared: the cube whose use
/// curve needs its tolerant-coedge record, and the cube whose edge has an
/// ownership record, are both written with their records.
#[test]
fn a_body_with_no_record_form_is_omitted_alone_beside_native_records() {
    use cadmpeg_asm::brep::records::EdgeOwnership;

    let (mut ir, _) = tolerant_coedge_cube([-2.0, 3.0]);
    append_cube(&mut ir, "synthetic:cube_two:", 30.0);
    let edge = ir
        .geometry
        .edges
        .iter()
        .find(|edge| edge.id.as_str().starts_with("synthetic:cube_two:"))
        .expect("the second cube has edges")
        .id
        .clone();
    f3d_native_mut(&mut ir).edge_ownerships = vec![EdgeOwnership {
        source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
        edge,
        record_index: 0,
        owner_coedge: None,
    }];
    let mut regionless = append_cube(&mut CadIr::empty(), "synthetic:cube_three:", 60.0);
    regionless.transform = None;
    ir.geometry.bodies.push(regionless.clone());

    let mut bytes = Vec::new();
    let report = cadmpeg_ir::codec::write::Encoder::plan(
        &crate::F3dCodec,
        cadmpeg_ir::codec::write::EncodeInput::new(&ir, None),
        cadmpeg_ir::codec::write::target::TargetRequest::Unnamed,
    )
    .and_then(|plan| plan.write_to(&mut bytes))
    .expect("the writer writes the document");
    let omitted_bodies = report
        .losses
        .iter()
        .filter(|loss| loss.message.contains("was not written, with the topology"))
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(omitted_bodies.len(), 1, "{:#?}", report.losses);
    assert!(
        omitted_bodies[0].starts_with(&format!("body {} was not written", regionless.id)),
        "{omitted_bodies:?}"
    );
    let decoded = write_and_decode(&ir);
    assert_eq!(decoded.geometry.bodies.len(), 2);
    assert!(decoded
        .geometry
        .coedges
        .iter()
        .any(|coedge| coedge.use_curve.is_some()));
    assert_eq!(decoded.geometry.faces.len(), 12);
    assert_eq!(f3d_native(&decoded).tolerant_coedge_parameters.len(), 1);
}
