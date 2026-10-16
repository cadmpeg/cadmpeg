// SPDX-License-Identifier: Apache-2.0
use super::{check_parasolid_capabilities, registry_rows, ReadDisposition};

#[test]
fn verified_membership_requires_a_global_read_cell_and_unknown_stays_residual() {
    let mut registry = registry_rows();
    check_parasolid_capabilities(&registry).expect("embedded host policies match registry");
    assert_eq!(
        registry.get("parasolid:format-13006"),
        Some(&ReadDisposition::Detected),
        "13006 remains globally detected while host admission stays codec-owned"
    );

    registry.insert(
        "parasolid:format-13006".to_owned(),
        ReadDisposition::UnclassifiedRecovered,
    );
    assert!(check_parasolid_capabilities(&registry)
        .expect_err("verified membership requires global read capability")
        .to_string()
        .contains("sldprt verifies parasolid:format-13006"));

    let mut registry = registry_rows();
    registry.insert("parasolid:unknown".to_owned(), ReadDisposition::Detected);
    assert!(check_parasolid_capabilities(&registry)
        .expect_err("unknown remains explicitly residual")
        .to_string()
        .contains("unknown Parasolid row must remain unclassified-recovered"));
}
