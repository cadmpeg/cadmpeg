// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use cadmpeg_ir::design::dimension::RefTarget;
use cadmpeg_ir::design::expr::{print, Expression, ExpressionToken};
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
use cadmpeg_ir::report::loss::LossNote;

use super::{charge_count, decode_with_losses, unresolved_kind};
use crate::loss::FreecadLossCode::{
    ParameterDependencyDropped, ParameterExpressionResultUnresolved, ParameterValueNonFinite,
};

const SHEET: &str = "fcstd:design:parameter#Sheet:cell:";
const CACHE_RELATIVE_TOLERANCE: f64 = 1e-13;

#[derive(Deserialize)]
struct Oracle {
    seed: u32,
    freecad: Vec<String>,
    rows: Vec<Row>,
}

#[derive(Deserialize)]
struct Row(
    String,
    String,
    String,
    String,
    Option<[i8; 8]>,
    Option<Number>,
    Option<String>,
    String,
    Option<String>,
    Option<Vec<String>>,
);

#[derive(Deserialize)]
#[serde(untagged)]
enum Number {
    Finite(f64),
    NonFinite(String),
}

impl Row {
    fn kind(&self) -> Option<ValueKind> {
        match self.3.as_str() {
            "bool" => Some(ValueKind::Boolean),
            "str" => Some(ValueKind::Text),
            "int" | "float" => Some(ValueKind::Quantity(Quantity::REAL)),
            "Quantity" => {
                let [length, mass, time, current, temperature, amount, intensity, angle] =
                    self.4.expect("FreeCAD quantity signature");
                (current == 0 && temperature == 0 && amount == 0 && intensity == 0)
                    .then_some(ValueKind::Quantity(Quantity::of(length, mass, angle, time)))
            }
            _ => None,
        }
    }

