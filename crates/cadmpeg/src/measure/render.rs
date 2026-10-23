// SPDX-License-Identifier: Apache-2.0
//! Text and TSV rendering of a measure report.
//!
//! Text output is for people and is not a stable interface. TSV prints one
//! table whose column names carry their unit; an empty cell is not
//! applicable or unavailable, and the `unavailable` cell lists the row's
//! reason codes.

use std::io::{self, Write};

use cadmpeg_measure::report::{
    BodyMeasures, BodyOrientation, CarrierParameters, EdgeMeasures, EdgeParameters, FaceMeasures,
    LoopMeasures, MeasureReport, NormalReading, OccurrenceMeasures, RegionRule, RowPlacement,
    SurfaceMeasures,
};
use cadmpeg_measure::value::{BoxValue, Measured, Point, Quantity, Range, Scalar, Sweep, Tensor};
use serde::Serialize;

use super::args::Level;

/// The serialized name of a unit-like enum value.
pub(super) fn name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// A text cell with tabs and newlines replaced by spaces.
fn cell(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

fn scalar_cell(quantity: Option<&Quantity<Scalar>>) -> String {
    quantity
        .and_then(Quantity::measured)
        .map_or_else(String::new, |measured| {
            measured.value().value.get().to_string()
        })
}

fn point_cells(quantity: Option<&Quantity<Point>>) -> [String; 3] {
    match quantity.and_then(Quantity::measured) {
        Some(measured) => {
            let point = measured.value().value.get();
            [point.x, point.y, point.z].map(|value| value.to_string())
        }
        None => [String::new(), String::new(), String::new()],
    }
}

fn tensor_cells(quantity: Option<&Quantity<Tensor>>) -> [String; 6] {
    match quantity.and_then(Quantity::measured) {
        Some(measured) => measured
            .value()
            .value
            .entries()
            .map(|entry| entry.to_string()),
        None => std::array::from_fn(|_| String::new()),
    }
}

fn moments_cells(moments: Option<&[Quantity<Scalar>; 3]>) -> [String; 3] {
    std::array::from_fn(|index| scalar_cell(moments.map(|moments| &moments[index])))
}

fn axes_cells(axes: Option<&[Quantity<Point>; 3]>) -> [String; 9] {
    let cells: [[String; 3]; 3] =
        std::array::from_fn(|index| point_cells(axes.map(|axes| &axes[index])));
    std::array::from_fn(|index| cells[index / 3][index % 3].clone())
}

fn box_cells(quantity: &Quantity<BoxValue>) -> [String; 7] {
    match quantity.measured() {
        Some(measured) => {
            let value = measured.value();
            let (min, max) = (value.min.get(), value.max.get());
            [
                min.x.to_string(),
                min.y.to_string(),
                min.z.to_string(),
                max.x.to_string(),
                max.y.to_string(),
                max.z.to_string(),
                name(&value.kind),
            ]
        }
        None => std::array::from_fn(|_| String::new()),
    }
}

fn range_cells(quantity: Option<&Quantity<Range>>) -> [String; 2] {
    match quantity.and_then(Quantity::measured) {
        Some(measured) => [
            measured.value().min.get().to_string(),
            measured.value().max.get().to_string(),
        ],
        None => [String::new(), String::new()],
    }
}

fn sweep_cells(quantity: Option<&Quantity<Sweep>>) -> [String; 2] {
    match quantity.and_then(Quantity::measured) {
        Some(measured) => [
            measured.value().value.get().to_string(),
            measured.value().full.to_string(),
        ],
        None => [String::new(), String::new()],
    }
}

/// The TSV `orientation` cell of a face.
fn face_orientation_cell(face: &FaceMeasures) -> &'static str {
    let orientation = face.orientation();
    match (*orientation.consistent(), *orientation.loop_normal()) {
        (Some(true), _) => "consistent",
        (Some(false), NormalReading::Mixed) => "mixed",
        (Some(false), _) => "inconsistent",
        (None, _) if *face.measures().region() == Some(RegionRule::DeclaredOrientation) => {
            "unconfirmed"
        }
        (None, _) => "not_determinable",
    }
}

/// The TSV `orientation` cell of a body.
fn body_orientation_cell(orientation: &BodyOrientation) -> &'static str {
    match *orientation.consistent() {
        Some(true) => "consistent",
        Some(false)
            if orientation.inconsistent().is_empty() && orientation.conflict().is_empty() =>
        {
            "mixed"
        }
        Some(false) => "inconsistent",
        None if !orientation.unconfirmed().is_empty() => "unconfirmed",
        None => "not_determinable",
    }
}

