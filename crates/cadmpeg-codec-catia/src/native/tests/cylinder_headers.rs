// SPDX-License-Identifier: Apache-2.0
//! Independent cylinder headers and support bindings.

use crate::test_support::test_a5_bound::{
    a5_native_edge_run_stream, a5_native_edge_run_stream_with_support,
};
use crate::test_support::test_b2::{
    b2_class60_reference_stream, b2_wide_cylinder_stream, b2_wide_cylinder_stream_with_operand,
};

#[test]
fn cylinder_headers_and_counted_lists_remain_independent() {
    let mut bytes = b2_class60_reference_stream();
    bytes.extend_from_slice(&b2_wide_cylinder_stream_with_operand(0x5711));
    bytes.extend_from_slice(&b2_class60_reference_stream());
    bytes.extend_from_slice(&b2_wide_cylinder_stream_with_operand(0x5717));
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_class60_records.len(), 4);
    assert_eq!(native.consolidated_cylinders.len(), 2);
    for (index, operand) in [(0, 0x5711), (1, 0x5717)] {
        let cylinder = &native.consolidated_cylinders[index];
        assert_eq!(
            cylinder
                .header
                .compact_operand()
                .expect("closed compact header")
                .value(),
            operand
        );
        assert_eq!(cylinder.id, format!("catia:consolidated:cylinder#{index}"));
    }
}

#[test]
fn cylinder_is_retained_without_a_preceding_reference_list() {
    let bytes = b2_wide_cylinder_stream();
    let native = crate::native::CatiaNative::decode(&bytes[7..]);
    assert!(native.consolidated_class60_records.is_empty());
    let [cylinder] = native.consolidated_cylinders.as_slice() else {
        panic!("one cylinder")
    };
    assert_eq!(
        cylinder
            .header
            .compact_operand()
            .expect("closed compact header")
            .value(),
        0x5678
    );
    assert_eq!(
        cylinder.u_range.endpoints(),
        [0.0, 4.0 * std::f64::consts::PI]
    );
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store cylinder");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("checked cylinder"),
        native
    );
}

#[test]
fn native_namespace_binds_edges_to_retained_cylinders() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_wide_cylinder_stream();
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
        Some(CatiaConsolidatedSupportBinding::Cylinder { .. })
    )));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store cylinder edge binding");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load cylinder edge binding"),
        native
    );
}

#[test]
fn native_namespace_binds_cylinder_by_unique_pcurve_support_operand() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_wide_cylinder_stream_with_operand(0x5678);
    bytes.extend_from_slice(&b2_wide_cylinder_stream_with_operand(0x9abc));
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
    let [first, second] = native.consolidated_cylinders.as_slice() else {
        panic!("two consolidated cylinders");
    };
    assert_ne!(first.header, second.header);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    let expected = Some(CatiaConsolidatedSupportBinding::Cylinder {
        byte_offset: first.byte_offset,
    });
    assert_eq!(run.support_bindings, [expected.clone(), expected]);
}

#[test]
fn native_namespace_withholds_duplicate_cylinder_support_operand() {
    let mut bytes = b2_wide_cylinder_stream_with_operand(0x5678);
    bytes.extend_from_slice(&b2_wide_cylinder_stream_with_operand(0x5678));
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
