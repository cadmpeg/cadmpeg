// SPDX-License-Identifier: Apache-2.0
//! A spreadsheet cell's content, read as `FreeCAD`'s `Cell::setContent`
//! reads it: an `=` content is a formula, a number, a number with a unit, or
//! a fraction of those is a value, and every other content is text.

use std::f64::consts::PI;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::units::{Quantity, Unit as IrUnit, UnitFactor, UnitSymbol};

pub(in crate::design) mod formula;

/// What a cell's content states.
pub(super) enum CellContent<'a> {
    /// A formula: the text after the `=`.
    Formula(&'a str),
    /// Text: the content after a leading `'`, or the whole content.
    Text(&'a str),
    /// A number, a number with a unit, or a fraction of those.
    Value(CellValue),
    /// An admitted literal expression that raises when evaluated.
    Error,
    /// A literal expression with version-dependent syntax or value.
    Unmodeled,
}

/// The value of a cell whose content is a value.
pub(super) struct CellValue {
    /// The value in the canonical unit of a `Quantity` kind, and the number
    /// as written for a `Native` kind.
    pub(super) number: f64,
    pub(super) kind: CellKind,
    /// The value as neutral expression text, where its number is finite and
    /// each of its units has a `UnitSymbol`.
    pub(super) neutral: Option<String>,
    pub(super) dimensions: Dimensions,
    pub(super) freecad_number: Option<f64>,
}

/// The kind of a cell's value.
pub(super) enum CellKind {
    Quantity(Quantity),
    /// A value whose unit has a dimension other than length, mass, angle, and
    /// time, named by the unit as written.
    Native(NonBlankString),
}

/// The reading of `content`.
pub(super) fn read(content: &str) -> CellContent<'_> {
    if let Some(formula) = content.strip_prefix('=') {
        return CellContent::Formula(formula);
    }
    if let Some(text) = content.strip_prefix('\'') {
        return CellContent::Text(text);
    }
    if content.is_empty() {
        return CellContent::Text(content);
    }
    let trimmed = content
        .trim_start_matches([' ', '\t', '\n', '\r', '\u{000b}', '\u{000c}'])
        .trim_end_matches([' ', '\t', '\n', '\r']);
    if trimmed.is_empty() && content.contains(['\u{000b}', '\u{000c}']) {
        return CellContent::Text(content);
    }
    if let Some(number) = c_number(trimmed) {
        return CellContent::Value(CellValue {
            number,
            kind: CellKind::Quantity(Quantity::REAL),
            neutral: number.is_finite().then(|| format!("{number}")),
            dimensions: Dimensions::REAL,
            freecad_number: Some(number),
        });
    }
    value(trimmed).unwrap_or(CellContent::Text(content))
}

/// `text` as C's `strtod` reads a whole number: a decimal, a hexadecimal
/// such as `0x1.8p1`, an infinity, or a NaN.
fn c_number(text: &str) -> Option<f64> {
    if text.is_empty() {
        return Some(0.0);
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if unsigned.eq_ignore_ascii_case("inf") || unsigned.eq_ignore_ascii_case("infinity") {
        return text.parse().ok();
    }
    if unsigned.eq_ignore_ascii_case("nan")
        || unsigned
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("nan("))
            && unsigned[4..].strip_suffix(')').is_some_and(|payload| {
                payload
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })
    {
        return Some(f64::NAN);
    }
    text.parse()
        .ok()
        .filter(|number| !decimal_out_of_range(text, *number))
        .or_else(|| hexadecimal(text).filter(|number| number.is_finite()))
}

fn decimal_out_of_range(text: &str, number: f64) -> bool {
    !number.is_finite()
        || number == 0.0
            && text
                .split(['e', 'E'])
                .next()
                .is_some_and(|mantissa| mantissa.bytes().any(|byte| matches!(byte, b'1'..=b'9')))
}

