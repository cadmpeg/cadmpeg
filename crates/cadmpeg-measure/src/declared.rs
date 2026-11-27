// SPDX-License-Identifier: Apache-2.0
//! Check findings that compare the quantities a document declares for its
//! bodies with the quantities measured on them.
//!
//! Each declared measure resolves to its member bodies, and the pass
//! measures every member body once. A measured value agrees with the
//! declared value when they differ by at most its numeric error, model
//! uncertainty, and source representation error; a centroid compares by
//! distance. A source declaration or validation property disagreement is an
//! error. A saved-cache disagreement or an unavailable comparison is a
//! warning.

use std::collections::{BTreeMap, BTreeSet};
use crate::{fast_map_with_capacity, FastMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::declared_measures::{
    DeclaredMeasure, DeclaredQuantity, DeclaredTarget, MeasureBasis,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::topology::{Body, BodyKind};
use cadmpeg_ir::CadIr;

use crate::report::{BodyMeasures, MeasureReport, ShellRole};
use crate::select::Selection;
use crate::session::{MeasureError, MeasureInput, MeasureOptions, Measurer};
use crate::value::{Point, Quantity, Scalar};

/// Why a declared measure has no measured counterpart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gap {
    /// The target names no bodies of the document.
    TargetUnresolved,
    /// The target is a state of the design, not the current geometry.
    TargetState,
    /// The target definition has child occurrences.
    TargetAssembly,
    /// A centroid has no member body, or its members weigh nothing.
    NoMembers,
    /// A wetted area covers a body with a void shell.
    VoidShell,
    /// The measurement reports no such quantity for a member body.
    NotReported,
    /// Aggregation cannot produce a finite value and error allowance.
    AggregateUnavailable,
    /// A geometry-level target has a different geometry class.
    GeometryClass,
}

impl Gap {
    const fn code(self) -> &'static str {
        match self {
            Self::TargetUnresolved => "declared.target-unresolved",
            Self::TargetState => "declared.target-state",
            Self::TargetAssembly => "declared.target-assembly",
            Self::NoMembers => "declared.no-members",
            Self::VoidShell => "declared.void-shell",
            Self::NotReported => "declared.not-reported",
            Self::AggregateUnavailable => "declared.aggregate-unavailable",
            Self::GeometryClass => "declared.geometry-class",
        }
    }
}

/// A declared measure with no measured counterpart: the reason code, the
/// entity it names, and human text.
#[derive(Debug)]
struct Unmeasured {
    code: &'static str,
    entity: Option<String>,
    message: String,
}

impl Unmeasured {
    fn gap(gap: Gap, message: impl Into<String>) -> Self {
        Self {
            code: gap.code(),
            entity: None,
            message: message.into(),
        }
    }
}

/// A measured aggregate with its two error terms.
#[derive(Debug, Clone, Copy)]
struct Reading<T> {
    value: T,
    numeric_error: f64,
    model_uncertainty: f64,
}

/// Resolved targets and required body scope for a document's declared measures.
pub(crate) struct ResolvedDeclared<'ir> {
    members: Vec<Result<Vec<&'ir Body>, Unmeasured>>,
    pub(crate) scope: BTreeSet<&'ir str>,
}

/// Resolve each declared measure of `ir` to its member bodies and collect the
/// union of body ids that must be measured.
pub(crate) fn resolve_declared(ir: &CadIr) -> ResolvedDeclared<'_> {
    let mut bodies: FastMap<&str, &Body> = fast_map_with_capacity(ir.geometry.bodies.len());
    for body in &ir.geometry.bodies {
        bodies.insert(body.id.as_str(), body);
    }
    let mut definitions: FastMap<&str, &cadmpeg_ir::products::ProductDefinition> =
        fast_map_with_capacity(ir.model.product_definitions.len());
    for definition in &ir.model.product_definitions {
        definitions.insert(definition.id.as_str(), definition);
    }
    let members: Vec<Result<Vec<&Body>, Unmeasured>> = ir
        .model
        .declared_measures
        .iter()
        .map(|measure| member_bodies(&bodies, &definitions, measure))
        .collect();
    let scope: BTreeSet<&str> = members
        .iter()
        .flatten()
        .flatten()
        .map(|body| body.id.as_str())
        .collect();
    ResolvedDeclared { members, scope }
}