    fn disagreements(&self, cell: &DesignParameter, losses: &[LossNote]) -> Vec<String> {
        let Self(family, address, input, result, signature, number, text, restored, display, refs) =
            self;
        let formula = input.starts_with('=');
        let error = result == "error";
        let rejected =
            result == "str" && restored.starts_with('\'') && text.as_ref() == Some(input);
        let extra_unit = result == "Quantity" && self.kind().is_none();
        let native = if extra_unit && !formula {
            let unit = signature.expect("native quantity signature");
            assert!(input.contains("mA") && unit[3].abs() == 1);
            let spelling = if unit[3] < 0 { "1/mA" } else { "mA" };
            Some((spelling, 1e-3_f64.powi(i32::from(unit[3]))))
        } else {
            None
        };
        let producer_dependent_cache = !formula
            && input.contains("9223372036854775806")
            && !matches!(result.as_str(), "error" | "str");
        let strict =
            family == "review" && !matches!(address.as_str(), "X8" | "X16" | "X24" | "X25");
        let charge = |code| charge_count(losses, code, cell.id.as_str());
        let unknown = charge(ParameterExpressionResultUnresolved);
        let mut wrong = Vec::new();
        let ParameterDefinition::Expression(expression) = &cell.definition else {
            return vec!["the cell has no expression".to_owned()];
        };
        if unknown != 0 {
            let allowed = error
                || producer_dependent_cache
                || self.kind().is_none()
                || formula && !rejected && !strict;
            if unknown != 1
                || cell.kind != unresolved_kind()
                || cell.value.is_some()
                || !matches!(expression, Expression::Source { .. })
                || !allowed
            {
                wrong.push(format!(
                    "unresolved result, kind {:?}, charge {unknown}",
                    cell.kind
                ));
            }
        } else if self.kind().as_ref() != Some(&cell.kind)
            && !native.is_some_and(|(unit, _)| {
                matches!(&cell.kind, ValueKind::Native(found) if found.as_str() == unit)
            })
        {
            wrong.push(format!("kind {:?}", cell.kind));
        }

        match (number, &cell.value) {
            (Some(Number::Finite(number)), cache)
                if (!extra_unit || native.is_some())
                    && ((!formula && !producer_dependent_cache) || cache.is_some()) =>
            {
                let angle = signature.map_or(0, |unit| i32::from(unit[7]));
                let scale = native.map_or(1.0, |(_, scale)| scale);
                let expected = number * (std::f64::consts::PI / 180.0).powi(angle) / scale;
                if !matches!(cache, Some(ParameterValue::Number(found))
                    if (found.get() - expected).abs() <= expected.abs() * CACHE_RELATIVE_TOLERANCE)
                {
                    wrong.push(format!("cache {cache:?}, expected {expected}"));
                }
            }
            (_, cache) if result == "str" && (!formula || rejected) => {
                if !matches!(cache, Some(ParameterValue::Text(found)) if Some(found) == text.as_ref())
                {
                    wrong.push(format!("text cache {cache:?}"));
                }
            }
            (_, Some(cache)) if !extra_unit => wrong.push(format!("unexpected cache {cache:?}")),
            _ => {}
        }
        let nonfinite = number.as_ref().is_some_and(|number| match number {
            Number::NonFinite(spelling) => matches!(spelling.as_str(), "nan" | "inf" | "-inf"),
            Number::Finite(_) => false,
        });
        if charge(ParameterValueNonFinite) != usize::from(!formula && nonfinite) {
            wrong.push("nonfinite value charge".to_owned());
        }
        let expected_factors: &[(UnitSymbol, i8)] = match display.as_deref() {
            Some("cm") if address == "X13" => &[(UnitSymbol::Cm, 1)],
            Some("mm/s") => &[(UnitSymbol::Mm, 1), (UnitSymbol::S, -1)],
            None | Some("s" | "mm") => &[],
            other => panic!("unexpected oracle display unit {other:?}"),
        };
        let factors = cell
            .unit
            .iter()
            .flat_map(|unit| {
                unit.0
                    .iter()
                    .map(|factor| (factor.symbol, factor.power.get()))
            })
            .collect::<Vec<_>>();
        if factors != expected_factors {
            wrong.push(format!(
                "display unit {factors:?}, expected {expected_factors:?}"
            ));
        }
        let wire = cadmpeg_core::serde_erase::json::to_value(cell).unwrap();
        for (field, expected) in [
            (
                "source_settings",
                display.as_deref().filter(|_| address == "X20"),
            ),
            (
                "native_options",
                display.as_deref().filter(|_| address == "X14"),
            ),
        ] {
            let option = &wire[field]["fcstd:displayUnit"];
            if expected.is_some() == option.is_null()
                || expected
                    .is_some_and(|value| !option.to_string().contains(&format!("\"{value}\"")))
            {
                wrong.push(format!(
                    "{field} display unit {option}, expected {expected:?}"
                ));
            }
        }

        if let Some(refs) = refs {
            let refs = if rejected { &[] } else { refs.as_slice() };
            let references = match expression {
                Expression::Neutral(neutral) => print(&neutral.0).unwrap().refs,
                Expression::Source { tokens, .. } => tokens
                    .iter()
                    .filter_map(|token| match token {
                        ExpressionToken::Reference(reference) => Some(reference.clone()),
                        ExpressionToken::Text(_) => None,
                    })
                    .collect(),
            };
            let found = references
                .into_iter()
                .filter_map(|reference| match &reference.target {
                    RefTarget::Parameter(id) => id.as_str().strip_prefix(SHEET).map(str::to_owned),
                    RefTarget::External { .. } => None,
                })
                .collect::<BTreeSet<_>>();
            if found.iter().ne(refs.iter()) {
                wrong.push(format!(
                    "reference set in {expression:?}, expected {refs:?}"
                ));
            }
            let expected = usize::from(matches!(address.as_str(), "X23" | "X24" | "X25"));
            let found = charge(ParameterDependencyDropped);
            if found != expected {
                wrong.push(format!("dependency charge {found}, expected {expected}"));
            }
        }
        wrong
    }
}

#[test]
fn spreadsheet_wire_agrees_with_freecad() {
    let table: Oracle = serde_json::from_str(include_str!("oracle/formula_oracle.json")).unwrap();
    let (ir, losses) = decode_with_losses(include_str!("oracle/formula_oracle.xml"));
    let mut cells = ir
        .design
        .parameters
        .iter()
        .filter_map(|cell| Some((cell.id.as_str().strip_prefix(SHEET)?, cell)))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(cells.len(), table.rows.len());

    let mut disagreements = Vec::new();
    for row in &table.rows {
        let Row(family, address, input, result, ..) = row;
        let cell = cells.remove(address.as_str()).expect(address);
        let wrong = row.disagreements(cell, &losses);
        if !wrong.is_empty() {
            disagreements.push(format!(
                "{family} {address} {input:?}, FreeCAD {result}: {}",
                wrong.join(", ")
            ));
        }
    }
    assert!(cells.is_empty());
    assert!(
        disagreements.is_empty(),
        "FreeCAD {:?}, seed {:#x}, {} disagreement(s):\n{}",
        table.freecad,
        table.seed,
        disagreements.len(),
        disagreements.join("\n")
    );
}