const FACE_HEADER: [&str; 30] = [
    "face",
    "space",
    "occurrence",
    "body",
    "surface",
    "kind",
    "radius_mm",
    "diameter_mm",
    "half_angle_rad",
    "major_radius_mm",
    "minor_radius_mm",
    "axial_min_mm",
    "axial_max_mm",
    "axial_extent_mm",
    "angular_sweep_rad",
    "sweep_full",
    "area_mm2",
    "boundary_length_mm",
    "seam_length_mm",
    "bbox_min_x_mm",
    "bbox_min_y_mm",
    "bbox_min_z_mm",
    "bbox_max_x_mm",
    "bbox_max_y_mm",
    "bbox_max_z_mm",
    "bbox_kind",
    "orientation",
    "status",
    "unavailable",
    "caveats",
];

const BODY_HEADER: [&str; 50] = [
    "body",
    "space",
    "occurrence",
    "name",
    "kind",
    "faces",
    "shells",
    "closed",
    "area_mm2",
    "volume_mm3",
    "length_mm",
    "centroid_x_mm",
    "centroid_y_mm",
    "centroid_z_mm",
    "second_moment_xx_mm5",
    "second_moment_yy_mm5",
    "second_moment_zz_mm5",
    "second_moment_xy_mm5",
    "second_moment_yz_mm5",
    "second_moment_zx_mm5",
    "principal_moment_1_mm5",
    "principal_moment_2_mm5",
    "principal_moment_3_mm5",
    "principal_axis_1_x",
    "principal_axis_1_y",
    "principal_axis_1_z",
    "principal_axis_2_x",
    "principal_axis_2_y",
    "principal_axis_2_z",
    "principal_axis_3_x",
    "principal_axis_3_y",
    "principal_axis_3_z",
    "mass_kg",
    "inertia_xx_kg_mm2",
    "inertia_yy_kg_mm2",
    "inertia_zz_kg_mm2",
    "inertia_xy_kg_mm2",
    "inertia_yz_kg_mm2",
    "inertia_zx_kg_mm2",
    "bbox_min_x_mm",
    "bbox_min_y_mm",
    "bbox_min_z_mm",
    "bbox_max_x_mm",
    "bbox_max_y_mm",
    "bbox_max_z_mm",
    "bbox_kind",
    "orientation",
    "status",
    "unavailable",
    "caveats",
];

const EDGE_HEADER: [&str; 19] = [
    "edge",
    "space",
    "occurrence",
    "kind",
    "length_mm",
    "radius_mm",
    "arc_angle_rad",
    "parameter_span",
    "start_x_mm",
    "start_y_mm",
    "start_z_mm",
    "end_x_mm",
    "end_y_mm",
    "end_z_mm",
    "closed",
    "degenerate",
    "faces",
    "status",
    "unavailable",
];

const SURFACE_HEADER: [&str; 14] = [
    "surface",
    "space",
    "occurrence",
    "kind",
    "radius_mm",
    "diameter_mm",
    "half_angle_rad",
    "major_radius_mm",
    "minor_radius_mm",
    "carrier_placement",
    "bodies",
    "faces",
    "status",
    "unavailable",
];

/// The `space` and `occurrence` cells of a row.
fn space_cells(placement: &RowPlacement) -> [String; 2] {
    match placement {
        RowPlacement::Model {} => ["model".to_owned(), String::new()],
        RowPlacement::Occurrence { occurrence } => ["occurrence".to_owned(), cell(occurrence)],
        RowPlacement::Stored {} => ["stored".to_owned(), String::new()],
    }
}

/// The `radius_mm`, `diameter_mm`, `half_angle_rad`, `major_radius_mm` and
/// `minor_radius_mm` cells of a carrier.
fn carrier_cells(parameters: &CarrierParameters) -> [String; 5] {
    let (radius, diameter, half_angle, major, minor) = match parameters {
        CarrierParameters::Cylinder {
            radius, diameter, ..
        }
        | CarrierParameters::Sphere {
            radius, diameter, ..
        } => (Some(radius), Some(diameter), None, None, None),
        CarrierParameters::Cone { half_angle, .. } => (None, None, Some(half_angle), None, None),
        CarrierParameters::Torus {
            major_radius,
            minor_radius,
            ..
        } => (
            Some(minor_radius),
            None,
            None,
            Some(major_radius),
            Some(minor_radius),
        ),
        _ => (None, None, None, None, None),
    };
    [radius, diameter, half_angle, major, minor].map(scalar_cell)
}

