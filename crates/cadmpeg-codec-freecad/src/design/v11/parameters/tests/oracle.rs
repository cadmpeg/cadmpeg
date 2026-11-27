// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{charge_count, decode_with_losses};
use crate::loss::FreecadLossCode::ParameterQuantityAssumed;

use cadmpeg_ir::design::units::{Quantity, ValueKind};

#[derive(Deserialize)]
struct Oracle {
    freecad: Vec<String>,
    rows: Vec<Row>,
}

#[derive(Deserialize)]
struct Row(String, String, String, Option<[i8; 8]>);

impl Row {
    fn kind(&self) -> Option<ValueKind> {
        let Self(_, _, result, signature) = self;
        match result.as_str() {
            "bool" => Some(ValueKind::Boolean),
            "str" => Some(ValueKind::Text),
            "int" | "float" => Some(ValueKind::Quantity(Quantity::REAL)),
            "Quantity" => {
                let [length, mass, time, current, temperature, amount, intensity, angle] =
                    signature.expect("FreeCAD quantity signature");
                (current == 0 && temperature == 0 && amount == 0 && intensity == 0)
                    .then_some(ValueKind::Quantity(Quantity::of(length, mass, angle, time)))
            }
            _ => None,
        }
    }
}

#[derive(Default, Debug)]
struct Counts {
    exact: usize,
    assumption: usize,
    error_form: usize,
    disagreement: usize,
}

#[test]
fn spreadsheet_kinds_agree_with_freecad() {
    let table: Oracle = serde_json::from_str(include_str!("oracle/formula_oracle.json")).unwrap();
    let (ir, losses) = decode_with_losses(include_str!("oracle/formula_oracle.xml"));
    let cells = ir
        .design
        .parameters
        .iter()
        .filter_map(|cell| {
            Some((
                cell.id
                    .as_str()
                    .strip_prefix("fcstd:design:parameter#Sheet:cell:")?,
                cell,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(cells.len(), table.rows.len());

    let mut counts = BTreeMap::<_, Counts>::new();
    let mut disagreements = Vec::new();
    let mut remaining = cells;
    for row in &table.rows {
        let Row(family, address, result, signature) = row;
        let cell = remaining.remove(address.as_str()).expect(address);
        let quantity_charge = charge_count(&losses, ParameterQuantityAssumed, cell.id.as_str());
        let counts = counts.entry(family).or_default();
        let expected = row.kind();
        let exact = expected.as_ref() == Some(&cell.kind);
        let fallback = cell.kind == ValueKind::Quantity(Quantity::REAL) && cell.value.is_none();

        match (result.as_str(), quantity_charge) {
            ("error", 1) if fallback => counts.error_form += 1,
            (_, 1) if fallback => counts.assumption += 1,
            (_, 0) if exact => counts.exact += 1,
            _ => {
                counts.disagreement += 1;
                disagreements.push(format!(
                    "{family} {address}: FreeCAD {result} {signature:?}, codec {:?} {:?}, quantity-assumed={quantity_charge}",
                    cell.kind, cell.value,
                ));
            }
        }
    }
    assert!(remaining.is_empty());
    eprintln!("FreeCAD {:?}: {counts:#?}", table.freecad);
    assert!(disagreements.is_empty(), "{}", disagreements.join("\n"));
}
