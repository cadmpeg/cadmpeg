// SPDX-License-Identifier: Apache-2.0
//! Compared rows of two bodies and of two documents.
//!
//! A value measured on both sides compares within its tolerance. A value
//! measured on one side and unavailable on the other disagrees when the
//! unavailable category is `input` or `undefined`, and is not compared for
//! `budget`, `unsupported` or `placement`. Values unavailable on both sides
//! compare by their candidates of the same reading, else by their reason
//! codes when both categories are `input` or `undefined`.

use cadmpeg_ir::topology::BodyKind;

use super::{Operand, PlacementClass, Placements, Row, Rule};
use crate::reason::{Category, CompareCause};
use crate::report::{BodyMeasures, Histogram, MeasureReport};
use crate::value::{
    BoxKind, BoxValue, Candidate, Measured, Point, Quantity, Reading, Scalar, Unavailable, Unit,
};

/// What every tolerance of one comparison reads.
#[derive(Debug, Clone, Copy)]
pub(super) struct Context {
    /// The relative floor.
    pub(super) floor: f64,
    /// The larger document box diagonal.
    pub(super) scale: f64,
}

impl Context {
    /// `e_a + e_b + u_a + u_b + floor × max(|a|, |b|, L^d)`.
    fn tolerance(&self, dimension: i32, values: [f64; 2], errors: [Errors; 2]) -> f64 {
        let [a, b] = values;
        let magnitude = a.abs().max(b.abs()).max(self.scale.powi(dimension));
        errors[0].numeric
            + errors[0].model
            + errors[1].numeric
            + errors[1].model
            + self.floor * magnitude
    }
}

/// The two error terms of one value.
#[derive(Debug, Clone, Copy)]
struct Errors {
    numeric: f64,
    model: f64,
}

fn measured_errors<T>(value: &Measured<T>) -> Errors {
    Errors {
        numeric: value.numeric_error().get(),
        model: value.model_uncertainty().get(),
    }
}

fn candidate_errors<T>(value: &Candidate<T>) -> Errors {
    Errors {
        numeric: value.numeric_error().get(),
        model: value.model_uncertainty().get(),
    }
}

/// The scalar components a value compares by.
trait Components {
    fn components(&self) -> Vec<(&'static str, f64)>;
}

impl Components for Scalar {
    fn components(&self) -> Vec<(&'static str, f64)> {
        vec![("", self.value.get())]
    }
}

impl Components for Point {
    fn components(&self) -> Vec<(&'static str, f64)> {
        let point = self.value.get();
        vec![(".x", point.x), (".y", point.y), (".z", point.z)]
    }
}

impl Components for BoxValue {
    fn components(&self) -> Vec<(&'static str, f64)> {
        let (low, high) = (self.min.get(), self.max.get());
        vec![
            (".min.x", low.x),
            (".min.y", low.y),
            (".min.z", low.z),
            (".max.x", high.x),
            (".max.y", high.y),
            (".max.z", high.z),
        ]
    }
}

/// The serialized name of a reading.
const fn reading_name(reading: Reading) -> &'static str {
    match reading {
        Reading::FaceSense => "face_sense",
        Reading::LoopWinding => "loop_winding",
        Reading::Declared => "declared",
        Reading::Complement => "complement",
    }
}

const fn kind_name(kind: BodyKind) -> &'static str {
    match kind {
        BodyKind::Solid => "solid",
        BodyKind::Sheet => "sheet",
        BodyKind::Wire => "wire",
        BodyKind::General => "general",
    }
}

const fn box_kind_name(kind: BoxKind) -> &'static str {
    match kind {
        BoxKind::Tight => "tight",
        BoxKind::Conservative => "conservative",
    }
}

/// One quantity's measure: its name, unit and dimension.
#[derive(Clone, Copy)]
struct Quantum<'a> {
    name: &'a str,
    unit: Unit,
    dimension: i32,
}

fn not_compared(quantity: &str, unit: Option<Unit>, cause: CompareCause) -> Row {
    Row {
        quantity: quantity.to_owned(),
        unit,
        a: None,
        b: None,
        difference: None,
        tolerance: None,
        rule: Rule::NotCompared,
        agrees: None,
        cause: Some(cause),
    }
}

fn exact(quantity: &str, a: Operand, b: Operand) -> Row {
    let agrees = a == b;
    Row {
        quantity: quantity.to_owned(),
        unit: None,
        a: Some(a),
        b: Some(b),
        difference: None,
        tolerance: None,
        rule: Rule::Exact,
        agrees: Some(agrees),
        cause: None,
    }
}