fn face_row(face: &FaceMeasures) -> Vec<String> {
    let carrier = face.carrier();
    let measures = face.measures();
    let [axial_min, axial_max] = range_cells(measures.axial_range().as_ref());
    let [sweep, full] = sweep_cells(measures.angular_sweep().as_ref());
    let mut row = vec![cell(face.face())];
    row.extend(space_cells(face.placement()));
    row.extend([
        cell(face.body().as_deref().unwrap_or("")),
        cell(carrier.surface()),
        name(carrier.kind()),
    ]);
    row.extend(carrier_cells(carrier.parameters()));
    row.extend([
        axial_min,
        axial_max,
        scalar_cell(measures.axial_extent().as_ref()),
        sweep,
        full,
        scalar_cell(Some(measures.area())),
        scalar_cell(Some(measures.boundary_length())),
        scalar_cell(Some(measures.seam_length())),
    ]);
    row.extend(box_cells(measures.bbox()));
    row.push(face_orientation_cell(face).to_owned());
    row.push(name(face.status()));
    row.push(face.unavailable_codes().join(","));
    let mut caveats: Vec<String> = face
        .caveats()
        .iter()
        .map(|caveat| name(&caveat.code()))
        .collect();
    caveats.sort();
    caveats.dedup();
    row.push(caveats.join(","));
    row
}

fn body_row(body: &BodyMeasures) -> Vec<String> {
    let closed = !body.shells().is_empty() && body.shells().iter().all(|shell| *shell.closed());
    let mut row = vec![cell(body.body())];
    row.extend(space_cells(body.placement()));
    row.extend([
        cell(body.name().as_deref().unwrap_or("")),
        name(body.kind()),
        body.faces().to_string(),
        body.shells().len().to_string(),
        closed.to_string(),
        scalar_cell(body.area().as_ref()),
        scalar_cell(body.volume().as_ref()),
        scalar_cell(body.length().as_ref()),
    ]);
    row.extend(point_cells(body.centroid().as_ref()));
    row.extend(tensor_cells(body.second_moments().as_ref()));
    row.extend(moments_cells(body.principal_moments().as_ref()));
    row.extend(axes_cells(body.principal_axes().as_ref()));
    row.push(scalar_cell(body.mass().as_ref()));
    row.extend(tensor_cells(body.inertia().as_ref()));
    row.extend(box_cells(body.bbox()));
    let orientation = if *body.faces() == 0 {
        ""
    } else {
        body_orientation_cell(body.orientation())
    };
    row.push(orientation.to_owned());
    row.push(name(body.status()));
    row.push(body.unavailable_codes().join(","));
    let caveats: Vec<String> = body
        .caveats()
        .iter()
        .map(|caveat| name(caveat.code()))
        .collect();
    row.push(caveats.join(","));
    row
}

fn edge_row(edge: &EdgeMeasures) -> Vec<String> {
    let radius = match edge.parameters() {
        EdgeParameters::Circle { radius, .. } => Some(radius),
        _ => None,
    };
    let mut row = vec![cell(edge.edge())];
    row.extend(space_cells(edge.placement()));
    row.extend([
        name(edge.kind()),
        scalar_cell(Some(edge.length())),
        scalar_cell(radius),
        scalar_cell(edge.arc_angle().as_ref()),
        scalar_cell(edge.parameter_span().as_ref()),
    ]);
    row.extend(point_cells(Some(edge.start())));
    row.extend(point_cells(Some(edge.end())));
    row.push(edge.closed().to_string());
    row.push(edge.degenerate().to_string());
    row.push(cell(&edge.faces().join(",")));
    row.push(name(edge.status()));
    row.push(edge.unavailable_codes().join(","));
    row
}

fn surface_row(surface: &SurfaceMeasures) -> Vec<String> {
    let carrier = surface.carrier();
    let mut row = vec![cell(carrier.surface())];
    row.extend(space_cells(surface.placement()));
    row.push(name(carrier.kind()));
    row.extend(carrier_cells(carrier.parameters()));
    row.push(name(&carrier.placement().class()));
    row.push(cell(&surface.bodies().join(",")));
    row.push(cell(&surface.faces().join(",")));
    row.push(name(surface.status()));
    row.push(surface.unavailable_codes().join(","));
    row
}

/// The TSV table of one row level.
pub(super) fn tsv(report: &MeasureReport, level: Level) -> String {
    let (header, rows): (&[&str], Vec<Vec<String>>) = match level {
        Level::Bodies => (&BODY_HEADER, report.bodies().iter().map(body_row).collect()),
        Level::Faces => (&FACE_HEADER, report.faces().iter().map(face_row).collect()),
        Level::Edges => (&EDGE_HEADER, report.edges().iter().map(edge_row).collect()),
        Level::Surfaces => (
            &SURFACE_HEADER,
            report.surfaces().iter().map(surface_row).collect(),
        ),
    };
    let mut text = header.join("\t");
    text.push('\n');
    for row in rows {
        text.push_str(&row.join("\t"));
        text.push('\n');
    }
    text
}

/// A number with 12 significant digits.
pub(super) fn number(value: f64) -> String {
    significant(value, 11)
}

