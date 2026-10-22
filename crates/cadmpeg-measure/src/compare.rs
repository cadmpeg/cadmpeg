// SPDX-License-Identifier: Apache-2.0
//! ID-free comparison of two measured documents.
//!
//! Bodies are matched by their measured geometry, not by their IDs. Each
//! quantity compares within the tolerance
//! `t = e_a + e_b + u_a + u_b + floor × max(|a|, |b|, L^d)`, with `e` the
//! numeric error, `u` the model uncertainty, `L` the larger document box
//! diagonal and `d` the quantity's dimension. Two bodies are compatible when
//! at least one numeric quantity is compared and every compared quantity
//! agrees. Compatibility over the bodies of both documents splits them into
//! classes; a class whose members are all pairwise compatible pairs its
//! bodies by multiplicity in ID order, and a class that is not is
//! ambiguous.

mod classes;
mod placement;
mod rows;

use std::collections::BTreeMap;

use cadmpeg_ir::CadIr;
use serde::Serialize;

use crate::reason::CompareCause;
use crate::report::{BodyMeasures, MeasureReport};
use crate::select::SurfaceKind;
use crate::tolerance::COMPARE_RELATIVE_FLOOR;
use crate::value::Unit;
use classes::{classes, Class};
use rows::{body_rows, document_rows, BodyComparison, Context};

/// Options of an ID-free comparison.
#[derive(Debug, Clone, Copy)]
pub struct CompareOptions {
    relative_floor: f64,
}

impl CompareOptions {
    /// Options with the relative floor `relative_floor`, which must be
    /// finite, at least 0 and less than 1.
    #[must_use]
    pub fn new(relative_floor: f64) -> Option<Self> {
        (relative_floor.is_finite() && (0.0..1.0).contains(&relative_floor))
            .then_some(Self { relative_floor })
    }

    /// The relative floor of every tolerance.
    #[must_use]
    pub const fn relative_floor(&self) -> f64 {
        self.relative_floor
    }
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            relative_floor: COMPARE_RELATIVE_FLOOR,
        }
    }
}

/// The verdict of a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Every compared quantity agrees and every body is paired.
    Agree,
    /// A compared quantity disagrees or a body has no partner.
    Disagree,
    /// Nothing disagrees, but a cause keeps the comparison from deciding.
    Inconclusive,
}

/// How a document places its bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementClass {
    /// Each body is placed once, as in model space.
    Trivial,
    /// Occurrences place bodies; model-space bodies are not instances.
    Occurrences,
}

/// The placement class of each document.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Placements {
    a: PlacementClass,
    b: PlacementClass,
}

impl Placements {
    /// The first document's placement.
    #[must_use]
    pub const fn a(&self) -> PlacementClass {
        self.a
    }

    /// The second document's placement.
    #[must_use]
    pub const fn b(&self) -> PlacementClass {
        self.b
    }
}

/// How one row compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Scalars or components within the tolerance.
    Equal,
    /// Each side's boundary box inside the other side's box widened by the
    /// tolerance.
    Containment,
    /// Booleans, counts and reason codes.
    Exact,
    /// One part of a body signature.
    Signature,
    /// The row is not compared; `cause` states why.
    NotCompared,
}

/// One side's value in a row.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Operand {
    /// A number in the row's unit.
    Number(f64),
    /// A boolean.
    Flag(bool),
    /// A count.
    Count(usize),
    /// A name or code.
    Text(String),
    /// A sorted set of codes.
    Texts(Vec<String>),
}

/// One compared quantity.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    quantity: String,
    unit: Option<Unit>,
    a: Option<Operand>,
    b: Option<Operand>,
    difference: Option<f64>,
    tolerance: Option<f64>,
    rule: Rule,
    agrees: Option<bool>,
    cause: Option<CompareCause>,
}

impl Row {
    /// The quantity name, with a component suffix for vectors.
    #[must_use]
    pub fn quantity(&self) -> &str {
        &self.quantity
    }

    /// The unit of the operands, difference and tolerance.
    #[must_use]
    pub const fn unit(&self) -> Option<Unit> {
        self.unit
    }

    /// The first document's value.
    #[must_use]
    pub const fn a(&self) -> Option<&Operand> {
        self.a.as_ref()
    }

    /// The second document's value.
    #[must_use]
    pub const fn b(&self) -> Option<&Operand> {
        self.b.as_ref()
    }

    /// The difference of a numeric row, or the largest violation of a
    /// containment row.
    #[must_use]
    pub const fn difference(&self) -> Option<f64> {
        self.difference
    }

    /// The tolerance of a numeric row.
    #[must_use]
    pub const fn tolerance(&self) -> Option<f64> {
        self.tolerance
    }

