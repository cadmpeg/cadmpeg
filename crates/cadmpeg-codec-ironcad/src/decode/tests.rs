// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::{classify, transfer, Bodies};
use crate::loss::IronCadLossCode;
use crate::test_support::{
    acis_stream, face_body, node, parasolid_record, plane_face_body, record, Sab,
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
    assert_eq!(count(&losses, IronCadLossCode::BodyUnreadable), 0);
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

#[test]
fn a_result_record_that_does_not_frame_is_unreadable_and_retained() {
    let broken = acis_stream(&[Sab::Name("body"), Sab::Ref(-1)]);
    let payload = part(&plane_face_body(), &broken);
    let (bodies, losses) = classified(&payload);
    assert_eq!(count(&losses, IronCadLossCode::BodyUnreadable), 1);
    assert!(bodies.brep.is_none());
    assert_eq!(bodies.retained.len(), 2);

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
    assert!(decoded.ir.geometry.bodies.is_empty());
    let retained = decoded
        .source_fidelity
        .retained_record("ironcad:source:kernel-record#1")
        .unwrap();
    assert_eq!(retained.data(), Some(broken.as_slice()));
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
    let placed = super::place(
        &ctx,
        &mut ir,
        vec![(None, Vec::new())],
        &[],
        &[],
        &mut losses,
    )
    .unwrap();
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

#[test]
fn a_parasolid_result_with_user_fields_is_unreadable_and_retained() {
    let payload = parasolid_part(&parasolid_record(CREATED, 1));
    let (bodies, losses) = classified(&payload);
    assert_eq!(count(&losses, IronCadLossCode::BodyUnreadable), 1);
    assert!(bodies.parasolid.is_empty());
    assert_eq!(bodies.retained.len(), 2);
}

#[test]
fn a_parasolid_result_that_the_neutral_framer_refuses_is_unreadable_and_retained() {
    let payload = parasolid_part(&parasolid_record(": another description", 0));
    let (bodies, losses) = classified(&payload);
    assert_eq!(count(&losses, IronCadLossCode::BodyUnreadable), 1);
    assert!(bodies.parasolid.is_empty());
    assert_eq!(bodies.retained.len(), 2);
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

/// A part or assembly record translated by `z` millimetres.
fn scene_record(name: &str, has_features: bool, z: f64) -> crate::placement::SceneRecord {
    crate::placement::SceneRecord {
        name: name.to_owned(),
        has_features,
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
    let placed =
        super::place(&ctx, &mut ir, results, &scene, tree_assemblies, &mut losses).unwrap();
    (ir, placed, losses)
}

#[test]
fn an_assembly_of_the_tree_holds_its_parts_and_is_placed_by_its_record() {
    let tree = [crate::tree::Assembly { parent: None }];
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
        crate::tree::Assembly { parent: Some(1) },
        crate::tree::Assembly { parent: None },
    ];
    let payload = plane_face_body();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    let results = vec![(Some(1), Vec::new()), (Some(0), Vec::new())];
    let placed = super::place(&ctx, &mut ir, results, &scene, &tree, &mut losses).unwrap();
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
