// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::FaceId;

use crate::records::sketch_links::PersistentSubentityTag;

fn fixture() -> (FaceId, Vec<PersistentSubentityTag>) {
    let face = FaceId::mint("f3d:brep:entity#candidate-1").expect("identity grammar");
    let tags = vec![PersistentSubentityTag {
        id: "f3d:asm:persistent-subentity-tag#7".into(),
        target: AttributeTarget::Face(face.clone()),
        selector: 1,
        token: cadmpeg_core::text::NonBlankString::try_from("3").expect("nonblank tag token"),
        design_references: vec![303],
        ordinal: 0,
    }];
    (face, tags)
}

fn scoped_candidate_faces<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    design_reference: i64,
    tags: &[PersistentSubentityTag],
    owner_id: Option<&str>,
) -> Result<(Vec<FaceId>, cadmpeg_core::decode::ScopedReservation<'ctx>), cadmpeg_core::CodecError>
{
    ctx.with_scoped_storage(
        "F3D validation expected edge operand candidate faces",
        || super::super::edge_operand_candidate_faces(ctx, design_reference, tags, owner_id),
    )
}

#[test]
fn edge_operand_candidate_faces_returns_matching_face() {
    let (face, tags) = fixture();
    crate::design::test_support::with_test_decode_context(|ctx| {
        let _storage;
        let decoded;
        (decoded, _storage) =
            scoped_candidate_faces(ctx, 303, &tags, None).expect("matching edge operand candidate");
        assert_eq!(decoded, [face]);
    });
}

#[test]
fn edge_operand_candidate_faces_refuses_tag_scan_work() {
    let (_, tags) = fixture();
    let operation = "scan F3D edge operand candidate tags";
    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| scoped_candidate_faces(ctx, 303, &tags, None).map(|_| ()),
    );
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == operation
                && failure.additional == 1
    ));
}

#[test]
fn edge_operand_candidate_faces_refuses_reference_comparison_work() {
    let (_, tags) = fixture();
    let operation = "find F3D edge operand candidate design reference";
    for (skip, additional) in [1_u64, 8, 8].into_iter().enumerate() {
        let refusal = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            skip,
            |ctx| scoped_candidate_faces(ctx, 303, &tags, None).map(|_| ()),
        );
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == operation
                    && failure.additional == additional
        ));
    }
}

#[test]
fn edge_operand_candidate_faces_refuses_face_id_copy_work() {
    let (face, tags) = fixture();
    let operation = "f3d operand face candidate ID";
    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| scoped_candidate_faces(ctx, 303, &tags, None).map(|_| ()),
    );
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == operation
                && failure.additional == u64_from_index(face.as_str().len())
    ));
}

#[test]
fn edge_operand_candidate_faces_refuses_face_id_copy_storage() {
    let (face, tags) = fixture();
    let operation = "f3d operand face candidate ID";
    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| scoped_candidate_faces(ctx, 303, &tags, None).map(|_| ()),
    );
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::MaterializedBytes
                && failure.operation == operation
                && failure.additional == u64_from_index(face.as_str().len())
    ));
}

#[test]
fn edge_operand_candidate_faces_refuses_collection_work() {
    let (_, tags) = fixture();
    let operation = "collect F3D edge operand candidate faces";
    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| scoped_candidate_faces(ctx, 303, &tags, None).map(|_| ()),
    );
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == operation
                && failure.additional == 1
    ));
}

#[test]
fn edge_operand_candidate_faces_refuses_collection_items() {
    let (_, tags) = fixture();
    let operation = "collect F3D edge operand candidate faces";
    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| scoped_candidate_faces(ctx, 303, &tags, None).map(|_| ()),
    );
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == operation
                && failure.additional == 1
    ));
}

#[test]
fn edge_operand_candidate_faces_refuses_collection_storage() {
    let (_, tags) = fixture();
    let operation = "collect F3D edge operand candidate faces";
    let minimum_bytes = std::mem::size_of::<FaceId>()
        .checked_mul(4)
        .expect("four FaceId slots");
    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| scoped_candidate_faces(ctx, 303, &tags, None).map(|_| ()),
    );
    // The first scoped growth reserves four FaceId slots.
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::MaterializedBytes
                && failure.operation == operation
                && failure.additional == u64_from_index(minimum_bytes)
    ));
}
