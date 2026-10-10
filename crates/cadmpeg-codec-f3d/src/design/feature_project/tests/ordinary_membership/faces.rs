// SPDX-License-Identifier: Apache-2.0
//! Full, partial, grouped and profile membership use their actual callers.

use super::{assert_original_refusal, fixtures};
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::feature::surface_ops::{
    DesignSurfaceOffsetOperation, DesignSurfaceOffsetSupport,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    FaceSelection, FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef,
};

#[derive(Clone, Copy)]
enum Lane {
    Full,
    Partial,
    Thread,
    Offset,
    Extrude,
}

enum Selection {
    Face(FaceSelection),
    Extrude(PlanarProfileRef),
}

impl Lane {
    fn operations(self) -> [&'static str; 2] {
        match self {
            Self::Full => [
                "visit F3D direct historical face membership",
                "compare F3D direct historical face identity",
            ],
            Self::Partial => [
                "visit F3D direct partial face membership",
                "compare F3D direct partial face identity",
            ],
            Self::Thread => [
                "visit F3D Thread selected face membership",
                "compare F3D Thread selected face identity",
            ],
            Self::Offset => [
                "visit F3D surface offset face membership",
                "compare F3D surface offset face identity",
            ],
            Self::Extrude => [
                "visit F3D Extrude profile face membership",
                "compare F3D Extrude profile face identity",
            ],
        }
    }
}

fn scope(lane: Lane) -> crate::records::feature::scope::DesignParameterScope {
    if matches!(lane, Lane::Extrude) {
        let mut scope = super::super::extrude_limits::scope();
        scope
            .try_edit(|draft| {
                draft.previous_history_state_id = Some(7);
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    } else {
        fixtures::scope(
            match lane {
                Lane::Thread => DesignFeatureKind::Thread,
                Lane::Offset => DesignFeatureKind::SurfaceOffset,
                _ => DesignFeatureKind::OffsetFaces,
            },
            true,
        )
    }
}

fn project(lane: Lane, slots: Vec<i64>, ctx: &DecodeContext<'_>) -> Result<Selection, CodecError> {
    let scope = scope(lane);
    let mut operands = vec![fixtures::face(200, 0, None, slots)];
    match lane {
        Lane::Full | Lane::Partial => {
            if matches!(lane, Lane::Partial) {
                operands.push(fixtures::face(201, 1, None, Vec::new()));
            }
            Ok(Selection::Face(
                super::super::super::direct_face_selection(ctx, &scope, &operands)?.unwrap(),
            ))
        }
        Lane::Thread | Lane::Offset => {
            let role = if matches!(lane, Lane::Thread) {
                DesignOperandRole::ROLE_0X10
            } else {
                DesignOperandRole::PROFILE
            };
            let group = fixtures::group(100, 0, role);
            operands[0].group = Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            });
            if matches!(lane, Lane::Thread) {
                super::super::super::project_thread_face_selection(
                    ctx,
                    &scope,
                    &[100],
                    &[group],
                    &operands,
                )
                .map(Selection::Face)
            } else {
                let operation = DesignSurfaceOffsetOperation {
                    distance: crate::test_support::real(0.25),
                    distance_offset: 0,
                    distance_record_index: 90,
                    support: DesignSurfaceOffsetSupport::FaceGroups {
                        group_record_indices: vec![100],
                    },
                };
                let Some(FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
                    faces,
                    distance,
                })) = super::super::super::project_surface_offset(
                    ctx,
                    &scope,
                    &operation,
                    &[group],
                    &operands,
                )?
                else {
                    panic!("resolved surface offset");
                };
                assert_eq!(distance.unwrap().get(), 2.5);
                Ok(Selection::Face(faces))
            }
        }
        Lane::Extrude => {
            let groups = [
                super::super::extrude_limits::profile_group(100, 0),
                super::super::extrude_limits::profile_group(101, 1),
            ];
            operands[0].group = Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            });
            operands.push(fixtures::face(201, 1, Some(101), vec![42, 44]));
            let parameter = crate::design::decode::parameters::parse_design_parameter_record(
                &crate::design::test_support::parameter_record(
                    Some(44),
                    "value",
                    "AlongDistance",
                    Some("mm"),
                    "d1",
                    0.55,
                ),
            )
            .unwrap();
            let Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(profile),
                ..
            })) = super::super::super::project_extrude(
                ctx,
                &scope,
                &[(0, &parameter)],
                &groups,
                &operands,
                &[],
                &[],
            )?
            else {
                panic!("historical multi-profile Extrude");
            };
            Ok(Selection::Extrude(profile))
        }
    }
}

