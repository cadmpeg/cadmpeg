// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::BrepEligibleFaceIndexes;

fn source_loop() -> crate::topology::Loop {
    crate::test_support::closed_loop(std::num::NonZeroU32::new(5), vec![crate::topology::HalfEdgeId {
            curve_id: 10,
            side: crate::topology::Side::Zero,
        }])
}

fn source_row() -> crate::curve::CurveTopologyRow {
    crate::curve::CurveTopologyRow {
        id: 10,
        type_byte: 0,
        feature_id: 1,
        directions: [0, 0],
        faces: [std::num::NonZeroU32::new(5), None],
        next_edges: [0, 0],
        offset: 12,
    }
}

fn index_limit_error(limit: u64) -> CodecError {
    let lp = source_loop();
    let faces = BTreeMap::from([(5, vec![&lp])]);
    let rows = [source_row()];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    BrepEligibleFaceIndexes::from_faces(&ctx, &faces, &rows)
        .err()
        .expect("eligible-face index allocation refused")
}

fn assert_collection_refusal(limit: u64, operation: &'static str) {
    let error = index_limit_error(limit);
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn brep_eligible_loop_refs_refuse_collection_limit() {
    assert_collection_refusal(0, "creo B-rep eligible loop refs");
}

#[test]
fn brep_emitted_half_edge_nodes_refuse_collection_limit() {
    assert_collection_refusal(1, "creo B-rep emitted half-edge nodes");
}

#[test]
fn brep_face_curve_id_nodes_refuse_collection_limit() {
    assert_collection_refusal(2, "creo B-rep face curve ID nodes");
}

#[test]
fn brep_single_edge_curve_nodes_refuse_collection_limit() {
    assert_collection_refusal(3, "creo B-rep single-edge curve nodes");
}

#[test]
fn brep_row_offset_nodes_refuse_collection_limit() {
    assert_collection_refusal(4, "creo B-rep row-offset nodes");
}

#[test]
fn brep_curve_face_nodes_refuse_collection_limit() {
    assert_collection_refusal(7, "creo B-rep curve-face nodes");
}

#[test]
fn brep_eligible_face_id_nodes_refuse_collection_limit() {
    assert_collection_refusal(8, "creo B-rep eligible face ID nodes");
}

#[test]
fn brep_eligible_face_indexes_preserve_service_order() {
    let lp = source_loop();
    let faces = BTreeMap::from([(5, vec![&lp])]);
    let rows = [source_row()];
    let indexes = crate::decode::with_test_decode_ctx(|ctx| {
        BrepEligibleFaceIndexes::from_faces(ctx, &faces, &rows)
    })
    .expect("service eligible-face indexes admitted");
    assert_eq!(
        indexes.emitted_half_edges,
        BTreeSet::from([lp.half_edges()[0]])
    );
    assert_eq!(indexes.face_curves, BTreeSet::from([10]));
    assert_eq!(indexes.closed_single_edge_curves, BTreeSet::from([10]));
    assert_eq!(indexes.row_offsets[&10], 12);
    assert_eq!(indexes.curve_faces[&10], [5, 0]);
    assert_eq!(indexes.eligible_face_ids, BTreeSet::from([5]));
}
