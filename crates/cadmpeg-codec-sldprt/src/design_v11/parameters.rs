// SPDX-License-Identifier: Apache-2.0
//! The part's parameters (§5.12): each dimension of a feature-tree node is a
//! model parameter owned by its feature, and each global variable of the
//! equations folder is a user parameter.
//!
//! A dimension whose text is one literal holds a `Neutral` expression that
//! keeps the literal's authored unit. Any other text is a `Source`
//! expression in the `SolidWorks` dialect, its references to parameters as
//! reference tokens. A value is the cache the file stores: the literal, the
//! dimension's stored `Value`, or the value the decoder read from a record
//! that stores it in a stated unit.
//!
//! Keywords displays a dimension's number in the document's length unit or
//! in degrees, with no unit (`docs/formats/sldprt.md`). Where the root
//! definition states its length unit, a length dimension whose text is such
//! a display is a literal in that unit, and an angle dimension's a literal
//! in degrees (§5.12): the stored value where a record stores one, else the
//! displayed number, which Keywords rounds to the display precision and the
//! `design.parameter-display-rounded` note names. Where it states none, a
//! length display is no cache (§5.13). A parameter that a reference
//! dimension binds is `Driven` and keeps its value (§6.1). A dimension's kind is the quantity its stored record states: the
//! archive class of the `ResolvedFeatures` scalar it is bound to, the kind
//! the decoder's own parameter took from a record, a relation, or the
//! feature, or the quantity of the sketch or feature dimension that binds
//! it. A dimension whose kind no record states keeps the number its bound
//! scalar stores, as a `Native` kind holds it (§5.12). The
//! `design.parameter-value-unread` note names each dimension whose text is a
//! bare number and that still has no value.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroI8;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{
    Expression, ExpressionDialect, ExpressionToken, Neutral, Node, UnaryOp,
};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::{
    DesignParameter, DimensionDisplay, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};

use super::tree::Timeline;
use crate::history::literals::dimension_display;
use crate::history::parameters::eval::exact_integer_f64;
use crate::history::parameters::{
    expression_identifier_tokens, native_parameter_is_length, parse_native_parameter_literal,
};
use crate::history::project::{neutral_parameter_id, projected_parameter_names};
use crate::loss::{NamedCount, SldprtLossCode};
use crate::records::{ArchiveParameterClass, Feature, FeatureHistory, FeatureInputLane};

/// The kind of a dimension whose quantity the decoder has not read: a bare
/// number, or an expression over other parameters, that no stored record
/// types.
const UNREAD_KIND: &str = "sldprt:dimension";

/// Relative tolerance within which the decoder's value is a reading of a
/// display's number.
const EPS_DISPLAY_READING: f64 = 1.0e-12;

/// The decoder's readings of the part's dimensions: its own parameters by
/// id, and the archive class of each `ResolvedFeatures` scalar by id (`None`
/// where two lanes give one id two classes).
pub(super) struct Decoded<'a> {
    pub(super) parameters: HashMap<&'a str, &'a cadmpeg_ir::features::DesignParameter>,
    pub(super) classes: HashMap<&'a str, Option<ArchiveParameterClass>>,
}

impl<'a> Decoded<'a> {
    pub(super) fn new(
        parameters: &'a [cadmpeg_ir::features::DesignParameter],
        lanes: &'a [FeatureInputLane],
    ) -> Self {
        let mut classes = HashMap::new();
        for scalar in lanes.iter().flat_map(|lane| &lane.scalars) {
            let Some(class) = scalar.archive_class else {
                continue;
            };
            classes
                .entry(scalar.id.as_str())
                .and_modify(|existing: &mut Option<ArchiveParameterClass>| {
                    if *existing != Some(class) {
                        *existing = None;
                    }
                })
                .or_insert(Some(class));
        }
        Self {
            parameters: parameters
                .iter()
                .map(|parameter| (parameter.id.as_str(), parameter))
                .collect(),
            classes,
        }
    }

