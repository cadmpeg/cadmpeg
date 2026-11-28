// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::{classify, transfer, Bodies};
use cadmpeg_ir::unknown::part::{gives_up, record_undecoded_kind, record_unreadable_kind};

use crate::loss::IronCadLossCode;
use crate::test_support::{
    acis_stream, face_body, face_body_on, node, parasolid_record, plane_face_body, record, Sab,
};
use crate::{dialect, tree};

const ACIS: u32 = 0x8000_0001;

fn classified(payload: &[u8]) -> (Bodies<'_>, Vec<LossNote>) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).unwrap();
    let tree = tree::read(View::over_retained(payload)).unwrap();
    let mut losses = Vec::new();
    let bodies = classify(&ctx, &tree.records, &mut dialect::layers(), &mut losses).unwrap();
    (bodies, losses)
}

fn count(losses: &[LossNote], code: IronCadLossCode) -> usize {
    let code = code.note("").code;
    losses.iter().filter(|loss| loss.code == code).count()
}

/// Binds `scene` by rank and places it.
fn ranked(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    results: &[(Option<usize>, Vec<cadmpeg_ir::ids::BodyId>)],
    scene: &[crate::placement::SceneRecord],
    tree: &[crate::tree::Assembly],
    losses: &mut Vec<LossNote>,
) -> Vec<cadmpeg_ir::ids::OccurrenceId> {
    let Some(places) = super::rank_binding(scene, results.len(), tree, losses) else {
        return Vec::new();
    };
    super::place(ctx, ir, results, scene, &places, tree, losses).unwrap()
}

/// A part pair: a feature record inside a node, then the result record.
fn part(feature: &[u8], result: &[u8]) -> Vec<u8> {
    node(&[
        node(&[node(&[node(&[])]), record(ACIS, feature)]),
        record(ACIS, result),
    ])
}

#[test]
fn the_result_record_decodes_and_the_feature_record_is_retained() {
    let body = plane_face_body();
    let payload = part(&body, &body);
    let (bodies, losses) = classified(&payload);
    assert_eq!(count(&losses, IronCadLossCode::FeatureShapeRetained), 1);
    assert_eq!(count(&losses, IronCadLossCode::BodyExtensionsRetained), 1);
    assert!(bodies.given_up.is_empty());
    let ids: Vec<_> = bodies
        .retained
        .iter()
        .map(|record| record.id().as_str().to_owned())
        .collect();
    assert_eq!(
        ids,
        [
            "ironcad:source:kernel-record#0",
            "ironcad:source:kernel-record#1"
        ]
    );

    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let decoded = transfer(
        &ctx,
        CadIr::empty(),
        bodies,
        super::Scene::default(),
        losses,
        Sha256Digest::digest(&payload),
    )
    .unwrap();
    assert_eq!(decoded.ir.geometry.bodies.len(), 1);
    assert_eq!(decoded.ir.geometry.faces.len(), 1);
    assert_eq!(
        count(&decoded.body.losses, IronCadLossCode::FaceBoundaryUnstated),
        1
    );
    assert_eq!(decoded.ir.model.product_definitions.len(), 1);
}

#[test]
fn result_records_of_two_parts_keep_disjoint_identities() {
    let body = plane_face_body();
    let payload = node(&[part(&body, &body), part(&body, &body)]);
    let (bodies, _) = classified(&payload);
    let brep = bodies.brep.unwrap();
    let ids: Vec<_> = brep.bodies.iter().map(|body| body.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "ironcad:brep/record-1/brep:entity#0",
            "ironcad:brep/record-3/brep:entity#0"
        ]
    );
}

/// The give-up loss of `bodies`' one given-up part, after the transfer
/// commits it, and the bytes its stub retains.
fn given_up(payload: &[u8], bodies: Bodies, losses: Vec<LossNote>) -> (LossNote, Vec<u8>) {
    let [part] = bodies.given_up.as_slice() else {
        panic!("one given-up part");
    };
    let stub = part.id().as_str().to_owned();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).unwrap();
    let decoded = transfer(
        &ctx,
        CadIr::empty(),
        bodies,
        super::Scene::default(),
        losses,
        Sha256Digest::digest(payload),
    )
    .unwrap();
    assert!(decoded.ir.geometry.bodies.is_empty());
    let given_up: Vec<_> = decoded
        .body
        .losses
        .iter()
        .filter(|loss| gives_up(&loss.code))
        .cloned()
        .collect();
    let [loss] = given_up.as_slice() else {
        panic!("one give-up loss: {given_up:?}");
    };
    let retained = decoded.source_fidelity.retained_record(&stub).unwrap();
    (loss.clone(), retained.data().unwrap().to_vec())
}

