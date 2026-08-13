// SPDX-License-Identifier: Apache-2.0
use super::has_in_plane_component;

#[test]
fn view_up_component_test_is_scale_invariant() {
    assert!(has_in_plane_component(
        [0.0, 0.0, 1.0e-200],
        [0.0, 1.0e-200, 0.0]
    ));
    assert!(has_in_plane_component(
        [1.0e200, 0.0, 0.0],
        [1.0e200, 1.0e184, 0.0]
    ));
    assert!(!has_in_plane_component(
        [1.0e200, 0.0, 0.0],
        [1.0e200, 0.0, 0.0]
    ));
}
