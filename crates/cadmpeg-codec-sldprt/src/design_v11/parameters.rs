// SPDX-License-Identifier: Apache-2.0
//! The part's parameters (§5.12): each dimension of a feature-tree node is a
//! model parameter owned by its feature, and each global variable of the
//! equations folder is a user parameter.
//!
//! A dimension whose text is one literal holds a `Neutral` expression that
//! keeps the literal's authored unit. A parameter whose text is one
//! reference to a parameter of its kind holds that reference as a `Neutral`
//! link (§5.13). [`link_bare_references`] keeps as one `Text` token a text
//! that is one reference to a parameter whose kind differs or is not read,
//! and `equations::bare_references` states its unit-blind tree where that
//! tree is one reference or the document's values confirm it. Any other
//! text is a `Source` expression in the `SolidWorks` dialect, its references
//! to parameters as reference tokens. A value is the cache the file stores:
//! the literal, the dimension's stored `Value`, or the value the decoder
//! read from a record that stores it in a stated unit.
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
//! archive class of the `ResolvedFeatures` scalar it is bound to and the
//! parameter class that scalar's own record states (where the two differ,
//! the dimension is read as one no record types), the kind the decoder's
//! own parameter took from a record, a relation, or the feature, or the
//! quantity of the sketch or feature dimension that binds it. A bare display
//! that none of these types takes the class and stored value of the scalar
//! its feature holds under its name, where that value shows the display
//! (Rule H1): a length at the part's length precision, where its units table
//! states one, as Keywords rounds a length to that many places whether it
//! drops trailing zeros or keeps them. A bare display that rounds the length
//! the equation manager stores beside the dimension at that precision takes
//! that length where the line that drives the dimension, evaluated over the
//! values stored beside its names, gives it, unless the decoder bound it to a
//! PMI record. A
//! dimension whose kind no record states keeps the number its bound scalar
//! stores, as a `Native` kind holds it (§5.12). The
//! `design.parameter-value-unread` note names each dimension whose text is a
//! bare number and that still has no value.
//!
//! A dimension that the history does not name and that the decoder read from
//! another record (a `ResolvedFeatures` scalar, a PMI dimension, a Move Face
//! translation) is a parameter of its feature under the decoder's id, its
//! literal the stored value (§5.12). A parameter of a feature-tree node
//! outside the timeline (the annotations node, a detail folder) has no owner
//! that §5.12 types; the `design.parameter-owner-outside-timeline` note names
//! it. Interim reading until the specification types that owner.

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
use crate::records::{
    ArchiveParameterClass, Feature, FeatureHistory, FeatureInputLane, FeatureInputScalarRole,
};
use crate::resolved_features::assembly::ConfigurationMaps;
use crate::resolved_features::parameter_classes::{stated_parameters, StatedParameter, Statement};

/// The kind of a dimension whose quantity the decoder has not read: a bare
/// number, or an expression over other parameters, that no stored record
/// types.
const UNREAD_KIND: &str = "sldprt:dimension";

/// Relative tolerance within which the decoder's value is a reading of a
/// display's number.
const EPS_DISPLAY_READING: f64 = 1.0e-12;

/// Relative tolerance within which a stored value is a number a display
/// shows, beyond the display's own rounding.
const EPS_STORED_DISPLAY: f64 = 1.0e-9;

/// Millimetres in a metre: a length scalar stores metres.
const MM_PER_M: f64 = 1000.0;

/// The decoder's readings of the part's dimensions: its own parameters by
/// id, the archive class of each `ResolvedFeatures` scalar by id (`None`
/// where two lanes give one id two classes), what each scalar's own record
/// states of its parameter, and the scalars each history feature holds under
/// each name.
pub(super) struct Decoded<'a> {
    pub(super) parameters: HashMap<&'a str, &'a cadmpeg_ir::features::parameters::DesignParameter>,
    pub(super) classes: HashMap<&'a str, Option<ArchiveParameterClass>>,
    pub(super) stated: HashMap<&'a str, Statement>,
    pub(super) named: HashMap<(&'a str, &'a str), Vec<NamedScalar>>,
}

/// A scalar a history feature holds under a dimension's name: what its
/// record states of its parameter, and whether it drives the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct NamedScalar {
    pub(super) stated: Option<Statement>,
    pub(super) driving: bool,
}

impl<'a> Decoded<'a> {
    pub(super) fn new(
        parameters: &'a [cadmpeg_ir::features::parameters::DesignParameter],
        lanes: &'a [FeatureInputLane],
        configuration_maps: &ConfigurationMaps,
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
        let mut stated = HashMap::new();
        let mut named = HashMap::<_, Vec<_>>::new();
        for lane in lanes {
            let lane_stated = stated_parameters(lane, configuration_maps.get(&lane.id));
            let names = lane
                .names
                .iter()
                .map(|name| (name.id.as_str(), name.value.as_str()))
                .collect::<HashMap<_, _>>();
            for scalar in &lane.scalars {
                let (Some(feature), Some(&name)) = (
                    scalar.feature_ref.as_deref(),
                    names.get(scalar.name.as_str()),
                ) else {
                    continue;
                };
                named.entry((feature, name)).or_default().push(NamedScalar {
                    stated: lane_stated.get(scalar.id.as_str()).copied(),
                    driving: scalar.role == FeatureInputScalarRole::Driving,
                });
            }
            stated.extend(lane_stated);
        }
        Self {
            parameters: parameters
                .iter()
                .map(|parameter| (parameter.id.as_str(), parameter))
                .collect(),
            classes,
            stated,
            named,
        }
    }