    /// The kind of a dimension and its cache, from the decoder's parameter of
    /// the same id and name: the archive class of the scalar it is bound to,
    /// else the quantity of its value. The cache is that value only where a
    /// stored record gave it: a bound scalar, or a value that is no reading of
    /// the dimension's bare display `display`. `None` where neither states a
    /// kind.
    fn dimension(
        &self,
        id: &ParameterId,
        name: &str,
        display: Option<f64>,
    ) -> Option<(ValueKind, Option<ParameterValue>)> {
        use cadmpeg_ir::features::ParameterValue as V10;
        let parameter = self.parameter(id, name)?;
        let class = parameter
            .native_ref
            .as_deref()
            .and_then(|scalar| self.classes.get(scalar).copied().flatten());
        let kind = match (class, &parameter.value) {
            (Some(ArchiveParameterClass::Length), _) | (None, Some(V10::Length(_))) => {
                ValueKind::Quantity(Quantity::LENGTH)
            }
            (Some(ArchiveParameterClass::Angle), _) | (None, Some(V10::Angle(_))) => {
                ValueKind::Quantity(Quantity::ANGLE)
            }
            (Some(ArchiveParameterClass::Integer), _) => ValueKind::Integer,
            (Some(ArchiveParameterClass::Scaler), _) => ValueKind::Quantity(Quantity::REAL),
            (None, _) => return None,
        };
        let value = parameter
            .value
            .clone()
            .filter(|_| stored(parameter, display))
            .and_then(|value| typed_cache(&kind, value));
        Some((kind, value))
    }

    /// The cache of a dimension whose kind no record states: the value of the
    /// decoder's parameter of the same id and name where a stored record gave
    /// it, in the variant a `Native` kind holds it in (§5.12): a real as
    /// `Number`, unscaled, an integer as `Integer`.
    fn native_cache(
        &self,
        id: &ParameterId,
        name: &str,
        display: Option<f64>,
    ) -> Option<ParameterValue> {
        use cadmpeg_ir::features::ParameterValue as V10;
        let parameter = self.parameter(id, name)?;
        match parameter
            .value
            .as_ref()
            .filter(|_| stored(parameter, display))?
        {
            V10::Real(real) => FiniteReal::new(real.get()).map(ParameterValue::Number),
            V10::Integer(integer) => Some(ParameterValue::Integer(*integer)),
            V10::Boolean(flag) => Some(ParameterValue::Boolean(*flag)),
            V10::String(text) => Some(ParameterValue::Text(text.clone())),
            V10::Length(_) | V10::Angle(_) => None,
        }
    }

    /// The decoder's parameter of id `id` named `name`.
    fn parameter(
        &self,
        id: &ParameterId,
        name: &str,
    ) -> Option<&'a cadmpeg_ir::features::DesignParameter> {
        self.parameters
            .get(id.as_str())
            .copied()
            .filter(|parameter| parameter.name == name)
    }
}

/// Whether a stored record gave the value of the decoder's `parameter`: a
/// bound scalar, or a value that is no reading of the dimension's bare
/// display `display`.
fn stored(parameter: &cadmpeg_ir::features::DesignParameter, display: Option<f64>) -> bool {
    parameter.native_ref.is_some()
        || display.is_some_and(|display| {
            parameter
                .value
                .as_ref()
                .is_some_and(|value| !reads_display(value, display))
        })
}

/// Whether the decoder's `value` is the display's number read in a unit the
/// display does not state: as millimetres, metres, or radians, or as itself.
fn reads_display(value: &cadmpeg_ir::features::ParameterValue, display: f64) -> bool {
    use cadmpeg_ir::features::ParameterValue as V10;
    let number = match value {
        V10::Length(length) => length.get(),
        V10::Angle(angle) => angle.get(),
        V10::Real(real) => real.get(),
        V10::Integer(integer) => match exact_integer_f64(*integer) {
            Some(number) => number,
            None => return false,
        },
        V10::Boolean(_) | V10::String(_) => return false,
    };
    let close = |reading: f64| {
        (number - reading).abs() <= EPS_DISPLAY_READING * number.abs().max(reading.abs()).max(1.0)
    };
    close(display) || close(display * 1000.0) || close(display.to_radians())
}

