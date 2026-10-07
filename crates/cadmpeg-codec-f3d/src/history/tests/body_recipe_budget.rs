// SPDX-License-Identifier: Apache-2.0

fn complete_body_topology() -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    AsmHistoricalTopology {
        bodies: vec![1],
        regions: vec![2],
        shells: vec![3],
        faces: vec![10],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, vec![10])],
        ..Default::default()
    }
}

fn complete_body_error(operation: &str) -> cadmpeg_core::CodecError {
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| {
            let topology = complete_body_topology();
            let index = super::super::body_face_index(ctx, &topology)?;
            super::super::complete_body_face_slots(ctx, &index, 1)
        },
    )
}

macro_rules! complete_body_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let error = complete_body_error($operation);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

complete_body_limit_test!(
    complete_body_entity_counts_refuse_collection_limit,
    "index F3D complete body entity counts"
);
complete_body_limit_test!(
    complete_body_relation_owners_refuse_collection_limit,
    "index F3D complete body relation owners"
);
complete_body_limit_test!(
    complete_body_relation_members_refuse_collection_limit,
    "index F3D complete body relation members"
);
complete_body_limit_test!(
    complete_body_regions_refuse_collection_limit,
    "collect F3D complete body regions"
);
complete_body_limit_test!(
    complete_body_shells_refuse_collection_limit,
    "collect F3D complete body shells"
);
complete_body_limit_test!(
    complete_body_faces_refuse_collection_limit,
    "collect F3D complete body faces"
);
complete_body_limit_test!(
    complete_body_face_slots_refuse_collection_limit,
    "collect F3D complete body face slots"
);
