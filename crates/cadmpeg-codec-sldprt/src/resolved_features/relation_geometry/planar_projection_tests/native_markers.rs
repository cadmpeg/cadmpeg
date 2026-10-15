use super::{assert_owned_loci_refusal, project_native_marker_with_policy};

#[test]
fn planar_native_marker_refuses_nesting_limit() {
    assert_owned_loci_refusal(project_native_marker_with_policy);
}
