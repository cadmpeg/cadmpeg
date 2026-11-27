// SPDX-License-Identifier: Apache-2.0
//! Scene rows compose with document occurrence ownership and archive scope.

use crate::scene::SceneBinding;
use crate::test_support::scene::{override_row, with_world};
use cadmpeg_core::serde_erase::json;

#[test]
fn native_scene_visibility_preserves_geometry_and_the_source_image() {
    let baseline = super::decode(crate::test_support::scene::archive(&[]));
    let mut world = Vec::new();
    crate::test_support::lp_utf16(&mut world, "VisibilityAttribute");
    world.extend_from_slice(&[0; 6]);
    let bytes = crate::test_support::scene::archive(&world);
    let decoded = super::decode(bytes.clone());
    assert_eq!(decoded.ir().geometry, baseline.ir().geometry);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::SceneVisibilityNative.kind()));
    let mut output = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut output)
        .unwrap();
    assert_eq!(output, bytes);
}

#[test]
fn scene_local_paths_bind_without_changing_occurrences_or_joints() {
    let (bulk, meta) = crate::test_support::component_design::nested_local_component_design(&[300]);
    let bytes = crate::test_support::body_map_test::f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let original = super::decode(bytes.clone());
    let mut world = override_row(&[100, 401]);
    world.extend(override_row(&[100, 401, 200, 402]));
    world.extend(override_row(&[100, 404, 200, 402]));
    let displayed = super::decode(with_world(bytes, &world));
    assert_eq!(
        original.ir().model.occurrences,
        displayed.ir().model.occurrences
    );
    assert_eq!(
        original.ir().model.assembly_joints,
        displayed.ir().model.assembly_joints
    );
    let native = crate::test_support::native_test::f3d_native(displayed.ir());
    assert_eq!(native.display_poses.len(), 3);
    let bound = native
        .display_poses
        .iter()
        .map(|row| {
            let SceneBinding::Bound { occurrence } = &row.binding else {
                panic!("expected bound row: {row:?}")
            };
            occurrence
        })
        .collect::<std::collections::BTreeSet<_>>();
    // §8.1: the module, placed twice, owns its child once, so the poses of
    // the child under both placements bind the one child occurrence; each
    // row keeps its own path.
    assert_eq!(bound.len(), 2);
}

#[test]
fn scene_archive_reads_root_rows_and_excludes_member_rows() {
    use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
    let root = with_world(
        f3d_without_brep("assembly-design", "root.f3d", &[("child.f3d", XREF_ROLE)]),
        &override_row(&[3, 17]),
    );
    let child = with_world(
        f3d_without_brep("part-design", "child.f3d", &[]),
        &override_row(&[3, 29]),
    );
    let result = super::decode(f3z_archive(
        "root.f3d",
        &[("root.f3d", &root), ("child.f3d", &child)],
    ));
    let native = crate::test_support::native_test::f3d_native(result.ir());
    assert_eq!(native.display_poses.len(), 1);
    assert!(matches!(
        native.display_poses[0].binding,
        SceneBinding::Unbound {}
    ));
    assert_eq!(
        json::to_value(&native.display_poses[0].path).unwrap(),
        cadmpeg_core::json!([3, 17])
    );
}

#[test]
fn scene_fully_tagged_local_paths_bind_without_changing_stored_placements() {
    let (bulk, meta) = crate::test_support::component_design::nested_local_component_design(&[300]);
    let bytes = crate::test_support::body_map_test::f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let original = super::decode(bytes.clone());
    let mut world = crate::test_support::scene::override_row_tagged(&[100, 401], 0);
    world.extend(crate::test_support::scene::override_row_tagged(
        &[100, 401, 200, 402],
        0,
    ));
    let displayed = super::decode(with_world(bytes, &world));
    let native = crate::test_support::native_test::f3d_native(displayed.ir());
    assert_eq!(native.display_poses.len(), 2);
    for row in &native.display_poses {
        assert_eq!(row.tagged_from, Some(0));
        assert!(matches!(row.binding, SceneBinding::Bound { .. }));
    }
    assert_eq!(
        original.ir().model.occurrences,
        displayed.ir().model.occurrences
    );
    assert_eq!(
        original.ir().model.assembly_joints,
        displayed.ir().model.assembly_joints
    );
}