/// The cache of the decoder's `value` for a parameter of `kind`, or `None`
/// where the value is not of that kind.
fn typed_cache(
    kind: &ValueKind,
    value: cadmpeg_ir::features::ParameterValue,
) -> Option<ParameterValue> {
    use cadmpeg_ir::features::ParameterValue as V10;
    let number = |value: f64| FiniteReal::new(value).map(ParameterValue::Number);
    match (kind, value) {
        (ValueKind::Quantity(Quantity::LENGTH), V10::Length(length)) => number(length.get()),
        (ValueKind::Quantity(Quantity::ANGLE), V10::Angle(angle)) => number(angle.get()),
        (ValueKind::Quantity(Quantity::REAL), V10::Real(real)) => number(real.get()),
        (ValueKind::Quantity(Quantity::REAL), V10::Integer(integer)) => {
            number(exact_integer_f64(integer)?)
        }
        (ValueKind::Integer, V10::Integer(integer)) => Some(ParameterValue::Integer(integer)),
        _ => None,
    }
}

/// The number of a dimension display that states no unit, after its count
/// (`4X`), its diameter or radius modifier, and a parenthesized comment: the
/// number Keywords shows in the document's unit (`docs/formats/sldprt.md`).
pub(super) fn bare_display_number(text: &str) -> Option<f64> {
    let mut body = text.trim();
    if let Some((count, rest)) = body.split_once(['X', 'x']) {
        if !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit()) {
            body = rest.trim_start();
        }
    }
    for modifier in [
        "<MOD-DIAM>",
        "&lt;MOD-DIAM&gt;",
        "<MOD-RHO>",
        "&lt;MOD-RHO&gt;",
        "\u{2300}",
        "\u{00d8}",
        "R",
        "r",
    ] {
        if let Some(rest) = body.strip_prefix(modifier) {
            body = rest.trim_start();
            break;
        }
    }
    if let Some((number, comment)) = body.split_once('(') {
        if comment.trim_end().ends_with(')') {
            body = number.trim_end();
        }
    }
    body.parse::<f64>().ok().filter(|number| number.is_finite())
}

/// One parameter of the source, before its expression is read.
struct Entry<'a> {
    feature: &'a Feature,
    owner: Option<FeatureId>,
    name: NonBlankString,
    id: ParameterId,
    text: &'a str,
}

/// What the decoder read beside a parameter's text: its own parameters and
/// scalar classes, the definition's length unit, and the quantity of each
/// dimension that binds a parameter.
pub(super) struct Readings<'a> {
    pub(super) decoded: Decoded<'a>,
    pub(super) length_unit: Option<UnitSymbol>,
    pub(super) bound: HashMap<ParameterId, Quantity>,
    /// The parameters a radius or diameter dimension binds, by what it
    /// measures: such a parameter states no display of that measure.
    pub(super) measured: HashMap<ParameterId, DimensionDisplay>,
    /// The parameters a reference dimension binds.
    pub(super) driven: HashSet<ParameterId>,
}

/// The parameters of the timeline's features and of the equations folders,
/// before their expressions are read.
pub(super) struct Entries<'a> {
    list: Vec<Entry<'a>>,
    by_owner: HashMap<(FeatureId, String), ParameterId>,
    ids: HashSet<ParameterId>,
}

impl<'a> Entries<'a> {
    pub(super) fn new(
        histories: &'a [FeatureHistory],
        timeline: &Timeline,
    ) -> Result<Self, CodecError> {
        let entries = entries(histories, timeline)?;
        let by_owner = entries
            .iter()
            .filter_map(|entry| {
                Some((
                    (entry.owner.clone()?, entry.name.as_str().to_owned()),
                    entry.id.clone(),
                ))
            })
            .collect();
        let ids = entries.iter().map(|entry| entry.id.clone()).collect();
        Ok(Self {
            list: entries,
            by_owner,
            ids,
        })
    }

    /// The parameter of `owner` named `name`.
    pub(super) fn owned(&self, owner: &FeatureId, name: &str) -> Option<&ParameterId> {
        self.by_owner.get(&(owner.clone(), name.to_owned()))
    }

    /// Whether `id` is one of these parameters.
    pub(super) fn contains(&self, id: &ParameterId) -> bool {
        self.ids.contains(id)
    }