/// Compare each declared measure of `ir` against precomputed body rows.
pub(crate) fn findings_from_rows(
    ir: &CadIr,
    resolved: ResolvedDeclared<'_>,
    rows: &BTreeMap<&str, &BodyMeasures>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (measure, members) in ir.model.declared_measures.iter().zip(resolved.members) {
        let related = members.as_ref().map_or_else(
            |_| target_ids(&measure.target),
            |bodies| {
                if bodies.is_empty() {
                    target_ids(&measure.target)
                } else {
                    bodies.iter().map(|body| body.id.to_string()).collect()
                }
            },
        );
        let outcome = members.and_then(|bodies| compare(measure, &bodies, rows));
        let finding = match outcome {
            Ok(None) => continue,
            Ok(Some(message)) => Finding {
                check: Check::DeclaredMeasure,
                rule: cadmpeg_ir::rule_name!("measure/declared.declared-quantity-differs"),
                severity: match measure.basis {
                    Some(MeasureBasis::Cache) => Severity::Warning,
                    None | Some(MeasureBasis::Validation) => Severity::Error,
                },
                message,
                entity: Some(measure.id.to_string()),
                related,
            },
            Err(unmeasured) => {
                let mut related = related;
                if let Some(entity) = unmeasured.entity {
                    if !related.contains(&entity) {
                        related.push(entity);
                    }
                }
                Finding {
                    rule: cadmpeg_ir::rule_name!("measure/declared.declared-not-compared"),
                    check: Check::DeclaredMeasure,
                    severity: Severity::Warning,
                    message: format!(
                        "declared {} {} is not compared ({}): {}",
                        quantity_description(measure),
                        declared_text(&measure.quantity),
                        unmeasured.code,
                        unmeasured.message
                    ),
                    entity: Some(measure.id.to_string()),
                    related,
                }
            }
        };
        findings.push(finding);
    }
    findings
}

/// The `check` findings that compare each declared measure of `ir` with
/// the measured quantity of its member bodies, under one measurement pass
/// of `options`. A document without declared measures is not measured. A
/// session refusal of `ctx` is the error.
pub fn declared_findings(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    options: MeasureOptions,
) -> Result<Vec<Finding>, CodecError> {
    if ir.model.declared_measures.is_empty() {
        return Ok(Vec::new());
    }
    let resolved = resolve_declared(ir);
    let report = if resolved.scope.is_empty() {
        None
    } else {
        let selection = Selection {
            bodies: resolved
                .scope
                .iter()
                .filter_map(|id| {
                    ir.geometry
                        .bodies
                        .iter()
                        .find(|body| body.id.as_str() == *id)
                        .map(|body| body.id.clone())
                })
                .collect(),
            ..Selection::default()
        };
        let input = MeasureInput {
            ir,
            fidelity: None,
            geometry_transferred: true,
        };
        Some(
            Measurer::new(input, options)
                .measure(ctx, &selection)
                .map_err(|error| match error {
                    MeasureError::Codec(error) => error,
                    other => CodecError::Malformed(other.to_string()),
                })?,
        )
    };
    let rows: BTreeMap<&str, &BodyMeasures> = report
        .iter()
        .flat_map(MeasureReport::bodies)
        .map(|row| (row.body().as_str(), row))
        .collect();
    Ok(findings_from_rows(ir, resolved, &rows))
}

/// The identities the target names.
fn target_ids(target: &DeclaredTarget) -> Vec<String> {
    match target {
        DeclaredTarget::Definition { definition } => vec![definition.to_string()],
        DeclaredTarget::Bodies { bodies } => bodies.iter().map(ToString::to_string).collect(),
        DeclaredTarget::State(_) | DeclaredTarget::Unresolved => Vec::new(),
    }
}

