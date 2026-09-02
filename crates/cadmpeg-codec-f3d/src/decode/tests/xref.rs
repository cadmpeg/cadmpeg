// SPDX-License-Identifier: Apache-2.0
//! Decode-owner external-reference projection tests.

use super::super::report_xref_placement_overrides;
use crate::loss::F3dLossCode;

#[test]
fn superseded_xref_placements_have_a_distinct_loss_note() {
    let mut report = cadmpeg_ir::codec::DecodeBody {
        geometry_transferred: true,
        coverage: Default::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    let table = crate::xref::XrefTable {
        designs: Vec::new(),
        references: vec![crate::records::XrefReference {
            id: "f3d:xref:reference#4-occurrence-0".into(),
            ordinal: 4,
            occurrence_ordinal: 0,
            from: "root.f3d".into(),
            relative_path: "part.f3d".into(),
            neutron_role: "role-guid".into(),
            neutron_data: "data-guid".into(),
            transform: None,
        }],
        placement_failures: Vec::new(),
        placement_overrides: vec![(4, 2)],
    };

    report_xref_placement_overrides(&mut report, &table);

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::XrefPlacementSuperseded.kind())
        .expect("superseded placement loss");
    assert_eq!(loss.message, "2 structured placement record(s) for external occurrence part.f3d and role role-guid were superseded by scope-bound Component Insert carrier(s)");
}