/// `value` with `1 + precision` significant digits, trailing zeros removed
/// and `-0` printed as `0`.
fn significant(value: f64, precision: u8) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let scientific = format!("{value:.*e}", usize::from(precision));
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific;
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return scientific;
    };
    if (-5..=i32::from(precision)).contains(&exponent) {
        let Ok(decimals) = usize::try_from(i32::from(precision) - exponent) else {
            return scientific;
        };
        let fixed = format!("{value:.decimals$}");
        if fixed.contains('.') {
            fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            fixed
        }
    } else {
        let mantissa = if mantissa.contains('.') {
            mantissa.trim_end_matches('0').trim_end_matches('.')
        } else {
            mantissa
        };
        format!("{mantissa}e{exponent}")
    }
}

fn errors<T>(measured: &Measured<T>) -> String {
    let numeric = measured.numeric_error().get();
    let model = measured.model_uncertainty().get();
    let numeric = if numeric > 0.0 {
        format!("  numeric ±{}", significant(numeric, 0))
    } else {
        String::new()
    };
    let model = if model > 0.0 {
        format!("  model ±{}", significant(model, 0))
    } else {
        String::new()
    };
    format!("{numeric}{model}")
}

fn unavailable_text<T>(quantity: &Quantity<T>) -> Option<String> {
    quantity.unavailable().map(|unavailable| {
        format!(
            "unavailable  {} ({}): {}",
            unavailable.reason().code(),
            name(&unavailable.category()),
            unavailable.message()
        )
    })
}

fn scalar_text(quantity: &Quantity<Scalar>, unit: &str) -> String {
    if let Some(text) = unavailable_text(quantity) {
        return text;
    }
    let Some(measured) = quantity.measured() else {
        return String::new();
    };
    let value = measured.value().value.get();
    let degrees = if unit == "rad" {
        format!(" ({}°)", significant(value.to_degrees(), 5))
    } else {
        String::new()
    };
    format!(
        "{} {unit}{degrees}  {}{}{}",
        number(value),
        name(&measured.method()),
        reading_text(measured),
        errors(measured)
    )
}

/// A point with its unit, or a direction when `unit` is empty.
fn point_text(quantity: &Quantity<Point>, unit: &str) -> String {
    if let Some(text) = unavailable_text(quantity) {
        return text;
    }
    let Some(measured) = quantity.measured() else {
        return String::new();
    };
    let point = measured.value().value.get();
    let unit = if unit.is_empty() {
        String::new()
    } else {
        format!(" {unit}")
    };
    format!(
        "({}, {}, {}){unit}  {}{}",
        number(point.x),
        number(point.y),
        number(point.z),
        name(&measured.method()),
        reading_text(measured)
    )
}

/// A point value's coordinates with its unit.
fn point_value(value: &Point, unit: &str) -> String {
    let point = value.value.get();
    format!(
        "({}, {}, {}) {unit}",
        number(point.x),
        number(point.y),
        number(point.z)
    )
}

/// A symmetric tensor as `xx yy zz xy yz zx` with its unit.
fn tensor_text(quantity: &Quantity<Tensor>, unit: &str) -> String {
    if let Some(text) = unavailable_text(quantity) {
        return text;
    }
    let Some(measured) = quantity.measured() else {
        return String::new();
    };
    let entries: Vec<String> = measured
        .value()
        .value
        .entries()
        .iter()
        .zip(["xx", "yy", "zz", "xy", "yz", "zx"])
        .map(|(entry, name)| format!("{name} {}", number(*entry)))
        .collect();
    format!(
        "{} {unit}  {}{}",
        entries.join(", "),
        name(&measured.method()),
        errors(measured)
    )
}

fn moments_text(moments: &[Quantity<Scalar>; 3]) -> String {
    let moments: Vec<String> = moments
        .iter()
        .map(|moment| scalar_text(moment, "mm⁵"))
        .collect();
    moments.join("; ")
}

fn axes_text(axes: &[Quantity<Point>; 3]) -> String {
    let axes: Vec<String> = axes.iter().map(|axis| point_text(axis, "")).collect();
    axes.join("; ")
}

fn box_text(quantity: &Quantity<BoxValue>) -> String {
    if let Some(text) = unavailable_text(quantity) {
        return text;
    }
    let Some(measured) = quantity.measured() else {
        return String::new();
    };
    let value = measured.value();
    let (min, max) = (value.min.get(), value.max.get());
    format!(
        "({}, {}, {}) .. ({}, {}, {}) mm  {}{}",
        number(min.x),
        number(min.y),
        number(min.z),
        number(max.x),
        number(max.y),
        number(max.z),
        name(&value.kind),
        reading_text(measured)
    )
}

/// The corners of a box value.
fn box_corners(value: &BoxValue) -> String {
    let (min, max) = (value.min.get(), value.max.get());
    format!(
        "({}, {}, {}) .. ({}, {}, {}) mm",
        number(min.x),
        number(min.y),
        number(min.z),
        number(max.x),
        number(max.y),
        number(max.z)
    )
}

