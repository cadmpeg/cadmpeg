// SPDX-License-Identifier: Apache-2.0
//! Borrowed unit-kind native wire checks.

use super::super::{PmDcUnitDimension, PmDcUnitKind, PmDcUnitKindWire};
use crate::pmdc::{PmDcPairedReferenceList, PmDcReference, PMDC_LIST_CLONE_COUNT};
use cadmpeg_ir::scalar::FiniteReal;
use serde::Serialize;

#[derive(Serialize)]
struct Record<'a> {
    id: &'static str,
    value: &'a PmDcUnitKind,
}

#[test]
fn unit_definition_borrowed_wire_refuses_retained_limit_before_clone() {
    let reference = PmDcReference {
        index: 7,
        qualified: false,
    };
    let unit = PmDcUnitKind::Definition {
        numerators: PmDcPairedReferenceList::new(Some([1, 2]), vec![reference])
            .expect("paired numerator"),
        denominators: PmDcPairedReferenceList::new(Some([3, 4]), vec![reference])
            .expect("paired denominator"),
        visible: true,
        derived: reference,
    };
    let wire = PmDcUnitKindWire::from(unit.clone());
    assert_eq!(
        serde_json::to_vec(&unit).expect("borrowed unit"),
        serde_json::to_vec(&wire).expect("owned unit")
    );
    let record = Record {
        id: "inventor:pmdc:unit-kind#1",
        value: &unit,
    };
    PMDC_LIST_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": wire}),
    );
    PMDC_LIST_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn unit_base_borrowed_wire_preserves_symbol_bytes() {
    let one = FiniteReal::new(1.0).expect("finite scalar");
    let unit = PmDcUnitKind::Base {
        dimension: PmDcUnitDimension::Length,
        symbol: "mm".to_owned(),
        scale_to_internal: one,
        magnitude: one,
        factor: one,
    };
    let wire = PmDcUnitKindWire::from(unit.clone());
    assert_eq!(
        serde_json::to_vec(&unit).expect("borrowed unit"),
        serde_json::to_vec(&wire).expect("owned unit")
    );
    let record = Record {
        id: "inventor:pmdc:unit-base#1",
        value: &unit,
    };
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": wire}),
    );
}