    /// The kind of a dimension and its cache, from the decoder's parameter of
    /// the same id and name: the archive class of the scalar it is bound to,
    /// else the parameter that scalar's record states where it agrees with
    /// the dimension's bare display `display` in the part's length unit
    /// `unit` and length precision `places` (Rule H1), else the quantity of
    /// its value. The cache is that value only where a stored record gave it:
    /// a bound scalar, or a value that is no reading of the display; else the
    /// stated parameter's value. `Ok(None)` where none of these states a
    /// kind. A bound scalar whose class token the decoder's class sources
    /// name differently, or whose archive class differs from the class its
    /// record states, gives no kind and no value, whatever the earlier
    /// reading: [`Unvalued::ClassesDisagree`]. The kind is assumed where it
    /// is the class of a [`Statement::Assumed`].
    fn dimension(
        &self,
        id: &ParameterId,
        name: &str,
        display: Option<Shown>,
        unit: Option<UnitSymbol>,
        places: Option<u32>,
    ) -> Result<Option<Typed>, Unvalued> {
        use cadmpeg_ir::features::parameters::ParameterValue as V10;
        let Some(parameter) = self.parameter(id, name) else {
            return Ok(None);
        };
        let scalar = parameter.native_ref.as_deref();
        let archive = scalar.and_then(|scalar| self.classes.get(scalar).copied().flatten());
        let (stated, assumed) = match scalar.and_then(|scalar| self.stated.get(scalar)) {
            Some(Statement::Contested) => return Err(Unvalued::ClassesDisagree),
            Some(Statement::Parameter(stated)) => (Some(*stated), false),
            Some(Statement::Assumed(stated)) => (Some(*stated), true),
            None => (None, false),
        };
        if archive
            .zip(stated)
            .is_some_and(|(archive, stated)| archive != stated.class())
        {
            return Err(Unvalued::ClassesDisagree);
        }
        let stated = stated.filter(|stated| {
            display.is_none_or(|display| agrees(*stated, display, unit, places) != Some(false))
        });
        let assumed = assumed && archive.is_none() && stated.is_some();
        let class = archive.or(stated.map(StatedParameter::class));
        let kind = match (class, &parameter.value) {
            (Some(ArchiveParameterClass::Length), _) | (None, Some(V10::Length(_))) => {
                ValueKind::Quantity(Quantity::LENGTH)
            }
            (Some(ArchiveParameterClass::Angle), _) | (None, Some(V10::Angle(_))) => {
                ValueKind::Quantity(Quantity::ANGLE)
            }
            (Some(ArchiveParameterClass::Integer), _) => ValueKind::Integer,
            (Some(ArchiveParameterClass::Scaler), _) => ValueKind::Quantity(Quantity::REAL),
            (None, _) => return Ok(None),
        };
        let value = parameter
            .value
            .clone()
            .filter(|_| stored(parameter, display.map(|display| display.number)))
            .and_then(|value| typed_cache(&kind, value))
            .or_else(|| {
                stated
                    .and_then(typed)
                    .filter(|(stated, _)| *stated == kind)
                    .map(|(_, value)| value)
            });
        Ok(Some(Typed {
            kind,
            value,
            assumed,
        }))
    }

    /// The kind and value of a dimension of the history feature `feature`
    /// named `name` whose text is the bare display `display`, from the
    /// scalars the feature holds under that name, under Rule H1: a scalar
    /// counts where the parameter its record states shows `display` at the
    /// part's length precision `places` (see [`agrees`]), and, for a length
    /// in a part whose length unit `unit` is not read, where it drives the
    /// model, a reading that holds on the stored results of typed
    /// dimensions. A driving scalar outranks the others. The scalars that
    /// count state one parameter where they state one class and values that
    /// are one number (see [`same_parameter`]); its class is the kind and the
    /// first counted scalar's stored value the value, assumed where every one
    /// of them is a [`Statement::Assumed`]. Otherwise the reason the
    /// dimension has no value.
    fn named_dimension(
        &self,
        feature: &str,
        name: &str,
        display: Shown,
        unit: Option<UnitSymbol>,
        places: Option<u32>,
    ) -> Result<Typed, Unvalued> {
        let scalars = self.named.get(&(feature, name)).ok_or(Unvalued::NoScalar)?;
        let mut counted = Vec::new();
        let mut reason = Unvalued::NoScalar;
        for scalar in scalars {
            let stated = match scalar.stated {
                Some(Statement::Parameter(stated) | Statement::Assumed(stated)) => stated,
                Some(Statement::Contested) => {
                    reason = reason.max(Unvalued::ClassesDisagree);
                    continue;
                }
                None => {
                    reason = reason.max(Unvalued::ClassUnread);
                    continue;
                }
            };
            match agrees(stated, display, unit, places) {
                Some(true) => counted.push(*scalar),
                None if scalar.driving => counted.push(*scalar),
                None => reason = reason.max(Unvalued::LengthUnit),
                Some(false) => reason = reason.max(Unvalued::Disagrees),
            }
        }
        if counted.iter().any(|scalar| scalar.driving) {
            counted.retain(|scalar| scalar.driving);
        }
        let Some(first) = counted.first() else {
            return Err(reason);
        };
        let parameter = |scalar: &NamedScalar| scalar.stated.and_then(Statement::parameter);
        if counted.iter().any(|scalar| {
            !parameter(scalar)
                .zip(parameter(first))
                .is_some_and(|(stated, first)| same_parameter(stated, first))
        }) {
            return Err(Unvalued::Disagrees);
        }
        let (kind, value) = parameter(first)
            .and_then(typed)
            .ok_or(Unvalued::Disagrees)?;
        Ok(Typed {
            kind,
            value: Some(value),
            assumed: counted
                .iter()
                .all(|scalar| matches!(scalar.stated, Some(Statement::Assumed(_)))),
        })
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
        use cadmpeg_ir::features::parameters::ParameterValue as V10;
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

    /// Whether the decoder's parameter of id `id` named `name` is bound to a
    /// PMI record, which never yields to the equation manager's value
    /// (`docs/formats/sldprt.md`).
    fn pmi_bound(&self, id: &ParameterId, name: &str) -> bool {
        self.parameter(id, name)
            .is_some_and(|parameter| parameter.pmi.is_some())
    }

    /// The decoder's parameter of id `id` named `name`.
    fn parameter(
        &self,
        id: &ParameterId,
        name: &str,
    ) -> Option<&'a cadmpeg_ir::features::parameters::DesignParameter> {
        self.parameters
            .get(id.as_str())
            .copied()
            .filter(|parameter| parameter.name == name)
    }
}

