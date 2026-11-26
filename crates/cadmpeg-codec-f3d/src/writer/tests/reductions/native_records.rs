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
fn an_edge_continuity_the_asm_edge_record_does_not_hold_is_dropped_alone() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let rows = vec![
        continuity(&ir, 0, 0, "convex"),
        continuity(&ir, 1, 1, "tangent"),
    ];
    let convex = rows[0].id().clone();
    f3d_native_mut(&mut ir).edge_continuities = rows;

    let report = assert_written_with_loss(
        &ir,
        None,
        OMITTED,
        "of edge continuities other than `tangent` and `unknown`",
    );
    assert_no_whole_drop(&report);
    assert!(
        omitted(&report)
            .iter()
            .any(|message| message.ends_with(&format!(": {convex}"))),
        "{:#?}",
        report.losses
    );
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
fn a_tolerant_coedge_record_with_another_range_is_dropped_alone() {
    let (ir, id) = tolerant_coedge_cube([-1.0, 3.0]);
    let report = assert_written_with_loss(
        &ir,
        None,
        OMITTED,
        "of tolerant-coedge records whose parameter range does not give the use-curve range",
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
    assert!(f3d_native(&decoded).tolerant_coedge_parameters.is_empty());
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
