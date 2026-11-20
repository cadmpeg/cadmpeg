// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::features::surfaces::UntrimLoops;

use crate::design::feature_project::surface_untrim::project_surface_untrim;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::feature::surface_ops::{
    DesignSurfaceUntrimLoops, DesignSurfaceUntrimOperation,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_ir::features::{EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:scope#40";

fn group(record_index: u32, member: u32) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:Design/BulkStream.dat:group#{record_index}"),
        "scope_record_index": 40,
        "scope_reference_ordinal": 1,
        "record_index": record_index,
        "byte_offset": 1000,
        "class_tag": "300",
        "members": [member],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": 38_654_705_664_u64,
        "role_offset": 1054,
        "paired_class_tag": "258",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

fn scope(operation: Option<DesignSurfaceUntrimOperation>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(SCOPE_ID, DesignFeatureKind::SurfaceUntrim, 40);
    let DesignScopePayloadMut::SurfaceUntrim(slot) = scope.payload_mut() else {
        panic!("expected a SurfaceUntrim payload");
    };
    *slot = operation;
    scope
}

fn operation(loops: DesignSurfaceUntrimLoops, loop_groups: &[u32]) -> DesignSurfaceUntrimOperation {
    let mut operation = DesignSurfaceUntrimOperation::new(loops, 36, 41, 44);
    operation.extension = Some(crate::test_support::real(0.3));
    operation.extension_offset = Some(400);
    operation.loop_group_record_indices = loop_groups.to_vec();
    operation
}

fn project(
    operation: Option<DesignSurfaceUntrimOperation>,
    groups: &[DesignConstructionOperandGroup],
) -> (FaceSelection, UntrimLoops, Option<f64>, Vec<LossNote>) {
    let mut losses = Vec::new();
    let FeatureDefinition::Operation(FeatureOperation::UntrimSurface {
        faces,
        loops,
        extension,
    }) = project_surface_untrim(&scope(operation), groups, &[], &[], &mut losses)
    else {
        panic!("expected an untrim");
    };
    (
        faces,
        loops,
        extension.map(cadmpeg_ir::scalar::Length::get),
        losses,
    )
}

fn count(losses: &[LossNote], code: F3dLossCode) -> usize {
    losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

#[test]
fn an_all_loops_untrim_projects_its_face_group_and_extension_in_millimetres() {
    let (faces, loops, extension, losses) = project(
        Some(operation(DesignSurfaceUntrimLoops::All, &[])),
        &[group(44, 47)],
    );
    assert_eq!(
        faces,
        FaceSelection::Native("f3d:Design/BulkStream.dat:group#44".into())
    );
    assert_eq!(loops, UntrimLoops::All {});
    assert!((extension.unwrap() - 3.0).abs() < 1e-12);
    assert!(losses.is_empty());
}

#[test]
fn interior_and_exterior_loop_types_keep_the_settled_choice() {
    for (stored, expected) in [
        (DesignSurfaceUntrimLoops::Internal, UntrimLoops::Internal {}),
        (DesignSurfaceUntrimLoops::External, UntrimLoops::External {}),
    ] {
        let (_, loops, _, losses) = project(Some(operation(stored, &[])), &[group(44, 47)]);
        assert_eq!(loops, expected);
        assert!(losses.is_empty());
    }
}

#[test]
fn an_all_loops_untrim_with_a_stored_loop_group_charges_the_loop_type_assumption() {
    let (_, loops, _, losses) = project(
        Some(operation(DesignSurfaceUntrimLoops::All, &[50])),
        &[group(44, 47), group(50, 53)],
    );
    assert_eq!(loops, UntrimLoops::All {});
    assert_eq!(
        count(&losses, F3dLossCode::SurfaceUntrimStoredLoopGroupsAssumed),
        1
    );
}

#[test]
fn manual_loops_select_the_loop_group() {
    let (faces, loops, _, losses) = project(
        Some(operation(DesignSurfaceUntrimLoops::Manual, &[50])),
        &[group(44, 47), group(50, 53)],
    );
    assert_eq!(
        faces,
        FaceSelection::Native("f3d:Design/BulkStream.dat:group#44".into())
    );
    assert_eq!(
        loops,
        UntrimLoops::Selected {
            edges: EdgeSelection::Native("f3d:Design/BulkStream.dat:group#50".into()),
        }
    );
    assert!(losses.is_empty());
}

#[test]
fn missing_groups_keep_the_untrim_and_charge_the_group_assumption() {
    let (faces, loops, _, losses) = project(
        Some(operation(DesignSurfaceUntrimLoops::Manual, &[50])),
        &[],
    );
    assert_eq!(faces, FaceSelection::Native(SCOPE_ID.into()));
    assert_eq!(
        loops,
        UntrimLoops::Selected {
            edges: EdgeSelection::Native(SCOPE_ID.into()),
        }
    );
    assert_eq!(
        count(&losses, F3dLossCode::SurfaceUntrimGroupSelectionAssumed),
        2
    );
}

#[test]
fn manual_loop_groups_are_retained_as_a_scope_aggregate() {
    for missing in [false, true] {
        let mut groups = vec![group(44, 47), group(50, 53)];
        if !missing {
            groups.push(group(60, 63));
        }
        let (_, loops, _, losses) = project(
            Some(operation(DesignSurfaceUntrimLoops::Manual, &[50, 60])),
            &groups,
        );
        assert_eq!(
            loops,
            UntrimLoops::Selected {
                edges: EdgeSelection::Native(SCOPE_ID.into())
            }
        );
        assert_eq!(
            count(
                &losses,
                F3dLossCode::SurfaceUntrimLoopGroupsAggregatedAssumed
            ),
            1
        );
        assert_eq!(
            count(&losses, F3dLossCode::SurfaceUntrimGroupSelectionAssumed),
            usize::from(missing)
        );
    }
}

#[test]
fn duplicate_and_wrong_role_groups_preserve_the_scope_selection() {
    let mut wrong = serde_json::to_value(group(44, 47)).unwrap();
    wrong["role"] = serde_json::json!(68_719_476_736_u64);
    for groups in [
        vec![group(44, 47), group(44, 47)],
        vec![serde_json::from_value(wrong).unwrap()],
    ] {
        let (faces, loops, _, losses) =
            project(Some(operation(DesignSurfaceUntrimLoops::All, &[])), &groups);
        assert_eq!(faces, FaceSelection::Native(SCOPE_ID.into()));
        assert_eq!(loops, UntrimLoops::All {});
        assert_eq!(
            count(&losses, F3dLossCode::SurfaceUntrimGroupSelectionAssumed),
            1
        );
    }
}

#[test]
fn signed_missing_and_overflowing_extensions_keep_the_other_settings() {
    for (value, expected, loss) in [
        (
            Some(-0.3),
            Some(-3.0),
            Some(F3dLossCode::SurfaceUntrimNegativeExtensionAssumed),
        ),
        (
            Some(f64::MAX),
            None,
            Some(F3dLossCode::SurfaceUntrimExtensionUnrepresentable),
        ),
        (None, None, None),
    ] {
        let mut operation = operation(DesignSurfaceUntrimLoops::Manual, &[50]);
        operation.extension = value.map(crate::test_support::real);
        operation.extension_offset = value.map(|_| 400);
        let (faces, loops, extension, losses) =
            project(Some(operation), &[group(44, 47), group(50, 53)]);
        assert_eq!(
            faces,
            FaceSelection::Native("f3d:Design/BulkStream.dat:group#44".into())
        );
        assert_eq!(
            loops,
            UntrimLoops::Selected {
                edges: EdgeSelection::Native("f3d:Design/BulkStream.dat:group#50".into())
            }
        );
        assert_eq!(extension, expected);
        assert_eq!(losses.len(), usize::from(loss.is_some()));
        if let Some(loss) = loss {
            assert_eq!(count(&losses, loss), 1);
        }
    }
}

#[test]
fn an_unread_frame_keeps_the_untrim_with_unresolved_loops_and_extension() {
    let (faces, loops, extension, losses) = project(None, &[group(44, 47)]);
    assert_eq!(faces, FaceSelection::Native(SCOPE_ID.into()));
    assert_eq!(loops, UntrimLoops::Unresolved {});
    assert_eq!(extension, None);
    assert_eq!(count(&losses, F3dLossCode::SurfaceUntrimFrameAssumed), 1);
}