    /// How the row compares.
    #[must_use]
    pub const fn rule(&self) -> Rule {
        self.rule
    }

    /// Whether the row agrees; `None` when it is not compared.
    #[must_use]
    pub const fn agrees(&self) -> Option<bool> {
        self.agrees
    }

    /// Why the row is not compared.
    #[must_use]
    pub const fn cause(&self) -> Option<CompareCause> {
        self.cause
    }

    /// Whether the row compares a numeric quantity.
    const fn numeric(&self) -> bool {
        matches!(self.rule, Rule::Equal | Rule::Containment)
    }
}

/// Facts of a pair that never affect the verdict.
#[derive(Debug, Clone, Serialize)]
pub struct Informational {
    surface_kinds_a: BTreeMap<SurfaceKind, usize>,
    surface_kinds_b: BTreeMap<SurfaceKind, usize>,
    visible_a: Option<bool>,
    visible_b: Option<bool>,
}

/// Two paired bodies and their compared quantities.
#[derive(Debug, Clone, Serialize)]
pub struct Pair {
    a: String,
    b: String,
    quantities: Vec<Row>,
    informational: Informational,
}

impl Pair {
    /// The first document's body.
    #[must_use]
    pub fn a(&self) -> &str {
        &self.a
    }

    /// The second document's body.
    #[must_use]
    pub fn b(&self) -> &str {
        &self.b
    }

    /// The compared quantities.
    #[must_use]
    pub fn quantities(&self) -> &[Row] {
        &self.quantities
    }
}

/// A class of bodies whose compatibility is not transitive.
#[derive(Debug, Clone, Serialize)]
pub struct Ambiguous {
    a: Vec<String>,
    b: Vec<String>,
}

impl Ambiguous {
    /// The class's bodies of the first document.
    #[must_use]
    pub fn a(&self) -> &[String] {
        &self.a
    }

    /// The class's bodies of the second document.
    #[must_use]
    pub fn b(&self) -> &[String] {
        &self.b
    }
}

/// The result of an ID-free comparison.
#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    verdict: Verdict,
    relative_floor: f64,
    placement: Placements,
    document: Vec<Row>,
    pairs: Vec<Pair>,
    unmatched_a: Vec<String>,
    unmatched_b: Vec<String>,
    unmeasured_a: Vec<String>,
    unmeasured_b: Vec<String>,
    ambiguous: Vec<Ambiguous>,
    inconclusive_causes: Vec<CompareCause>,
}

impl Comparison {
    /// The verdict.
    #[must_use]
    pub const fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// The relative floor of every tolerance.
    #[must_use]
    pub const fn relative_floor(&self) -> f64 {
        self.relative_floor
    }

    /// How each document places its bodies.
    #[must_use]
    pub const fn placement(&self) -> Placements {
        self.placement
    }

    /// The document rows.
    #[must_use]
    pub fn document(&self) -> &[Row] {
        &self.document
    }

    /// The paired bodies, in the first document's ID order.
    #[must_use]
    pub fn pairs(&self) -> &[Pair] {
        &self.pairs
    }

    /// Bodies of the first document with no partner.
    #[must_use]
    pub fn unmatched_a(&self) -> &[String] {
        &self.unmatched_a
    }

    /// Bodies of the second document with no partner.
    #[must_use]
    pub fn unmatched_b(&self) -> &[String] {
        &self.unmatched_b
    }

    /// Bodies of the first document with no comparable quantity.
    #[must_use]
    pub fn unmeasured_a(&self) -> &[String] {
        &self.unmeasured_a
    }

    /// Bodies of the second document with no comparable quantity.
    #[must_use]
    pub fn unmeasured_b(&self) -> &[String] {
        &self.unmeasured_b
    }

    /// Classes whose compatibility is not transitive.
    #[must_use]
    pub fn ambiguous(&self) -> &[Ambiguous] {
        &self.ambiguous
    }

    /// The causes of an inconclusive verdict, sorted.
    #[must_use]
    pub fn inconclusive_causes(&self) -> &[CompareCause] {
        &self.inconclusive_causes
    }
}

/// The larger diagonal of the two document boxes.
fn scale(a: &MeasureReport, b: &MeasureReport) -> f64 {
    [a, b]
        .iter()
        .filter_map(|report| report.document().bbox().as_ref())
        .filter_map(|bbox| bbox.measured())
        .map(|bbox| bbox.value().min.get().distance(bbox.value().max.get()))
        .fold(0.0, f64::max)
}

