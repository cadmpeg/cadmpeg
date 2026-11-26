// SPDX-License-Identifier: Apache-2.0

use super::{bind_shell_removed_faces, removed_face_selection, wall_offset, ShellWalls};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalEntityDelta,
    AsmHistoricalPlane, AsmHistoricalRowsDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition,
};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::operation_settings::ShellMode;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::math::{Point3, Vector3};

fn point(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn vector(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// One state of plane and cylinder faces. Face `n` uses surface `100 + n`.
#[derive(Default)]
struct State(AsmHistoricalTopology);

impl State {
    fn bind(&mut self, face: i64) -> i64 {
        let surface = 100 + face;
        self.0.faces.push(face);
        self.0.surfaces.push(surface);
        self.0.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: face,
            carrier: surface,
        });
        surface
    }

    fn plane(mut self, face: i64, origin: Point3, normal: Vector3) -> Self {
        let surface = self.bind(face);
        self.0.surface_planes.push(AsmHistoricalPlane {
            surface,
            origin,
            normal,
        });
        self
    }

    fn cylinder(mut self, face: i64, origin: Point3, axis: Vector3, radius: f64) -> Self {
        let surface = self.bind(face);
        self.0.surface_cylinders.push(AsmHistoricalCylinder {
            surface,
            origin,
            axis,
            radius,
        });
        self
    }
}

/// A 40 x 30 x 20 mm box: bottom 1, top 2, sides 3 to 6.
fn box_faces() -> State {
    State::default()
        .plane(1, point(20.0, 15.0, 0.0), vector(0.0, 0.0, 1.0))
        .plane(2, point(20.0, 15.0, 20.0), vector(0.0, 0.0, 1.0))
        .plane(3, point(0.0, 30.0, 0.0), vector(-1.0, 0.0, 0.0))
        .plane(4, point(40.0, 0.0, 0.0), vector(1.0, 0.0, 0.0))
        .plane(5, point(0.0, 0.0, 0.0), vector(0.0, -1.0, 0.0))
        .plane(6, point(40.0, 30.0, 0.0), vector(0.0, 1.0, 0.0))
}

/// The box shelled 2 mm inside with its top open: the top keeps its slot as
/// the rim, and faces 11 to 15 are the inner walls of faces 1 and 3 to 6.
fn inside_shell() -> State {
    box_faces()
        .plane(11, point(20.0, 15.0, 2.0), vector(0.0, 0.0, -1.0))
        .plane(13, point(2.0, 30.0, 0.0), vector(1.0, 0.0, 0.0))
        .plane(14, point(38.0, 0.0, 0.0), vector(-1.0, 0.0, 0.0))
        .plane(15, point(0.0, 2.0, 0.0), vector(0.0, 1.0, 0.0))
        .plane(16, point(40.0, 28.0, 0.0), vector(0.0, -1.0, 0.0))
}

fn transition(updated: &[i64], deleted: &[i64]) -> AsmHistoricalTransition {
    AsmHistoricalTransition {
        geometry_unchanged: false,
        previous_state_id: Some(3),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta {
            faces: AsmHistoricalEntityDelta {
                updated: updated.to_vec(),
                deleted: deleted.to_vec(),
                ..AsmHistoricalEntityDelta::default()
            },
            ..AsmHistoricalTopologyDelta::default()
        },
        record_revisions: Vec::new(),
        rows: AsmHistoricalRowsDelta::default(),
    }
}

fn removed(
    members: &[u32],
    offset: f64,
    transition: &AsmHistoricalTransition,
    output: &State,
) -> Option<Vec<i64>> {
    removed_face_selection(members, offset, transition, &box_faces().0, &output.0)
}

#[test]
fn the_changed_face_without_a_wall_is_the_removed_face() {
    assert_eq!(
        removed(&[7], 2.0, &transition(&[2], &[]), &inside_shell()),
        Some(vec![2])
    );
}

