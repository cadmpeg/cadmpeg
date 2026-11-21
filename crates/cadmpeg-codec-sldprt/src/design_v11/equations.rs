// SPDX-License-Identifier: Apache-2.0
//! The equation manager's lines as parameter expressions (§5.12, §5.13).
//!
//! Each configuration stream `Contents/Config-N` holds a `moRelMgr_c` whose
//! relations are the lines of the equation manager (`docs/formats/sldprt.md`).
//! The active configuration's lines are read; a line `"D1@Sketch1" = …`
//! states the expression of the parameter it assigns, each quoted name it
//! reads a reference to that parameter. A line whose names all resolve and
//! whose text the neutral grammar holds is a `Neutral` tree, read by the
//! unit-blind rule of §5.13. A tree that is one reference translates exactly
//! and is stated whatever the caches hold; any other tree holds a literal, a
//! unit-blind reference or a rounding, is under Rule H1, and is stated only
//! where the document's caches agree with its evaluation. Any other line is a
//! `Source` expression of the `SolidWorks` dialect. A line on a dependency
//! cycle is stated like any other; the cycle is the document's. A
//! parameter's kind and value are the ones its dimension states; a line
//! changes its definition and comment only. A global variable that a line
//! assigns and that names no parameter is a user parameter of the scope
//! (§5.12): dimensionless, or of the quantity its literal's unit states, with
//! the literal's value where the line is one literal.

mod line;
mod translate;

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::edit::{evaluate, Value};
use cadmpeg_ir::design::expr::{
    self as expr, Expression, ExpressionDialect, ExpressionToken, Neutral, Node,
};
use cadmpeg_ir::design::feature::Feature;
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::Tolerances;

use self::line::{Line, Parsed, Segment};
use crate::container::{configuration_index, ContainerScan};
use crate::loss::{NamedCount, SldprtLossCode};
use crate::records::FeatureHistory;

/// The declaration of the manager class, its relation class, and the class of
/// a relation's first equation side: a new-class tag, schema 1, the name's
/// length, and the name.
const MANAGER: &[u8] = b"\xff\xff\x01\x00\x0a\x00moRelMgr_c";
const RELATION: &[u8] = b"\xff\xff\x01\x00\x0c\x00moRelation_c";
const SIDE: &[u8] = b"\xff\xff\x01\x00\x13\x00moRelEquationSide_c";
/// The tag of a counted UTF-16LE string.
const STRING: &[u8] = b"\xff\xfe\xff";
/// The bytes between a relation's text and its first equation side.
const AFTER_TEXT: usize = 6;
/// A u16 class token with this bit set repeats an earlier class.
const REPEAT: u16 = 0x8000;
/// A later relation's side token is the relation token plus this.
const SIDE_AFTER_RELATION: u16 = 2;

/// The relative tolerance of a cache that is neither a length nor an angle
/// (part 10, Caches).
const RELATIVE: f64 = 1e-9;

/// The equation managers of the configuration streams.
#[derive(Debug, Default)]
pub(crate) struct Managers(Vec<Manager>);

#[derive(Debug)]
struct Manager {
    slot: usize,
    lines: Walk,
}

/// A manager's relation list.
#[derive(Debug, PartialEq)]
enum Walk {
    /// The relation texts, in stored order.
    Lines(Vec<String>),
    /// The list stops before its stated count.
    Stopped,
}

impl Walk {
    fn lines(&self) -> Option<&Vec<String>> {
        match self {
            Self::Lines(lines) => Some(lines),
            Self::Stopped => None,
        }
    }
}

/// The equation managers of every `Contents/Config-N` section of `scan`.
pub(crate) fn managers(scan: &ContainerScan) -> Managers {
    Managers(
        scan.sections()
            .filter_map(|section| {
                let name = section.name()?;
                if !super::units::is_configuration(name) {
                    return None;
                }
                let slot = configuration_index(name)?;
                Some(Manager {
                    slot,
                    lines: walk(section.payload())?,
                })
            })
            .collect(),
    )
}