#[test]
fn a_result_record_that_does_not_frame_is_given_up_as_unreadable() {
    let broken = acis_stream(&[Sab::Name("body"), Sab::Ref(-1)]);
    let payload = part(&plane_face_body(), &broken);
    let (bodies, losses) = classified(&payload);
    assert!(bodies.brep.is_none());
    assert_eq!(bodies.retained.len(), 1);
    let (loss, retained) = given_up(&payload, bodies, losses);
    assert_eq!(loss.code, record_unreadable_kind());
    assert_eq!(retained, broken);
}

#[test]
fn a_result_record_whose_acis_prologue_does_not_read_is_given_up_as_undecoded() {
    let unknown = b"a kernel record of another ACIS save form".to_vec();
    let payload = part(&plane_face_body(), &unknown);
    let (bodies, losses) = classified(&payload);
    assert_eq!(count(&losses, IronCadLossCode::KernelPrologueUnreadable), 0);
    assert!(bodies.brep.is_none());
    let (loss, retained) = given_up(&payload, bodies, losses);
    assert_eq!(loss.code, record_undecoded_kind());
    assert_eq!(retained, unknown);
}

#[test]
fn a_face_on_an_unrecognized_surface_record_is_charged() {
    let body = face_body("mystery");
    let payload = part(&body, &body);
    let (bodies, losses) = classified(&payload);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let decoded = transfer(
        &ctx,
        CadIr::empty(),
        bodies,
        super::Scene::default(),
        losses,
        Sha256Digest::digest(&payload),
    )
    .unwrap();
    let codes: Vec<_> = decoded
        .body
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect();
    assert_eq!(
        count(&decoded.body.losses, IronCadLossCode::SurfaceUntyped),
        1,
        "{codes:?}"
    );
    assert_eq!(
        count(&decoded.body.losses, IronCadLossCode::BodyRecordsRetained),
        1,
        "{codes:?}"
    );
}

#[test]
fn part_records_that_do_not_match_the_part_results_are_unbound() {
    let payload = plane_face_body();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    let placed = ranked(&ctx, &mut ir, &[(None, Vec::new())], &[], &[], &mut losses);
    assert!(placed.is_empty());
    assert_eq!(count(&losses, IronCadLossCode::PlacementUnbound), 1);
    assert!(ir.model.product_definitions.is_empty());
}

#[test]
fn bytes_after_the_solved_records_are_charged_and_retained() {
    let mut body = plane_face_body();
    body.extend_from_slice(b"tail");
    let payload = part(&plane_face_body(), &body);
    let (bodies, losses) = classified(&payload);
    assert_eq!(count(&losses, IronCadLossCode::BodyPartitionUnread), 1);
    assert!(bodies.brep.is_some());
    assert_eq!(bodies.retained.len(), 2);
}

const PARASOLID: u32 = 0x8000_0002;

const CREATED: &str = ": TRANSMIT FILE created by modeller version 3401176";

/// A Parasolid part pair whose result record is `result`.
fn parasolid_part(result: &[u8]) -> Vec<u8> {
    node(&[
        node(&[
            node(&[node(&[])]),
            record(PARASOLID, &parasolid_record(CREATED, 0)),
        ]),
        record(PARASOLID, result),
    ])
}

/// Classifies a Parasolid part pair whose result record is `result`, and
/// asserts that the result record is given up as undecoded.
fn assert_parasolid_result_given_up(result: &[u8]) {
    let payload = parasolid_part(result);
    let (bodies, losses) = classified(&payload);
    assert!(bodies.parasolid.is_empty());
    assert_eq!(bodies.retained.len(), 1);
    let (loss, retained) = given_up(&payload, bodies, losses);
    assert_eq!(loss.code, record_undecoded_kind());
    assert_eq!(retained, result);
}

#[test]
fn a_parasolid_result_with_user_fields_is_given_up_as_undecoded() {
    assert_parasolid_result_given_up(&parasolid_record(CREATED, 1));
}