/// The bodies a declared measure covers.
fn member_bodies<'ir>(
    bodies_by_id: &FastMap<&'ir str, &'ir Body>,
    definitions_by_id: &FastMap<&'ir str, &'ir cadmpeg_ir::products::ProductDefinition>,
    measure: &DeclaredMeasure,
) -> Result<Vec<&'ir Body>, Unmeasured> {
    let body = |id: &BodyId| bodies_by_id.get(id.as_str()).copied();
    match &measure.target {
        DeclaredTarget::Unresolved => Err(Unmeasured::gap(
            Gap::TargetUnresolved,
            "the source names a target that is not a body or a definition of the document",
        )),
        DeclaredTarget::State(_) => Err(Unmeasured::gap(
            Gap::TargetState,
            "the target is a state of the design, whose bodies the current geometry need not hold",
        )),
        DeclaredTarget::Bodies { bodies } => {
            if bodies.is_empty() {
                return Err(Unmeasured::gap(
                    Gap::TargetUnresolved,
                    "the target lists no bodies",
                ));
            }
            bodies
                .iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|id| {
                    body(id).ok_or_else(|| {
                        Unmeasured::gap(
                            Gap::TargetUnresolved,
                            format!("the target body {id} is not in the document"),
                        )
                    })
                })
                .collect()
        }
        DeclaredTarget::Definition { definition } => {
            let Some(found) = definitions_by_id.get(definition.as_str()).copied() else {
                return Err(Unmeasured::gap(
                    Gap::TargetUnresolved,
                    format!("the target definition {definition} is not in the document"),
                ));
            };
            if !found.occurrences.is_empty() {
                return Err(Unmeasured::gap(
                    Gap::TargetAssembly,
                    format!("the target definition {definition} places child occurrences"),
                ));
            }
            let class = match measure.quantity {
                DeclaredQuantity::Volume { .. }
                | DeclaredQuantity::SolidArea { .. }
                | DeclaredQuantity::WettedArea { .. }
                | DeclaredQuantity::SolidCentroid { .. } => BodyKind::Solid,
                DeclaredQuantity::SheetArea { .. } | DeclaredQuantity::SheetCentroid { .. } => {
                    BodyKind::Sheet
                }
            };
            Ok(found
                .bodies
                .iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter_map(body)
                .filter(|body| body.kind == class && body.visible != Some(false))
                .collect())
        }
    }
}

/// `None` when the measured quantity agrees with the declared one, the
/// disagreement text when it does not.
fn compare(
    measure: &DeclaredMeasure,
    bodies: &[&Body],
    rows: &BTreeMap<&str, &BodyMeasures>,
) -> Result<Option<String>, Unmeasured> {
    if bodies.is_empty() {
        return Err(Unmeasured::gap(
            Gap::NoMembers,
            "the target has no member body of the declared geometry class",
        ));
    }
    let class = match measure.quantity {
        DeclaredQuantity::SheetArea { .. } | DeclaredQuantity::SheetCentroid { .. } => {
            BodyKind::Sheet
        }
        _ => BodyKind::Solid,
    };
    if let Some(body) = bodies.iter().find(|body| {
        body.kind != class && !matches!(measure.quantity, DeclaredQuantity::Volume { .. })
    }) {
        return Err(Unmeasured {
            code: Gap::GeometryClass.code(),
            entity: Some(body.id.to_string()),
            message: format!(
                "{} has a different geometry class from the declared quantity",
                body.id
            ),
        });
    }
    let rows: Vec<&BodyMeasures> = bodies
        .iter()
        .map(|body| {
            rows.get(body.id.as_str())
                .copied()
                .ok_or_else(|| Unmeasured {
                    code: Gap::TargetUnresolved.code(),
                    entity: Some(body.id.to_string()),
                    message: format!("the measurement pass has no row for {}", body.id),
                })
        })
        .collect::<Result<_, _>>()?;
    let name = quantity_description(measure);
    let source_error = measure
        .precision
        .map_or(0.0, cadmpeg_ir::scalar::NonNegativeReal::get);
    match &measure.quantity {
        DeclaredQuantity::Volume { value } => {
            let measured = sum(&rows, VOLUME)?;
            scalar_disagreement(&name, value.get(), measured, "mm3", source_error)
        }
        DeclaredQuantity::SolidArea { value } | DeclaredQuantity::SheetArea { value } => {
            let measured = sum(&rows, AREA)?;
            scalar_disagreement(&name, value.get(), measured, "mm2", source_error)
        }
        DeclaredQuantity::WettedArea { value } => {
            if let Some(row) = rows.iter().find(|row| {
                row.shells()
                    .iter()
                    .any(|shell| *shell.role() == ShellRole::Void)
            }) {
                return Err(Unmeasured {
                    code: Gap::VoidShell.code(),
                    entity: Some(row.body().clone()),
                    message: format!(
                        "{} has a void shell; its measured area includes the void faces",
                        row.body()
                    ),
                });
            }
            let measured = sum(&rows, AREA)?;
            scalar_disagreement(&name, value.get(), measured, "mm2", source_error)
        }
        DeclaredQuantity::SolidCentroid { value } => {
            let measured = centroid(&rows, VOLUME, CENTROID)?;
            point_disagreement(&name, value.get(), measured, point_error(source_error))
        }
        DeclaredQuantity::SheetCentroid { value } => {
            let measured = centroid(&rows, AREA, AREA_CENTROID)?;
            point_disagreement(&name, value.get(), measured, point_error(source_error))
        }
    }
}