/// The relation list of the manager in `payload`, `None` where it holds
/// none.
fn walk(payload: &[u8]) -> Option<Walk> {
    let at = find(payload, MANAGER, 0)?;
    let count_at = at + MANAGER.len();
    let Some(count) = View::u16_le_at(payload, count_at) else {
        return Some(Walk::Stopped);
    };
    let mut lines = Vec::new();
    let mut cursor = count_at + 2;
    let mut token = None;
    for index in 0..count {
        let found = if index == 0 {
            first_relation(payload, cursor)
        } else {
            later_relation(payload, cursor, &mut token)
        };
        let Some((text, end)) = found else {
            return Some(Walk::Stopped);
        };
        lines.push(text);
        cursor = end;
    }
    Some(Walk::Lines(lines))
}

/// The first relation after `from`: its class declaration, its text, the six
/// bytes after it, and the declaration of its first side. Returns the text and
/// the offset after the side's declaration.
fn first_relation(payload: &[u8], from: usize) -> Option<(String, usize)> {
    let at = find(payload, RELATION, from)?;
    let (text, end) = counted(payload, at + RELATION.len())?;
    let side = after_text(payload, end)?;
    (payload.get(side..side.checked_add(SIDE.len())?) == Some(SIDE))
        .then(|| (text, side + SIDE.len()))
}

/// The next relation after `from`: a counted string behind a repeat token
/// `t` (the same token for every later relation), the six bytes after it,
/// and the side token `t + 2`. Returns the text and the offset after the
/// side token.
fn later_relation(payload: &[u8], from: usize, token: &mut Option<u16>) -> Option<(String, usize)> {
    let mut at = find(payload, STRING, from)?;
    loop {
        let candidate = at
            .checked_sub(2)
            .and_then(|tag| View::u16_le_at(payload, tag))
            .filter(|tag| tag & REPEAT != 0 && token.is_none_or(|known| known == *tag));
        if let Some(tag) = candidate {
            if let Some((text, end)) = counted(payload, at) {
                if let Some(side) = after_text(payload, end) {
                    if View::u16_le_at(payload, side) == tag.checked_add(SIDE_AFTER_RELATION) {
                        *token = Some(tag);
                        return Some((text, side + 2));
                    }
                }
            }
        }
        at = find(payload, STRING, at + 1)?;
    }
}

/// The offset after the six bytes that follow a relation's text, where bytes
/// 3 to 5 are zero.
fn after_text(payload: &[u8], end: usize) -> Option<usize> {
    let bytes = payload.get(end..end.checked_add(AFTER_TEXT)?)?;
    (bytes[3..] == [0, 0, 0]).then_some(end + AFTER_TEXT)
}

/// The counted UTF-16LE string at `at`: `ff fe ff`, a u8 count (`ff` then a
/// u16 count), the code units. Returns the text and the offset after it.
fn counted(payload: &[u8], at: usize) -> Option<(String, usize)> {
    if payload.get(at..at.checked_add(STRING.len())?) != Some(STRING) {
        return None;
    }
    let mut cursor = at + STRING.len();
    let mut units = usize::from(*payload.get(cursor)?);
    cursor += 1;
    if units == 0xff {
        units = usize::from(View::u16_le_at(payload, cursor)?);
        cursor += 2;
    }
    let end = cursor.checked_add(units.checked_mul(2)?)?;
    let bytes = payload.get(cursor..end)?;
    let code_units = (0..units).filter_map(|unit| View::u16_le_at(bytes, unit * 2));
    let text = char::decode_utf16(code_units)
        .collect::<Result<String, _>>()
        .ok()?;
    Some((text, end))
}

fn find(payload: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    payload
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| from + position)
}

