// SPDX-License-Identifier: Apache-2.0
use super::{localized_fillet_scope, localized_fillet_operand_groups, localized_fillet_parameter, localized_fillet_owner};
use crate::design::decode::operands::decode_fillet_radius_groups as decode_fillet_radius_groups_charged;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::fillet::DesignFilletRadiusGroup;

pub(super) fn decode_fillet_radius_groups(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Vec<DesignFilletRadiusGroup> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    decode_fillet_radius_groups_charged(&ctx, scopes, groups, owners, parameters).unwrap()
}

#[test]
fn fillet_radius_group_collections_and_ids_refuse_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let scope = localized_fillet_scope();
    let groups = localized_fillet_operand_groups();
    let parameter = localized_fillet_parameter;
    let owner = localized_fillet_owner;
    let scenarios = [
        (
            vec![parameter(10, 11, "Radius", Some("mm"), 0.5), parameter(30, 31, "TangencyWeight", None, 1.0)],
            vec![owner(10, 11, 0), owner(30, 31, 1)],
        ),
        (
            vec![parameter(10, 11, "ChordLen", Some("mm"), 0.5)],
            vec![owner(10, 11, 0)],
        ),
        (
            vec![parameter(10, 11, "EdgeOffset1", Some("mm"), 0.2), parameter(20, 21, "EdgeOffset2", Some("mm"), 0.3), parameter(30, 31, "TangencyWeight", None, 1.0)],
            vec![owner(10, 11, 0), owner(20, 21, 1), owner(30, 31, 2)],
        ),
        (
            vec![parameter(10, 11, "StartRadius", Some("mm"), 0.2), parameter(20, 21, "EndRadius", Some("mm"), 0.3), parameter(30, 31, "MidRadius", Some("mm"), 0.4), parameter(40, 41, "MidParams", None, 0.5), parameter(50, 51, "TangencyWeight", None, 1.0)],
            vec![owner(10, 11, 0), owner(20, 21, 1), owner(30, 31, 2), owner(40, 41, 3), owner(50, 51, 4)],
        ),
    ];
    let mut refused = std::collections::HashSet::new();
    for (parameters, owners) in &scenarios {
        for limit in 0..32 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match decode_fillet_radius_groups_charged(&ctx, std::slice::from_ref(&scope), &groups[..1], owners, parameters) {
                Err(CodecError::ResourceLimit(failure)) => {
                    assert_eq!(failure.dimension, ResourceDimension::CollectionItems);
                    refused.insert(failure.operation);
                }
                Ok(assignments) => assert_eq!(assignments.len(), 1),
                Err(error) => panic!("unexpected Fillet decode error: {error}"),
            }
        }
    }
    for operation in [
        "f3d Fillet parameter index", "f3d Fillet scope groups",
        "f3d Fillet owned parameters", "f3d Fillet radius parameters",
        "f3d Fillet weight parameters", "f3d Fillet chord lengths",
        "f3d Fillet asymmetric offsets", "f3d Fillet variable parameters",
        "f3d Fillet middle parameters", "f3d Fillet edge operand indices",
        "f3d Fillet group output",
    ] {
        assert!(refused.contains(operation), "no limit refusal at {operation}");
    }

    let stream = "f3d:native/BulkStream.dat";
    let group_id_len = stream.len() + ":design-fillet-radius-group#".len() + 3;
    for (limit, operation) in [
        (stream.len() - 1, "f3d Fillet group stream ID"),
        (group_id_len - 1, "f3d Fillet group ID suffix"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(limit).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            decode_fillet_radius_groups_charged(&ctx, std::slice::from_ref(&scope), &groups[..1], &scenarios[0].1, &scenarios[0].0),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes && failure.operation == operation
        ));
    }
}