/// A dimension's kind and cache as the decoder's readings type it, and
/// whether its kind is the class a compound-file lane's own declarations
/// alone give its scalar's class token ([`Statement::Assumed`]).
struct Typed {
    kind: ValueKind,
    value: Option<ParameterValue>,
    assumed: bool,
}

/// Whether a stored record gave the value of the decoder's `parameter`: a
/// bound scalar, or a value that is no reading of the dimension's bare
/// display `display`.
fn stored(
    parameter: &cadmpeg_ir::features::parameters::DesignParameter,
    display: Option<f64>,
) -> bool {
    parameter.native_ref.is_some()
        || display.is_some_and(|display| {
            parameter
                .value
                .as_ref()
                .is_some_and(|value| !reads_display(value, display))
        })
}

/// A bare display: the number Keywords shows, and half a unit in its last
/// decimal place, the most the shown number can differ from the value.
#[derive(Debug, Clone, Copy)]
struct Shown {
    number: f64,
    rounding: f64,
}

impl Shown {
    /// The display `text` shows where it is a bare number.
    fn of(text: &str) -> Option<Self> {
        let body = display_body(text);
        let number = body
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())?;
        let rounding = if body.contains(['e', 'E']) {
            0.0
        } else {
            let places = body.split_once('.').map_or(0, |(_, places)| places.len());
            0.5 * 10f64.powi(-i32::try_from(places).ok()?)
        };
        Some(Self { number, rounding })
    }

    /// This display as a length the part shows at `places` decimal places
    /// (`docs/formats/sldprt.md`): the text is the value rounded to those
    /// places, with trailing zeros dropped or kept, so the value is within
    /// half a unit in the last of those places, or in the display's own last
    /// place where that is finer. `None` keeps the display's own place.
    fn at_places(self, places: Option<u32>) -> Self {
        let Some(places) = places else {
            return self;
        };
        let rounding = i32::try_from(places).map_or(0.0, |places| 0.5 * 10f64.powi(-places));
        Self {
            rounding: self.rounding.min(rounding),
            ..self
        }
    }

    /// Whether `value` is a number this display shows.
    fn shows(self, value: f64) -> bool {
        (value - self.number).abs()
            <= self.rounding + EPS_STORED_DISPLAY * value.abs().max(self.number.abs())
    }
}

/// A bare number Keywords shows for a length, and the millimetres in one of
/// the unit it is in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DisplayedLength {
    pub(crate) number: f64,
    pub(crate) mm_per_unit: f64,
}

/// Millimetres per unit of the units a length display can be in where the
/// document states none: millimetre, inch and metre.
const UNSTATED_UNIT_MM: [f64; 3] = [1.0, 25.4, 1000.0];

/// The unit and number of the bare display `text` of a length whose
/// `Linear` or `Ordinate` record states `metres` (`docs/formats/sldprt.md`).
/// Where the document's `unit` is known, the record's value in that unit,
/// rounded to the places the text shows (and no coarser than the part's
/// `places`), is the number. Where it is unknown, exactly one of the
/// millimetre, inch and metre readings is. `None` where the text is not a
/// nonzero bare number without an exponent, the record is zero, or the
/// readings do not decide one unit.
pub(crate) fn displayed_length(
    text: &str,
    metres: f64,
    unit: Option<UnitSymbol>,
    places: Option<u32>,
) -> Option<DisplayedLength> {
    let text = text.trim();
    if text.contains(['e', 'E']) || display_body(text) != text {
        return None;
    }
    let shown = Shown::of(text)?.at_places(places);
    if shown.number == 0.0 || metres == 0.0 {
        return None;
    }
    let millimetres = metres * MM_PER_M;
    // The exact form: the number is the record's metres, or its value in the
    // document's unit (in any of the three units where that is not read).
    let document_mm = match unit {
        Some(unit) => match canonical(unit)? {
            (Quantity::LENGTH, factor) => Some(factor),
            _ => return None,
        },
        None => None,
    };
    let exact_units = match document_mm {
        Some(factor) => vec![MM_PER_M, factor],
        None => UNSTATED_UNIT_MM.to_vec(),
    };
    if let Some(mm_per_unit) = exact_units
        .into_iter()
        .find(|mm_per_unit| same_stored(shown.number * mm_per_unit, millimetres))
    {
        return Some(DisplayedLength {
            number: shown.number,
            mm_per_unit,
        });
    }
    let reads = |mm_per_unit: f64| shown.shows(millimetres / mm_per_unit);
    let mm_per_unit = match document_mm {
        Some(factor) => reads(factor).then_some(factor)?,
        None => {
            let mut readings = UNSTATED_UNIT_MM.into_iter().filter(|factor| reads(*factor));
            let only = readings.next()?;
            readings.next().is_none().then_some(only)?
        }
    };
    Some(DisplayedLength {
        number: shown.number,
        mm_per_unit,
    })
}