    /// The parameters, their expressions read, the note naming those whose
    /// literal is a rounded display, and the note naming the dimensions
    /// whose text is a bare number and that have no value.
    pub(super) fn parameters(
        &self,
        scope: &ProductDefinitionId,
        readings: &Readings<'_>,
    ) -> (Vec<DesignParameter>, Vec<LossNote>) {
        let aliases = Aliases::new(&self.list);
        let mut rounded = NamedCount::default();
        let mut unread_kind = NamedCount::default();
        let mut unread_unit = NamedCount::default();
        let parameters = self
            .list
            .iter()
            .map(|entry| {
                let reading = read(entry, &aliases, readings);
                if reading.rounded {
                    rounded.push(entry.id.as_str());
                }
                match reading.unvalued {
                    Some(Unvalued::Kind) => unread_kind.push(entry.id.as_str()),
                    Some(Unvalued::LengthUnit) => unread_unit.push(entry.id.as_str()),
                    None => {}
                }
                parameter(entry, reading, scope, readings)
            })
            .collect();
        let mut losses = Vec::new();
        if !rounded.is_empty() {
            losses.push(SldprtLossCode::DesignParameterDisplayRounded.note(format!(
                "{} parameters hold their dimension's displayed number, rounded to the display \
                 precision, as their literal and value: {rounded}",
                rounded.count()
            )));
        }
        let unvalued = [
            (
                unread_kind,
                "whose dimension no record types or binds to a stored value of its kind",
            ),
            (
                unread_unit,
                "lengths in a part whose length unit is not read",
            ),
        ]
        .into_iter()
        .filter(|(named, _)| !named.is_empty())
        .map(|(named, why)| format!("{} {why} ({named})", named.count()))
        .collect::<Vec<_>>();
        if !unvalued.is_empty() {
            losses.push(SldprtLossCode::DesignParameterValueUnread.note(format!(
                "Parameters whose dimension text is a bare number hold no value: {}",
                unvalued.join("; ")
            )));
        }
        (parameters, losses)
    }
}

fn entries<'a>(
    histories: &'a [FeatureHistory],
    timeline: &Timeline,
) -> Result<Vec<Entry<'a>>, CodecError> {
    let mut entries = Vec::new();
    let mut user_names = HashSet::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        let owner = match timeline.features_by_record.get(&feature.id) {
            Some(owner) => Some(owner.clone()),
            None if timeline.equation_records.contains(&feature.id) => None,
            None => continue,
        };
        for (ordinal, name) in projected_parameter_names(feature).into_iter().enumerate() {
            let Some(text) = feature.parameters.get(name.as_str()) else {
                continue;
            };
            if owner.is_none() && !user_names.insert(name.clone()) {
                continue;
            }
            let Some(key) = NonBlankString::new(name) else {
                continue;
            };
            entries.push(Entry {
                feature,
                owner: owner.clone(),
                name: key,
                id: neutral_parameter_id(feature, ordinal)?,
                text,
            });
        }
    }
    Ok(entries)
}

fn parameter(
    entry: &Entry<'_>,
    reading: Reading,
    scope: &ProductDefinitionId,
    readings: &Readings<'_>,
) -> DesignParameter {
    DesignParameter {
        id: entry.id.clone(),
        scope: scope.clone(),
        owner: entry.owner.clone().map(ParameterOwner::Feature),
        name: Some(entry.name.clone()),
        kind: reading.kind,
        unit: None,
        definition: if readings.driven.contains(&entry.id) {
            ParameterDefinition::Driven
        } else {
            ParameterDefinition::Expression(reading.expression)
        },
        value: reading.value,
        display: reading.display,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    }
}

struct Reading {
    kind: ValueKind,
    expression: Expression,
    value: Option<ParameterValue>,
    display: Option<DimensionDisplay>,
    /// The literal and value are a display's rounded number.
    rounded: bool,
    /// Why a dimension whose text is a bare number has no value.
    unvalued: Option<Unvalued>,
}

/// Why a dimension whose text is a bare number has no value.
#[derive(Clone, Copy)]
enum Unvalued {
    /// No record types the dimension, or binds it to a stored value of its
    /// kind.
    Kind,
    /// The dimension is a length and the part's length unit is not read.
    LengthUnit,
}