/// Compare two documents measured in model space. `a_ir` and `b_ir` supply
/// the occurrence structure that decides whether model-space bodies are the
/// documents' instances.
#[must_use]
pub fn compare(
    a_ir: &CadIr,
    a: &MeasureReport,
    b_ir: &CadIr,
    b: &MeasureReport,
    options: &CompareOptions,
) -> Comparison {
    let placement = Placements {
        a: placement::class(a_ir),
        b: placement::class(b_ir),
    };
    let context = Context {
        floor: options.relative_floor,
        scale: scale(a, b),
    };
    let bodies: Vec<(bool, &BodyMeasures)> = a
        .bodies()
        .iter()
        .map(|body| (false, body))
        .chain(b.bodies().iter().map(|body| (true, body)))
        .collect();
    let count = bodies.len();
    let mut comparisons: BTreeMap<(usize, usize), BodyComparison> = BTreeMap::new();
    for first in 0..count {
        for second in first + 1..count {
            comparisons.insert(
                (first, second),
                body_rows(bodies[first].1, bodies[second].1, &context),
            );
        }
    }
    let compatible = |first: usize, second: usize| {
        let key = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        comparisons
            .get(&key)
            .is_some_and(|comparison| comparison.compatible)
    };
    let sides: Vec<bool> = bodies.iter().map(|(side, _)| *side).collect();
    let measurable: Vec<bool> = bodies
        .iter()
        .map(|(_, body)| rows::measurable(body))
        .collect();
    let found = classes(&sides, &measurable, &compatible);
    let id = |index: usize| bodies[index].1.body().to_owned();

    let mut causes: Vec<CompareCause> = Vec::new();
    let mut pairs = Vec::new();
    let mut unmatched = [Vec::new(), Vec::new()];
    let mut unmeasured = [Vec::new(), Vec::new()];
    let mut ambiguous = Vec::new();
    for class in found {
        match class {
            Class::Paired(paired) => {
                for (first, second) in paired {
                    let Some(comparison) = comparisons.remove(&(first, second)) else {
                        continue;
                    };
                    let (body_a, body_b) = (bodies[first].1, bodies[second].1);
                    pairs.push(Pair {
                        a: id(first),
                        b: id(second),
                        quantities: comparison.rows,
                        informational: Informational {
                            surface_kinds_a: body_a.surface_kinds().clone(),
                            surface_kinds_b: body_b.surface_kinds().clone(),
                            visible_a: *body_a.visible(),
                            visible_b: *body_b.visible(),
                        },
                    });
                }
            }
            Class::Unmatched(index) => unmatched[usize::from(sides[index])].push(id(index)),
            Class::Unmeasured(index) => {
                causes.push(CompareCause::UnmeasuredBody);
                unmeasured[usize::from(sides[index])].push(id(index));
            }
            Class::Ambiguous(members) => {
                causes.push(CompareCause::AmbiguousClass);
                let (mut class_a, mut class_b) = (Vec::new(), Vec::new());
                for index in members {
                    if sides[index] {
                        class_b.push(id(index));
                    } else {
                        class_a.push(id(index));
                    }
                }
                class_a.sort();
                class_b.sort();
                ambiguous.push(Ambiguous {
                    a: class_a,
                    b: class_b,
                });
            }
        }
    }
    pairs.sort_by(|left, right| left.a.cmp(&right.a));
    for list in unmatched.iter_mut().chain(unmeasured.iter_mut()) {
        list.sort();
    }
    let document = document_rows(a, b, &context, placement);
    let rows = pairs
        .iter()
        .flat_map(|pair| pair.quantities.iter())
        .chain(document.iter());
    let mut disagrees = !unmatched[0].is_empty() || !unmatched[1].is_empty();
    let mut compared = false;
    for row in rows {
        if let Some(cause) = row.cause {
            causes.push(cause);
        }
        if row.agrees == Some(false) {
            disagrees = true;
        }
        if row.agrees.is_some() && (row.numeric() || row.rule == Rule::Signature) {
            compared = true;
        }
    }
    if !compared {
        causes.push(CompareCause::NothingCompared);
    }
    causes.sort();
    causes.dedup();
    let verdict = if disagrees {
        Verdict::Disagree
    } else if causes.is_empty() {
        Verdict::Agree
    } else {
        Verdict::Inconclusive
    };
    let [unmatched_a, unmatched_b] = unmatched;
    let [unmeasured_a, unmeasured_b] = unmeasured;
    Comparison {
        verdict,
        relative_floor: options.relative_floor,
        placement,
        document,
        pairs,
        unmatched_a,
        unmatched_b,
        unmeasured_a,
        unmeasured_b,
        ambiguous,
        inconclusive_causes: causes,
    }
}
