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

#[test]
fn binary_units_cgs_selects_centimeters() {
    let selected = binary_principal_unit(b"_principal_sys_units_id\0\x32");
    assert_eq!(
        selected,
        BinaryUnitSelection::Selected(crate::legacy::PrincipalUnitSystem::CentimeterGramSecond)
    );
    let BinaryUnitSelection::Selected(system) = selected else {
        panic!("CGS must select its coordinate unit");
    };
    assert_eq!(
        system
            .length_scale_mm()
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(10.0)
    );
}

#[test]
fn binary_units_compare_complete_compact_references() {
    assert_eq!(
        binary_principal_unit(
            b"_principal_sys_units_id\0\xa7\x10 _principal_sys_units_id\0\xa7\x11"
        ),
        BinaryUnitSelection::Conflicting
    );
    assert_eq!(
        binary_principal_unit(
            b"_principal_sys_units_id\0\xa7\x10 _principal_sys_units_id\0\xa7\x10"
        ),
        BinaryUnitSelection::Unsupported
    );
    assert_eq!(
        binary_principal_unit(b"_principal_sys_units_id\0\x32 _principal_sys_units_id\0\xa7\x10"),
        BinaryUnitSelection::Conflicting
    );
}

#[test]
fn binary_units_do_not_infer_unknown_selectors_or_truncated_references() {
    assert_eq!(
        binary_principal_unit(b"_principal_sys_units_id\0\x33 _principal_sys_units_id\0\xa7"),
        BinaryUnitSelection::Unsupported
    );
    assert_eq!(
        binary_principal_unit(b"_principal_sys_units_id\0\x80\x33"),
        BinaryUnitSelection::Selected(crate::legacy::PrincipalUnitSystem::MillimeterNewtonSecond)
    );

    for tail in [
        &b"\x31"[..],
        &b"\x34"[..],
        &b"\x35"[..],
        &b"\xa7"[..],
        &b"\xe0"[..],
        &b""[..],
    ] {
        let mut data = b"_principal_sys_units_id\0".to_vec();
        data.extend_from_slice(tail);
        assert_eq!(
            binary_principal_unit(&data),
            BinaryUnitSelection::Unsupported
        );
    }
}