fn hexadecimal(text: &str) -> Option<f64> {
    let (negative, unsigned) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))?;
    let (mantissa, exponent) = match digits.split_once(['p', 'P']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (digits, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let mut value = 0.0_f64;
    for digit in whole.chars().chain(fraction.chars()) {
        value = value * 16.0 + f64::from(digit.to_digit(16)?);
    }
    let shift = i32::try_from(fraction.len()).ok()?.checked_mul(4)?;
    let value = value * 2.0_f64.powi(exponent.checked_sub(shift)?);
    Some(if negative { -value } else { value })
}

/// The unit `text` names, where its quantity is one of length, mass, angle,
/// and time and each of its words has a `UnitSymbol`.
pub(super) fn neutral_unit(text: &str) -> Option<(Quantity, IrUnit)> {
    let mut cursor = Cursor { text, at: 0 };
    cursor.skip_space();
    let unit = cursor.unit_expression()?;
    cursor.skip_space();
    if !cursor.done() || unit.state != UnitState::Known {
        return None;
    }
    let quantity = unit.dimensions().quantity()?;
    let factors = unit
        .symbols?
        .into_iter()
        .map(|(symbol, power)| {
            Some(UnitFactor {
                symbol,
                power: std::num::NonZeroI8::new(i8::try_from(power).ok()?)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((quantity, IrUnit(factors)))
}

/// Number and single-unit productions shared with `Base::Quantity::parse`.
fn quantity_literal(text: &str) -> Option<(f64, Dimensions)> {
    let text = text.split('\0').next()?.trim_matches([' ', '\t', '\n']);
    if text
        .chars()
        .any(|ch| ch.is_whitespace() && !matches!(ch, ' ' | '\t' | '\n'))
    {
        return None;
    }
    let mut cursor = Cursor { text, at: 0 };
    if cursor.done() {
        return Some((f64::MIN_POSITIVE, Dimensions::REAL));
    }
    let negative = cursor.eat('-') || cursor.eat('\u{2212}');
    cursor.skip_space();
    let number = cursor.number();
    cursor.skip_space();
    if cursor.done() {
        let number = number?;
        return Some((if negative { -number } else { number }, Dimensions::REAL));
    }
    if negative && number.is_none() {
        return None;
    }
    let word = &cursor.text[cursor.at..];
    // These spellings include both base and derived units of the quantity
    // lexer. Its grammar and the expression grammar differ on compounds.
    if !matches!(
        word,
        "nm" | "um"
            | "µm"
            | "mm"
            | "cm"
            | "dm"
            | "m"
            | "km"
            | "in"
            | "ft"
            | "yd"
            | "mi"
            | "l"
            | "ml"
            | "mg"
            | "g"
            | "kg"
            | "s"
            | "min"
            | "h"
            | "Hz"
            | "mN"
            | "N"
            | "kN"
            | "MN"
            | "Pa"
            | "kPa"
            | "MPa"
            | "GPa"
            | "bar"
            | "A"
            | "K"
            | "mol"
            | "cd"
            | "V"
            | "W"
            | "J"
            | "°"
            | "deg"
            | "rad"
            | "gon"
    ) {
        return None;
    }
    let unit = unit_word(word)?;
    let number = number.unwrap_or(1.0) * unit.freecad_scale();
    Some((if negative { -number } else { number }, unit.dimensions()))
}

/// `NUM [UNIT] [/ (NUM [UNIT] | UNIT)]`, the forms `FreeCAD` takes as a
/// value: a number with a unit, and a division whose numerator is a number
/// or a number with a unit.
fn value(text: &str) -> Option<CellContent<'_>> {
    let mut cursor = Cursor { text, at: 0 };
    let numerator = cursor.term()?;
    cursor.skip_space();
    let denominator = if cursor.eat('/') {
        cursor.skip_space();
        Some(match cursor.term() {
            Some(term) => term,
            None => (1.0, Some(cursor.unit().filter(|unit| !unit.compound)?)),
        })
    } else {
        None
    };
    cursor.skip_space();
    if !cursor.done()
        || denominator.is_none() && numerator.1.as_ref().is_some_and(|unit| unit.compound)
    {
        return None;
    }
    let one = Unit::one();
    let (top, top_unit) = (numerator.0, numerator.1.as_ref().unwrap_or(&one));
    let (bottom, bottom_unit) = denominator.as_ref().map_or((1.0, &one), |(number, unit)| {
        (*number, unit.as_ref().unwrap_or(&one))
    });
    if numerator.1.is_none()
        && denominator.as_ref().is_some_and(|(_, unit)| unit.is_none())
        && bottom == 0.0
    {
        return Some(CellContent::Error);
    }
    let unit = top_unit.clone().times(bottom_unit.clone(), -1);
    match unit.state {
        UnitState::Error => return Some(CellContent::Error),
        UnitState::Unmodeled => return Some(CellContent::Unmodeled),
        UnitState::Known => {}
    }
    let freecad_number =
        Some((top * top_unit.freecad_scale()) / (bottom * bottom_unit.freecad_scale()));
    let dimensions = unit.dimensions();
    let Some(quantity) = dimensions.quantity() else {
        let name = match (&numerator.1, &denominator) {
            (Some(top), Some((_, Some(bottom)))) => format!("{}/{}", top.text, bottom.text),
            (None, Some((_, Some(bottom)))) => format!("1/{}", bottom.text),
            (Some(top), _) => top.text.clone(),
            (None, _) => return None,
        };
        return Some(CellContent::Value(CellValue {
            number: top / bottom,
            kind: CellKind::Native(NonBlankString::new(name)?),
            neutral: None,
            dimensions,
            freecad_number,
        }));
    };
    let number = (top * top_unit.scale) / (bottom * bottom_unit.scale);
    let spell = |number: f64, unit: Option<&Unit>| -> Option<String> {
        match unit {
            None => Some(format!("{number}")),
            Some(unit) => Some(format!("{number}[{}]", unit.spelling()?)),
        }
    };
    let neutral = number
        .is_finite()
        .then(|| {
            let top = spell(numerator.0, numerator.1.as_ref())?;
            match &denominator {
                None => Some(top),
                Some((number, unit)) => Some(format!("{top} / {}", spell(*number, unit.as_ref())?)),
            }
        })
        .flatten();
    Some(CellContent::Value(CellValue {
        number,
        kind: CellKind::Quantity(quantity),
        neutral,
        dimensions,
        freecad_number,
    }))
}

/// A unit as `FreeCAD` writes it: its exponents of length, mass, angle, and
/// time, its factor to their canonical units, and its symbols.
#[derive(Debug, Clone)]
pub(in crate::design) struct Unit {
    exponents: [i8; 4],
    scale: f64,
    extra: [i8; 4],
    /// The symbol and power of each word, where each word has a symbol.
    symbols: Option<Vec<(UnitSymbol, i32)>>,
    text: String,
    compound: bool,
    state: UnitState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitState {
    Known,
    Error,
    Unmodeled,
}

/// `FreeCAD`'s length, mass, angle, time, current, temperature, amount and
/// luminous intensity exponents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::design) struct Dimensions(pub(super) [i8; 8]);

impl Dimensions {
    pub(super) const REAL: Self = Self::from_quantity(Quantity::REAL);

    pub(super) const fn from_quantity(q: Quantity) -> Self {
        Self([q.length, q.mass, q.angle, q.time, 0, 0, 0, 0])
    }

    fn quantity(self) -> Option<Quantity> {
        let [length, mass, angle, time, current, temperature, amount, intensity] = self.0;
        (current == 0 && temperature == 0 && amount == 0 && intensity == 0)
            .then_some(Quantity::of(length, mass, angle, time))
    }
}

impl Unit {
    fn one() -> Self {
        Self {
            exponents: [0; 4],
            scale: 1.0,
            extra: [0; 4],
            symbols: Some(Vec::new()),
            text: String::new(),
            compound: false,
            state: UnitState::Known,
        }
    }

    fn dimensions(&self) -> Dimensions {
        let [length, mass, angle, time] = self.exponents;
        let [current, temperature, amount, intensity] = self.extra;
        Dimensions([
            length,
            mass,
            angle,
            time,
            current,
            temperature,
            amount,
            intensity,
        ])
    }

    fn freecad_scale(&self) -> f64 {
        self.scale * (180.0 / PI).powi(i32::from(self.exponents[2]))
    }

    /// The product of the unit and `other` to the power `sign`, 1 or -1.
    fn times(self, other: Self, sign: i8) -> Self {
        match (self.state, other.state) {
            (UnitState::Unmodeled, _) | (_, UnitState::Unmodeled) => {
                return self.with_state(UnitState::Unmodeled);
            }
            (UnitState::Error, _) | (_, UnitState::Error) => {
                return self.with_state(UnitState::Error);
            }
            (UnitState::Known, UnitState::Known) => {}
        }
        let mut exponents = self.exponents;
        for (slot, other) in exponents.iter_mut().zip(other.exponents) {
            *slot += other * sign;
        }
        let mut extra = self.extra;
        for (slot, other) in extra.iter_mut().zip(other.extra) {
            *slot += other * sign;
        }
        let symbols = match (self.symbols, other.symbols) {
            (Some(mut left), Some(right)) => right
                .into_iter()
                .try_for_each(|(symbol, power)| {
                    left.push((symbol, power.checked_mul(i32::from(sign))?));
                    Some(())
                })
                .map(|()| left),
            _ => None,
        };
        Self {
            exponents,
            scale: if sign < 0 {
                self.scale / other.scale
            } else {
                self.scale * other.scale
            },
            extra,
            symbols,
            text: format!(
                "{}{}{}",
                self.text,
                if sign < 0 { '/' } else { '*' },
                other.text
            ),
            compound: true,
            state: UnitState::Known,
        }
        .check_exponents()
    }

    fn power(self, power: i64) -> Self {
        if self.state != UnitState::Known {
            let state = self.state;
            return self.with_state(state);
        }
        let Ok(power) = i32::try_from(power) else {
            return self.with_state(UnitState::Unmodeled);
        };
        let mut result = self.dimensions().0;
        for slot in &mut result {
            let Some(exponent) = i32::from(*slot)
                .checked_mul(power)
                .and_then(|value| i8::try_from(value).ok())
            else {
                return self.with_state(UnitState::Unmodeled);
            };
            *slot = exponent;
        }
        let [length, mass, angle, time, current, temperature, amount, intensity] = result;
        Self {
            exponents: [length, mass, angle, time],
            scale: self.scale.powi(power),
            extra: [current, temperature, amount, intensity],
            symbols: self.symbols.and_then(|symbols| {
                symbols
                    .into_iter()
                    .map(|(symbol, exponent)| Some((symbol, exponent.checked_mul(power)?)))
                    .collect()
            }),
            text: format!("{}^{power}", self.text),
            compound: true,
            state: UnitState::Known,
        }
        .check_exponents()
    }

    fn with_state(mut self, state: UnitState) -> Self {
        self.compound = true;
        self.state = state;
        self.symbols = None;
        self
    }

    fn check_exponents(self) -> Self {
        if self.dimensions().in_range() {
            self
        } else {
            self.with_state(UnitState::Error)
        }
    }

    /// The unit in neutral expression syntax.
    pub(in crate::design) fn spelling(&self) -> Option<String> {
        let symbols = self.symbols.as_ref()?;
        let factors = symbols
            .iter()
            .map(|(symbol, power)| match power {
                1 => symbol.name().to_owned(),
                _ => format!("{}^{power}", symbol.name()),
            })
            .collect::<Vec<_>>();
        (!factors.is_empty()).then(|| factors.join("*"))
    }
}

struct Cursor<'a> {
    text: &'a str,
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.at..].chars().next()
    }

    fn done(&self) -> bool {
        self.at == self.text.len()
    }

    fn eat(&mut self, expected: char) -> bool {
        let found = self.peek() == Some(expected);
        if found {
            self.at += expected.len_utf8();
        }
        found
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += self.peek().map_or(0, char::len_utf8);
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - start
    }

    /// A number as `FreeCAD`'s expression lexer reads one: `d*[.,]d+(e[+-]?d+)?`,
    /// `d+e[+-]?d+`, or `d+`.
    fn number(&mut self) -> Option<f64> {
        let start = self.at;
        let whole = self.digits();
        let rest = &self.text.as_bytes()[self.at..];
        if matches!(rest, [b'.' | b',', next, ..] if next.is_ascii_digit()) {
            self.at += 1;
            self.digits();
        } else if whole == 0 {
            self.at = start;
            return None;
        }
        let rest = &self.text.as_bytes()[self.at..];
        let exponent = match rest {
            [b'e' | b'E', next, ..] if next.is_ascii_digit() => 1,
            [b'e' | b'E', b'+' | b'-', next, ..] if next.is_ascii_digit() => 2,
            _ => 0,
        };
        if exponent > 0 {
            self.at += exponent;
            self.digits();
        }
        self.text[start..self.at].replace(',', ".").parse().ok()
    }

    /// A number and the unit that follows it.
    fn term(&mut self) -> Option<(f64, Option<Unit>)> {
        let start = self.at;
        let number = self.number()?;
        if decimal_out_of_range(&self.text[start..self.at], number) {
            self.at = start;
            return None;
        }
        let before = self.at;
        self.skip_space();
        let unit = self.unit();
        if unit.is_none() {
            self.at = before;
        }
        Some((number, unit))
    }

    /// A unit word, or a parenthesized unit expression, with an optional
    /// integer power.
    fn unit(&mut self) -> Option<Unit> {
        let start = self.at;
        let atom = if self.eat('(') {
            self.skip_space();
            let inner = self.unit_expression();
            self.skip_space();
            match inner {
                Some(mut inner) if self.eat(')') => {
                    inner.text = format!("({})", inner.text);
                    Some(inner)
                }
                _ => None,
            }
        } else {
            self.word()
        };
        let Some(atom) = atom else {
            self.at = start;
            return None;
        };
        let before = self.at;
        self.skip_space();
        if !self.eat('^') {
            self.at = before;
            return Some(atom);
        }
        self.skip_space();
        let negative = self.eat('-') || self.eat('\u{2212}');
        let start = self.at;
        let value = self.number()?;
        let power = formula::unit_integer(&self.text[start..self.at], value)?;
        Some(atom.power(if negative {
            power.checked_neg()?
        } else {
            power
        }))
    }

    /// Units joined by `*` and `/`.
    fn unit_expression(&mut self) -> Option<Unit> {
        let mut unit = self.unit()?;
        loop {
            let before = self.at;
            self.skip_space();
            let sign = if self.eat('*') {
                1
            } else if self.eat('/') {
                -1
            } else {
                self.at = before;
                return Some(unit);
            };
            self.skip_space();
            unit = unit.times(self.unit()?, sign);
        }
    }

    fn word(&mut self) -> Option<Unit> {
        let rest = &self.text[self.at..];
        let length = match rest.chars().next()? {
            ch if ch.is_alphabetic() => rest
                .char_indices()
                .find(|(_, ch)| !ch.is_alphabetic())
                .map_or(rest.len(), |(at, _)| at),
            ch @ ('°' | '′' | '″' | '\'' | '"') => ch.len_utf8(),
            _ => return None,
        };
        let word = &rest[..length];
        let unit = unit_word(word)?;
        self.at += length;
        Some(unit)
    }
}

const LENGTH: [i8; 4] = [1, 0, 0, 0];
const MASS: [i8; 4] = [0, 1, 0, 0];
const ANGLE: [i8; 4] = [0, 0, 1, 0];
const TIME: [i8; 4] = [0, 0, 0, 1];
const FREQUENCY: [i8; 4] = [0, 0, 0, -1];
const FORCE: [i8; 4] = [1, 1, 0, -2];
const PRESSURE: [i8; 4] = [-1, 1, 0, -2];
const POWER: [i8; 4] = [2, 1, 0, -3];
const ENERGY: [i8; 4] = [2, 1, 0, -2];

/// The SI prefix micro.
const MICRO: f64 = 1e-6;
/// The SI prefix nano.
const NANO: f64 = 1e-9;
/// The SI prefix pico.
const PICO: f64 = 1e-12;
/// Kilograms per avoirdupois pound.
const POUND: f64 = 0.453_592_37;
/// Pascals per pound-force per square inch.
const PSI: f64 = 6_894.757_293_168;
/// Joules per electronvolt.
const ELECTRONVOLT: f64 = 1.602_176_634e-19;

/// A unit word of `FreeCAD`'s expression lexer.
fn unit_word(word: &str) -> Option<Unit> {
    use UnitSymbol as S;
    let (exponents, scale, symbol) = match word {
        "nm" => (LENGTH, MICRO, Some(S::Nm)),
        "um" | "µm" => (LENGTH, 1e-3, Some(S::Um)),
        "mm" => (LENGTH, 1.0, Some(S::Mm)),
        "cm" => (LENGTH, 10.0, Some(S::Cm)),
        "dm" => (LENGTH, 100.0, None),
        "m" => (LENGTH, 1e3, Some(S::M)),
        "km" => (LENGTH, 1e6, Some(S::Km)),
        "in" | "\"" => (LENGTH, 25.4, Some(S::In)),
        "ft" | "'" => (LENGTH, 304.8, Some(S::Ft)),
        "thou" | "mil" => (LENGTH, 0.0254, None),
        "yd" => (LENGTH, 914.4, Some(S::Yd)),
        "mi" => (LENGTH, 1_609_344.0, Some(S::Mi)),
        "sqft" => ([2, 0, 0, 0], 92_903.04, None),
        "cft" => ([3, 0, 0, 0], 28_316_846.592, None),
        "l" => ([3, 0, 0, 0], 1e6, None),
        "ml" => ([3, 0, 0, 0], 1e3, None),
        "ug" | "µg" => (MASS, NANO, None),
        "mg" => (MASS, MICRO, None),
        "g" => (MASS, 1e-3, Some(S::G)),
        "kg" => (MASS, 1.0, Some(S::Kg)),
        "t" => (MASS, 1e3, None),
        "lb" | "lbm" => (MASS, POUND, Some(S::Lb)),
        "oz" => (MASS, POUND / 16.0, None),
        "st" => (MASS, POUND * 14.0, None),
        "cwt" => (MASS, POUND * 112.0, None),
        "s" => (TIME, 1.0, Some(S::S)),
        "min" => (TIME, 60.0, Some(S::Min)),
        "h" => (TIME, 3_600.0, Some(S::H)),
        "Hz" => (FREQUENCY, 1.0, None),
        "kHz" => (FREQUENCY, 1e3, None),
        "MHz" => (FREQUENCY, 1e6, None),
        "GHz" => (FREQUENCY, 1e9, None),
        "THz" => (FREQUENCY, 1e12, None),
        "mph" => ([1, 0, 0, -1], 447.04, None),
        "mN" => (FORCE, 1.0, None),
        "N" => (FORCE, 1e3, None),
        "kN" => (FORCE, 1e6, None),
        "MN" => (FORCE, 1e9, None),
        "lbf" => (FORCE, POUND * 9.806_65 * 1e3, None),
        "Pa" => (PRESSURE, 1e-3, None),
        "kPa" => (PRESSURE, 1.0, None),
        "MPa" => (PRESSURE, 1e3, None),
        "GPa" => (PRESSURE, 1e6, None),
        "bar" => (PRESSURE, 1e2, None),
        "mbar" => (PRESSURE, 1e-1, None),
        "Torr" => (PRESSURE, 101_325.0 / 760.0 * 1e-3, None),
        "mTorr" => (PRESSURE, 101_325.0 / 760.0 * MICRO, None),
        "uTorr" | "µTorr" => (PRESSURE, 101_325.0 / 760.0 * NANO, None),
        "psi" => (PRESSURE, PSI * 1e-3, None),
        "ksi" => (PRESSURE, PSI, None),
        "Mpsi" => (PRESSURE, PSI * 1e3, None),
        "nW" => (POWER, 1e-3, None),
        "uW" | "µW" => (POWER, 1.0, None),
        "mW" => (POWER, 1e3, None),
        "W" | "VA" => (POWER, 1e6, None),
        "kW" => (POWER, 1e9, None),
        "mJ" => (ENERGY, 1e3, None),
        "J" | "Nm" | "VAs" | "CV" | "Ws" => (ENERGY, 1e6, None),
        "kJ" => (ENERGY, 1e9, None),
        "kWh" => (ENERGY, 3.6e12, None),
        "eV" => (ENERGY, ELECTRONVOLT * 1e6, None),
        "keV" => (ENERGY, ELECTRONVOLT * 1e9, None),
        "MeV" => (ENERGY, ELECTRONVOLT * 1e12, None),
        "cal" => (ENERGY, 4.1868e6, None),
        "kcal" => (ENERGY, 4.1868e9, None),
        "°" | "deg" => (ANGLE, PI / 180.0, Some(S::Deg)),
        "rad" => (ANGLE, 1.0, Some(S::Rad)),
        "gon" => (ANGLE, PI / 200.0, Some(S::Grad)),
        "M" | "′" => (ANGLE, PI / 10_800.0, Some(S::Arcmin)),
        "AS" | "″" => (ANGLE, PI / 648_000.0, Some(S::Arcsec)),
        _ => return native_unit_word(word),
    };
    Some(Unit {
        exponents,
        scale,
        extra: [0; 4],
        symbols: symbol.map(|symbol| vec![(symbol, 1)]),
        text: word.to_owned(),
        compound: false,
        state: if matches!(word, "nW" | "uW" | "µW") {
            UnitState::Unmodeled
        } else {
            UnitState::Known
        },
    })
}

fn native_unit_word(word: &str) -> Option<Unit> {
    const CURRENT: [i8; 8] = [0, 0, 0, 0, 1, 0, 0, 0];
    const TEMPERATURE: [i8; 8] = [0, 0, 0, 0, 0, 1, 0, 0];
    const AMOUNT: [i8; 8] = [0, 0, 0, 0, 0, 0, 1, 0];
    const INTENSITY: [i8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
    const POTENTIAL: [i8; 8] = [2, 1, 0, -3, -1, 0, 0, 0];
    const CONDUCTANCE: [i8; 8] = [-2, -1, 0, 3, 2, 0, 0, 0];
    const RESISTANCE: [i8; 8] = [2, 1, 0, -3, -2, 0, 0, 0];
    const CHARGE: [i8; 8] = [0, 0, 0, 1, 1, 0, 0, 0];
    const FLUX_DENSITY: [i8; 8] = [0, 1, 0, -2, -1, 0, 0, 0];
    const FLUX: [i8; 8] = [2, 1, 0, -2, -1, 0, 0, 0];
    const CAPACITANCE: [i8; 8] = [-2, -1, 0, 4, 2, 0, 0, 0];
    const INDUCTANCE: [i8; 8] = [2, 1, 0, -2, -2, 0, 0, 0];

    let (exponents, scale) = match word {
        "A" => (CURRENT, 1.0),
        "nA" => (CURRENT, NANO),
        "uA" | "µA" => (CURRENT, MICRO),
        "mA" => (CURRENT, 1e-3),
        "kA" => (CURRENT, 1e3),
        "MA" => (CURRENT, 1e6),
        "K" => (TEMPERATURE, 1.0),
        "mK" => (TEMPERATURE, 1e-3),
        "uK" | "µK" => (TEMPERATURE, MICRO),
        "mol" => (AMOUNT, 1.0),
        "nmol" => (AMOUNT, NANO),
        "umol" | "µmol" => (AMOUNT, MICRO),
        "mmol" => (AMOUNT, 1e-3),
        "cd" => (INTENSITY, 1.0),
        "V" => (POTENTIAL, 1e6),
        "kV" => (POTENTIAL, 1e9),
        "mV" => (POTENTIAL, 1e3),
        "MS" => (CONDUCTANCE, 1.0),
        "kS" => (CONDUCTANCE, 1e-3),
        "S" => (CONDUCTANCE, MICRO),
        "mS" => (CONDUCTANCE, NANO),
        "uS" | "µS" => (CONDUCTANCE, PICO),
        "Ohm" => (RESISTANCE, 1e6),
        "kOhm" => (RESISTANCE, 1e9),
        "MOhm" => (RESISTANCE, 1e12),
        "C" => (CHARGE, 1.0),
        "T" => (FLUX_DENSITY, 1.0),
        "mT" => (FLUX_DENSITY, 1e-3),
        "G" => (FLUX_DENSITY, 1e-4),
        "Wb" => (FLUX, 1e6),
        "F" => (CAPACITANCE, MICRO),
        "mF" => (CAPACITANCE, NANO),
        "uF" | "µF" => (CAPACITANCE, PICO),
        "nF" => (CAPACITANCE, 1e-15),
        "pF" => (CAPACITANCE, 1e-18),
        "H" => (INDUCTANCE, 1e6),
        "mH" => (INDUCTANCE, 1e3),
        "uH" | "µH" => (INDUCTANCE, 1.0),
        "nH" => (INDUCTANCE, 1e-3),
        _ => return None,
    };
    let [length, mass, angle, time, current, temperature, amount, intensity] = exponents;
    Some(Unit {
        exponents: [length, mass, angle, time],
        scale,
        extra: [current, temperature, amount, intensity],
        symbols: None,
        text: word.to_owned(),
        compound: false,
        state: UnitState::Known,
    })
}

#[cfg(test)]
mod tests;