#[test]
fn a_changed_face_with_a_wall_is_kept() {
    assert_eq!(
        removed(
            &[7],
            2.0,
            &transition(&[1, 2, 3, 4, 5, 6], &[]),
            &inside_shell()
        ),
        Some(vec![2])
    );
}

#[test]
fn a_deleted_face_without_a_wall_is_removed() {
    let mut output = State::default()
        .plane(1, point(20.0, 15.0, 0.0), vector(0.0, 0.0, 1.0))
        .plane(3, point(0.0, 30.0, 0.0), vector(-1.0, 0.0, 0.0))
        .plane(4, point(40.0, 0.0, 0.0), vector(1.0, 0.0, 0.0))
        .plane(5, point(0.0, 0.0, 0.0), vector(0.0, -1.0, 0.0))
        .plane(6, point(40.0, 30.0, 0.0), vector(0.0, 1.0, 0.0));
    for (face, origin, normal) in [
        (11, point(20.0, 15.0, 2.0), vector(0.0, 0.0, -1.0)),
        (13, point(2.0, 30.0, 0.0), vector(1.0, 0.0, 0.0)),
        (14, point(38.0, 0.0, 0.0), vector(-1.0, 0.0, 0.0)),
        (15, point(0.0, 2.0, 0.0), vector(0.0, 1.0, 0.0)),
        (16, point(40.0, 28.0, 0.0), vector(0.0, -1.0, 0.0)),
    ] {
        output = output.plane(face, origin, normal);
    }
    assert_eq!(
        removed(&[7], 2.0, &transition(&[], &[2]), &output),
        Some(vec![2])
    );
}

#[test]
fn an_unchanged_face_without_a_wall_is_not_removed() {
    let output = inside_shell().plane(30, point(90.0, 0.0, 0.0), vector(1.0, 0.0, 0.0));
    let input = box_faces().plane(30, point(90.0, 0.0, 0.0), vector(1.0, 0.0, 0.0));
    assert_eq!(
        removed_face_selection(&[7], 2.0, &transition(&[2], &[]), &input.0, &output.0),
        Some(vec![2])
    );
}

#[test]
fn every_changed_face_without_a_wall_counts_against_the_members() {
    let open_side = box_faces()
        .plane(11, point(20.0, 15.0, 2.0), vector(0.0, 0.0, -1.0))
        .plane(13, point(2.0, 30.0, 0.0), vector(1.0, 0.0, 0.0))
        .plane(15, point(0.0, 2.0, 0.0), vector(0.0, 1.0, 0.0))
        .plane(16, point(40.0, 28.0, 0.0), vector(0.0, -1.0, 0.0));
    let changed = transition(&[2, 4], &[]);
    assert_eq!(
        removed(&[7, 8], 2.0, &changed, &open_side),
        Some(vec![2, 4])
    );
    assert_eq!(removed(&[7], 2.0, &changed, &open_side), None);
    assert_eq!(removed(&[7, 8, 9], 2.0, &changed, &open_side), None);
}

#[test]
fn a_repeated_member_counts_once() {
    assert_eq!(
        removed(&[7, 7], 2.0, &transition(&[2], &[]), &inside_shell()),
        Some(vec![2])
    );
}

#[test]
fn an_empty_group_states_no_selection() {
    assert_eq!(
        removed(&[], 2.0, &transition(&[1], &[]), &inside_shell()),
        None
    );
}

#[test]
fn no_changed_face_without_a_wall_states_no_selection() {
    assert_eq!(
        removed(&[7], 2.0, &transition(&[1], &[]), &inside_shell()),
        None
    );
    assert_eq!(
        removed(&[7], 2.0, &transition(&[], &[]), &inside_shell()),
        None
    );
}

