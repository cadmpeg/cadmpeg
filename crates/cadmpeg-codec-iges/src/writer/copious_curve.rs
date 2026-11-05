// SPDX-License-Identifier: Apache-2.0
//! Regenerate source copious linear paths from their current neutral curves.

use super::{
    number, CadIr, CurveGeometry, CurveId, CurveSpan, Entity, EntityStatus, FiniteReal, Sense,
    SolvedCurveGeometry,
};

pub(super) fn regenerate(
    ir: &CadIr,
    id: &CurveId,
    geometry: &CurveGeometry,
    span: Option<&CurveSpan>,
    sense: Sense,
) -> Option<Entity> {
    if sense != Sense::Forward {
        return None;
    }
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = geometry else {
        return None;
    };
    if curve.degree() != 1 || curve.periodic() || curve.weights().is_some() {
        return None;
    }
    let points = curve.control_points();
    if points.len() < 2 {
        return None;
    }
    let end = cadmpeg_core::convert::f64_from_index(points.len().checked_sub(1)?)?;
    if span.is_some_and(|span| span.range.get() != [0.0, end]) {
        return None;
    }
    let mut knots = vec![0.0, 0.0];
    for index in 1..points.len() {
        knots.push(cadmpeg_core::convert::f64_from_index(index)?);
    }
    knots.push(end);
    if curve.knots().as_slice() != knots {
        return None;
    }
    let source = ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == id)?
        .source_object
        .as_ref()?;
    if source.format != cadmpeg_ir::CodecFormat::Iges {
        return None;
    }
    let source_id = format!(
        "iges:entity:directory#{}",
        source.object_id.as_str().strip_prefix('D')?
    );
    let records = ir.native.namespace("iges")?.arenas().get("copious_data")?;
    let record = records.iter().find(|record| {
        record
            .field("source_entity")
            .as_ref()
            .and_then(serde_json::Value::as_str)
            == Some(source_id.as_str())
    })?;
    let form = record.field("form")?.as_i64()?;
    let interpretation = record.field("interpretation")?.as_i64()?;
    if !matches!((form, interpretation), (11, 1) | (12, 2)) {
        return None;
    }
    let positions = points.iter().map(|point| point.get()).collect::<Vec<_>>();
    if interpretation == 1 && positions.iter().any(|point| point.z != positions[0].z) {
        return None;
    }
    let mut values = vec![interpretation.to_string(), positions.len().to_string()];
    if interpretation == 1 {
        values.push(number(FiniteReal::new(positions[0].z)?));
    }
    for point in positions {
        values.push(number(FiniteReal::new(point.x)?));
        values.push(number(FiniteReal::new(point.y)?));
        if interpretation == 2 {
            values.push(number(FiniteReal::new(point.z)?));
        }
    }
    let mut entity = Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: vec![record.id().to_owned(), source_id],
        type_code: 106,
        form,
        label: String::new(),
        status: EntityStatus::Independent,
        parameter_body: format!("{};", values.join(",")).into_bytes(),
        transform: None,
        transform_link: None,
        subscript: 0,
    };
    super::native_records::numeric::retain(ir, record, &mut entity);
    Some(entity)
}