/// Whether two stored values are one number within [`EPS_STORED_DISPLAY`],
/// relative to the larger.
fn same_stored(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS_STORED_DISPLAY * a.abs().max(b.abs())
}

/// Whether the records `a` and `b` state one parameter: one class, and one
/// count or real values that are one number (see [`same_stored`]).
fn same_parameter(a: StatedParameter, b: StatedParameter) -> bool {
    match (a, b) {
        (StatedParameter::Length(a), StatedParameter::Length(b))
        | (StatedParameter::Angle(a), StatedParameter::Angle(b))
        | (StatedParameter::Scaler(a), StatedParameter::Scaler(b)) => same_stored(a, b),
        (StatedParameter::Count(a), StatedParameter::Count(b)) => a == b,
        _ => false,
    }
}

/// Whether the value of `stated` is what `display` shows: a length in the
/// part's length unit `unit` at its length precision `places`, an angle in
/// degrees, a count or a real as itself. `None` for a length in a part whose
/// length unit is not read.
fn agrees(
    stated: StatedParameter,
    display: Shown,
    unit: Option<UnitSymbol>,
    places: Option<u32>,
) -> Option<bool> {
    let (shown, display) = match stated {
        StatedParameter::Length(metres) => (
            metres * MM_PER_M / canonical(unit?)?.1,
            display.at_places(places),
        ),
        StatedParameter::Angle(radians) => (radians.to_degrees(), display),
        StatedParameter::Count(count) => (f64::from(count), display),
        StatedParameter::Scaler(real) => (real, display),
    };
    Some(display.shows(shown))
}

/// The kind and canonical value of the parameter `stated`.
fn typed(stated: StatedParameter) -> Option<(ValueKind, ParameterValue)> {
    let number = |quantity, value: f64| {
        Some((
            ValueKind::Quantity(quantity),
            ParameterValue::Number(FiniteReal::new(value)?),
        ))
    };
    match stated {
        StatedParameter::Length(metres) => number(Quantity::LENGTH, metres * MM_PER_M),
        StatedParameter::Angle(radians) => number(Quantity::ANGLE, radians),
        StatedParameter::Scaler(real) => number(Quantity::REAL, real),
        StatedParameter::Count(count) => Some((
            ValueKind::Integer,
            ParameterValue::Integer(i64::from(count)),
        )),
    }
}

/// Whether the decoder's `value` is the display's number read in a unit the
/// display does not state: as millimetres, metres, or radians, or as itself.
fn reads_display(value: &cadmpeg_ir::features::parameters::ParameterValue, display: f64) -> bool {
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
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
    value: cadmpeg_ir::features::parameters::ParameterValue,
) -> Option<ParameterValue> {
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
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

/// The number of a dimension display that states no unit (see
/// [`display_body`]): the number Keywords shows in the document's unit
/// (`docs/formats/sldprt.md`).
pub(super) fn bare_display_number(text: &str) -> Option<f64> {
    display_body(text)
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

/// The number text of a dimension display, after its count, its diameter or
/// radius modifier, and a parenthesized comment.
fn display_body(text: &str) -> &str {
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
    body
}

/// One parameter of the source, before its expression is read.
struct Entry<'a> {
    feature: &'a Feature,
    owner: Option<FeatureId>,
    name: NonBlankString,
    id: ParameterId,
    text: &'a str,
    /// The history names no such dimension: the decoder's own readings of
    /// the feature's records hold it, and `text` is the decoder's rendering.
    added: bool,
}

/// What the decoder read beside a parameter's text: its own parameters and
/// scalar classes, the definition's length unit, and the quantity of each
/// dimension that binds a parameter.
pub(super) struct Readings<'a> {
    pub(super) decoded: Decoded<'a>,
    pub(super) length_unit: Option<UnitSymbol>,
    /// The number of decimal places at which the part shows lengths, where
    /// its units tables state one (the largest, where they differ).
    pub(super) length_places: Option<u32>,
    pub(super) bound: HashMap<ParameterId, Quantity>,
    /// The parameters a radius or diameter dimension binds, by what it
    /// measures: such a parameter states no display of that measure.
    pub(super) measured: HashMap<ParameterId, DimensionDisplay>,
    /// The parameters a reference dimension binds.
    pub(super) driven: HashSet<ParameterId>,
    /// The value the active configuration's equation manager stores beside
    /// each dimension its lines name: metres, radians, or a count.
    pub(super) manager: HashMap<ParameterId, f64>,
    /// The dimensions whose stored value the line that drives them gives,
    /// evaluated over the values stored beside the names it reads.
    pub(super) confirmed: HashSet<ParameterId>,
}

/// The parameters of the timeline's features and of the equations folders,
/// before their expressions are read.
pub(super) struct Entries<'a> {
    list: Vec<Entry<'a>>,
    /// The decoder's parameters of the features outside the timeline that
    /// are no equations folder.
    outside: Vec<ParameterId>,
    by_owner: HashMap<(FeatureId, String), ParameterId>,
    ids: HashSet<ParameterId>,
}