#[test]
fn a_zero_offset_states_no_selection() {
    let without_top = State::default()
        .plane(1, point(20.0, 15.0, 0.0), vector(0.0, 0.0, 1.0))
        .plane(3, point(0.0, 30.0, 0.0), vector(-1.0, 0.0, 0.0));
    assert_eq!(
        removed(&[7], 0.0, &transition(&[], &[2]), &without_top),
        None,
        "a deleted face has no wall at any offset"
    );
}

#[test]
fn a_changed_face_without_a_row_states_no_selection() {
    let mut input = box_faces();
    input.0.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 9,
        carrier: 109,
    });
    assert_eq!(
        removed_face_selection(
            &[7],
            2.0,
            &transition(&[2, 9], &[]),
            &input.0,
            &inside_shell().0
        ),
        None
    );
}

#[test]
fn a_surface_with_a_plane_and_a_cylinder_row_states_no_selection() {
    let mut input = box_faces();
    input.0.surface_cylinders.push(AsmHistoricalCylinder {
        surface: 102,
        origin: point(0.0, 0.0, 0.0),
        axis: vector(0.0, 0.0, 1.0),
        radius: 3.0,
    });
    assert_eq!(
        removed_face_selection(
            &[7],
            2.0,
            &transition(&[2], &[]),
            &input.0,
            &inside_shell().0
        ),
        None
    );
}

#[test]
fn a_wall_at_another_offset_does_not_wall_its_face() {
    assert_eq!(
        removed(&[7], 1.0, &transition(&[1, 2], &[]), &inside_shell()),
        None,
        "the bottom wall is 2 mm away"
    );
}

#[test]
fn a_plane_moved_within_itself_does_not_wall_its_face() {
    let output = inside_shell().plane(20, point(22.0, 15.0, 20.0), vector(0.0, 0.0, 1.0));
    assert_eq!(
        removed(&[7], 2.0, &transition(&[2], &[]), &output),
        Some(vec![2])
    );
}

#[test]
fn a_turned_plane_does_not_wall_its_face() {
    let turned = State::default()
        .plane(2, point(20.0, 15.0, 20.0), vector(0.0, 0.0, 1.0))
        .plane(11, point(20.0, 15.0, 22.0), vector(0.0, 0.6, 0.8));
    let input = State::default().plane(2, point(20.0, 15.0, 20.0), vector(0.0, 0.0, 1.0));
    assert_eq!(
        removed_face_selection(&[7], 2.0, &transition(&[2], &[]), &input.0, &turned.0),
        Some(vec![2])
    );
}

#[test]
fn a_cylinder_is_walled_by_a_radius_change_on_its_axis_line() {
    let bore =
        |face| State::default().cylinder(face, point(10.0, 10.0, 0.0), vector(0.0, 0.0, 1.0), 5.0);
    let walled = bore(1).cylinder(11, point(10.0, 10.0, 7.0), vector(0.0, 0.0, -1.0), 3.0);
    let off_axis = bore(1).cylinder(11, point(10.5, 10.0, 0.0), vector(0.0, 0.0, 1.0), 3.0);
    let turned = bore(1).cylinder(11, point(10.0, 10.0, 0.0), vector(1.0, 0.0, 0.0), 3.0);
    let changed = transition(&[1], &[]);
    assert_eq!(
        removed_face_selection(&[7], 2.0, &changed, &bore(1).0, &walled.0),
        None
    );
    for output in [off_axis, turned] {
        assert_eq!(
            removed_face_selection(&[7], 2.0, &changed, &bore(1).0, &output.0),
            Some(vec![1])
        );
    }
}

#[test]
fn a_shell_on_both_sides_walls_each_side_at_half_its_thickness() {
    assert_eq!(wall_offset(2.0, Some(ShellMode::BothSides)), Some(1.0));
    assert_eq!(wall_offset(2.0, None), Some(2.0));
    assert_eq!(wall_offset(2.0, Some(ShellMode::Skin)), None);
    assert_eq!(wall_offset(2.0, Some(ShellMode::Pipe)), None);
}