#[test]
fn a_parasolid_result_that_the_neutral_framer_refuses_is_given_up_as_undecoded() {
    assert_parasolid_result_given_up(&parasolid_record(": another description", 0));
}

#[test]
fn a_failed_result_leaves_a_single_decoded_result_unqualified() {
    let broken = acis_stream(&[Sab::Name("body"), Sab::Ref(-1)]);
    let body = plane_face_body();
    let payload = node(&[part(&body, &broken), part(&body, &body)]);
    let (bodies, _) = classified(&payload);
    let ranks: Vec<_> = bodies
        .results
        .iter()
        .map(|(ordinal, ids)| (*ordinal, ids.len()))
        .collect();
    assert_eq!(ranks, [(1, 0), (3, 1)]);
    let brep = bodies.brep.unwrap();
    let ids: Vec<_> = brep.bodies.iter().map(|body| body.id.as_str()).collect();
    assert_eq!(ids, ["ironcad:brep:entity#0"]);
}

#[test]
fn a_history_partition_is_charged_and_retained() {
    let solved = plane_face_body();
    let end = solved.len() - 18;
    let mut body = solved[..end].to_vec();
    body.extend_from_slice(&[0x0d, 16]);
    body.extend_from_slice(b"delta_state%");
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.push(0x04);
    body.extend_from_slice(&0_i32.to_le_bytes());
    body.push(0x11);
    body.extend_from_slice(&solved[end..]);
    let payload = part(&plane_face_body(), &body);
    let (bodies, losses) = classified(&payload);
    assert_eq!(count(&losses, IronCadLossCode::BodyPartitionUnread), 1);
    assert!(bodies.brep.is_some());
    assert_eq!(bodies.retained.len(), 2);
}

#[test]
fn a_face_whose_surface_names_no_record_is_given_up_in_its_result_record() {
    let body = face_body_on("plane", 9);
    let payload = node(&[part(&body, &body), part(&body, &body)]);
    let (bodies, losses) = classified(&payload);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let decoded = transfer(
        &ctx,
        CadIr::empty(),
        bodies,
        super::Scene::default(),
        losses,
        Sha256Digest::digest(&payload),
    )
    .unwrap();
    let mut charges: Vec<_> = decoded
        .body
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect();
    charges.sort_unstable();
    assert_eq!(charges.len(), 2, "{charges:#?}");
    for (charge, ordinal) in charges.iter().zip([1, 3]) {
        assert!(
            charge.starts_with(&format!(
                "part face stream=\"ironcad:Brep Data/record-{ordinal}\" offset="
            )),
            "{charge}"
        );
        assert!(
            charge.contains("the face record's surface reference names no record"),
            "{charge}"
        );
        assert!(
            charge.ends_with(&format!(
                "; dropped=[ironcad:brep/record-{ordinal}/brep:entity#3]"
            )),
            "{charge}"
        );
    }
}

#[test]
fn a_sole_result_record_gives_up_its_withheld_face_under_the_unqualified_id() {
    let body = face_body_on("plane", 9);
    let payload = part(&body, &body);
    let (bodies, losses) = classified(&payload);
    assert!(bodies.pending.is_empty());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let decoded = transfer(
        &ctx,
        CadIr::empty(),
        bodies,
        super::Scene::default(),
        losses,
        Sha256Digest::digest(&payload),
    )
    .unwrap();
    let charges: Vec<_> = decoded
        .body
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect();
    assert_eq!(charges.len(), 1, "{charges:#?}");
    assert!(
        charges[0].starts_with("part face stream=\"ironcad:Brep Data/record-1\" offset="),
        "{}",
        charges[0]
    );
    assert!(
        charges[0].ends_with("; dropped=[ironcad:brep:entity#3]"),
        "{}",
        charges[0]
    );
}

/// A part or assembly record translated by `z` millimetres.
fn scene_record(name: &str, has_features: bool, z: f64) -> crate::placement::SceneRecord {
    use crate::placement::Role;
    crate::placement::SceneRecord {
        name: name.to_owned(),
        role: if has_features {
            Role::Part
        } else {
            Role::Assembly
        },
        id: None,
        copy: false,
        paths: Vec::new(),
        matrix_searched: false,
        version_unrecognized: false,
        embedded: None,
        transform: cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, z],
        ]),
        properties: Vec::new(),
        unread_property: None,
    }
}

