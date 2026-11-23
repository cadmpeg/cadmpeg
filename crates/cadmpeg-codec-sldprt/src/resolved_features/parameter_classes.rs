// SPDX-License-Identifier: Apache-2.0
//! The parameter each named scalar's own record states (`docs/formats/sldprt.md`
//! §2): the class of its parameter object, written immediately before its
//! name record as a class declaration or a class token, and its value read in
//! that class's cell.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::View;

use super::{is_class_token, COMPACT_SCALAR_HEADER, VALUE_ONLY_SCALAR_HEADER};
use crate::archive::{compound_opening, resolved_count};
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

/// What a scalar's own record states of its parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Statement {
    /// The parameter, in the class its record states.
    Parameter(StatedParameter),
    /// The parameter, in the class a compound-file lane's own declarations
    /// alone give its class token. The stream does not state the index at
    /// which `Config-N` ends (§2.2), so reading the token as the lane's own is
    /// an assumption (open item EV-05).
    Assumed(StatedParameter),
    /// A class token whose class the decoder's sources name differently
    /// (see [`token_classes`]), so the record states no class it reads.
    Contested,
}

impl Statement {
    /// The parameter the record states, in a class read or assumed.
    pub(crate) fn parameter(self) -> Option<StatedParameter> {
        match self {
            Self::Parameter(parameter) | Self::Assumed(parameter) => Some(parameter),
            Self::Contested => None,
        }
    }
}

/// The classes of the `Config-N` map a `ResolvedFeatures` lane that states
/// a map count continues (`docs/formats/sldprt.md` §2.2), read where a walk
/// of `Config-N` reconciles with that count. The lane's own declarations
/// take the indices from the count on, and a token below it names the class
/// `Config-N` holds at that index. The map holds the class at each index
/// below the count at which the walk places one: its parameter class, or
/// `None` for a class that is no parameter class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigurationMap(BTreeMap<u16, Option<ArchiveParameterClass>>);

impl ConfigurationMap {
    /// The classes below the count `end` of `classes`, the map of a walk of
    /// `Config-N` that reconciles with `end`.
    pub(crate) fn reconciled(end: u32, classes: &BTreeMap<u32, String>) -> Self {
        Self(
            classes
                .range(..end)
                .filter_map(|(&index, name)| {
                    Some((
                        u16::try_from(index).ok()?,
                        ArchiveParameterClass::from_class_name(name),
                    ))
                })
                .collect(),
        )
    }
}

/// The class a class token names, from the sources that name it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TokenClass {
    Parameter(ArchiveParameterClass),
    /// A class only a compound-file lane's own declarations give the token.
    Assumed(ArchiveParameterClass),
    Contested,
}

/// What a scalar's parameter-class slot holds.
#[derive(Debug, Clone, Copy)]
enum Slot<'a> {
    Declared(&'a str),
    Token(u16),
}

/// What each scalar of `lane` states of its parameter, by scalar id. A
/// scalar whose slot holds a class token states the class its sources give
/// that token (see [`token_classes`]): `configuration`, the classes of the
/// `Config-N` map the lane continues; the lane's declarations; and the
/// relation class declared beside the token. It is contested where they
/// name different classes, and assumed where only the declarations of a
/// compound-file lane name it. A scalar whose class is not a parameter
/// class, or whose token no source names, states nothing.
pub(crate) fn stated_parameters<'l>(
    lane: &'l FeatureInputLane,
    configuration: Option<&ConfigurationMap>,
) -> HashMap<&'l str, Statement> {
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
    let related = slots
        .iter()
        .filter_map(|&(_, name, slot)| match slot {
            Slot::Token(index) => Some((
                index,
                relation_parameter_class(payload, &declarations, name)?,
            )),
            Slot::Declared(_) => None,
        })
        .collect::<Vec<_>>();
    let tokens = token_classes(
        configuration,
        resolved_count(payload),
        compound_opening(payload),
        &lane.classes,
        &slots,
        &related,
    );
    slots
        .into_iter()
        .filter_map(|(scalar, _, slot)| {
            let (class, assumed) = match slot {
                Slot::Declared(name) => (ArchiveParameterClass::from_class_name(name)?, false),
                Slot::Token(index) => match *tokens.get(&index)? {
                    TokenClass::Parameter(class) => (class, false),
                    TokenClass::Assumed(class) => (class, true),
                    TokenClass::Contested => {
                        return Some((scalar.id.as_str(), Statement::Contested))
                    }
                },
            };
            let stated = stated(payload, scalar, class)?;
            Some((
                scalar.id.as_str(),
                if assumed {
                    Statement::Assumed(stated)
                } else {
                    Statement::Parameter(stated)
                },
            ))
        })
        .collect()
}

/// The parameter class of the relation class declared in place before the
/// parameter-class token of the name record at `name` (§2): its declaration
/// ends where the token form's relation token ends, at prefix +2, and the
/// zero u32 follows it. `None` where no relation class of a stated kind is
/// declared there.
fn relation_parameter_class(
    payload: &[u8],
    declarations: &HashMap<u64, &str>,
    name: u64,
) -> Option<ArchiveParameterClass> {
    let start = usize::try_from(name).ok()?.checked_sub(prefix::LEN)?;
    let relation_end = start.checked_add(prefix::ZERO)?;
    let zero = payload.get(relation_end..start.checked_add(prefix::PARAMETER_CLASS_TOKEN)?)?;
    if zero.iter().any(|&byte| byte != 0) {
        return None;
    }
    relation_kind(declarations.get(&u64::try_from(relation_end).ok()?)?)
}