/// A body-row quantity and its name.
type Field<T> = (&'static str, fn(&BodyMeasures) -> Option<&Quantity<T>>);

const VOLUME: Field<Scalar> = ("volume", |row| row.volume().as_ref());
const AREA: Field<Scalar> = ("area", |row| row.area().as_ref());
const CENTROID: Field<Point> = ("centroid", |row| row.centroid().as_ref());
const AREA_CENTROID: Field<Point> = ("area centroid", |row| row.area_centroid().as_ref());

/// The measured value of `quantity` on `row`, or the reason it has none.
fn measured<'a, T>(
    row: &BodyMeasures,
    name: &str,
    quantity: Option<&'a Quantity<T>>,
) -> Result<&'a crate::value::Measured<T>, Unmeasured> {
    match quantity {
        Some(Quantity::Measured(measured)) => Ok(measured),
        Some(Quantity::Unavailable(unavailable)) => Err(Unmeasured {
            code: unavailable.reason().code(),
            entity: Some(
                unavailable
                    .entity()
                    .map_or_else(|| row.body().clone(), str::to_owned),
            ),
            message: format!("{}: {}", row.body(), unavailable.message()),
        }),
        None => Err(Unmeasured {
            code: Gap::NotReported.code(),
            entity: Some(row.body().clone()),
            message: format!("the measurement reports no {name} for {}", row.body()),
        }),
    }
}

/// The sum of a scalar over the member rows; errors add.
fn sum(
    rows: &[&BodyMeasures],
    (name, quantity): Field<Scalar>,
) -> Result<Reading<f64>, Unmeasured> {
    let mut total = Reading {
        value: 0.0,
        numeric_error: 0.0,
        model_uncertainty: 0.0,
    };
    for row in rows {
        let value = measured(row, name, quantity(row))?;
        total.value += value.value().value.get();
        total.numeric_error += value.numeric_error().get();
        total.model_uncertainty += value.model_uncertainty().get();
    }
    if !total.value.is_finite()
        || !total.numeric_error.is_finite()
        || !total.model_uncertainty.is_finite()
    {
        return Err(Unmeasured::gap(
            Gap::AggregateUnavailable,
            "the aggregate or its error allowance is not finite",
        ));
    }
    Ok(total)
}

/// The centroid weighted by volume or area. Error bounds include uncertainty
/// in both the member locations and their weights.
fn centroid(
    rows: &[&BodyMeasures],
    (weight_name, weight): Field<Scalar>,
    (point_name, point): Field<Point>,
) -> Result<Reading<Point3>, Unmeasured> {
    let mut members = Vec::with_capacity(rows.len());
    for row in rows {
        let point = measured(row, point_name, point(row))?;
        let weight = measured(row, weight_name, weight(row))?;
        members.push((point, weight));
    }
    if let [(point, _)] = members.as_slice() {
        return Ok(Reading {
            value: point.value().value.get(),
            numeric_error: point.numeric_error().get(),
            model_uncertainty: point.model_uncertainty().get(),
        });
    }
    let total: f64 = members
        .iter()
        .map(|(_, weight)| weight.value().value.get())
        .sum();
    if members.is_empty() || total <= 0.0 || !total.is_finite() {
        return Err(Unmeasured::gap(
            Gap::NoMembers,
            "the target has no member body with a positive weight",
        ));
    }
    let weight_error: f64 = members
        .iter()
        .map(|(_, weight)| weight.numeric_error().get() + weight.model_uncertainty().get())
        .sum();
    let minimum_weight = total - weight_error;
    if minimum_weight <= 0.0 || !minimum_weight.is_finite() {
        return Err(Unmeasured::gap(
            Gap::AggregateUnavailable,
            "the combined weight uncertainty reaches zero weight",
        ));
    }
    let mut moment = [0.0; 3];
    for (point, weight) in &members {
        let at = point.value().value.get();
        let weight = weight.value().value.get();
        moment[0] += weight * at.x;
        moment[1] += weight * at.y;
        moment[2] += weight * at.z;
    }
    let value = Point3::new(moment[0] / total, moment[1] / total, moment[2] / total);
    let mut numeric_error = 0.0;
    let mut model_uncertainty = 0.0;
    for (point, weight) in &members {
        let lever = distance(point.value().value.get(), value);
        let mass = weight.value().value.get().abs()
            + weight.numeric_error().get()
            + weight.model_uncertainty().get();
        numeric_error += mass * point.numeric_error().get() + lever * weight.numeric_error().get();
        model_uncertainty +=
            mass * point.model_uncertainty().get() + lever * weight.model_uncertainty().get();
    }
    numeric_error /= minimum_weight;
    model_uncertainty /= minimum_weight;
    if ![value.x, value.y, value.z, numeric_error, model_uncertainty]
        .iter()
        .all(|value| value.is_finite())
    {
        return Err(Unmeasured::gap(
            Gap::AggregateUnavailable,
            "the centroid aggregate or its error allowance is not finite",
        ));
    }
    Ok(Reading {
        value,
        numeric_error,
        model_uncertainty,
    })
}

