// SPDX-License-Identifier: Apache-2.0
//! The parameter each named scalar's own record states (`docs/formats/sldprt.md`
//! §2): the class of its parameter object, written immediately before its
//! name record as a class declaration or a class token, and its value read in
//! that class's cell.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::View;

use super::{is_class_token, COMPACT_SCALAR_HEADER, VALUE_ONLY_SCALAR_HEADER};
use crate::archive::resolved_count;
use crate::layout::feature_input_scalar_class_prefix as prefix;
use crate::records::{
    ArchiveParameterClass, FeatureInputClass, FeatureInputLane, FeatureInputScalar,
    FeatureInputScalarRole,
};

/// The bytes of a class declaration before its name: the `ff ff 01 00`
/// marker and the u16 name length.
const DECLARATION_PREFIX: u64 = 6;

/// The bytes of a count's i32 cell.
const COUNT_LEN: usize = 4;

/// The offset of the scalar object identifier in a scalar's trailer, after
/// three zero bytes.
const TRAILER_OBJECT_ID: usize = 3;

/// The bound below which a count trailer's scalar object identifier lies.
const COUNT_OBJECT_ID_BOUND: u32 = 1 << 24;

/// A parameter a scalar's record states: its class, and its value in that
/// class's unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StatedParameter {
    /// `moLengthParameter_c`: metres.
    Length(f64),
    /// `moAngleParameter_c`: radians.
    Angle(f64),
    /// `moIntegerParameter_c`: a count, from its four-byte cell.
    Count(i32),
    /// `moScalerParameter_c`: a plain real.
    Scaler(f64),
}

impl StatedParameter {
    /// The parameter's class.
    pub(crate) fn class(self) -> ArchiveParameterClass {
        match self {
            Self::Length(_) => ArchiveParameterClass::Length,
            Self::Angle(_) => ArchiveParameterClass::Angle,
            Self::Count(_) => ArchiveParameterClass::Integer,
            Self::Scaler(_) => ArchiveParameterClass::Scaler,
        }
    }
}

/// The `Config-N` class map a `ResolvedFeatures` lane's class tokens continue
/// (`docs/formats/sldprt.md` §2.2). The lane's own declarations take the
/// indices from the end of that map on, and a token below it names the class
/// `Config-N` holds at that index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigurationMap {
    /// A stream that states the map count `end`. `parameters` holds the
    /// parameter class at each index of the `Config-N` map that holds one,
    /// when a walk of `Config-N` reconciles with `end`.
    Counted {
        end: u32,
        parameters: Option<BTreeMap<u16, ArchiveParameterClass>>,
    },
    /// A compound-file stream, which states no count: the indices at which
    /// a walk of `Config-N` places a class.
    Unstated { classes: BTreeSet<u32> },
}

impl ConfigurationMap {
    /// The map of a stream that states the count `end`, with the `Config-N`
    /// class at each index from `classes`, a walk that reconciles with `end`.
    pub(crate) fn counted(end: u32, classes: Option<&BTreeMap<u32, String>>) -> Self {
        let parameters = classes.map(|classes| {
            classes
                .iter()
                .filter(|&(&index, _)| index < end)
                .filter_map(|(&index, name)| {
                    Some((
                        u16::try_from(index).ok()?,
                        ArchiveParameterClass::from_class_name(name)?,
                    ))
                })
                .collect()
        });
        Self::Counted { end, parameters }
    }

    /// Whether `index` names a class `Config-N` holds rather than one the
    /// lane declares.
    fn holds(&self, index: u16) -> bool {
        match self {
            Self::Counted { end, .. } => u32::from(index) < *end,
            Self::Unstated { classes } => classes.contains(&u32::from(index)),
        }
    }

    /// The parameter class at each index of the `Config-N` map, when a walk
    /// reconciles with the stated count.
    fn parameters(&self) -> Option<&BTreeMap<u16, ArchiveParameterClass>> {
        match self {
            Self::Counted { parameters, .. } => parameters.as_ref(),
            Self::Unstated { .. } => None,
        }
    }
}

/// What a scalar's parameter-class slot holds.
#[derive(Debug, Clone, Copy)]
enum Slot<'a> {
    Declared(&'a str),
    Token(u16),
}