/// The slot of the active configuration's stream: its `SourceIndex`.
pub(super) fn active_slot(
    histories: &[FeatureHistory],
    source: Option<&SourceMeta>,
) -> Option<usize> {
    let configurations = histories
        .iter()
        .flat_map(|history| &history.configurations)
        .collect::<Vec<_>>();
    let selectors = configurations
        .iter()
        .map(|configuration| {
            (
                Some(configuration.name.as_str()),
                configuration.source_index,
            )
        })
        .collect::<Vec<_>>();
    let active = crate::decode::active_configuration_position(source, &selectors)?;
    usize::try_from(configurations[active].source_index?).ok()
}

/// What the lines are read against.
pub(super) struct Linking<'a> {
    pub(super) managers: &'a Managers,
    /// The scope's definition, which a minted global variable belongs to.
    pub(super) scope: &'a ProductDefinitionId,
    /// The active configuration's stream slot.
    pub(super) active: Option<usize>,
    /// The timeline's features, whose names qualify their dimensions.
    pub(super) features: &'a [Feature],
    /// The definition's length unit.
    pub(super) length_unit: Option<UnitSymbol>,
    pub(super) tolerances: &'a Tolerances,
}

/// Why a line is not read into its parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unread {
    NoActiveStream,
    ManagerUnwalked,
    OtherManagerUnwalked,
    NoDesign,
    Malformed,
    AssignedTwice,
    ConfigurationsDiffer,
    NameUnresolved,
    Driven,
}

impl Unread {
    const fn reason(self) -> &'static str {
        match self {
            Self::NoActiveStream => "no stream of the active configuration holds the manager",
            Self::ManagerUnwalked => "the active configuration's manager is not read",
            Self::OtherManagerUnwalked => "another configuration's manager is not read",
            Self::NoDesign => "the part states no design",
            Self::Malformed => "the line is not a quoted name, `=`, and an expression",
            Self::AssignedTwice => "two lines assign the name",
            Self::ConfigurationsDiffer => {
                "another configuration's manager does not hold the same line"
            }
            Self::NameUnresolved => "the assigned name names no parameter",
            Self::Driven => "the assigned parameter is a reference dimension",
        }
    }
}

/// Why a line's expression is `Source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kept {
    NameUnresolved,
    Untranslated,
    Untested,
    Disagrees,
    BareReference,
}

impl Kept {
    const fn reason(self) -> &'static str {
        match self {
            Self::NameUnresolved => "a name it reads names no parameter",
            Self::Untranslated => {
                "its text is outside the neutral grammar or its kinds do not type"
            }
            Self::Untested => "the document holds no value to test its reading against",
            Self::Disagrees => "its evaluation disagrees with the stored value",
            Self::BareReference => {
                "it is one reference to a parameter whose kind differs or is not read"
            }
        }
    }
}

/// Reads the active configuration's equation-manager lines into the
/// expressions of the parameters they assign, and returns the loss notes of
/// the lines not read and of those stated as `Source`.
pub(super) fn link(parameters: &mut Vec<DesignParameter>, linking: &Linking<'_>) -> Vec<LossNote> {
    let lines = match active_lines(linking.managers, linking.active) {
        Ok(lines) => lines,
        Err(why) => return vec![manager_unread(why)],
    };
    let mut unread = Losses::default();
    let mut kept = Losses::default();
    for minted in mint(parameters, &lines, linking.scope) {
        kept.push(Kept::Untranslated, parameters[minted].id.as_str());
    }
    let names = Names::new(parameters, linking.features);
    for line in lines {
        let line = match line {
            Ok(line) => line,
            Err((why, name)) => {
                unread.push(why, &name);
                continue;
            }
        };
        let Some(target) = names.resolve(&line.name) else {
            unread.push(Unread::NameUnresolved, &line.name);
            continue;
        };
        if matches!(parameters[target].definition, ParameterDefinition::Driven) {
            unread.push(Unread::Driven, &line.name);
            continue;
        }
        let reads = line
            .segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Name(name) => Some(names.resolve(name)),
                Segment::Text(_) => None,
            })
            .collect::<Vec<_>>();
        if reads.is_empty() {
            continue;
        }
        let (expression, why) = expression(parameters, target, &line, &names, linking);
        if let Some(why) = why {
            kept.push(why, parameters[target].id.as_str());
        }
        let parameter = &mut parameters[target];
        parameter.definition = ParameterDefinition::Expression(expression);
        if line.comment.is_some() {
            parameter.comment = line.comment;
        }
    }
    unread.notes(&kept)
}

