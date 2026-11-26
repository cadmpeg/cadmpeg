// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::{classify, transfer, Bodies};
use crate::loss::IronCadLossCode;
use crate::test_support::{acis_stream, face_body, node, plane_face_body, record, Sab};
use crate::{dialect, tree};

const ACIS: u32 = 0x8000_0001;

fn classified(payload: &[u8]) -> (Bodies, Vec<LossNote>) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).unwrap();
    let records = tree::read(View::over_retained(payload)).unwrap();
    let mut losses = Vec::new();
    let bodies = classify(&ctx, &records, &mut dialect::layers(), &mut losses).unwrap();
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
        None,
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
        None,
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
    let placed = super::place(&ctx, &mut ir, vec![Vec::new()], &[], &mut losses).unwrap();
    assert!(placed.is_empty());
    assert_eq!(count(&losses, IronCadLossCode::PlacementUnbound), 1);
    assert!(ir.model.product_definitions.is_empty());
}