impl<'a> Entries<'a> {
    /// The parameters of `parameter_histories`, the copy of `histories`
    /// whose features also hold the dimensions the decoder read from other
    /// records.
    pub(super) fn new(
        histories: &'a [FeatureHistory],
        parameter_histories: &'a [FeatureHistory],
        timeline: &Timeline,
    ) -> Result<Self, CodecError> {
        let (entries, outside) = entries(histories, parameter_histories, timeline)?;
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
            outside,
            by_owner,
            ids,
        })
    }

    /// The note naming the decoder's parameters, of `decoded`, that a
    /// feature-tree node outside the timeline owns: §5.12 gives such an
    /// owner no home.
    pub(super) fn outside(
        &self,
        decoded: &[cadmpeg_ir::features::parameters::DesignParameter],
    ) -> Option<LossNote> {
        let decoded = decoded
            .iter()
            .map(|parameter| &parameter.id)
            .collect::<HashSet<_>>();
        let mut named = NamedCount::default();
        for id in self.outside.iter().filter(|id| decoded.contains(id)) {
            named.push(id.as_str());
        }
        (!named.is_empty()).then(|| {
            SldprtLossCode::DesignParameterOwnerOutsideTimeline.note(format!(
                "{} model parameters whose owner is a feature-tree node outside the timeline \
                 state no design parameter: {named}",
                named.count()
            ))
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
    /// literal is a rounded display, the note naming the dimensions whose
    /// text is a bare number and that have no value, and the parameters whose
    /// text is one reference to a parameter whose kind differs or is not read,
    /// kept as text for `equations::bare_references`.
    pub(super) fn parameters(
        &self,
        scope: &ProductDefinitionId,
        readings: &Readings<'_>,
    ) -> (Vec<DesignParameter>, Vec<LossNote>, Vec<ParameterId>) {
        let aliases = Aliases::new(&self.list);
        let mut read_entries = self
            .list
            .iter()
            .map(|entry| read(entry, &aliases, readings, None))
            .collect::<Vec<_>>();
        if let Some(places) = readings
            .length_places
            .filter(|_| !readings.confirmed.is_empty())
        {
            for (entry, reading) in self.list.iter().zip(&mut read_entries) {
                if readings.confirmed.contains(&entry.id) {
                    *reading = read(entry, &aliases, readings, Some(places));
                }
            }
        }
        let as_text = link_bare_references(&self.list, &mut read_entries, &readings.driven);
        let mut rounded = NamedCount::default();
        let mut assumed = NamedCount::default();
        let mut decided = NamedCount::default();
        let mut unvalued = Unvalued::ALL.map(|reason| (reason, NamedCount::default()));
        let parameters = self
            .list
            .iter()
            .zip(read_entries)
            .map(|(entry, reading)| {
                if reading.rounded {
                    rounded.push(entry.id.as_str());
                }
                if reading.assumed {
                    assumed.push(entry.id.as_str());
                }
                if reading.decided {
                    decided.push(entry.id.as_str());
                }
                if let Some((_, named)) = unvalued
                    .iter_mut()
                    .find(|(reason, _)| Some(*reason) == reading.unvalued)
                {
                    named.push(entry.id.as_str());
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
        if !assumed.is_empty() {
            losses.push(SldprtLossCode::DesignParameterClassAssumed.note(format!(
                "{} dimensions take their kind from the parameter class that a compound-file \
                 lane's own declarations alone give their scalar's class token. The stream does \
                 not state the index at which the Config-N class map ends, so the decoder reads \
                 the token as the lane's own: {assumed}",
                assumed.count()
            )));
        }
        if !decided.is_empty() {
            losses.push(SldprtLossCode::DesignParameterClassesDisagree.note(format!(
                "{} dimensions whose scalars' parameter classes the decoder's class sources name \
                 differently take the length the equation manager stores beside them for the line \
                 that names them, where their display shows that length in the part's length unit: \
                 {decided}",
                decided.count()
            )));
        }
        let unvalued = unvalued
            .into_iter()
            .filter(|(_, named)| !named.is_empty())
            .map(|(reason, named)| format!("{} {} ({named})", named.count(), reason.why()))
            .collect::<Vec<_>>();
        if !unvalued.is_empty() {
            losses.push(SldprtLossCode::DesignParameterValueUnread.note(format!(
                "Parameters whose dimension text is a bare number hold no value: {}",
                unvalued.join("; ")
            )));
        }
        (parameters, losses, as_text)
    }
}

/// The entries of the features of `parameter_histories`, each id the
/// decoder's own (its ordinal among the enriched feature's names), and the
/// ids of the parameters of the features outside the timeline that are no
/// equations folder. A dimension the history names keeps the history's
/// feature and text.
fn entries<'a>(
    histories: &'a [FeatureHistory],
    parameter_histories: &'a [FeatureHistory],
    timeline: &Timeline,
) -> Result<(Vec<Entry<'a>>, Vec<ParameterId>), CodecError> {
    let stored = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let mut entries = Vec::new();
    let mut outside = Vec::new();
    let mut user_names = HashSet::new();
    for enriched in parameter_histories
        .iter()
        .flat_map(|history| &history.features)
    {
        let owner = match timeline.features_by_record.get(&enriched.id) {
            Some(owner) => Some(owner.clone()),
            None if timeline.equation_records.contains(&enriched.id) => None,
            None => {
                for (ordinal, name) in projected_parameter_names(enriched).into_iter().enumerate() {
                    if enriched.parameters.contains_key(name.as_str()) {
                        outside.push(neutral_parameter_id(enriched, ordinal)?);
                    }
                }
                continue;
            }
        };
        let feature = stored
            .get(enriched.id.as_str())
            .copied()
            .unwrap_or(enriched);
        for (ordinal, name) in projected_parameter_names(enriched).into_iter().enumerate() {
            let (text, added) = match feature.parameters.get(name.as_str()) {
                Some(text) => (text, false),
                None => match enriched.parameters.get(name.as_str()) {
                    Some(text) => (text, true),
                    None => continue,
                },
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
                id: neutral_parameter_id(enriched, ordinal)?,
                text,
                added,
            });
        }
    }
    Ok((entries, outside))
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
    /// The kind is a class a compound-file lane's own declarations alone
    /// give the scalar's class token.
    assumed: bool,
    /// The decoder's class sources name different classes for the
    /// dimension's scalars, and the length the equation manager stores
    /// beside it gave its value.
    decided: bool,
    /// Why a dimension whose text is a bare number has no value.
    unvalued: Option<Unvalued>,
}

/// Why a dimension whose text is a bare number has no value. Of the reasons
/// the scalars under its name give, a later variant is the more specific.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Unvalued {
    /// No record types the dimension, or binds it to a stored value of its
    /// kind.
    Kind,
    /// Its feature holds no scalar under its name.
    NoScalar,
    /// No record of a scalar under its name states a parameter class the
    /// decoder reads.
    ClassUnread,
    /// The decoder's sources of the parameter class of its bound scalar, or
    /// of a scalar under its name, name different classes: the sources of a
    /// class token, or a bound scalar's archive class and the class its record
    /// states.
    ClassesDisagree,
    /// The dimension is a length and the part's length unit is not read.
    LengthUnit,
    /// The stored values of the scalars under its name differ from its
    /// display or from each other.
    Disagrees,
    /// The line that drives it gives the length the equation manager stores,
    /// and its display rounds that length at fewer decimal places than the
    /// document's length precision.
    Coarse,
}

impl Unvalued {
    const ALL: [Self; 7] = [
        Self::Kind,
        Self::NoScalar,
        Self::ClassUnread,
        Self::ClassesDisagree,
        Self::LengthUnit,
        Self::Disagrees,
        Self::Coarse,
    ];

    /// The note's words for the dimensions with this reason.
    fn why(self) -> &'static str {
        match self {
            Self::Kind => "whose dimension no record types or binds to a stored value of its kind",
            Self::NoScalar => "whose feature holds no scalar under the dimension's name",
            Self::ClassUnread => {
                "whose scalars' records state no parameter class the decoder reads"
            }
            Self::ClassesDisagree => {
                "whose scalars' parameter classes the decoder's class sources name differently"
            }
            Self::LengthUnit => "lengths in a part whose length unit is not read",
            Self::Disagrees => "whose stored values differ from the display or from each other",
            Self::Coarse => {
                "whose display rounds the length their equation gives at fewer decimal places than \
                 the document's length precision"
            }
        }
    }
}

/// The reading of `entry`. Keywords shows a length rounded to the document's
/// length precision `places`, the number of decimal places its units table
/// states (`docs/formats/sldprt.md`). A dimension in `readings.confirmed`
/// takes its stored length where its display is that length at `places`;
/// `None` leaves every dimension unrounded.
fn read(
    entry: &Entry<'_>,
    aliases: &Aliases,
    readings: &Readings<'_>,
    places: Option<u32>,
) -> Reading {
    if entry.added {
        return added(entry, readings);
    }
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
            assumed: false,
            decided: false,
            unvalued: None,
        };
    }
    let tokens = source_tokens(text, entry.owner.as_ref(), aliases);
    let references = tokens
        .iter()
        .any(|token| matches!(token, ExpressionToken::Reference(_)));
    let shown = Shown::of(text);
    let bare = shown.map(|shown| shown.number);
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
            assumed: false,
            decided: false,
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
            cadmpeg_ir::features::parameters::ParameterValue::Integer(_)
                | cadmpeg_ir::features::parameters::ParameterValue::Real(_)
        )
    });
    let bound = readings.bound.get(&entry.id).copied();
    let managed = readings
        .manager
        .get(&entry.id)
        .filter(|_| !decoded.pmi_bound(&entry.id, name));
    let mut unread = None;
    let mut assumed = false;
    let mut decided = false;
    let (kind, value) = match parsed.and_then(cache) {
        Some((kind, value)) => (kind, Some(value)),
        None if user => (ValueKind::Quantity(Quantity::REAL), None),
        None => match decoded
            .dimension(
                &entry.id,
                name,
                shown,
                readings.length_unit,
                readings.length_places,
            )
            .transpose()
            .or_else(|| {
                shown.filter(|_| !references).map(|shown| {
                    decoded.named_dimension(
                        &feature.id,
                        name,
                        shown,
                        readings.length_unit,
                        readings.length_places,
                    )
                })
            }) {
            Some(Ok(reading)) => {
                assumed = reading.assumed;
                (reading.kind, reading.value)
            }
            unvalued => {
                unread = unvalued.and_then(Result::err);
                let manager = shown
                    .filter(|_| !references && bound.is_none_or(|q| q == Quantity::LENGTH))
                    .zip(managed)
                    .and_then(|(shown, stored)| {
                        manager_length(shown.number, *stored, readings.length_unit)
                    });
                if let Some(value) = manager {
                    decided = unread == Some(Unvalued::ClassesDisagree);
                    (ValueKind::Quantity(Quantity::LENGTH), Some(value))
                } else if native_parameter_is_length(feature, name, Some(text)) {
                    (ValueKind::Quantity(Quantity::LENGTH), None)
                } else if let Some(quantity) = bound {
                    (ValueKind::Quantity(quantity), None)
                } else {
                    (
                        ValueKind::Native(cadmpeg_core::nonblank_const!(UNREAD_KIND)),
                        decoded.native_cache(&entry.id, name, bare),
                    )
                }
            }
        },
    };
    let (kind, value) = match (kind, value, places) {
        (
            kind @ (ValueKind::Quantity(Quantity::LENGTH) | ValueKind::Native(_)),
            None,
            Some(places),
        ) if !user && !references && readings.confirmed.contains(&entry.id) => {
            let rounded = |places| {
                shown.zip(managed).and_then(|(shown, stored)| {
                    manager_rounded(shown, *stored, readings.length_unit, places)
                })
            };
            match rounded(Some(places)) {
                Some(value) => {
                    decided = unread == Some(Unvalued::ClassesDisagree);
                    (ValueKind::Quantity(Quantity::LENGTH), Some(value))
                }
                None => {
                    if rounded(None).is_some() {
                        unread = Some(Unvalued::Coarse);
                    }
                    (kind, None)
                }
            }
        }
        (kind, value, _) => (kind, value),
    };
    let display = display
        .filter(|_| kind == ValueKind::Quantity(Quantity::LENGTH))
        .map(|display| match display {
            cadmpeg_ir::features::parameters::DimensionDisplay::Diameter => {
                DimensionDisplay::Diameter
            }
            cadmpeg_ir::features::parameters::DimensionDisplay::Radius => DimensionDisplay::Radius,
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
                assumed,
                decided,
                unvalued: None,
            };
        }
    }
    let unvalued = (number.is_some() && value.is_none()).then_some(
        if kind == ValueKind::Quantity(Quantity::LENGTH) && readings.length_unit.is_none() {
            Unvalued::LengthUnit
        } else {
            unread.unwrap_or(Unvalued::Kind)
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
        assumed,
        decided,
        unvalued,
    }
}