/// The loss note of the active configuration's lines, for a part that states
/// no design.
pub(super) fn without_design(managers: &Managers, active: Option<usize>) -> Vec<LossNote> {
    let Ok(lines) = active_lines(managers, active) else {
        return vec![manager_unread(Unread::NoDesign)];
    };
    let mut unread = Losses::default();
    for line in &lines {
        let name = match line {
            Ok(line) => &line.name,
            Err((_, name)) => name,
        };
        unread.push(Unread::NoDesign, name);
    }
    unread.notes(&Losses::default())
}

fn manager_unread(why: Unread) -> LossNote {
    SldprtLossCode::DesignEquationUnread.note(format!(
        "The equation-manager lines are not read into parameter expressions, because {}.",
        why.reason()
    ))
}

/// Mints a user parameter for each global variable a line assigns that no
/// parameter is named, ignoring letter case, and returns the positions of
/// those whose line is neither one literal nor reads a name: their
/// expression is the line's text.
fn mint(
    parameters: &mut Vec<DesignParameter>,
    lines: &[LineResult],
    scope: &ProductDefinitionId,
) -> Vec<usize> {
    let known = Names::new(parameters, &[]);
    let mut minted_names = HashSet::new();
    let mut textual = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Ok(line) = line else {
            continue;
        };
        if line.name.contains('@')
            || known.knows(&line.name)
            || !minted_names.insert(line.name.to_lowercase())
        {
            continue;
        }
        let Some(name) = NonBlankString::new(line.name.clone()) else {
            continue;
        };
        let id = ParameterId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "model", "parameter"),
            cadmpeg_ir::identity_key!("equation:").then(index),
        );
        let (kind, expression, value) = match translate::literal(&line.expression) {
            Some((node, quantity, value)) => (
                ValueKind::Quantity(quantity),
                Expression::Neutral(Neutral(node)),
                FiniteReal::new(value).map(ParameterValue::Number),
            ),
            None => {
                if !line
                    .segments
                    .iter()
                    .any(|segment| matches!(segment, Segment::Name(_)))
                {
                    textual.push(parameters.len());
                }
                (
                    ValueKind::Quantity(Quantity::REAL),
                    Expression::Source {
                        dialect: ExpressionDialect::SolidWorks,
                        tokens: vec![ExpressionToken::Text(line.expression.clone())],
                    },
                    None,
                )
            }
        };
        parameters.push(DesignParameter {
            id,
            scope: scope.clone(),
            owner: None,
            name: Some(name),
            kind,
            unit: None,
            definition: ParameterDefinition::Expression(expression),
            value,
            display: None,
            tolerance: None,
            decimals: None,
            comment: line.comment.clone(),
            native_options: cadmpeg_ir::design::options::NativeOptions::new(),
            source_settings: cadmpeg_ir::design::options::NativeOptions::new(),
            native_ref: None,
        });
    }
    textual
}

type LineResult = Result<Line, (Unread, String)>;