fn read(entry: &Entry<'_>, aliases: &Aliases, readings: &Readings<'_>) -> Reading {
    let decoded = &readings.decoded;
    let user = entry.owner.is_none();
    let (feature, name, text) = (entry.feature, entry.name.as_str(), entry.text);
    if let Some((node, kind, value)) = literal(text, user) {
        return Reading {
            kind,
            expression: Expression::Neutral(Neutral(node)),
            value: Some(value),
            display: None,
            rounded: false,
            unvalued: None,
        };
    }
    let tokens = source_tokens(text, entry.owner.as_ref(), aliases);
    let references = tokens
        .iter()
        .any(|token| matches!(token, ExpressionToken::Reference(_)));
    let bare = bare_display_number(text);
    let parsed = bare
        .is_none()
        .then(|| parse_native_parameter_literal(feature, name, text))
        .flatten();
    let display = dimension_display(text);
    if parsed.is_none() && display.is_none() && !references && is_text(text) {
        return Reading {
            kind: ValueKind::Text,
            expression: Expression::Neutral(Neutral(Node::Text(text.to_owned()))),
            value: Some(ParameterValue::Text(text.to_owned())),
            display: None,
            rounded: false,
            unvalued: None,
        };
    }
    let stored = feature
        .dimension_properties
        .get(name)
        .and_then(|properties| properties.get("Value"));
    let parsed = parsed.or_else(|| {
        stored
            .filter(|value| bare_display_number(value).is_none())
            .and_then(|value| parse_native_parameter_literal(feature, name, value))
    });
    let parsed = parsed.filter(|value| {
        user || !matches!(
            value,
            cadmpeg_ir::features::ParameterValue::Integer(_)
                | cadmpeg_ir::features::ParameterValue::Real(_)
        )
    });
    let bound = readings.bound.get(&entry.id).copied();
    let (kind, value) = match parsed.and_then(cache) {
        Some((kind, value)) => (kind, Some(value)),
        None if user => (ValueKind::Quantity(Quantity::REAL), None),
        None => match decoded.dimension(&entry.id, name, bare) {
            Some(reading) => reading,
            None if native_parameter_is_length(feature, name, Some(text)) => {
                (ValueKind::Quantity(Quantity::LENGTH), None)
            }
            None => match bound {
                Some(quantity) => (ValueKind::Quantity(quantity), None),
                None => (
                    ValueKind::Native(cadmpeg_core::nonblank_const!(UNREAD_KIND)),
                    decoded.native_cache(&entry.id, name, bare),
                ),
            },
        },
    };
    let display = display
        .filter(|_| kind == ValueKind::Quantity(Quantity::LENGTH))
        .map(|display| match display {
            cadmpeg_ir::features::DimensionDisplay::Diameter => DimensionDisplay::Diameter,
            cadmpeg_ir::features::DimensionDisplay::Radius => DimensionDisplay::Radius,
        })
        .filter(|display| readings.measured.get(&entry.id) != Some(display));
    let number = bare.filter(|_| !user && !references);
    if let Some(number) = number {
        if let Some((node, literal)) =
            displayed_literal(number, &kind, value.as_ref(), readings.length_unit)
        {
            return Reading {
                kind,
                expression: Expression::Neutral(Neutral(node)),
                rounded: value.is_none(),
                value: Some(literal),
                display,
                unvalued: None,
            };
        }
    }
    let unvalued = (number.is_some() && value.is_none()).then_some(
        if kind == ValueKind::Quantity(Quantity::LENGTH) && readings.length_unit.is_none() {
            Unvalued::LengthUnit
        } else {
            Unvalued::Kind
        },
    );
    Reading {
        kind,
        expression: Expression::Source {
            dialect: ExpressionDialect::SolidWorks,
            tokens,
        },
        value,
        display,
        rounded: false,
        unvalued,
    }
}

/// The literal of a dimension whose text is the bare display `number`, and
/// its value: a length in the definition's length unit `length_unit`, an
/// angle in degrees. The literal's number is the stored value `cache` in that
/// unit, to 15 significant digits, where a record stores one, else the
/// displayed number (§5.12).
fn displayed_literal(
    number: f64,
    kind: &ValueKind,
    cache: Option<&ParameterValue>,
    length_unit: Option<UnitSymbol>,
) -> Option<(Node<ParameterRef>, ParameterValue)> {
    let symbol = match kind {
        ValueKind::Quantity(Quantity::LENGTH) => length_unit?,
        ValueKind::Quantity(Quantity::ANGLE) => UnitSymbol::Deg,
        _ => return None,
    };
    let (_, scale) = canonical(symbol)?;
    let (number, value) = match cache {
        Some(ParameterValue::Number(value)) => (significant(value.get() / scale)?, value.get()),
        Some(_) => return None,
        None => (number, number * scale),
    };
    Some((
        number_node(number, Some(symbol))?,
        ParameterValue::Number(FiniteReal::new(value)?),
    ))
}