/// `, reading R` for a value measured under a named reading.
fn reading_text<T>(measured: &Measured<T>) -> String {
    measured.reading().map_or_else(String::new, |reading| {
        format!(", reading {}", name(&reading))
    })
}

/// One line per candidate reading of a value.
fn candidates_text<T>(
    out: &mut impl Write,
    quantity: &Quantity<T>,
    show: impl Fn(&T) -> String,
) -> io::Result<()> {
    let candidates = match quantity {
        Quantity::Measured(measured) => measured.candidates(),
        Quantity::Unavailable(unavailable) => unavailable.candidates(),
    };
    for candidate in candidates {
        writeln!(
            out,
            "                  {:<13} {}",
            name(&candidate.reading()),
            show(candidate.value())
        )?;
    }
    Ok(())
}

fn range_text(quantity: &Quantity<Range>, unit: &str) -> String {
    if let Some(text) = unavailable_text(quantity) {
        return text;
    }
    let Some(measured) = quantity.measured() else {
        return String::new();
    };
    let (min, max) = (measured.value().min.get(), measured.value().max.get());
    let degrees = if unit == "rad" {
        format!(
            " ({}° .. {}°)",
            significant(min.to_degrees(), 5),
            significant(max.to_degrees(), 5)
        )
    } else {
        String::new()
    };
    format!(
        "{} .. {} {unit}{degrees}  {}{}{}",
        number(min),
        number(max),
        name(&measured.method()),
        reading_text(measured),
        errors(measured)
    )
}

fn sweep_text(quantity: &Quantity<Sweep>) -> String {
    if let Some(text) = unavailable_text(quantity) {
        return text;
    }
    let Some(measured) = quantity.measured() else {
        return String::new();
    };
    let sweep = measured.value();
    let value = sweep.value.get();
    let coverage = if sweep.full {
        "full".to_owned()
    } else {
        let intervals: Vec<String> = sweep
            .intervals
            .iter()
            .map(|[start, end]| format!("[{}, {}]", number(start.get()), number(end.get())))
            .collect();
        intervals.join(" ")
    };
    format!(
        "{} rad ({}°) {coverage}  {}{}{}",
        number(value),
        significant(value.to_degrees(), 5),
        name(&measured.method()),
        reading_text(measured),
        errors(measured)
    )
}

fn line(out: &mut impl Write, label: &str, value: &str) -> io::Result<()> {
    writeln!(out, "  {label:<13} {value}")
}

fn parameters_text(out: &mut impl Write, parameters: &CarrierParameters) -> io::Result<()> {
    match parameters {
        CarrierParameters::Plane {
            origin,
            normal,
            u_axis,
        } => {
            line(out, "origin", &point_text(origin, "mm"))?;
            line(out, "normal", &point_text(normal, ""))?;
            line(out, "u_axis", &point_text(u_axis, ""))
        }
        CarrierParameters::Cylinder {
            radius,
            diameter,
            axis_point,
            axis_direction,
            ..
        } => {
            line(out, "radius", &scalar_text(radius, "mm"))?;
            line(out, "diameter", &scalar_text(diameter, "mm"))?;
            line(out, "axis point", &point_text(axis_point, "mm"))?;
            line(out, "axis", &point_text(axis_direction, ""))
        }
        CarrierParameters::Cone {
            half_angle,
            apex,
            opening_direction,
            ..
        } => {
            line(out, "half angle", &scalar_text(half_angle, "rad"))?;
            if let Some(apex) = apex {
                line(out, "apex", &point_text(apex, "mm"))?;
            }
            line(out, "opening", &point_text(opening_direction, ""))
        }
        CarrierParameters::Sphere {
            radius,
            diameter,
            center,
            axis_direction,
            ref_direction,
            normal_direction,
        } => {
            line(out, "radius", &scalar_text(radius, "mm"))?;
            line(out, "diameter", &scalar_text(diameter, "mm"))?;
            line(out, "center", &point_text(center, "mm"))?;
            line(out, "axis", &point_text(axis_direction, ""))?;
            line(out, "ref direction", &point_text(ref_direction, ""))?;
            line(out, "normal", &name(normal_direction))
        }
        CarrierParameters::Torus {
            major_radius,
            minor_radius,
            center,
            form,
            ..
        } => {
            line(out, "major radius", &scalar_text(major_radius, "mm"))?;
            line(out, "minor radius", &scalar_text(minor_radius, "mm"))?;
            line(out, "center", &point_text(center, "mm"))?;
            line(out, "form", &name(form))
        }
        CarrierParameters::Nurbs {
            u_degree,
            v_degree,
            u_poles,
            v_poles,
            rational,
            u_periodic,
            v_periodic,
            ..
        } => line(
            out,
            "nurbs",
            &format!(
                "degree {u_degree}×{v_degree}, {u_poles}×{v_poles} poles, {}, {}",
                if *rational { "rational" } else { "polynomial" },
                match (u_periodic, v_periodic) {
                    (false, false) => "not periodic",
                    (true, false) => "periodic in u",
                    (false, true) => "periodic in v",
                    (true, true) => "periodic in u and v",
                }
            ),
        ),
        CarrierParameters::None {} => Ok(()),
    }
}