/// The component rows of two values within their tolerances.
fn equal_rows<T: Components>(
    quantum: Quantum<'_>,
    values: [&T; 2],
    errors: [Errors; 2],
    context: &Context,
    rows: &mut Vec<Row>,
) {
    for ((suffix, a), (_, b)) in values[0]
        .components()
        .into_iter()
        .zip(values[1].components())
    {
        let tolerance = context.tolerance(quantum.dimension, [a, b], errors);
        let difference = (a - b).abs();
        rows.push(Row {
            quantity: format!("{}{suffix}", quantum.name),
            unit: Some(quantum.unit),
            a: Some(Operand::Number(a)),
            b: Some(Operand::Number(b)),
            difference: Some(difference),
            tolerance: Some(tolerance),
            rule: Rule::Equal,
            agrees: Some(difference <= tolerance),
            cause: None,
        });
    }
}

/// Whether an unavailable category keeps the value comparable as a fact of
/// the document.
const fn decided(category: Category) -> bool {
    matches!(category, Category::Input | Category::Undefined)
}

/// The row of a value measured on one side and unavailable on the other.
fn one_sided<T>(quantum: Quantum<'_>, unavailable: &Unavailable<T>, measured_first: bool) -> Row {
    if !decided(unavailable.category()) {
        return not_compared(quantum.name, Some(quantum.unit), CompareCause::Unavailable);
    }
    let measured = Operand::Text("measured".to_owned());
    let code = Operand::Text(unavailable.reason().code().to_owned());
    if measured_first {
        exact(quantum.name, measured, code)
    } else {
        exact(quantum.name, code, measured)
    }
}

/// The rows of one quantity of two bodies or documents.
fn quantity_rows<T: Components>(
    quantum: Quantum<'_>,
    pair: [Option<&Quantity<T>>; 2],
    context: &Context,
    rows: &mut Vec<Row>,
) {
    match pair {
        [None, None] => {}
        [Some(_), None] | [None, Some(_)] => rows.push(exact(
            quantum.name,
            Operand::Flag(pair[0].is_some()),
            Operand::Flag(pair[1].is_some()),
        )),
        [Some(Quantity::Measured(a)), Some(Quantity::Measured(b))] => {
            let (first, second, errors) = same_reading(a, b);
            equal_rows(quantum, [first, second], errors, context, rows);
        }
        [Some(Quantity::Measured(_)), Some(Quantity::Unavailable(b))] => {
            rows.push(one_sided(quantum, b, true));
        }
        [Some(Quantity::Unavailable(a)), Some(Quantity::Measured(_))] => {
            rows.push(one_sided(quantum, a, false));
        }
        [Some(Quantity::Unavailable(a)), Some(Quantity::Unavailable(b))] => {
            let mut found = false;
            for candidate in a.candidates() {
                let Some(other) = b
                    .candidates()
                    .iter()
                    .find(|other| other.reading() == candidate.reading())
                else {
                    continue;
                };
                found = true;
                let name = format!("{}.{}", quantum.name, reading_name(candidate.reading()));
                equal_rows(
                    Quantum {
                        name: &name,
                        ..quantum
                    },
                    [candidate.value(), other.value()],
                    [candidate_errors(candidate), candidate_errors(other)],
                    context,
                    rows,
                );
            }
            if found {
                return;
            }
            if decided(a.category()) && decided(b.category()) {
                rows.push(exact(
                    quantum.name,
                    Operand::Text(a.reason().code().to_owned()),
                    Operand::Text(b.reason().code().to_owned()),
                ));
            } else {
                rows.push(not_compared(
                    quantum.name,
                    Some(quantum.unit),
                    CompareCause::Unavailable,
                ));
            }
        }
    }
}

/// The two values to compare: the main values when their readings match,
/// else a value under the other side's reading when one side has it as a
/// candidate.
fn same_reading<'a, T>(a: &'a Measured<T>, b: &'a Measured<T>) -> (&'a T, &'a T, [Errors; 2]) {
    let main = (
        a.value(),
        b.value(),
        [measured_errors(a), measured_errors(b)],
    );
    if a.reading() == b.reading() {
        return main;
    }
    if let Some(reading) = a.reading() {
        if let Some(other) = b.candidates().iter().find(|c| c.reading() == reading) {
            return (
                a.value(),
                other.value(),
                [measured_errors(a), candidate_errors(other)],
            );
        }
    }
    if let Some(reading) = b.reading() {
        if let Some(other) = a.candidates().iter().find(|c| c.reading() == reading) {
            return (
                other.value(),
                b.value(),
                [candidate_errors(other), measured_errors(b)],
            );
        }
    }
    main
}

