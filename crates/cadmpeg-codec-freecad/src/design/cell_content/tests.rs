// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::design::units::{Quantity, UnitSymbol};

use super::{neutral_unit, read, CellContent, CellKind};

/// The kind, number, and neutral text of a value content.
fn value(content: &str) -> (Option<Quantity>, Option<String>, f64, Option<String>) {
    let CellContent::Value(value) = read(content) else {
        panic!("{content:?} is no value");
    };
    let (quantity, native) = match value.kind {
        CellKind::Quantity(quantity) => (Some(quantity), None),
        CellKind::Native(unit) => (None, Some(unit.as_str().to_owned())),
    };
    (quantity, native, value.number, value.neutral)
}

#[test]
fn content_that_is_no_value_is_text() {
    for content in [
        "3 bolts", "-abc", "2*3", "-5 mm", "+5 mm", "1 m*s", "1/2/3", "1ft 2in", "note", "5. mm",
        "mm", "1 mm2", "",
    ] {
        assert!(
            matches!(read(content), CellContent::Text(text) if text == content),
            "{content:?}"
        );
    }
    assert!(matches!(read("'Plate"), CellContent::Text("Plate")));
    assert!(matches!(read("'=2*3"), CellContent::Text("=2*3")));
    assert!(matches!(read("'"), CellContent::Text("")));
}

#[test]
fn an_equals_sign_starts_a_formula() {
    assert!(matches!(read("=2*3"), CellContent::Formula("2*3")));
    assert!(matches!(read("=-5 mm"), CellContent::Formula("-5 mm")));
}

#[test]
fn a_number_is_a_real() {
    for (content, number, neutral) in [
        ("12", 12.0, "12"),
        (" -5 ", -5.0, "-5"),
        ("1e3", 1000.0, "1000"),
        (".5", 0.5, "0.5"),
        ("12,5", 12.5, "12.5"),
    ] {
        assert_eq!(
            value(content),
            (Some(Quantity::REAL), None, number, Some(neutral.to_owned())),
            "{content:?}"
        );
    }
}

#[test]
fn a_number_with_a_unit_is_a_quantity_in_canonical_units() {
    let speed = Quantity::of(1, 0, 0, -1);
    for (content, quantity, number, neutral) in [
        ("12 mm", Quantity::LENGTH, 12.0, Some("12[mm]")),
        ("12,5mm", Quantity::LENGTH, 12.5, Some("12.5[mm]")),
        ("2in", Quantity::LENGTH, 50.8, Some("2[in]")),
        ("2\"", Quantity::LENGTH, 50.8, Some("2[in]")),
        ("3 dm", Quantity::LENGTH, 300.0, None),
        ("1 mm^2", Quantity::AREA, 1.0, Some("1[mm^2]")),
        ("1 (m/s)", speed, 1000.0, Some("1[m*s^-1]")),
        ("1 m/s", speed, 1000.0, Some("1[m] / 1[s]")),
        ("1/2", Quantity::REAL, 0.5, Some("1 / 2")),
        ("3 mm / 2", Quantity::LENGTH, 1.5, Some("3[mm] / 2")),
        ("2 kg", Quantity::of(0, 1, 0, 0), 2.0, Some("2[kg]")),
        ("2 mg", Quantity::of(0, 1, 0, 0), 2e-6, None),
        ("2 N", Quantity::of(1, 1, 0, -2), 2000.0, None),
    ] {
        let (found, native, stored, text) = value(content);
        assert_eq!(
            (found, native, text.as_deref()),
            (Some(quantity), None, neutral),
            "{content:?}"
        );
        assert!(
            (stored - number).abs() <= 1e-12 * number.abs(),
            "{content:?}"
        );
    }
    let (found, _, stored, text) = value("90°");
    assert_eq!(found, Some(Quantity::ANGLE));
    assert!((stored - std::f64::consts::FRAC_PI_2).abs() < 1e-15);
    assert_eq!(text.as_deref(), Some("90[deg]"));
}

#[test]
fn a_unit_of_another_dimension_is_a_native_kind_as_written() {
    assert_eq!(value("5 mA"), (None, Some("mA".to_owned()), 5.0, None));
    assert_eq!(
        value("6 V / 2 s"),
        (None, Some("V/s".to_owned()), 3.0, None)
    );
}

#[test]
fn a_non_finite_number_has_no_neutral_text() {
    let (quantity, _, number, neutral) = value("inf");
    assert_eq!(quantity, Some(Quantity::REAL));
    assert!(number.is_infinite());
    assert_eq!(neutral, None);
}

#[test]
fn a_display_unit_is_a_neutral_unit_where_each_word_has_a_symbol() {
    let (quantity, unit) = neutral_unit("mm").unwrap();
    assert_eq!(quantity, Quantity::LENGTH);
    assert_eq!(
        unit.0
            .iter()
            .map(|factor| (factor.symbol, factor.power.get()))
            .collect::<Vec<_>>(),
        [(UnitSymbol::Mm, 1)]
    );
    let (quantity, unit) = neutral_unit("m/s^2").unwrap();
    assert_eq!(quantity, Quantity::of(1, 0, 0, -2));
    assert_eq!(
        unit.0
            .iter()
            .map(|factor| (factor.symbol, factor.power.get()))
            .collect::<Vec<_>>(),
        [(UnitSymbol::M, 1), (UnitSymbol::S, -2)]
    );
    for text in ["dm", "A", "mm 2", ""] {
        assert!(neutral_unit(text).is_none(), "{text:?}");
    }
}