/// The reading of a dimension that only the decoder's own readings hold:
/// the kind and value of the decoder's parameter of the same id and name, a
/// real typed by the archive class of the scalar it is bound to, as a
/// literal (§5.12). A value whose kind no record states keeps its number
/// under a `Native` kind, and a parameter with no value states none, each
/// with a `Source` expression from which nothing was read (§3.3).
fn added(entry: &Entry<'_>, readings: &Readings<'_>) -> Reading {
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    let name = entry.name.as_str();
    let decoded = readings.decoded.parameter(&entry.id, name);
    let mut assumed = false;
    let typed = decoded
        .and_then(|parameter| parameter.value.clone())
        .and_then(|value| match value {
            V10::Real(real) => {
                match readings
                    .decoded
                    .dimension(&entry.id, name, None, readings.length_unit, None)
                {
                    Ok(Some(Typed {
                        kind,
                        value: Some(value),
                        assumed: from_lane,
                    })) => {
                        assumed = from_lane;
                        Some((kind, value))
                    }
                    _ => Some((
                        ValueKind::Native(cadmpeg_core::nonblank_const!(UNREAD_KIND)),
                        ParameterValue::Number(FiniteReal::new(real.get())?),
                    )),
                }
            }
            V10::Integer(integer) => Some((ValueKind::Integer, ParameterValue::Integer(integer))),
            value => cache(value),
        });
    let unread = || Expression::Source {
        dialect: ExpressionDialect::SolidWorks,
        tokens: Vec::new(),
    };
    let (kind, expression, value) = match typed {
        Some((kind, value)) => {
            let expression = stored_literal(&kind, &value, readings.length_unit)
                .map_or_else(unread, |node| Expression::Neutral(Neutral(node)));
            (kind, expression, Some(value))
        }
        None => (
            ValueKind::Native(cadmpeg_core::nonblank_const!(UNREAD_KIND)),
            unread(),
            None,
        ),
    };
    let display = decoded
        .and_then(|parameter| parameter.display)
        .filter(|_| kind == ValueKind::Quantity(Quantity::LENGTH))
        .map(|display| match display {
            cadmpeg_ir::features::parameters::DimensionDisplay::Diameter => {
                DimensionDisplay::Diameter
            }
            cadmpeg_ir::features::parameters::DimensionDisplay::Radius => DimensionDisplay::Radius,
        })
        .filter(|display| readings.measured.get(&entry.id) != Some(display));
    Reading {
        kind,
        expression,
        value,
        display,
        rounded: false,
        assumed,
        decided: false,
        unvalued: None,
    }
}