/// The parameter each scalar of `lane` states, by scalar id. A scalar whose
/// slot holds a class token states the class `configuration`, the
/// `Config-N` map the lane continues, or the lane's declarations give that
/// token (see [`token_classes`]); a scalar whose class is not a parameter
/// class, or whose token neither answers, states none.
pub(crate) fn stated_parameters<'l>(
    lane: &'l FeatureInputLane,
    configuration: Option<&ConfigurationMap>,
) -> HashMap<&'l str, StatedParameter> {
    let payload = lane.native_payload.as_slice();
    let name_offsets = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.offset))
        .collect::<HashMap<_, _>>();
    let declarations = lane
        .classes
        .iter()
        .filter_map(|class| {
            let length = u64::try_from(class.name.len()).ok()?;
            Some((
                class
                    .offset
                    .checked_add(DECLARATION_PREFIX)?
                    .checked_add(length)?,
                class.name.as_str(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let slots = lane
        .scalars
        .iter()
        .filter_map(|scalar| {
            let name = *name_offsets.get(scalar.name.as_str())?;
            Some((scalar, name, slot(payload, &declarations, name)?))
        })
        .collect::<Vec<_>>();
    let tokens = token_classes(
        configuration,
        resolved_count(payload),
        &lane.classes,
        &slots,
    );
    slots
        .into_iter()
        .filter_map(|(scalar, _, slot)| {
            let class = match slot {
                Slot::Declared(name) => ArchiveParameterClass::from_class_name(name)?,
                Slot::Token(index) => *tokens.get(&index)?,
            };
            Some((scalar.id.as_str(), stated(payload, scalar, class)?))
        })
        .collect()
}

/// The parameter-class slot before the name record at `name`: a declaration
/// that ends there, else the class token in the slot.
fn slot<'a>(payload: &[u8], declarations: &HashMap<u64, &'a str>, name: u64) -> Option<Slot<'a>> {
    if let Some(declared) = declarations.get(&name) {
        return Some(Slot::Declared(declared));
    }
    let start = usize::try_from(name)
        .ok()?
        .checked_sub(prefix::LEN - prefix::PARAMETER_CLASS_TOKEN)?;
    let token = View::u16_le_at(payload, start)?;
    is_class_token(token).then_some(Slot::Token(token & 0x7fff))
}

/// The value of `scalar` in the cell of `class`.
fn stated(
    payload: &[u8],
    scalar: &FeatureInputScalar,
    class: ArchiveParameterClass,
) -> Option<StatedParameter> {
    let value = scalar.value.get();
    Some(match class {
        ArchiveParameterClass::Length => StatedParameter::Length(value),
        ArchiveParameterClass::Angle => StatedParameter::Angle(value),
        ArchiveParameterClass::Scaler => StatedParameter::Scaler(value),
        ArchiveParameterClass::Integer => StatedParameter::Count(View::i32_le_at(
            payload,
            count_cell(payload, usize::try_from(scalar.offset).ok()?)?,
        )?),
    })
}

/// The offset of the i32 count of an `moIntegerParameter_c` scalar framed at
/// `value`, the end of a 14-byte value-only header. Behind the 18-byte
/// compact header, whose last four bytes are zero, the count follows those
/// four bytes; each count's trailer begins after the count.
fn count_cell(payload: &[u8], value: usize) -> Option<usize> {
    let header = value.checked_sub(VALUE_ONLY_SCALAR_HEADER.len())?;
    let compact = header.checked_add(COMPACT_SCALAR_HEADER.len())?;
    let behind_compact_header = payload.get(header..compact) == Some(COMPACT_SCALAR_HEADER)
        && !count_trailer(payload, value.checked_add(COUNT_LEN)?)
        && count_trailer(payload, compact.checked_add(COUNT_LEN)?);
    Some(if behind_compact_header {
        compact
    } else {
        value
    })
}

/// Whether a count's trailer begins at `at`: three zero bytes, then the
/// nonzero scalar object identifier, below 2^24.
fn count_trailer(payload: &[u8], at: usize) -> bool {
    let Some(id_at) = at.checked_add(TRAILER_OBJECT_ID) else {
        return false;
    };
    payload.get(at..id_at) == Some(&[0, 0, 0])
        && View::u32_le_at(payload, id_at).is_some_and(|id| id != 0 && id < COUNT_OBJECT_ID_BOUND)
}

/// The parameter class each class-token index of the lane's parameter slots
/// names. A token of a class `Config-N` holds (see [`ConfigurationMap`];
/// without one, a token below the stream's opening map count `stated`)
/// names the parameter class `Config-N` holds at that index, read from
/// `configuration` when its walk reconciles with the stated count; the
/// lane's declarations name the other tokens (see
/// [`declared_token_classes`]).
fn token_classes(
    configuration: Option<&ConfigurationMap>,
    stated: Option<u32>,
    classes: &[FeatureInputClass],
    slots: &[(&FeatureInputScalar, u64, Slot<'_>)],
) -> HashMap<u16, ArchiveParameterClass> {
    let held = |index: u16| match configuration {
        Some(configuration) => configuration.holds(index),
        None => stated.is_some_and(|count| u32::from(index) < count),
    };
    let own = slots
        .iter()
        .copied()
        .filter(|&(_, _, slot)| matches!(slot, Slot::Token(index) if !held(index)))
        .collect::<Vec<_>>();
    let mut named = declared_token_classes(classes, &own);
    if let Some(parameters) = configuration.and_then(ConfigurationMap::parameters) {
        named.extend(parameters.iter().map(|(&index, &class)| (index, class)));
    }
    named
}

/// The parameter class the lane's declarations give each class-token index
/// of `slots`. Declarations take ascending indices in stream order, and a
/// token follows its class's declaration. The tokens of driving and display
/// scalars, ascending, are given the lane's parameter-class declarations, in
/// stream order, such that both orders agree and each token is first used
/// after its declaration. A token to which every such assignment gives one
/// class has that class; a token the assignments disagree on, or that no
/// assignment exists for, is not named.
fn declared_token_classes(
    classes: &[FeatureInputClass],
    slots: &[(&FeatureInputScalar, u64, Slot<'_>)],
) -> HashMap<u16, ArchiveParameterClass> {
    let mut first_use = BTreeMap::<u16, u64>::new();
    let mut framed = BTreeSet::new();
    for &(scalar, name, slot) in slots {
        let Slot::Token(index) = slot else {
            continue;
        };
        first_use
            .entry(index)
            .and_modify(|first| *first = (*first).min(name))
            .or_insert(name);
        if matches!(
            scalar.role,
            FeatureInputScalarRole::Driving | FeatureInputScalarRole::Display
        ) {
            framed.insert(index);
        }
    }
    let tokens = framed
        .into_iter()
        .filter_map(|index| Some((index, *first_use.get(&index)?)))
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return HashMap::new();
    }
    let mut declared = classes
        .iter()
        .filter_map(|class| {
            Some((
                class.offset,
                ArchiveParameterClass::from_class_name(&class.name)?,
            ))
        })
        .collect::<Vec<_>>();
    declared.sort_by_key(|&(offset, _)| offset);
    let redeclared = declared.iter().enumerate().any(|(index, &(_, class))| {
        declared[..index]
            .iter()
            .any(|&(_, earlier)| earlier == class)
    });
    if redeclared {
        return HashMap::new();
    }
    let mut found = Vec::new();
    assignments(&tokens, &declared, 0, &mut Vec::new(), &mut found);
    let Some((first, others)) = found.split_first() else {
        return HashMap::new();
    };
    tokens
        .iter()
        .enumerate()
        .filter(|&(position, _)| {
            others
                .iter()
                .all(|other| other[position] == first[position])
        })
        .map(|(position, &(index, _))| (index, declared[first[position]].1))
        .collect()
}

/// Add to `found` the order-preserving assignments of tokens to declarations
/// that extend `chosen`, the declarations of the first `chosen.len()` tokens,
/// with declarations from `from` on. The declarations name distinct
/// parameter classes, so there are at most four, and at most six assignments.
fn assignments(
    tokens: &[(u16, u64)],
    declared: &[(u64, ArchiveParameterClass)],
    from: usize,
    chosen: &mut Vec<usize>,
    found: &mut Vec<Vec<usize>>,
) {
    let Some(&(_, first_use)) = tokens.get(chosen.len()) else {
        found.push(chosen.clone());
        return;
    };
    for (index, &(offset, _)) in declared.iter().enumerate().skip(from) {
        if offset < first_use {
            chosen.push(index);
            assignments(tokens, declared, index + 1, chosen, found);
            chosen.pop();
        }
    }
}

#[cfg(test)]
mod tests;
