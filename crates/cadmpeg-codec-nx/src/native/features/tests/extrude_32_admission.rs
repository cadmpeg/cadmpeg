// SPDX-License-Identifier: Apache-2.0

#[test]
fn extrude_32_join_admits_fixture() {
    let (reference, branch) = super::source_and_sketch::extrude_32_fixture();
    let decode = || {
        crate::native::features::construction_records::feature_extrude_32_constructions(
            std::slice::from_ref(&reference),
            std::slice::from_ref(&branch),
        )
    };
    assert_eq!(decode().expect("admitted extrude 32 construction").len(), 1);
}