fn loop_text(out: &mut impl Write, entry: &LoopMeasures) -> io::Result<()> {
    let mut parts = Vec::new();
    if let Some(role) = entry.role() {
        parts.push(format!(
            "{} (declared {})",
            name(role),
            name(entry.declared_role())
        ));
    }
    if let Some(winding) = entry.winding() {
        parts.push(name(winding));
    }
    let order = entry.listed_order().map_or_else(String::new, |order| {
        format!(" listed in {} order", name(&order))
    });
    parts.push(format!("coedges {}{order}", entry.coedges().len()));
    if let Some(length) = entry.length() {
        parts.push(format!("length {} mm", number(*length)));
    }
    line(
        out,
        "loop",
        &format!("{}  {}", entry.loop_id(), parts.join(", ")),
    )
}

fn face_text(out: &mut impl Write, face: &FaceMeasures) -> io::Result<()> {
    match face.name() {
        Some(face_name) => writeln!(out, "face {} \"{}\"", face.face(), cell(face_name))?,
        None => writeln!(out, "face {}", face.face())?,
    }
    if let Some(body) = face.body() {
        line(out, "body", body)?;
    }
    let carrier = face.carrier();
    line(
        out,
        "surface",
        &format!("{}  {}", carrier.surface(), name(carrier.kind())),
    )?;
    line(out, "sense", &name(face.sense()))?;
    parameters_text(out, carrier.parameters())?;
    let measures = face.measures();
    line(out, "area", &scalar_text(measures.area(), "mm²"))?;
    candidates_text(out, measures.area(), |area| {
        format!("{} mm²", number(area.value.get()))
    })?;
    line(
        out,
        "boundary",
        &scalar_text(measures.boundary_length(), "mm"),
    )?;
    if measures
        .seam_length()
        .measured()
        .is_some_and(|seam| seam.value().value.get() > 0.0)
    {
        line(out, "seam", &scalar_text(measures.seam_length(), "mm"))?;
    }
    line(out, "bbox", &box_text(measures.bbox()))?;
    candidates_text(out, measures.bbox(), box_corners)?;
    if let Some(axial) = measures.axial_range() {
        line(out, "axial range", &range_text(axial, "mm"))?;
    }
    if let Some(extent) = measures.axial_extent() {
        line(out, "axial extent", &scalar_text(extent, "mm"))?;
    }
    if let Some(sweep) = measures.angular_sweep() {
        line(out, "sweep", &sweep_text(sweep))?;
        candidates_text(out, sweep, |sweep| {
            format!("{} rad", number(sweep.value.get()))
        })?;
    }
    if let Some(latitude) = measures.latitude_range() {
        line(out, "latitude", &range_text(latitude, "rad"))?;
        candidates_text(out, latitude, |range| {
            format!(
                "{} .. {} rad",
                number(range.min.get()),
                number(range.max.get())
            )
        })?;
    }
    if let Some(radius) = measures.radius_range() {
        line(out, "radius range", &range_text(radius, "mm"))?;
    }
    if let Some(range) = measures.u_range() {
        line(out, "u range", &range_text(range, ""))?;
    }
    if let Some(range) = measures.v_range() {
        line(out, "v range", &range_text(range, ""))?;
    }
    if let Some(region) = measures.region() {
        line(out, "region", &name(region))?;
    }
    for entry in measures.loops() {
        loop_text(out, entry)?;
    }
    let witness = face
        .orientation()
        .witness()
        .as_ref()
        .map_or_else(String::new, |witness| {
            format!(
                "; witness {} {} ({})",
                name(witness.kind()),
                witness.entity(),
                if *witness.agrees() {
                    "agrees"
                } else {
                    "disagrees"
                }
            )
        });
    line(
        out,
        "orientation",
        &format!("{}{witness}", face_orientation_cell(face)),
    )?;
    for caveat in face.caveats() {
        line(
            out,
            "caveat",
            &format!("{} ({})", name(&caveat.code()), name(&caveat.category())),
        )?;
    }
    Ok(())
}