/// The parameter class of a sketch dimension whose relation class is `name`
/// (§2).
fn relation_kind(name: &str) -> Option<ArchiveParameterClass> {
    Some(match name {
        "sgPntPntDist"
        | "sgLLDist"
        | "sgPntLineDist"
        | "sgPntPntHorDist"
        | "sgPntPntVertDist"
        | "sgPntLineHorDist"
        | "sgPntLineVertDist"
        | "sgOffsetDim"
        | "sgSkOffsetDim"
        | "sgPoint3DPlaneDist"
        | "sg3DPlane3DPlaneDist"
        | "sgCircleDim" => ArchiveParameterClass::Length,
        "sgAnglDim" | "sgValG3Dim" => ArchiveParameterClass::Angle,
        "sgCircularPattCntDim" | "sgLinearPattCntDim" => ArchiveParameterClass::Integer,
        _ => return None,
    })
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
/// four bytes; each count's trailer begins after the count. `None` unless a
/// trailer begins at the cell's +4 and none at its +8 (`docs/formats/sldprt.md`
/// §2): a padded value-only scalar frames an eight-byte f64 cell there, whose
/// low half is no count.
fn count_cell(payload: &[u8], value: usize) -> Option<usize> {
    let header = value.checked_sub(VALUE_ONLY_SCALAR_HEADER.len())?;
    let compact = header.checked_add(COMPACT_SCALAR_HEADER.len())?;
    let behind_compact_header = payload.get(header..compact) == Some(COMPACT_SCALAR_HEADER)
        && !count_trailer(payload, value.checked_add(COUNT_LEN)?)
        && count_trailer(payload, compact.checked_add(COUNT_LEN)?);
    let cell = if behind_compact_header {
        compact
    } else {
        value
    };
    (count_trailer(payload, cell.checked_add(COUNT_LEN)?)
        && !count_trailer(payload, cell.checked_add(size_of::<f64>())?))
    .then_some(cell)
}

/// Whether a count's trailer begins at `at`: three zero bytes, then the
/// nonzero scalar object identifier, below 2^24.
pub(super) fn count_trailer(payload: &[u8], at: usize) -> bool {
    let Some(id_at) = at.checked_add(TRAILER_OBJECT_ID) else {
        return false;
    };
    payload.get(at..id_at) == Some(&[0, 0, 0])
        && View::u32_le_at(payload, id_at).is_some_and(|id| id != 0 && id < COUNT_OBJECT_ID_BOUND)
}

/// The class each class-token index of the lane's parameter slots names,
/// from three sources, none of which outranks another:
///
/// - for a token below the stream's opening map count `stated`, a class
///   `Config-N` holds, the class the reconciled walk of `Config-N` places at
///   that index, where `configuration` holds one (see [`ConfigurationMap`]);
/// - for the lane's other tokens, its declarations (see
///   [`declared_token_classes`]);
/// - for any token, the relation class declared beside it, `related`.
///
/// A token that every source naming it gives one parameter class has that
/// class. A token at which the walk places a class that is no parameter
/// class, and that no other source names, names none. A token the sources
/// give different classes is contested. A `compound` lane, a compound-file
/// lane, continues the `Config-N` map without stating its count, so every
/// token joins the order rule over its declarations as the lane's own; a
/// token only that rule names has its class by that assumption.
fn token_classes(
    configuration: Option<&ConfigurationMap>,
    stated: Option<u32>,
    compound: bool,
    classes: &[FeatureInputClass],
    slots: &[(&FeatureInputScalar, u64, Slot<'_>)],
    related: &[(u16, ArchiveParameterClass)],
) -> HashMap<u16, TokenClass> {
    let held = |index: u16| stated.is_some_and(|count| u32::from(index) < count);
    let own = slots
        .iter()
        .copied()
        .filter(|&(_, _, slot)| matches!(slot, Slot::Token(index) if !held(index)))
        .collect::<Vec<_>>();
    let used = slots
        .iter()
        .filter_map(|&(_, _, slot)| match slot {
            Slot::Token(index) => Some(index),
            Slot::Declared(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let walked = configuration
        .into_iter()
        .flat_map(|configuration| &configuration.0)
        .filter(|&(&index, _)| used.contains(&index))
        .map(|(&index, &class)| (index, class, false));
    let ordered = declared_token_classes(classes, &own)
        .into_iter()
        .map(|(index, class)| (index, Some(class), compound));
    let related = related
        .iter()
        .map(|&(index, class)| (index, Some(class), false));
    let mut claims = HashMap::<u16, Vec<(Option<ArchiveParameterClass>, bool)>>::new();
    for (index, claim, assumed) in walked.chain(ordered).chain(related) {
        claims.entry(index).or_default().push((claim, assumed));
    }
    claims
        .into_iter()
        .filter_map(|(index, claims)| {
            let (&(first, _), rest) = claims.split_first()?;
            if rest.iter().any(|&(claim, _)| claim != first) {
                return Some((index, TokenClass::Contested));
            }
            let class = first?;
            Some((
                index,
                if claims.iter().all(|&(_, assumed)| assumed) {
                    TokenClass::Assumed(class)
                } else {
                    TokenClass::Parameter(class)
                },
            ))
        })
        .collect()
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
