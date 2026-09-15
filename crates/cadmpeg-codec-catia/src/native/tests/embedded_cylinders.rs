// SPDX-License-Identifier: Apache-2.0
//! Native embedded-cylinder ownership and support bindings.

use crate::test_support::*;

#[test]
fn embedded_cylinders_keep_group_identity_after_empty_groups() {
    let mut bytes = b2_group_stream();
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(17));
    bytes.extend_from_slice(&b2_group_stream());
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(23));
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_groups.len(), 4);
    assert_eq!(native.consolidated_embedded_cylinders.len(), 2);
    for (index, group_index, object_id) in [(0, 1, 17), (1, 3, 23)] {
        let cylinder = &native.consolidated_embedded_cylinders[index];
        let group = &native.consolidated_groups[group_index];
        assert_eq!(cylinder.object_id, object_id);
        assert_eq!(
            cylinder.id,
            format!("catia:consolidated:embedded-cylinder#{index}")
        );
        assert_eq!(
            cylinder.group,
            format!("catia:consolidated:group#{group_index}")
        );
        assert_eq!(cylinder.group, group.id);
        assert!(group.byte_offset < cylinder.byte_offset);
    }
}

#[test]
fn native_namespace_retains_embedded_cylinders_with_their_owning_group() {
    let native = crate::native::CatiaNative::decode(&b2_embedded_cylinder_stream());
    assert!(native.consolidated_cylinders.is_empty());
    let [group] = native.consolidated_groups.as_slice() else {
        panic!("one consolidated group");
    };
    let [cylinder] = native.consolidated_embedded_cylinders.as_slice() else {
        panic!("one embedded consolidated cylinder");
    };
    assert_eq!(group.group_type, 3);
    assert_eq!(cylinder.group, group.id);
    assert_eq!(cylinder.object_id, 0x5678);
    assert_eq!(cylinder.u_range.get(), [0.0, 4.0 * std::f64::consts::PI]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store embedded CATIA cylinder");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load embedded CATIA cylinder"),
        native
    );

    namespace
        .set_arena(
            "consolidated_groups",
            &Vec::<crate::native::CatiaConsolidatedGroup>::new(),
        )
        .expect("remove owning consolidated group");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut two_groups = b2_embedded_cylinder_stream();
    two_groups.extend_from_slice(&b2_embedded_cylinder_stream());
    let mut invalid = crate::native::CatiaNative::decode(&two_groups);
    assert_eq!(invalid.consolidated_groups.len(), 2);
    assert_eq!(invalid.consolidated_embedded_cylinders.len(), 2);
    invalid.consolidated_embedded_cylinders[1]
        .group
        .clone_from(&invalid.consolidated_groups[0].id);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store cross-group embedded cylinder");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_binds_edges_to_retained_embedded_cylinders() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_embedded_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::EmbeddedCylinder { .. })
    )));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store embedded-cylinder edge binding");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load embedded-cylinder edge binding"),
        native
    );
}

#[test]
fn native_namespace_binds_embedded_cylinder_by_unique_pcurve_support_identity() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_embedded_cylinder_stream_with_object_id(0x5678);
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(0x9abc));
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream_with_support(6, 139, 142, 0x5678));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [first, second] = native.consolidated_embedded_cylinders.as_slice() else {
        panic!("two embedded consolidated cylinders");
    };
    assert_ne!(first.object_id, second.object_id);
    let [first_group, _second_group] = native.consolidated_groups.as_slice() else {
        panic!("two consolidated groups");
    };
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    let expected = Some(CatiaConsolidatedSupportBinding::EmbeddedCylinder {
        byte_offset: first.byte_offset,
        wrapper_byte_offset: first_group.byte_offset,
    });
    assert_eq!(run.support_bindings, [expected.clone(), expected]);
}

#[test]
fn native_namespace_withholds_duplicate_embedded_pcurve_support_identity() {
    let mut bytes = b2_embedded_cylinder_stream_with_object_id(0x5678);
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(0x5678));
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream_with_support(6, 139, 142, 0x5678));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert_eq!(run.support_bindings, [None, None]);
}