/// The literal of the canonical `value` of a parameter of kind `kind` that
/// a record stores with no text: a length in the definition's length unit
/// `length_unit`, else in metres, the unit the records store it in; an angle
/// in degrees; a real or a count with no unit; a flag or a text as itself.
/// Its number is the value in that unit to 15 significant digits (§5.12).
fn stored_literal(
    kind: &ValueKind,
    value: &ParameterValue,
    length_unit: Option<UnitSymbol>,
) -> Option<Node<ParameterRef>> {
    match (kind, value) {
        (ValueKind::Quantity(quantity), ParameterValue::Number(number)) => {
            let symbol = match *quantity {
                Quantity::LENGTH => Some(length_unit.unwrap_or(UnitSymbol::M)),
                Quantity::ANGLE => Some(UnitSymbol::Deg),
                Quantity::REAL => None,
                _ => return None,
            };
            let scale = match symbol {
                Some(symbol) => canonical(symbol)?.1,
                None => 1.0,
            };
            number_node(significant(number.get() / scale)?, symbol)
        }
        (ValueKind::Integer, ParameterValue::Integer(integer)) => {
            number_node(exact_integer_f64(*integer)?, None)
        }
        (ValueKind::Boolean, ParameterValue::Boolean(flag)) => Some(Node::Boolean(*flag)),
        (ValueKind::Text, ParameterValue::Text(text)) => Some(Node::Text(text.clone())),
        _ => None,
    }
}