/// Significant decimal digits of a literal read from a stored value (§5.12).
const LITERAL_DIGITS: usize = 15;

/// `value` rounded to [`LITERAL_DIGITS`] significant decimal digits.
fn significant(value: f64) -> Option<f64> {
    format!("{value:.*e}", LITERAL_DIGITS - 1)
        .parse::<f64>()
        .ok()
        .filter(|rounded| rounded.is_finite())
}

/// The tree of the literal `signed` with the unit `symbol`.
fn number_node(signed: f64, symbol: Option<UnitSymbol>) -> Option<Node<ParameterRef>> {
    let unit = match symbol {
        Some(symbol) => Unit(vec![UnitFactor {
            symbol,
            power: NonZeroI8::new(1)?,
        }]),
        None => Unit::default(),
    };
    let number = Node::Number {
        value: NonNegativeReal::new(signed.abs())?,
        unit,
    };
    Some(if signed.is_sign_negative() && signed != 0.0 {
        Node::Unary {
            op: UnaryOp::Negate,
            operand: Box::new(number),
        }
    } else {
        number
    })
}

/// The kind and canonical cache of a value the v10 literal reader parsed. A
/// bare number reaches here only for a global variable, which has no
/// declared quantity, so a whole number is a dimensionless real.
fn cache(value: cadmpeg_ir::features::ParameterValue) -> Option<(ValueKind, ParameterValue)> {
    use cadmpeg_ir::features::ParameterValue as Parsed;
    let number = |quantity, value: f64| {
        Some((
            ValueKind::Quantity(quantity),
            ParameterValue::Number(FiniteReal::new(value)?),
        ))
    };
    match value {
        Parsed::Length(length) => number(Quantity::LENGTH, length.get()),
        Parsed::Angle(angle) => number(Quantity::ANGLE, angle.get()),
        Parsed::Real(real) => number(Quantity::REAL, real.get()),
        Parsed::Integer(integer) => number(Quantity::REAL, exact_integer_f64(integer)?),
        Parsed::Boolean(flag) => Some((ValueKind::Boolean, ParameterValue::Boolean(flag))),
        Parsed::String(text) => Some((ValueKind::Text, ParameterValue::Text(text))),
    }
}

/// Text that is a word or phrase, not an expression: no operator, no quoted
/// name, and no qualified name.
fn is_text(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && !text.contains([
            '+', '-', '*', '/', '^', '=', '<', '>', '(', ')', ',', '"', '@',
        ])
        && text.parse::<f64>().is_err()
}

/// The units a `SolidWorks` literal may carry, longest suffix first.
const LITERAL_UNITS: [(&str, UnitSymbol); 11] = [
    ("deg", UnitSymbol::Deg),
    ("rad", UnitSymbol::Rad),
    ("mm", UnitSymbol::Mm),
    ("cm", UnitSymbol::Cm),
    ("km", UnitSymbol::Km),
    ("um", UnitSymbol::Um),
    ("nm", UnitSymbol::Nm),
    ("in", UnitSymbol::In),
    ("ft", UnitSymbol::Ft),
    ("\u{00b0}", UnitSymbol::Deg),
    ("m", UnitSymbol::M),
];

/// A literal of one number with a unit the vocabulary names, a Boolean, or a
/// global variable's bare number, which is dimensionless: its tree, kind, and
/// canonical value.
///
/// A dimension's bare number is no literal here: it is a display in the
/// document's unit, which the root definition does not state (§5.13, §5.15).
fn literal(text: &str, user: bool) -> Option<(Node<ParameterRef>, ValueKind, ParameterValue)> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("false") {
        let flag = text.eq_ignore_ascii_case("true");
        return Some((
            Node::Boolean(flag),
            ValueKind::Boolean,
            ParameterValue::Boolean(flag),
        ));
    }
    let (number, unit) = LITERAL_UNITS
        .iter()
        .find_map(|(suffix, symbol)| Some((text.strip_suffix(suffix)?, Some(*symbol))))
        .unwrap_or((text, None));
    let number = number.trim();
    if number.is_empty()
        || !number
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+' | b'e' | b'E'))
    {
        return None;
    }
    let signed = number
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())?;
    let (kind, value) = match unit {
        Some(symbol) => {
            let (quantity, scale) = canonical(symbol)?;
            (
                ValueKind::Quantity(quantity),
                ParameterValue::Number(FiniteReal::new(signed * scale)?),
            )
        }
        None if user => (
            ValueKind::Quantity(Quantity::REAL),
            ParameterValue::Number(FiniteReal::new(signed)?),
        ),
        None => return None,
    };
    Some((number_node(signed, unit)?, kind, value))
}