fn edge_text(out: &mut impl Write, edge: &EdgeMeasures) -> io::Result<()> {
    writeln!(out, "edge {} {}", edge.edge(), name(edge.kind()))?;
    line(out, "length", &scalar_text(edge.length(), "mm"))?;
    if let EdgeParameters::Circle { radius, .. } = edge.parameters() {
        line(out, "radius", &scalar_text(radius, "mm"))?;
    }
    if let Some(angle) = edge.arc_angle() {
        line(out, "arc angle", &scalar_text(angle, "rad"))?;
    }
    line(out, "start", &point_text(edge.start(), "mm"))?;
    line(out, "end", &point_text(edge.end(), "mm"))?;
    line(out, "bbox", &box_text(edge.bbox()))?;
    line(out, "faces", &edge.faces().join(", "))
}

fn surface_text(out: &mut impl Write, surface: &SurfaceMeasures) -> io::Result<()> {
    let carrier = surface.carrier();
    writeln!(
        out,
        "surface {} {}",
        carrier.surface(),
        name(carrier.kind())
    )?;
    line(out, "placement", &name(&carrier.placement().class()))?;
    parameters_text(out, carrier.parameters())?;
    if let Some(unresolved) = surface.unresolved() {
        line(
            out,
            "parameters",
            &format!(
                "unavailable  {} ({}): {}",
                unresolved.reason().code(),
                name(&unresolved.category()),
                unresolved.message()
            ),
        )?;
    }
    line(out, "bodies", &surface.bodies().join(", "))?;
    line(out, "faces", &surface.faces().join(", "))
}

fn body_orientation_text(orientation: &BodyOrientation) -> String {
    let mut parts = vec![body_orientation_cell(orientation).to_owned()];
    for (label, faces) in [
        ("inconsistent faces", orientation.inconsistent()),
        ("mixed faces", orientation.mixed()),
        ("unconfirmed faces", orientation.unconfirmed()),
        ("conflict faces", orientation.conflict()),
    ] {
        if !faces.is_empty() {
            parts.push(format!("{label}: {}", faces.join(", ")));
        }
    }
    if let Some(closure) = orientation.closure_flux() {
        let flux = |quantity: &Quantity<Scalar>| {
            quantity.measured().map_or_else(
                || String::from("unavailable"),
                |measured| format!("{} mm²", number(measured.value().value.get())),
            )
        };
        parts.push(format!(
            "closure flux {} (face sense), {} (loop winding)",
            flux(closure.face_sense()),
            flux(closure.loop_winding())
        ));
    }
    parts.join("; ")
}

fn body_text(out: &mut impl Write, body: &BodyMeasures) -> io::Result<()> {
    match body.name() {
        Some(body_name) => writeln!(
            out,
            "body {} {} \"{}\"",
            body.body(),
            name(body.kind()),
            cell(body_name)
        )?,
        None => writeln!(out, "body {} {}", body.body(), name(body.kind()))?,
    }
    let kinds: Vec<String> = body
        .surface_kinds()
        .iter()
        .map(|(kind, count)| format!("{} {count}", kind.as_str()))
        .collect();
    line(
        out,
        "faces",
        &format!("{} ({})", body.faces(), kinds.join(", ")),
    )?;
    for shell in body.shells() {
        line(
            out,
            "shell",
            &format!(
                "{} {}, {}",
                shell.shell(),
                name(shell.role()),
                if *shell.closed() { "closed" } else { "open" }
            ),
        )?;
    }
    if let Some(area) = body.area() {
        line(out, "area", &scalar_text(area, "mm²"))?;
        candidates_text(out, area, |area| {
            format!("{} mm²", number(area.value.get()))
        })?;
    }
    if let Some(volume) = body.volume() {
        line(out, "volume", &scalar_text(volume, "mm³"))?;
        candidates_text(out, volume, |volume| {
            format!("{} mm³", number(volume.value.get()))
        })?;
    }
    if let Some(centroid) = body.centroid() {
        line(out, "centroid", &point_text(centroid, "mm"))?;
        candidates_text(out, centroid, |point| point_value(point, "mm"))?;
    }
    if let Some(second) = body.second_moments() {
        line(out, "second moments", &tensor_text(second, "mm⁵"))?;
    }
    if let Some(moments) = body.principal_moments() {
        line(out, "principal", &moments_text(moments))?;
    }
    if let Some(axes) = body.principal_axes() {
        line(out, "axes", &axes_text(axes))?;
    }
    if let Some(mass) = body.mass() {
        line(out, "mass", &scalar_text(mass, "kg"))?;
    }
    if let Some(inertia) = body.inertia() {
        line(out, "inertia", &tensor_text(inertia, "kg·mm²"))?;
    }
    if let Some(centroid) = body.area_centroid() {
        line(out, "area centroid", &point_text(centroid, "mm"))?;
        candidates_text(out, centroid, |point| point_value(point, "mm"))?;
    }
    if let Some(length) = body.length() {
        line(out, "length", &scalar_text(length, "mm"))?;
    }
    line(out, "bbox", &box_text(body.bbox()))?;
    candidates_text(out, body.bbox(), box_corners)?;
    if let Some(vertex_box) = body.vertex_box() {
        line(out, "vertex box", &box_text(vertex_box))?;
    }
    if *body.faces() > 0 {
        line(
            out,
            "orientation",
            &body_orientation_text(body.orientation()),
        )?;
    }
    for caveat in body.caveats() {
        line(
            out,
            "caveat",
            &format!("{} on {} faces", name(caveat.code()), caveat.faces()),
        )?;
    }
    Ok(())
}