/// The active configuration's lines, each parsed, with the lines another
/// configuration's manager does not confirm refused; or, where none can be
/// read, why.
fn active_lines(managers: &Managers, active: Option<usize>) -> Result<Vec<LineResult>, Unread> {
    let managers = &managers.0;
    let holds_lines =
        |manager: &&Manager| manager.lines.lines().is_none_or(|lines| !lines.is_empty());
    if !managers.iter().any(|manager| holds_lines(&manager)) {
        return Ok(Vec::new());
    }
    let active = if managers.len() == 1 {
        &managers[0]
    } else {
        let mut matching = managers
            .iter()
            .filter(|manager| Some(manager.slot) == active);
        match (matching.next(), matching.next()) {
            (Some(manager), None) => manager,
            _ => return Err(Unread::NoActiveStream),
        }
    };
    let Some(texts) = active.lines.lines() else {
        return Err(Unread::ManagerUnwalked);
    };
    let others = managers
        .iter()
        .filter(|manager| !std::ptr::eq(*manager, active))
        .filter(holds_lines)
        .map(|manager| {
            manager.lines.lines().map(|lines| {
                lines
                    .iter()
                    .filter_map(|text| match line::parse(text) {
                        Parsed::Line(line) => Some(line),
                        Parsed::Comment | Parsed::Malformed => None,
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let parsed = texts
        .iter()
        .filter_map(|text| match line::parse(text) {
            Parsed::Comment => None,
            Parsed::Line(line) => Some(Ok(line)),
            Parsed::Malformed => Some(Err((Unread::Malformed, text.trim().to_owned()))),
        })
        .collect::<Vec<_>>();
    let mut assigned = HashMap::<&str, usize>::new();
    for line in parsed.iter().flatten() {
        *assigned.entry(line.name.as_str()).or_default() += 1;
    }
    let twice = assigned
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(name, _)| (*name).to_owned())
        .collect::<HashSet<_>>();
    Ok(parsed
        .into_iter()
        .map(|line| {
            let line = line?;
            if twice.contains(&line.name) {
                return Err((Unread::AssignedTwice, line.name));
            }
            if others.iter().any(Option::is_none) {
                return Err((Unread::OtherManagerUnwalked, line.name));
            }
            let confirmed = others.iter().flatten().all(|lines| {
                let mut same = lines.iter().filter(|other| other.name == line.name);
                matches!(
                    (same.next(), same.next()),
                    (Some(other), None) if other.expression == line.expression
                )
            });
            if confirmed {
                Ok(line)
            } else {
                Err((Unread::ConfigurationsDiffer, line.name))
            }
        })
        .collect())
}

/// The expression a line states for parameter `target`, and why it is
/// `Source` where it is.
fn expression(
    parameters: &[DesignParameter],
    target: usize,
    line: &Line,
    names: &Names,
    linking: &Linking<'_>,
) -> (Expression, Option<Kept>) {
    let resolved = line
        .segments
        .iter()
        .map(|segment| match segment {
            Segment::Name(name) => names.resolve(name).map(Some),
            Segment::Text(_) => Some(None),
        })
        .collect::<Option<Vec<_>>>();
    let Some(resolved) = resolved else {
        return (source(parameters, line, names), Some(Kept::NameUnresolved));
    };
    let reads = resolved.iter().flatten().copied().collect::<Vec<_>>();
    let tree = translate::tree(
        &line.expression,
        &translate::Typing {
            parameters,
            names,
            target: &parameters[target].kind,
            length_unit: linking.length_unit,
        },
    );
    let tree = tree.filter(|tree| {
        expr::print(tree)
            .ok()
            .and_then(|wire| expr::read(&wire).ok())
            .as_ref()
            == Some(tree)
    });
    let Some(tree) = tree else {
        let why = if matches!(line.segments.as_slice(), [Segment::Name(_)]) {
            Kept::BareReference
        } else {
            Kept::Untranslated
        };
        return (source(parameters, line, names), Some(why));
    };
    if matches!(&tree, Node::Reference(_)) {
        return (Expression::Neutral(Neutral(tree)), None);
    }
    match test(parameters, target, &tree, &reads, linking.tolerances) {
        Test::Agrees => (Expression::Neutral(Neutral(tree)), None),
        Test::Untested => (source(parameters, line, names), Some(Kept::Untested)),
        Test::Disagrees => (source(parameters, line, names), Some(Kept::Disagrees)),
    }
}

/// The line's expression as `Source` tokens, each name that resolves a
/// reference. A token list with no text, a bare reference, is the whole text
/// as one `Text` token: a `Source` expression holds a `Text` token (part 10).
fn source(parameters: &[DesignParameter], line: &Line, names: &Names) -> Expression {
    let mut tokens = Vec::new();
    for segment in &line.segments {
        match segment {
            Segment::Name(name) => match names.resolve(name) {
                Some(index) => {
                    tokens.push(ExpressionToken::Reference(reference(&parameters[index])));
                }
                None => push_text(&mut tokens, &line::quoted(name)),
            },
            Segment::Text(text) => push_text(&mut tokens, text),
        }
    }
    if !tokens
        .iter()
        .any(|token| matches!(token, ExpressionToken::Text(_)))
    {
        tokens = vec![ExpressionToken::Text(line.expression.clone())];
    }
    Expression::Source {
        dialect: ExpressionDialect::SolidWorks,
        tokens,
    }
}

fn push_text(tokens: &mut Vec<ExpressionToken<ParameterRef>>, text: &str) {
    if let Some(ExpressionToken::Text(last)) = tokens.last_mut() {
        last.push_str(text);
    } else {
        tokens.push(ExpressionToken::Text(text.to_owned()));
    }
}

fn reference(parameter: &DesignParameter) -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(parameter.id.clone()),
        through: None,
    }
}

/// The outcome of setting a tree against the document's caches.
enum Test {
    Agrees,
    Disagrees,
    /// The parameter or a parameter the tree reads has no cache.
    Untested,
}

/// The tree's evaluation over the caches of the parameters it reads, against
/// the cache of the parameter it defines, compared as part 10 compares a
/// cache with its expression.
fn test(
    parameters: &[DesignParameter],
    target: usize,
    tree: &Node<ParameterRef>,
    reads: &[usize],
    tolerances: &Tolerances,
) -> Test {
    let Some(stored) = cached(&parameters[target]) else {
        return Test::Untested;
    };
    if reads
        .iter()
        .any(|&read| cached(&parameters[read]).is_none())
    {
        return Test::Untested;
    }
    let by_id = reads
        .iter()
        .map(|&read| (parameters[read].id.as_str(), read))
        .collect::<HashMap<_, _>>();
    let mut value_of = |reference: &ParameterRef| match &reference.target {
        RefTarget::Parameter(id) => by_id
            .get(id.as_str())
            .and_then(|&read| cached(&parameters[read])),
        RefTarget::External { .. } => None,
    };
    match evaluate(tree, &mut value_of) {
        Ok(evaluated) if agree(&evaluated, &stored, tolerances) => Test::Agrees,
        Ok(_) | Err(_) => Test::Disagrees,
    }
}

/// A parameter's cache as an expression value of its kind.
fn cached(parameter: &DesignParameter) -> Option<Value> {
    match (&parameter.kind, parameter.value.as_ref()?) {
        (ValueKind::Quantity(quantity), ParameterValue::Number(value)) => Some(Value::Number {
            value: value.get(),
            quantity: *quantity,
        }),
        (ValueKind::Integer, ParameterValue::Integer(value)) => Some(Value::Number {
            value: cadmpeg_core::convert::f64_from_i64(*value)?,
            quantity: Quantity::REAL,
        }),
        _ => None,
    }
}

/// Whether two values agree: a length within the linear tolerance, an angle
/// within the angular tolerance, another number within a relative 1e-9.
fn agree(left: &Value, right: &Value, tolerances: &Tolerances) -> bool {
    let (
        Value::Number {
            value: a,
            quantity: q,
        },
        Value::Number {
            value: b,
            quantity: r,
        },
    ) = (left, right)
    else {
        return false;
    };
    if q != r {
        return false;
    }
    let allowed = match *q {
        Quantity::LENGTH => tolerances.linear.get(),
        Quantity::ANGLE => tolerances.angular.get(),
        _ => RELATIVE * a.abs().max(b.abs()),
    };
    (a - b).abs() <= allowed
}

/// The parameters an equation name reaches: `name@feature` names the
/// dimension `name` of the timeline feature named `feature`, a name without
/// `@` the global variable of that name. A name reaches the one parameter it
/// names exactly, else the one it names ignoring letter case.
pub(super) struct Names {
    exact: HashMap<String, Option<usize>>,
    folded: HashMap<String, Option<usize>>,
}

impl Names {
    fn new(parameters: &[DesignParameter], features: &[Feature]) -> Self {
        let feature_names = features
            .iter()
            .filter_map(|feature| Some((feature.id.as_str(), feature.name.as_deref()?)))
            .collect::<HashMap<_, _>>();
        let mut names = Self {
            exact: HashMap::new(),
            folded: HashMap::new(),
        };
        for (position, parameter) in parameters.iter().enumerate() {
            let Some(name) = parameter.name.as_ref() else {
                continue;
            };
            let key = match &parameter.owner {
                None => name.as_str().to_owned(),
                Some(ParameterOwner::Feature(feature)) => {
                    let Some(feature) = feature_names.get(feature.as_str()) else {
                        continue;
                    };
                    format!("{}@{feature}", name.as_str())
                }
                Some(ParameterOwner::Container(_) | ParameterOwner::Attribute(_)) => continue,
            };
            insert(&mut names.folded, key.to_lowercase(), position);
            insert(&mut names.exact, key, position);
        }
        names
    }

    /// Whether a parameter, or two, bear `name`, ignoring letter case.
    fn knows(&self, name: &str) -> bool {
        self.folded.contains_key(&name.to_lowercase())
    }

    pub(super) fn resolve(&self, name: &str) -> Option<usize> {
        match self.exact.get(name) {
            Some(found) => *found,
            None => self.folded.get(&name.to_lowercase()).copied().flatten(),
        }
    }
}

fn insert(map: &mut HashMap<String, Option<usize>>, key: String, position: usize) {
    map.entry(key)
        .and_modify(|found| {
            if *found != Some(position) {
                *found = None;
            }
        })
        .or_insert(Some(position));
}

/// The lines not read, or stated as `Source`, by reason.
struct Losses<W> {
    by_reason: Vec<(W, NamedCount)>,
}

impl<W> Default for Losses<W> {
    fn default() -> Self {
        Self {
            by_reason: Vec::new(),
        }
    }
}

impl<W: Copy + PartialEq> Losses<W> {
    fn push(&mut self, why: W, name: &str) {
        match self.by_reason.iter_mut().find(|(reason, _)| *reason == why) {
            Some((_, named)) => named.push(name),
            None => {
                let mut named = NamedCount::default();
                named.push(name);
                self.by_reason.push((why, named));
            }
        }
    }
}

impl Losses<Unread> {
    fn notes(&self, kept: &Losses<Kept>) -> Vec<LossNote> {
        let mut notes = Vec::new();
        for (why, named) in &self.by_reason {
            notes.push(SldprtLossCode::DesignEquationUnread.note(format!(
                "{} equation-manager lines are not read into parameter expressions, because {}: \
                 {named}",
                named.count(),
                why.reason()
            )));
        }
        for (why, named) in &kept.by_reason {
            notes.push(SldprtLossCode::DesignEquationSource.note(format!(
                "{} parameters keep their equation as SolidWorks source text, because {}; a \
                 writer of another format writes their cached value: {named}",
                named.count(),
                why.reason()
            )));
        }
        notes
    }
}

#[cfg(test)]
mod tests;