/// Millimetres in one nanometre.
const MM_PER_NM: f64 = 1.0e-6;

/// The quantity of a literal unit and its factor to the canonical unit.
fn canonical(symbol: UnitSymbol) -> Option<(Quantity, f64)> {
    Some(match symbol {
        UnitSymbol::Nm => (Quantity::LENGTH, MM_PER_NM),
        UnitSymbol::Um => (Quantity::LENGTH, 1.0e-3),
        UnitSymbol::Mm => (Quantity::LENGTH, 1.0),
        UnitSymbol::Cm => (Quantity::LENGTH, 10.0),
        UnitSymbol::M => (Quantity::LENGTH, 1000.0),
        UnitSymbol::Km => (Quantity::LENGTH, 1.0e6),
        UnitSymbol::In => (Quantity::LENGTH, 25.4),
        UnitSymbol::Ft => (Quantity::LENGTH, 304.8),
        UnitSymbol::Rad => (Quantity::ANGLE, 1.0),
        UnitSymbol::Deg => (Quantity::ANGLE, std::f64::consts::PI / 180.0),
        _ => return None,
    })
}

/// The source text as tokens, each name that resolves to one parameter a
/// reference.
fn source_tokens(
    text: &str,
    owner: Option<&FeatureId>,
    aliases: &Aliases,
) -> Vec<ExpressionToken<ParameterRef>> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    for identifier in expression_identifier_tokens(text).unwrap_or_default() {
        if identifier.is_syntax() {
            continue;
        }
        let Some(parameter) = aliases.resolve(identifier.value(), owner) else {
            continue;
        };
        let end = text.len() - identifier.following().len();
        let start = end - identifier.raw().len();
        if start > cursor {
            tokens.push(ExpressionToken::Text(text[cursor..start].to_owned()));
        }
        tokens.push(ExpressionToken::Reference(ParameterRef {
            target: RefTarget::Parameter(parameter.clone()),
            through: None,
        }));
        cursor = end;
    }
    if cursor < text.len() {
        tokens.push(ExpressionToken::Text(text[cursor..].to_owned()));
    }
    tokens
}

/// The names by which an expression reaches a parameter: `name@feature`
/// anywhere, a dimension's bare name within its own feature, and a global
/// variable's name anywhere. A name that reaches two parameters resolves to
/// none.
struct Aliases {
    qualified: HashMap<String, Option<ParameterId>>,
    local: HashMap<(FeatureId, String), Option<ParameterId>>,
    global: HashMap<String, Option<ParameterId>>,
}

impl Aliases {
    fn new(entries: &[Entry<'_>]) -> Self {
        fn insert<K: Eq + std::hash::Hash>(
            map: &mut HashMap<K, Option<ParameterId>>,
            key: K,
            id: &ParameterId,
        ) {
            map.entry(key)
                .and_modify(|existing| {
                    if existing.as_ref() != Some(id) {
                        *existing = None;
                    }
                })
                .or_insert_with(|| Some(id.clone()));
        }
        let mut aliases = Self {
            qualified: HashMap::new(),
            local: HashMap::new(),
            global: HashMap::new(),
        };
        for entry in entries {
            let name = entry.name.as_str().to_owned();
            match &entry.owner {
                Some(owner) => {
                    if !entry.feature.name.is_empty() {
                        let qualified = format!("{name}@{}", entry.feature.name);
                        insert(&mut aliases.qualified, qualified, &entry.id);
                    }
                    insert(&mut aliases.local, (owner.clone(), name), &entry.id);
                }
                None => insert(&mut aliases.global, name, &entry.id),
            }
        }
        aliases
    }

    fn resolve(&self, name: &str, owner: Option<&FeatureId>) -> Option<&ParameterId> {
        if let Some(found) = self.qualified.get(name) {
            return found.as_ref();
        }
        if let Some(found) =
            owner.and_then(|owner| self.local.get(&(owner.clone(), name.to_owned())))
        {
            return found.as_ref();
        }
        self.global.get(name)?.as_ref()
    }
}
