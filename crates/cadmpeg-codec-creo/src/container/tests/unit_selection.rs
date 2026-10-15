// SPDX-License-Identifier: Apache-2.0
use super::super::{binary_principal_unit, BinaryUnitSelection};

#[test]
fn binary_units_admit_agreed_pair() {
    {
        assert_eq!(
            binary_principal_unit(b"_principal_sys_units_id\0\x36 _principal_sys_units_id\0\x36"),
            BinaryUnitSelection::Selected(crate::legacy::PrincipalUnitSystem::InchPoundMassSecond)
        );
    };
}

#[test]
fn binary_units_withhold_conflicting_pair() {
    {
        assert_eq!(
            binary_principal_unit(b"_principal_sys_units_id\0\x33 _principal_sys_units_id\0\x36"),
            BinaryUnitSelection::Conflicting
        );
    };
}

#[test]
fn binary_units_opaque_earlier_declaration_cannot_select_scale() {
    let bytes = crate::test_support::build_prt(
        "test",
        &[
            (
                "Auxiliary",
                b"opaque _principal_sys_units_id\0\x33".to_vec(),
            ),
            ("VisibGeom", b"_principal_sys_units_id\0\x36".to_vec()),
        ],
    );
    let scan = super::super::scan_bytes_ok(bytes);
    assert_eq!(scan.framing.principal_unit, None);
}

#[test]
fn binary_units_distinguish_absent_and_unsupported() {
    {
        assert_eq!(binary_principal_unit(b"other"), BinaryUnitSelection::Absent);
        assert_eq!(
            binary_principal_unit(b"_principal_sys_units_id\0\x07"),
            BinaryUnitSelection::Unsupported
        );
    };
}