/// Rewrites each `Source` expression whose text is one reference as that
/// reference where the target has the parameter's kind and that kind is
/// neutral (§5.13), and returns the parameters whose reference is kept as
/// text. A parameter that `driven` holds states no expression.
///
/// Where the kinds differ or are `Native`, the expression is one `Text`
/// token, the whole text, until `equations::bare_references` tests the
/// text's unit-blind tree (§5.13). Interim: part 10 refuses a `Source`
/// expression that is the reference token alone.
fn link_bare_references(
    entries: &[Entry<'_>],
    readings: &mut [Reading],
    driven: &HashSet<ParameterId>,
) -> Vec<ParameterId> {
    let kinds = entries
        .iter()
        .zip(readings.iter())
        .map(|(entry, reading)| (entry.id.clone(), reading.kind.clone()))
        .collect::<HashMap<_, _>>();
    let mut as_text = Vec::new();
    for (entry, reading) in entries.iter().zip(readings.iter_mut()) {
        if driven.contains(&entry.id) {
            continue;
        }
        let Some(reference) = bare_reference(&reading.expression).cloned() else {
            continue;
        };
        let linked = match &reference.target {
            RefTarget::Parameter(target) => kinds
                .get(target)
                .is_some_and(|kind| *kind == reading.kind && !matches!(kind, ValueKind::Native(_))),
            // §5.13: a whole-text external reference is the parameter's own
            // expression, of the parameter's kind.
            RefTarget::External { .. } => true,
        };
        reading.expression = if linked {
            Expression::Neutral(Neutral(Node::Reference(reference)))
        } else {
            as_text.push(entry.id.clone());
            Expression::Source {
                dialect: ExpressionDialect::SolidWorks,
                tokens: vec![ExpressionToken::Text(entry.text.to_owned())],
            }
        };
    }
    as_text
}

/// The reference of a `Source` expression that is one reference token.
fn bare_reference(expression: &Expression) -> Option<&ParameterRef> {
    let Expression::Source { tokens, .. } = expression else {
        return None;
    };
    let [ExpressionToken::Reference(reference)] = tokens.as_slice() else {
        return None;
    };
    Some(reference)
}

/// The length the equation manager's `stored` metres give a dimension whose
/// text is the bare display `number` and that no record types: the stored
/// value, where `number` in the part's length unit `unit` is that many
/// metres and is not the stored number itself, which a count, a ratio, or
/// radians store (`docs/formats/sldprt.md`).
fn manager_length(number: f64, stored: f64, unit: Option<UnitSymbol>) -> Option<ParameterValue> {
    let metres = number * canonical(unit?)?.1 / MM_PER_M;
    (number != 0.0 && same_stored(metres, stored) && !same_stored(number, stored))
        .then(|| FiniteReal::new(stored * MM_PER_M).map(ParameterValue::Number))
        .flatten()
}

/// The length the equation manager's `stored` metres give a dimension whose
/// bare display `shown` rounds them, where the line that drives the dimension
/// gives that value: the stored value, where `shown` is the stored value in
/// exactly one of millimetres and inches, the part's length unit `unit`,
/// within half a unit in its own last decimal place or in the last of
/// `places`, whichever is finer, and the text at its own places shows neither
/// the stored number itself nor the stored number in degrees
/// (`docs/formats/sldprt.md`).
fn manager_rounded(
    shown: Shown,
    stored: f64,
    unit: Option<UnitSymbol>,
    places: Option<u32>,
) -> Option<ParameterValue> {
    let allowed = shown.at_places(places);
    let shows = |symbol| Some(allowed.shows(stored * MM_PER_M / canonical(symbol)?.1));
    let (millimetres, inches) = (shows(UnitSymbol::Mm)?, shows(UnitSymbol::In)?);
    let in_unit = match unit? {
        UnitSymbol::Mm => millimetres && !inches,
        UnitSymbol::In => inches && !millimetres,
        _ => false,
    };
    let other_reading = shown.shows(stored) || shown.shows(stored.to_degrees());
    (shown.number != 0.0 && in_unit && !other_reading)
        .then(|| FiniteReal::new(stored * MM_PER_M).map(ParameterValue::Number))
        .flatten()
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
pub(super) fn number_node(signed: f64, symbol: Option<UnitSymbol>) -> Option<Node<ParameterRef>> {
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
fn cache(
    value: cadmpeg_ir::features::parameters::ParameterValue,
) -> Option<(ValueKind, ParameterValue)> {
    use cadmpeg_ir::features::parameters::ParameterValue as Parsed;
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
pub(crate) const LITERAL_UNITS: [(&str, UnitSymbol); 11] = [
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
pub(super) fn canonical(symbol: UnitSymbol) -> Option<(Quantity, f64)> {
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