/// The lower-bound box of a measured box: its boundary box, or the box
/// itself when it is tight.
fn lower_bound(value: &BoxValue) -> Option<([f64; 3], [f64; 3])> {
    let corners = |low: cadmpeg_ir::math::Point3, high: cadmpeg_ir::math::Point3| {
        ([low.x, low.y, low.z], [high.x, high.y, high.z])
    };
    match (&value.boundary, value.kind) {
        (Some(boundary), _) => Some(corners(boundary.min.get(), boundary.max.get())),
        (None, BoxKind::Tight) => Some(corners(value.min.get(), value.max.get())),
        (None, BoxKind::Conservative) => None,
    }
}

/// The rows of two boxes: equal corners when both are tight, else
/// containment of each side's lower bound in the other side's box.
fn bbox_rows(pair: [&Quantity<BoxValue>; 2], context: &Context, rows: &mut Vec<Row>) {
    let quantum = Quantum {
        name: "bbox",
        unit: Unit::Millimeter,
        dimension: 1,
    };
    let [Quantity::Measured(a), Quantity::Measured(b)] = pair else {
        quantity_rows(quantum, [Some(pair[0]), Some(pair[1])], context, rows);
        return;
    };
    let errors = [measured_errors(a), measured_errors(b)];
    if a.value().kind == BoxKind::Tight && b.value().kind == BoxKind::Tight {
        equal_rows(quantum, [a.value(), b.value()], errors, context, rows);
        return;
    }
    let outer = |value: &BoxValue| {
        let (low, high) = (value.min.get(), value.max.get());
        ([low.x, low.y, low.z], [high.x, high.y, high.z])
    };
    let mut violation: f64 = 0.0;
    let mut magnitude: f64 = 0.0;
    for (inner, other) in [(a.value(), b.value()), (b.value(), a.value())] {
        let Some((inner_low, inner_high)) = lower_bound(inner) else {
            continue;
        };
        let (low, high) = outer(other);
        for axis in 0..3 {
            violation = violation
                .max(low[axis] - inner_low[axis])
                .max(inner_high[axis] - high[axis]);
            magnitude = magnitude.max(low[axis].abs()).max(high[axis].abs());
        }
    }
    let tolerance = context.tolerance(1, [magnitude, 0.0], errors);
    rows.push(Row {
        quantity: "bbox".to_owned(),
        unit: Some(Unit::Millimeter),
        a: Some(Operand::Text(box_kind_name(a.value().kind).to_owned())),
        b: Some(Operand::Text(box_kind_name(b.value().kind).to_owned())),
        difference: Some(violation),
        tolerance: Some(tolerance),
        rule: Rule::Containment,
        agrees: Some(violation <= tolerance),
        cause: None,
    });
}

/// Whether a body has a measured numeric quantity.
fn has_numeric(body: &BodyMeasures) -> bool {
    let measured =
        |quantity: Option<&Quantity<Scalar>>| quantity.is_some_and(|q| q.measured().is_some());
    let point =
        |quantity: Option<&Quantity<Point>>| quantity.is_some_and(|q| q.measured().is_some());
    measured(body.area().as_ref())
        || measured(body.volume().as_ref())
        || measured(body.length().as_ref())
        || point(body.centroid().as_ref())
        || point(body.area_centroid().as_ref())
        || body.bbox().measured().is_some()
}

/// Whether a body compares by its signature: no measured numeric quantity,
/// and every unavailable value of category `input` or `undefined`.
fn signature_only(body: &BodyMeasures) -> bool {
    if has_numeric(body) {
        return false;
    }
    let mut histogram = Histogram::default();
    body.visit(&mut histogram);
    histogram.0.keys().all(|category| decided(*category))
}

/// Whether a body has a quantity another body can compare with.
pub(super) fn measurable(body: &BodyMeasures) -> bool {
    has_numeric(body) || signature_only(body)
}

/// The compared rows of two bodies and whether the bodies are compatible.
pub(super) struct BodyComparison {
    pub(super) rows: Vec<Row>,
    pub(super) compatible: bool,
}

fn closed(body: &BodyMeasures) -> bool {
    !body.shells().is_empty() && body.shells().iter().all(|shell| *shell.closed())
}

/// The signature rows of two bodies with no measured numeric quantity.
fn signature_rows(a: &BodyMeasures, b: &BodyMeasures) -> Vec<Row> {
    let row = |quantity: &str, left: Operand, right: Operand| {
        let agrees = left == right;
        Row {
            quantity: quantity.to_owned(),
            unit: None,
            a: Some(left),
            b: Some(right),
            difference: None,
            tolerance: None,
            rule: Rule::Signature,
            agrees: Some(agrees),
            cause: None,
        }
    };
    let codes = |body: &BodyMeasures| {
        Operand::Texts(
            body.unavailable_codes()
                .into_iter()
                .map(str::to_owned)
                .collect(),
        )
    };
    vec![
        row(
            "signature.kind",
            Operand::Text(kind_name(*a.kind()).to_owned()),
            Operand::Text(kind_name(*b.kind()).to_owned()),
        ),
        row(
            "signature.faces",
            Operand::Count(*a.faces()),
            Operand::Count(*b.faces()),
        ),
        row(
            "signature.edges",
            Operand::Count(*a.edges()),
            Operand::Count(*b.edges()),
        ),
        row(
            "signature.vertices",
            Operand::Count(*a.vertices()),
            Operand::Count(*b.vertices()),
        ),
        row("signature.reasons", codes(a), codes(b)),
    ]
}

