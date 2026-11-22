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

/// What a scalar's own record states of its parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Statement {
    /// The parameter, in the class its record states.
    Parameter(StatedParameter),
    /// A class token whose class the decoder's sources name differently
    /// (see [`token_classes`]), so the record states no class it reads.
    Contested,
}

/// The class of each index of a `Config-N` map at which a walk places one:
/// its parameter class, or `None` for a class that is no parameter class.
type HeldClasses = BTreeMap<u16, Option<ArchiveParameterClass>>;

/// The `Config-N` class map a `ResolvedFeatures` lane's class tokens continue
/// (`docs/formats/sldprt.md` §2.2). The lane's own declarations take the
/// indices from the end of that map on, and a token below it names the class
/// `Config-N` holds at that index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigurationMap {
    /// A stream that states the map count `end`. `classes` holds the class
    /// at each index below `end` at which a walk of `Config-N` that
    /// reconciles with `end` places one.
    Counted {
        end: u32,
        classes: Option<HeldClasses>,
    },
    /// A compound-file stream, which states no count: the class at each
    /// index at which a consistent walk of `Config-N` places one.
    Unstated { classes: HeldClasses },
}

impl ConfigurationMap {
    /// The map of a stream that states the count `end`, with the `Config-N`
    /// class at each index from `classes`, a walk that reconciles with `end`.
    pub(crate) fn counted(end: u32, classes: Option<&BTreeMap<u32, String>>) -> Self {
        let classes = classes.map(|classes| held_classes(classes.range(..end)));
        Self::Counted { end, classes }
    }

    /// The map of a compound-file stream, with the `Config-N` class at each
    /// index from `classes`, a walk that no count bounds.
    pub(crate) fn unstated(classes: &BTreeMap<u32, String>) -> Self {
        Self::Unstated {
            classes: held_classes(classes.iter()),
        }
    }

    /// Whether `index` names a class `Config-N` holds rather than one the
    /// lane declares.
    fn holds(&self, index: u16) -> bool {
        match self {
            Self::Counted { end, .. } => u32::from(index) < *end,
            Self::Unstated { classes } => classes.contains_key(&index),
        }
    }

    /// The class at each index at which the walk of `Config-N` places one,
    /// when the map is read.
    fn classes(&self) -> Option<&HeldClasses> {
        match self {
            Self::Counted { classes, .. } => classes.as_ref(),
            Self::Unstated { classes } => Some(classes),
        }
    }
}

/// Each of `classes` by its index as a class token, with its parameter
/// class where it names one.
fn held_classes<'c>(classes: impl Iterator<Item = (&'c u32, &'c String)>) -> HeldClasses {
    classes
        .filter_map(|(&index, name)| {
            Some((
                u16::try_from(index).ok()?,
                ArchiveParameterClass::from_class_name(name),
            ))
        })
        .collect()
}

/// The class a class token names, from the sources that name it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TokenClass {
    Parameter(ArchiveParameterClass),
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
/// that token: `configuration`, the `Config-N` map the lane continues, the
/// lane's declarations, and the relation class declared beside the token
/// (see [`token_classes`]); it is contested where they name different
/// classes. A scalar whose class is not a parameter class, or whose token no
/// source names, states nothing.
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
        &lane.classes,
        &slots,
        &related,
    );
    slots
        .into_iter()
        .filter_map(|(scalar, _, slot)| {
            let class = match slot {
                Slot::Declared(name) => ArchiveParameterClass::from_class_name(name)?,
                Slot::Token(index) => match *tokens.get(&index)? {
                    TokenClass::Parameter(class) => class,
                    TokenClass::Contested => {
                        return Some((scalar.id.as_str(), Statement::Contested))
                    }
                },
            };
            Some((
                scalar.id.as_str(),
                Statement::Parameter(stated(payload, scalar, class)?),
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

/// The class each class-token index of the lane's parameter slots names,
/// from three sources, none of which outranks another:
///
/// - for a token of a class `Config-N` holds (see [`ConfigurationMap`];
///   without one, a token below the stream's opening map count `stated`),
///   the class the walk of `Config-N` in `configuration` places at that
///   index, where it places one;
/// - for the lane's other tokens, its declarations (see
///   [`declared_token_classes`]);
/// - for any token, the relation class declared beside it, `related`.
///
/// A token that every source naming it gives one parameter class has that
/// class. A token at which the walk places a class that is no parameter
/// class, and that no other source names, names none. A token the sources
/// give different classes is contested.
fn token_classes(
    configuration: Option<&ConfigurationMap>,
    stated: Option<u32>,
    classes: &[FeatureInputClass],
    slots: &[(&FeatureInputScalar, u64, Slot<'_>)],
    related: &[(u16, ArchiveParameterClass)],
) -> HashMap<u16, TokenClass> {
    let held = |index: u16| match configuration {
        Some(configuration) => configuration.holds(index),
        None => stated.is_some_and(|count| u32::from(index) < count),
    };
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
        .and_then(ConfigurationMap::classes)
        .into_iter()
        .flatten()
        .filter(|&(&index, _)| used.contains(&index))
        .map(|(&index, &class)| (index, class));
    let ordered = declared_token_classes(classes, &own)
        .into_iter()
        .map(|(index, class)| (index, Some(class)));
    let related = related.iter().map(|&(index, class)| (index, Some(class)));
    let mut claims = HashMap::<u16, Vec<Option<ArchiveParameterClass>>>::new();
    for (index, claim) in walked.chain(ordered).chain(related) {
        claims.entry(index).or_default().push(claim);
    }
    claims
        .into_iter()
        .filter_map(|(index, claims)| {
            let (&first, rest) = claims.split_first()?;
            if rest.iter().any(|&claim| claim != first) {
                return Some((index, TokenClass::Contested));
            }
            Some((index, TokenClass::Parameter(first?)))
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