/// Three rows of a 3 × 4 affine map.
fn transform_text(rows: &[[f64; 4]; 3]) -> String {
    let rows: Vec<String> = rows
        .iter()
        .map(|row| {
            let row: Vec<String> = row.iter().map(|value| number(*value)).collect();
            format!("[{}]", row.join(", "))
        })
        .collect();
    rows.join(" ")
}

fn occurrence_text(out: &mut impl Write, occurrence: &OccurrenceMeasures) -> io::Result<()> {
    writeln!(out, "occurrence {}", occurrence.occurrence())?;
    if let Some(prototype) = occurrence.prototype() {
        line(out, "prototype", prototype)?;
    }
    line(out, "chain", &occurrence.chain().join(" > "))?;
    if let Some(rows) = occurrence.transform() {
        line(out, "transform", &transform_text(rows))?;
    }
    line(out, "bodies", &occurrence.bodies().join(", "))?;
    for descendant in occurrence.descendants() {
        let placement = descendant
            .transform()
            .as_ref()
            .map_or_else(String::new, |rows| {
                format!(", transform {}", transform_text(rows))
            });
        line(
            out,
            "descendant",
            &format!(
                "{} in {}, bodies [{}]{placement}",
                descendant.occurrence(),
                descendant.parent(),
                descendant.bodies().join(", ")
            ),
        )?;
    }
    if let Some(unavailable) = occurrence.unavailable() {
        line(
            out,
            "rows",
            &format!(
                "none: {} ({}): {}",
                unavailable.reason().code(),
                name(&unavailable.category()),
                unavailable.message()
            ),
        )?;
    }
    Ok(())
}

/// Human-readable blocks: one per row, then the document block.
pub(super) fn text(out: &mut impl Write, report: &MeasureReport) -> io::Result<()> {
    if let Some(occurrence) = report.occurrence() {
        occurrence_text(out, occurrence)?;
    }
    for face in report.faces() {
        face_text(out, face)?;
    }
    for edge in report.edges() {
        edge_text(out, edge)?;
    }
    for surface in report.surfaces() {
        surface_text(out, surface)?;
    }
    for body in report.bodies() {
        body_text(out, body)?;
    }
    let document = report.document();
    writeln!(out, "document")?;
    let area = document
        .area()
        .as_ref()
        .and_then(Quantity::measured)
        .map_or_else(String::new, |area| {
            format!(
                ", area {} mm²{}",
                number(area.value().value.get()),
                reading_text(area)
            )
        });
    let volume = document
        .closed_volume()
        .as_ref()
        .and_then(Quantity::measured)
        .map_or_else(String::new, |volume| {
            format!(", closed volume {} mm³", number(volume.value().value.get()))
        });
    line(
        out,
        "counts",
        &format!(
            "bodies {}, faces {}, edges {}, surfaces {}{area}{volume}",
            document.bodies(),
            document.faces(),
            document.edges(),
            document.surfaces()
        ),
    )?;
    if document.unavailable().is_empty() {
        line(out, "unavailable", "none")?;
    }
    for (category, codes) in document.unavailable() {
        for (code, count) in codes {
            line(
                out,
                "unavailable",
                &format!("{code} ({}) × {count}", name(category)),
            )?;
        }
    }
    for caveat in report.caveats() {
        line(out, "caveat", &name(&caveat.code()))?;
    }
    let work = document.work();
    line(
        out,
        "work",
        &format!(
            "{} of {} units{}",
            work.used(),
            work.allowance(),
            if work.exhausted() { ", exhausted" } else { "" }
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::significant;

    #[test]
    fn numbers_print_with_significant_digits() {
        assert_eq!(significant(1_884.955_592_153_875_8, 11), "1884.95559215");
        assert_eq!(significant(30.0, 11), "30");
        assert_eq!(significant(-0.0, 11), "0");
        assert_eq!(significant(-1.0e-17, 11), "-1e-17");
        assert_eq!(significant(3.7e-13, 0), "4e-13");
        assert_eq!(significant(std::f64::consts::TAU, 11), "6.28318530718");
        assert_eq!(significant(90.000_000_000_001, 5), "90");
        assert_eq!(significant(1.0e15, 11), "1e15");
    }
}
