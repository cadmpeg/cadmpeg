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
    if !(matches!((form, interpretation), (11, 1) | (12, 2) | (13, 3))
        || form == 63 && interpretation == 1 && points.first() == points.last())
    {
        return None;
    }
    let positions = points.iter().map(|point| point.get()).collect::<Vec<_>>();
    if interpretation == 1 && positions.iter().any(|point| point.z != positions[0].z) {
        return None;
    }
    if form == 63 {
        let tuples = record.field("tuples")?;
        let tuples = tuples.as_array()?;
        if tuples.len() != positions.len()
            || record.field("common_z")?.as_f64()? != positions[0].z
            || tuples.iter().zip(&positions).any(|(tuple, point)| {
                tuple.as_array().is_none_or(|tuple| {
                    tuple.len() != 2
                        || tuple[0].as_f64() != Some(point.x)
                        || tuple[1].as_f64() != Some(point.y)
                })
            })
        {
            return None;
        }
    }
    let vectors = if interpretation == 3 {
        let directory = ir
            .native
            .namespace("iges")?
            .arenas()
            .get("entities")?
            .iter()
            .find(|entity| entity.id() == source_id)?;
        if directory.field("transform")?.as_i64()? != 0 {
            return None;
        }
        let tuples = record.field("tuples")?;
        let tuples = tuples.as_array()?;
        if tuples.len() != positions.len() {
            return None;
        }
        Some(
            tuples
                .iter()
                .map(|tuple| {
                    let tuple = tuple.as_array()?;
                    if tuple.len() != 6 {
                        return None;
                    }
                    Some([
                        FiniteReal::new(tuple[3].as_f64()?)?,
                        FiniteReal::new(tuple[4].as_f64()?)?,
                        FiniteReal::new(tuple[5].as_f64()?)?,
                    ])
                })
                .collect::<Option<Vec<_>>>()?,
        )
    } else {
        None
    };
    let mut values = vec![interpretation.to_string(), positions.len().to_string()];
    if interpretation == 1 {
        values.push(number(FiniteReal::new(positions[0].z)?));
    }
    for (index, point) in positions.into_iter().enumerate() {
        values.push(number(FiniteReal::new(point.x)?));
        values.push(number(FiniteReal::new(point.y)?));
        if interpretation != 1 {
            values.push(number(FiniteReal::new(point.z)?));
        }
        if let Some(vectors) = &vectors {
            values.extend(vectors[index].iter().copied().map(number));
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

fn source_curve_on_surface<'a>(
    ir: &'a CadIr,
    pcurve: &cadmpeg_ir::geometry::pcurve::Pcurve,
) -> Option<&'a cadmpeg_ir::native::NativeRecord> {
    let mut owners = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.pcurves.iter().any(|use_| use_.pcurve == pcurve.id));
    let coedge = owners.next()?;
    if owners.next().is_some() {
        return None;
    }
    let boundary = ir
        .model
        .loops
        .iter()
        .find(|boundary| boundary.id == coedge.owner_loop)?;
    if boundary.coedges().len() != 1 {
        return None;
    }
    let face = ir
        .model
        .faces
        .iter()
        .find(|face| face.id == boundary.face)?;
    if !matches!(
        face.loops,
        cadmpeg_ir::topology::FaceLoops::Classified { .. }
    ) {
        return None;
    }
    let loop_index = face.loops.iter().position(|id| *id == boundary.id)?;
    let face_sequence = face
        .id
        .as_str()
        .strip_prefix("iges:model:face#D")?
        .parse::<u64>()
        .ok()?;
    if boundary.id.as_str() != format!("iges:model:loop#D{face_sequence}:{loop_index}") {
        return None;
    }
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)?;
    let source = surface
        .source_object
        .as_ref()
        .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)?;
    let surface_sequence = source
        .object_id
        .as_str()
        .strip_prefix('D')?
        .parse::<u64>()
        .ok()?;
    let records = ir.native.namespace("iges")?.arenas().get("entities")?;
    let find = |sequence| {
        records.iter().find(|record| {
            record
                .field("directory_sequence")
                .and_then(|value| value.as_u64())
                == Some(sequence)
        })
    };
    let face_record = find(face_sequence)?;
    if face_record.field("entity_type")?.as_u64()? != 144
        || face_record.field("transform")?.as_i64()? != 0
    {
        return None;
    }
    let face_fields = face_record.field("parameters")?;
    let face_fields = face_fields.as_array()?;
    let integer = |fields: &[serde_json::Value], index: usize| {
        fields.get(index)?.get("value")?.get("value")?.as_u64()
    };
    if integer(face_fields, 1)? != surface_sequence
        || integer(face_fields, 2)? != 1
        || usize::try_from(integer(face_fields, 3)?).ok()? != face.loops.len().checked_sub(1)?
    {
        return None;
    }
    let curve_on_surface = find(integer(face_fields, 4 + loop_index)?)?;
    if curve_on_surface.field("entity_type")?.as_u64()? != 142
        || curve_on_surface.field("transform")?.as_i64()? != 0
    {
        return None;
    }
    let curve_fields = curve_on_surface.field("parameters")?;
    let curve_fields = curve_fields.as_array()?;
    if integer(curve_fields, 2)? != surface_sequence {
        return None;
    }
    Some(curve_on_surface)
}

