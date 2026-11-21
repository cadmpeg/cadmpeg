// SPDX-License-Identifier: Apache-2.0
//! The parameter each named scalar's own record states (`docs/formats/sldprt.md`
//! §2): the class of its parameter object, written immediately before its
//! name record as a class declaration or a class token, and its value read in
//! that class's cell.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::View;

use super::{is_class_token, CLASS_MARKER};
use crate::archive::resolved_features_count;
use crate::layout::feature_input_scalar_parameter_class as class_slot;
use crate::records::{
    ArchiveParameterClass, FeatureInputClass, FeatureInputLane, FeatureInputScalar,
    FeatureInputScalarRole,
};

/// The bytes of a class declaration before its name: the `ff ff 01 00`
/// marker and the u16 name length.
const DECLARATION_PREFIX: u64 = 6;

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

/// What a scalar's parameter-class slot holds.
#[derive(Debug, Clone, Copy)]
enum Slot<'a> {
    Declared(&'a str),
    Token(u16),
}

/// The parameter each scalar of `lane` states, by scalar id. A scalar whose
/// slot holds a class token states the class the lane's declarations give
/// that token (see [`token_classes`]); a scalar whose class is not a
/// parameter class, or whose token no declaration answers, states none.
pub(crate) fn stated_parameters(lane: &FeatureInputLane) -> HashMap<&str, StatedParameter> {
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
    let tokens = token_classes(payload, &lane.classes, &slots);
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
    let start = usize::try_from(name).ok()?.checked_sub(class_slot::LEN)?;
    let token = View::u16_le_at(payload, start + class_slot::PARAMETER_CLASS_TOKEN)?;
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
            usize::try_from(scalar.offset).ok()?,
        )?),
    })
}

/// The parameter class each class-token index of the lane's parameter slots
/// names. A token names the class declared at that index of the stream's
/// class map; declarations take ascending indices in stream order, and a token
/// follows its class's declaration. The tokens of driving and display scalars,
/// ascending, are given the lane's parameter-class declarations, in stream
/// order, such that both orders agree and each token is first used after its
/// declaration. Where exactly one such assignment exists, it names each
/// token's class; otherwise no token is named. A token below the stream's
/// opening map count (see [`opening_count`]) names a class of the `Config-N`
/// map, which the lane does not hold.
fn token_classes(
    payload: &[u8],
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
    if tokens.is_empty()
        || opening_count(payload)
            .is_some_and(|count| tokens.iter().any(|&(index, _)| u32::from(index) < count))
    {
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
    match found.as_slice() {
        [assignment] => tokens
            .iter()
            .zip(assignment)
            .map(|(&(index, _), &declaration)| (index, declared[declaration].1))
            .collect(),
        _ => HashMap::new(),
    }
}

/// The map count the lane's stream opens with. A stream that opens with a
/// class declaration, as a `Config-N` stream does, states none.
fn opening_count(payload: &[u8]) -> Option<u32> {
    if payload.starts_with(CLASS_MARKER) {
        return None;
    }
    resolved_features_count(payload)
}

/// Add to `found`, until it holds two, the order-preserving assignments of
/// tokens to declarations that extend `chosen`, the declarations of the first
/// `chosen.len()` tokens, with declarations from `from` on.
fn assignments(
    tokens: &[(u16, u64)],
    declared: &[(u64, ArchiveParameterClass)],
    from: usize,
    chosen: &mut Vec<usize>,
    found: &mut Vec<Vec<usize>>,
) {
    if found.len() > 1 {
        return;
    }
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
