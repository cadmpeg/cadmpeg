// SPDX-License-Identifier: Apache-2.0
//! Validate the chamfer set lists of parameter-owned Chamfer scopes.
use super::{design_stream, push_constant_finding, Ctx};
use cadmpeg_ir::report::check::{Check, Finding};
use std::collections::HashSet;

/// Every chamfer set names a reference-table member of its scope as its edge
/// group, the scope itself, and owners of its scope whose parameters belong to
/// the namespace of its specification.
pub(super) fn validate(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    use crate::design::decode::scopes::chamfer_set::{
        two_distance_kinds_pair, ANGLE_KINDS, DISTANCE_KINDS, FIRST_DISTANCE_KINDS,
        SECOND_DISTANCE_KINDS,
    };
    use crate::records::feature::chamfer_set::DesignChamferSetSpec;
    for scope in &ctx.native.design_parameter_scopes {
        let Some(list) = scope.chamfer_sets() else {
            continue;
        };
        let native_stream = design_stream(&scope.id);
        let owned_kind = |owner_record_index: u32, kinds: &[&str]| {
            ctx.owners_by_index
                .get(&(native_stream, owner_record_index))
                .filter(|owner| owner.scope_record_index() == scope.record_index)
                .and_then(|owner| {
                    ctx.parameters_by_index
                        .get(&(native_stream, owner.parameter_record_index()))
                        .filter(|parameter| {
                            parameter.owner_record_index() == Some(owner.record_index())
                        })
                })
                .is_some_and(|parameter| kinds.contains(&parameter.source_kind_str()))
        };
        let parameter_kind = |owner_record_index: u32| {
            ctx.owners_by_index
                .get(&(native_stream, owner_record_index))
                .and_then(|owner| {
                    ctx.parameters_by_index
                        .get(&(native_stream, owner.parameter_record_index()))
                })
                .map(|parameter| parameter.source_kind_str())
        };
        let mut groups = HashSet::new();
        let valid = list.sets.iter().all(|set| {
            let spec = match set.spec {
                DesignChamferSetSpec::EqualDistance {
                    distance_owner_record_index,
                } => owned_kind(distance_owner_record_index, DISTANCE_KINDS),
                DesignChamferSetSpec::TwoDistances {
                    first_owner_record_index,
                    second_owner_record_index,
                } => {
                    owned_kind(first_owner_record_index, FIRST_DISTANCE_KINDS)
                        && owned_kind(second_owner_record_index, SECOND_DISTANCE_KINDS)
                        && matches!(
                            (
                                parameter_kind(first_owner_record_index),
                                parameter_kind(second_owner_record_index)
                            ),
                            (Some(first), Some(second))
                                if two_distance_kinds_pair(first, second)
                        )
                }
                DesignChamferSetSpec::DistanceAngle {
                    distance_owner_record_index,
                    angle_owner_record_index,
                } => {
                    owned_kind(distance_owner_record_index, DISTANCE_KINDS)
                        && owned_kind(angle_owner_record_index, ANGLE_KINDS)
                }
            };
            spec && groups.insert(set.group_record_index)
                && scope
                    .reference_members()
                    .values()
                    .any(|member| *member == set.group_record_index)
        }) && crate::design::design_feature_family(&scope.kind())
            == Some(crate::design::DesignFeatureFamily::Chamfer);
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Chamfer set names a record outside its scope or an owner of another parameter kind",
                Some(scope.id.clone()),
            );
        }
    }
}