pub(super) fn parameter_curve(
    ir: &CadIr,
    pcurve: &cadmpeg_ir::geometry::pcurve::Pcurve,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: cadmpeg_ir::units::FiniteVector<2>,
) -> Option<Entity> {
    let source = source_curve_on_surface(ir, pcurve)?;
    let fields = source.field("parameters")?;
    let source_parameter_sequence_value = fields
        .as_array()?
        .get(3)?
        .get("value")?
        .get("value")?
        .as_u64()?;
    let records = ir.native.namespace("iges")?.arenas().get("entities")?;
    let mut matches = ir.model.curves.iter().filter(|candidate| {
        let Some(SolvedCurveGeometry::Nurbs(solved)) = candidate.geometry.solved() else {
            return false;
        };
        if solved != curve
            || candidate
                .parameter_range
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
                != Some(range.get())
        {
            return false;
        }
        let Some(source) = candidate
            .source_object
            .as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)
        else {
            return false;
        };
        let Some(sequence) = source.object_id.as_str().strip_prefix('D') else {
            return false;
        };
        if sequence.parse::<u64>().ok() != Some(source_parameter_sequence_value) {
            return false;
        }
        let id = format!("iges:entity:directory#{sequence}");
        records.iter().any(|record| {
            record.id() == id
                && record.field("entity_type").and_then(|value| value.as_i64()) == Some(106)
                && record.field("form").and_then(|value| value.as_i64()) == Some(63)
                && record.field("use_flag").and_then(|value| value.as_i64()) == Some(5)
        })
    });
    let candidate = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let mut entity = regenerate(ir, &candidate.id, &candidate.geometry, None, Sense::Forward)?;
    entity.status = EntityStatus::ParameterCurve;
    entity.source_targets.push(candidate.id.to_string());
    Some(entity)
}

pub(super) fn clear_written_parameter_losses(
    ir: &CadIr,
    entities: &[Entity],
    losses: &mut Vec<super::LossNote>,
) {
    for curve in &ir.model.curves {
        if entities.iter().any(|entity| {
            entity.type_code == 106
                && matches!(entity.status, EntityStatus::ParameterCurve)
                && entity
                    .source_targets
                    .iter()
                    .any(|target| target == curve.id.as_str())
        }) {
            let missing = format!("1 support geometry record ({}) was withheld because its parameter-space carrier has no exported owner", curve.id);
            let duplicate_brep = format!(
                "free curve record {} duplicates a written parameter curve on the B-rep path",
                curve.id
            );
            let duplicate_sheet = format!("free curve record {} duplicates a written parameter curve on the trimmed-sheet path", curve.id);
            losses.retain(|loss| {
                loss.message != missing
                    && loss.message != duplicate_brep
                    && loss.message != duplicate_sheet
            });
        }
    }
}

pub(super) fn curve_on_surface_properties(
    ir: &CadIr,
    pcurve: &cadmpeg_ir::geometry::pcurve::Pcurve,
    parameter: &Entity,
    model: &Entity,
) -> Option<(String, String, i64)> {
    let source = source_curve_on_surface(ir, pcurve)?;
    let fields = source.field("parameters")?;
    let fields = fields.as_array()?;
    let integer = |index: usize| fields.get(index)?.get("value")?.get("value")?.as_i64();
    if integer(1)? != 0 || parameter.type_code != 106 || model.type_code != 106 {
        return None;
    }
    for (entity, index) in [(parameter, 3), (model, 4)] {
        let identity = format!("iges:entity:directory#{}", integer(index)?);
        if !entity.source_targets.contains(&identity) {
            return None;
        }
    }
    let preference = integer(5)?;
    if !(1..=3).contains(&preference) {
        return None;
    }
    let bytes: cadmpeg_ir::native::bytes::NativeBytes =
        serde_json::from_value(source.field("label")?).ok()?;
    let label = std::str::from_utf8(bytes.as_ref()).ok()?;
    if label.len() > 8
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        return None;
    }
    Some((source.id().to_owned(), label.to_owned(), preference))
}

pub(super) fn boundary_properties(
    ir: &CadIr,
    boundary: &super::WrittenBoundary<'_>,
    pcurve_indices: &std::collections::BTreeMap<String, usize>,
    entities: &[Entity],
    model_index: usize,
    parameter_reference: &str,
) -> (Vec<String>, String, i64) {
    let preserved = (|| {
        let [written] = boundary.coedges.as_slice() else {
            return None;
        };
        let [use_] = written.pcurves.as_slice() else {
            return None;
        };
        let parameter_index = *pcurve_indices.get(use_.pcurve.id.as_str())?;
        if parameter_reference != super::reference_marker(parameter_index) {
            return None;
        }
        curve_on_surface_properties(
            ir,
            use_.pcurve,
            entities.get(parameter_index)?,
            entities.get(model_index)?,
        )
    })();
    preserved.map_or_else(
        || {
            (
                Vec::new(),
                String::new(),
                i64::from(super::CURVE_ON_SURFACE_PREFERENCE_MODEL_CURVE),
            )
        },
        |(id, label, preference)| (vec![id], label, preference),
    )
}