/// The compared rows of two bodies.
pub(super) fn body_rows(a: &BodyMeasures, b: &BodyMeasures, context: &Context) -> BodyComparison {
    if signature_only(a) && signature_only(b) {
        let rows = signature_rows(a, b);
        let compatible = rows.iter().all(|row| row.agrees == Some(true));
        return BodyComparison { rows, compatible };
    }
    let mut rows = Vec::new();
    let area = Quantum {
        name: "area",
        unit: Unit::SquareMillimeter,
        dimension: 2,
    };
    quantity_rows(
        area,
        [a.area().as_ref(), b.area().as_ref()],
        context,
        &mut rows,
    );
    let volume = Quantum {
        name: "volume",
        unit: Unit::CubicMillimeter,
        dimension: 3,
    };
    quantity_rows(
        volume,
        [a.volume().as_ref(), b.volume().as_ref()],
        context,
        &mut rows,
    );
    for (name, pair) in [
        ("centroid", [a.centroid().as_ref(), b.centroid().as_ref()]),
        (
            "area_centroid",
            [a.area_centroid().as_ref(), b.area_centroid().as_ref()],
        ),
    ] {
        let quantum = Quantum {
            name,
            unit: Unit::Millimeter,
            dimension: 1,
        };
        quantity_rows(quantum, pair, context, &mut rows);
    }
    let length = Quantum {
        name: "length",
        unit: Unit::Millimeter,
        dimension: 1,
    };
    quantity_rows(
        length,
        [a.length().as_ref(), b.length().as_ref()],
        context,
        &mut rows,
    );
    bbox_rows([a.bbox(), b.bbox()], context, &mut rows);
    rows.push(exact(
        "closed",
        Operand::Flag(closed(a)),
        Operand::Flag(closed(b)),
    ));
    if let (Some(first), Some(second)) =
        (*a.orientation().consistent(), *b.orientation().consistent())
    {
        rows.push(exact(
            "orientation.consistent",
            Operand::Flag(first),
            Operand::Flag(second),
        ));
    }
    let comparable = rows.iter().any(|row| row.numeric() && row.agrees.is_some());
    let compatible = comparable && rows.iter().all(|row| row.agrees != Some(false));
    BodyComparison { rows, compatible }
}

/// The document rows: body count, total area, closed volume, union box and
/// occurrence placement.
pub(super) fn document_rows(
    a: &MeasureReport,
    b: &MeasureReport,
    context: &Context,
    placement: Placements,
) -> Vec<Row> {
    let mut rows = vec![exact(
        "body_count",
        Operand::Count(a.bodies().len()),
        Operand::Count(b.bodies().len()),
    )];
    let (first, second) = (a.document(), b.document());
    if let (Some(left), Some(right)) = (first.area().as_ref(), second.area().as_ref()) {
        if left.measured().is_some() && right.measured().is_some() {
            let quantum = Quantum {
                name: "area",
                unit: Unit::SquareMillimeter,
                dimension: 2,
            };
            quantity_rows(quantum, [Some(left), Some(right)], context, &mut rows);
        }
    }
    if let (Some(left), Some(right)) = (
        first.closed_volume().as_ref(),
        second.closed_volume().as_ref(),
    ) {
        if left.measured().is_some() && right.measured().is_some() {
            let quantum = Quantum {
                name: "closed_volume",
                unit: Unit::CubicMillimeter,
                dimension: 3,
            };
            quantity_rows(quantum, [Some(left), Some(right)], context, &mut rows);
        }
    }
    if let (Some(left), Some(right)) = (first.bbox().as_ref(), second.bbox().as_ref()) {
        bbox_rows([left, right], context, &mut rows);
    }
    let class = |class: PlacementClass| match class {
        PlacementClass::Trivial => "trivial",
        PlacementClass::Occurrences => "occurrences",
    };
    if placement.a == PlacementClass::Trivial && placement.b == PlacementClass::Trivial {
        rows.push(exact(
            "occurrences",
            Operand::Text(class(placement.a).to_owned()),
            Operand::Text(class(placement.b).to_owned()),
        ));
    } else {
        let mut row = not_compared("occurrences", None, CompareCause::OccurrencePlacement);
        row.a = Some(Operand::Text(class(placement.a).to_owned()));
        row.b = Some(Operand::Text(class(placement.b).to_owned()));
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests;