/// Places the parts `a` and `b` and the assembly record `asm` with the tree
/// assemblies `tree_assemblies`; `b` lies in the assembly at index
/// `b_assembly`.
fn placed_scene(
    tree_assemblies: &[crate::tree::Assembly],
    b_assembly: Option<usize>,
) -> (CadIr, Vec<cadmpeg_ir::ids::OccurrenceId>, Vec<LossNote>) {
    let scene = [
        scene_record("a", true, 1.0),
        scene_record("b", true, 2.0),
        scene_record("asm", false, 10.0),
    ];
    let payload = plane_face_body();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    let results = vec![(None, Vec::new()), (b_assembly, Vec::new())];
    let placed = ranked(
        &ctx,
        &mut ir,
        &results,
        &scene,
        tree_assemblies,
        &mut losses,
    );
    (ir, placed, losses)
}

#[test]
fn an_assembly_of_the_tree_holds_its_parts_and_is_placed_by_its_record() {
    let tree = [crate::tree::Assembly {
        parent: None,
        end: 0,
    }];
    let (ir, placed, losses) = placed_scene(&tree, Some(0));
    assert_eq!(count(&losses, IronCadLossCode::AssemblyBoundByOrder), 1);
    assert_eq!(count(&losses, IronCadLossCode::AssemblyFlattened), 0);
    let name = |id: &cadmpeg_ir::ids::OccurrenceId| {
        let occurrence = ir.model.occurrences.iter().find(|o| &o.id == id).unwrap();
        let z = occurrence.transform.unwrap().affine_rows()[2][3];
        (occurrence.name.clone().unwrap(), z)
    };
    let root: Vec<_> = placed.iter().map(name).collect();
    assert_eq!(root, [("a".to_owned(), 1.0), ("asm".to_owned(), 10.0)]);
    let assembly = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.source_name.as_deref() == Some("asm"))
        .unwrap();
    assert_eq!(
        assembly.kind,
        Some(cadmpeg_ir::products::ProductDefinitionKind::Assembly)
    );
    let members: Vec<_> = assembly.occurrences.iter().map(name).collect();
    assert_eq!(members, [("b".to_owned(), 2.0)]);
}

#[test]
fn assembly_records_that_do_not_match_the_tree_are_flattened() {
    let (ir, placed, losses) = placed_scene(&[], None);
    assert_eq!(count(&losses, IronCadLossCode::AssemblyFlattened), 1);
    assert_eq!(placed.len(), 2);
    assert!(ir
        .model
        .product_definitions
        .iter()
        .all(|definition| definition.source_name.as_deref() != Some("asm")));
}

#[test]
fn a_member_assembly_is_placed_in_the_assembly_that_holds_it() {
    let scene = [
        scene_record("a", true, 1.0),
        scene_record("b", true, 2.0),
        scene_record("inner", false, 10.0),
        scene_record("outer", false, 100.0),
    ];
    let tree = [
        crate::tree::Assembly {
            parent: Some(1),
            end: 0,
        },
        crate::tree::Assembly {
            parent: None,
            end: 0,
        },
    ];
    let payload = plane_face_body();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    let results = vec![(Some(1), Vec::new()), (Some(0), Vec::new())];
    let placed = ranked(&ctx, &mut ir, &results, &scene, &tree, &mut losses);
    assert_eq!(count(&losses, IronCadLossCode::AssemblyFlattened), 0);
    let name = |id: &cadmpeg_ir::ids::OccurrenceId| {
        let occurrence = ir.model.occurrences.iter().find(|o| &o.id == id).unwrap();
        occurrence.name.clone().unwrap()
    };
    let members = |assembly: &str| -> Vec<String> {
        let definition = ir
            .model
            .product_definitions
            .iter()
            .find(|definition| definition.source_name.as_deref() == Some(assembly))
            .unwrap();
        definition.occurrences.iter().map(name).collect()
    };
    assert_eq!(placed.iter().map(name).collect::<Vec<_>>(), ["outer"]);
    assert_eq!(members("outer"), ["a", "inner"]);
    assert_eq!(members("inner"), ["b"]);
}