fn distance(a: Point3, b: Point3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y).hypot(a.z - b.z)
}

fn scalar_disagreement(
    name: &str,
    declared: f64,
    measured: Reading<f64>,
    unit: &str,
    source_error: f64,
) -> Result<Option<String>, Unmeasured> {
    let difference = (measured.value - declared).abs();
    let allowed = combined_error_allowance(
        measured.numeric_error,
        measured.model_uncertainty,
        source_error,
    )?;
    Ok((difference > allowed).then(|| {
        format!(
            "declared {name} {declared} {unit} differs from the measured {value} {unit} by \
             {difference} {unit}, more than its numeric error {numeric} plus model uncertainty \
             {model} plus source error {source_error}",
            value = measured.value,
            numeric = measured.numeric_error,
            model = measured.model_uncertainty,
        )
    }))
}

fn point_disagreement(
    name: &str,
    declared: Point3,
    measured: Reading<Point3>,
    source_error: f64,
) -> Result<Option<String>, Unmeasured> {
    let difference = distance(measured.value, declared);
    let allowed = combined_error_allowance(
        measured.numeric_error,
        measured.model_uncertainty,
        source_error,
    )?;
    Ok((difference > allowed).then(|| {
        format!(
            "declared {name} ({}, {}, {}) mm lies {difference} mm from the measured ({}, {}, {}) \
             mm, more than its numeric error {} plus model uncertainty {} plus source error {}",
            declared.x,
            declared.y,
            declared.z,
            measured.value.x,
            measured.value.y,
            measured.value.z,
            measured.numeric_error,
            measured.model_uncertainty,
            source_error,
        )
    }))
}

/// The distance within which a stored point lies of the computed point when
/// each of its three coordinates lies within `precision` of its own.
fn point_error(precision: f64) -> f64 {
    precision * 3.0_f64.sqrt()
}

fn combined_error_allowance(
    numeric_error: f64,
    model_uncertainty: f64,
    source_error: f64,
) -> Result<f64, Unmeasured> {
    let allowed = numeric_error + model_uncertainty + source_error;
    if !allowed.is_finite() {
        return Err(Unmeasured::gap(
            Gap::AggregateUnavailable,
            "the combined measured and source error allowance is not finite",
        ));
    }
    Ok(allowed)
}

const fn quantity_name(quantity: &DeclaredQuantity) -> &'static str {
    match quantity {
        DeclaredQuantity::Volume { .. } => "volume",
        DeclaredQuantity::SolidArea { .. } => "solid area",
        DeclaredQuantity::WettedArea { .. } => "wetted area",
        DeclaredQuantity::SolidCentroid { .. } => "solid centroid",
        DeclaredQuantity::SheetArea { .. } => "sheet area",
        DeclaredQuantity::SheetCentroid { .. } => "sheet centroid",
    }
}

fn quantity_description(measure: &DeclaredMeasure) -> String {
    let quantity = quantity_name(&measure.quantity);
    measure.label.as_ref().map_or_else(
        || quantity.to_owned(),
        |label| format!("{quantity} ({label})"),
    )
}

fn declared_text(quantity: &DeclaredQuantity) -> String {
    match quantity {
        DeclaredQuantity::Volume { value } => format!("{} mm3", value.get()),
        DeclaredQuantity::SolidArea { value }
        | DeclaredQuantity::WettedArea { value }
        | DeclaredQuantity::SheetArea { value } => format!("{} mm2", value.get()),
        DeclaredQuantity::SolidCentroid { value } | DeclaredQuantity::SheetCentroid { value } => {
            let point = value.get();
            format!("({}, {}, {}) mm", point.x, point.y, point.z)
        }
    }
}

#[cfg(test)]
mod tests;