fn bind_selection(
    selection: &mut FaceSelection,
    member_count: usize,
    changed: &AsmHistoricalTransition,
    output: &State,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::topology::construction::DesignConstructionOperandGroup;

    let members: Vec<_> = (7..7 + member_count).collect();
    let offsets: Vec<_> = (0..member_count).map(|ordinal| ordinal * 11).collect();
    let group: DesignConstructionOperandGroup = json::from_value(cadmpeg_core::json!({
        "id": "f3d:test:group#43", "byte_offset": 0, "class_tag": "261",
        "record_index": 43, "scope_record_index": 42, "scope_reference_ordinal": 0,
        "members": members, "member_offsets": offsets, "role": 68_719_476_736_u64,
        "role_offset": 0, "paired_byte_offset": 200, "paired_class_tag": "266",
        "frame": {
            "member_count_offset": 0, "opaque_index": 1, "opaque_index_offset": 18,
            "opaque_scalar": 0.0, "opaque_scalar_offset": 22, "trailing_flags": [],
            "trailing_record_indices": [], "trailing_record_offsets": [], "variant": false
        }
    }))
    .unwrap();
    let scope = DesignParameterScope::empty("f3d:test:scope#42", DesignFeatureKind::Shell, 42);
    bind_shell_removed_faces(
        selection,
        &FeatureId::mint("f3d:test:feature#42").unwrap(),
        &scope,
        &[group],
        &ShellWalls {
            thickness: 2.0,
            mode: None,
            previous_state_id: 3,
            transition: changed,
            input: Some(&box_faces().0),
            output: Some(&output.0),
        },
    )
}

fn historical_selection(slots: &[i64]) -> FaceSelection {
    let feature = FeatureId::mint("f3d:test:feature#42").unwrap();
    let prefix = crate::ids::feature_input_prefix(&feature, 3);
    FaceSelection::historical(
        crate::ids::history_input_state_id(&prefix),
        slots
            .iter()
            .map(|&slot| crate::ids::history_input_face_id(&prefix, slot))
            .collect(),
        "f3d:test:group#43".into(),
    )
    .unwrap()
}

#[test]
fn recipe_faces_survive_a_different_wall_set_with_equal_cardinality() {
    let output = box_faces().plane(20, point(20.0, 15.0, 18.0), vector(0.0, 0.0, 1.0));
    let changed = transition(&[2, 4], &[]);
    assert_eq!(removed(&[7], 2.0, &changed, &output), Some(vec![4]));
    let expected = historical_selection(&[2]);
    let mut selection = expected.clone();
    let loss = bind_selection(&mut selection, 1, &changed, &output).unwrap();
    assert_eq!(selection, expected);
    assert_eq!(
        loss.code.local_code(),
        "feature.shell-removed-faces-assumed"
    );
    let feature = FeatureId::mint("f3d:test:feature#42").unwrap();
    let prefix = crate::ids::feature_input_prefix(&feature, 3);
    for slot in [2, 4] {
        assert!(loss
            .message
            .contains(crate::ids::history_input_face_id(&prefix, slot).as_str()));
    }
    assert!(loss.message.contains("the recipe faces are retained"));
}

#[test]
fn equal_face_sets_preserve_recipe_member_order_without_a_loss() {
    let expected = historical_selection(&[4, 2]);
    let mut selection = expected.clone();
    assert!(bind_selection(&mut selection, 2, &transition(&[4, 2], &[]), &box_faces()).is_none());
    assert_eq!(selection, expected);
}

#[test]
fn an_unresolved_group_keeps_the_wall_selection() {
    let mut selection = FaceSelection::Native("f3d:test:group#43".into());
    assert!(bind_selection(&mut selection, 1, &transition(&[2], &[]), &inside_shell()).is_none());
    assert_eq!(selection, historical_selection(&[2]));
}
