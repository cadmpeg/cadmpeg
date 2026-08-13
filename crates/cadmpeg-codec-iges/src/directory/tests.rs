// SPDX-License-Identifier: Apache-2.0
use super::Status;

#[test]
fn subordinate_switch_dependency_bits_follow_the_four_defined_values() {
    for (subordinate, physical, logical) in [
        (0, false, false),
        (1, true, false),
        (2, false, true),
        (3, true, true),
    ] {
        let status = Status {
            blank: 0,
            subordinate,
            use_flag: 0,
            hierarchy: 0,
        };
        assert_eq!(status.is_physically_dependent(), physical);
        assert_eq!(status.is_logically_dependent(), logical);
    }
}