#[test]
fn a_record_with_an_unread_property_is_charged() {
    let mut record = scene_record("Part1", true, 0.0);
    record.unread_property = Some("Odd".to_owned());
    let mut losses = Vec::new();
    super::property_losses(&[record, scene_record("Part2", true, 0.0)], &mut losses);
    assert_eq!(count(&losses, IronCadLossCode::PropertyUnread), 1);
}

#[test]
fn unit_notes_are_charged_by_kind() {
    let scene = crate::units::SceneUnits {
        units: None,
        unrecognized: vec!["unlisted".to_owned()],
        conflicts: vec!["contradicting".to_owned(), "contradicting".to_owned()],
    };
    let mut losses = Vec::new();
    assert!(super::unit_losses(scene, &mut losses).is_none());
    assert_eq!(count(&losses, IronCadLossCode::UnitsUnrecognized), 1);
    assert_eq!(count(&losses, IronCadLossCode::UnitsScaleConflict), 2);
}

#[test]
fn a_body_property_set_names_the_body_and_a_searched_matrix_is_charged() {
    use crate::placement::{PropertyValue, BODY_SET_CLASS};
    let body = plane_face_body();
    let payload = part(&body, &body);
    let (bodies, losses) = classified(&payload);
    let mut record = scene_record("Part1", true, 0.0);
    record.matrix_searched = true;
    record.embedded = Some((
        BODY_SET_CLASS,
        vec![
            (
                "PartName".to_owned(),
                PropertyValue::Text("Body<1>".to_owned()),
            ),
            (
                "PartVolume".to_owned(),
                PropertyValue::Text("0,5".to_owned()),
            ),
        ],
    ));
    let scene = super::Scene {
        records: Some(vec![record]),
        ..super::Scene::default()
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let decoded = transfer(
        &ctx,
        CadIr::empty(),
        bodies,
        scene,
        losses,
        Sha256Digest::digest(&payload),
    )
    .unwrap();
    assert_eq!(
        decoded.ir.geometry.bodies[0].name.as_deref(),
        Some("Body<1>")
    );
    let losses = &decoded.body.losses;
    assert_eq!(count(losses, IronCadLossCode::PlacementMatrixSearched), 1);
    let unread: Vec<_> = losses
        .iter()
        .filter(|loss| loss.code == IronCadLossCode::PropertyUnread.note("").code)
        .map(|loss| loss.message.clone())
        .collect();
    assert_eq!(unread.len(), 1);
    assert!(unread[0].contains("PartVolume") && !unread[0].contains("PartName"));
}

#[test]
fn a_copy_stored_before_its_part_leaves_the_definition_to_the_part_record() {
    use crate::binding::Place;
    use crate::placement::PropertyValue;
    let mut copy = scene_record("", true, 5.0);
    copy.copy = true;
    let mut part = scene_record("Stool", true, 1.0);
    part.properties = vec![(
        "Part Number".to_owned(),
        PropertyValue::Text("S-1".to_owned()),
    )];
    let scene = [copy, part];
    let payload = plane_face_body();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    let places = [Some(Place::Part(0)), Some(Place::Part(0))];
    let placed = super::place(
        &ctx,
        &mut ir,
        &[(None, Vec::new())],
        &scene,
        &places,
        &[],
        &mut losses,
    )
    .unwrap();
    assert_eq!(placed.len(), 2);
    assert_eq!(ir.model.product_definitions.len(), 1);
    let definition = &ir.model.product_definitions[0];
    assert_eq!(definition.source_name.as_deref(), Some("Stool"));
    assert_eq!(definition.part_number.as_deref(), Some("S-1"));
}

#[test]
fn part_results_in_unbound_assembly_nodes_are_charged_as_flattened() {
    let scene = [scene_record("a", true, 1.0)];
    let objects = [crate::shape_contents::SceneObject {
        id: 1,
        name: "a".to_owned(),
        end: 100,
        kind: crate::shape_contents::ObjectKind::Body,
    }];
    let spans = [crate::binding::ResultSpan {
        child_start: 10,
        end: 100,
        assembly: Some(0),
    }];
    let tree = [crate::tree::Assembly {
        parent: None,
        end: 200,
    }];
    let mut losses = Vec::new();
    let places = super::identity_binding(&scene, &objects, &spans, &tree, &mut losses);
    assert_eq!(places, [Some(crate::binding::Place::Part(0))]);
    assert_eq!(count(&losses, IronCadLossCode::AssemblyFlattened), 1);
}