fn assert_refusal(lane: Lane) {
    for operation in lane.operations() {
        assert_original_refusal(operation, |ctx| project(lane, vec![42, 43, 42, 44], ctx));
    }
}

fn assert_identity(lane: Lane) {
    crate::test_support::with_decode_context(|ctx| {
        let selection = project(lane, vec![42, 43, 42, 44], ctx).unwrap();
        let scope = scope(lane);
        let expected = fixtures::historical_faces(&scope, &[42, 43, 44]);
        let expected_state = fixtures::historical_state(&scope);
        match (lane, selection) {
            (
                Lane::Full | Lane::Thread,
                Selection::Face(FaceSelection::Historical {
                    state,
                    faces,
                    native,
                }),
            ) => {
                assert_eq!(state, expected_state);
                assert_eq!(faces.as_slice(), expected);
                let expected_native = if matches!(lane, Lane::Thread) {
                    fixtures::group(100, 0, DesignOperandRole::ROLE_0X10).id
                } else {
                    scope.id
                };
                assert_eq!(native.as_str(), expected_native);
            }
            (
                Lane::Partial,
                Selection::Face(FaceSelection::HistoricalPartial {
                    state,
                    faces,
                    unresolved,
                    native,
                }),
            ) => {
                assert_eq!(state, expected_state);
                assert_eq!(faces.as_slice(), expected);
                assert_eq!(
                    unresolved.as_slice(),
                    &[fixtures::face(201, 1, None, Vec::new()).id]
                );
                assert_eq!(native.as_str(), scope.id);
            }
            (Lane::Offset, Selection::Face(FaceSelection::Resolved { faces, native })) => {
                assert_eq!(
                    faces.iter().map(|face| face.as_str()).collect::<Vec<_>>(),
                    [
                        "f3d:brep:entity#42",
                        "f3d:brep:entity#43",
                        "f3d:brep:entity#44"
                    ]
                );
                assert_eq!(native, scope.id);
            }
            (
                Lane::Extrude,
                Selection::Extrude(PlanarProfileRef::HistoricalFaces {
                    state,
                    faces,
                    native,
                }),
            ) => {
                assert_eq!(state, expected_state);
                assert_eq!(faces.as_slice(), expected);
                assert_eq!(
                    native.as_slice(),
                    &[
                        super::super::extrude_limits::profile_group(100, 0).id,
                        super::super::extrude_limits::profile_group(101, 1).id,
                    ]
                );
            }
            _ => panic!("actual membership must preserve selection lane"),
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn membership_full_face_each_phase_keeps_original_refusal() {
    assert_refusal(Lane::Full);
}
#[test]
fn membership_partial_face_each_phase_keeps_original_refusal() {
    assert_refusal(Lane::Partial);
}
#[test]
fn membership_thread_face_each_phase_keeps_original_refusal() {
    assert_refusal(Lane::Thread);
}
#[test]
fn membership_offset_face_each_phase_keeps_original_refusal() {
    assert_refusal(Lane::Offset);
}
#[test]
fn membership_extrude_face_each_phase_keeps_original_refusal() {
    assert_refusal(Lane::Extrude);
}
#[test]
fn membership_full_face_keeps_native_identity_and_first_order() {
    assert_identity(Lane::Full);
}
#[test]
fn membership_partial_face_keeps_native_identity_and_first_order() {
    assert_identity(Lane::Partial);
}
#[test]
fn membership_thread_face_keeps_native_identity_and_first_order() {
    assert_identity(Lane::Thread);
}
#[test]
fn membership_offset_face_keeps_native_identity_and_first_order() {
    assert_identity(Lane::Offset);
}
#[test]
fn membership_extrude_face_keeps_native_identity_and_first_order() {
    assert_identity(Lane::Extrude);
}

#[test]
fn membership_single_face_skips_empty_query_in_all_direct_and_grouped_lanes() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    for lane in [Lane::Full, Lane::Partial, Lane::Thread, Lane::Offset] {
        for operation in lane.operations() {
            crate::test_support::with_decode_policy(&policy, |ctx| {
                let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
                assert!(project(lane, vec![42], ctx).is_ok());
                assert_eq!(ctx.resource_refusal(), None);
            });
        }
    }
}
