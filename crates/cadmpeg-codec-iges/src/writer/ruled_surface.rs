// SPDX-License-Identifier: Apache-2.0
//! Native Type 118 ruled surfaces written from a neutral ruled construction.

use super::{
    append_curve_entity, curve_reference_span, flatten_curve, model_surface_point,
    natural_nurbs_domain, reference_marker, same_point, CadIr, CodecError, CurveEntityRequest,
    CurveId, Entity, EntityStatus, EvaluationFailure, Sense, SolvedSurfaceGeometry, Surface,
};

/// A Type 118 ruled surface between two rails (IGES 5.3 §4.17). The neutral
/// construction states no direction flag, so the solved cache selects it:
/// `DIRFLG=1` when the cache joins the first rail's start to the second rail's
/// end. The neutral carrier holds no developability or parameterization form;
/// a source Type 118 that names the same rails supplies both, and otherwise
/// the surface is Form 1 with `DEVFLG=0`, which permits either result.
pub(super) fn entities(
    ir: &CadIr,
    surface: &Surface,
    rails: [&CurveId; 2],
    base_index: usize,
    version: crate::IgesVersion,
) -> Result<Vec<Entity>, CodecError> {
    let geometry = &surface.geometry;
    let (form, developable) = source_fields(ir, surface, rails).unwrap_or((1, 0));
    let mut entities = Vec::new();
    let mut indices = Vec::with_capacity(rails.len());
    let mut second_span = None;
    for rail in rails {
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *rail)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("IGES Type 118 rail {rail} is missing"))
            })?;
        let rail_geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
            CodecError::InvalidInput("IGES curve carrier has no solved geometry".into())
        })?)?;
        let span = curve
            .parameter_range
            .map(|_| curve_reference_span(ir, rail, &rail_geometry))
            .transpose()?;
        let local = append_curve_entity(
            &mut entities,
            ir,
            CurveEntityRequest {
                version,
                curve_id: rail,
                geometry: &rail_geometry,
                span: span.as_ref(),
                sense: Sense::Forward,
                status: EntityStatus::PhysicallyDependent,
                reference_offset: base_index,
            },
        )?;
        indices.push(
            base_index
                .checked_add(local)
                .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?,
        );
        second_span = span;
    }
    let reversed = second_span.and_then(|span| {
        let Some(SolvedSurfaceGeometry::Nurbs(cache)) = geometry.solved() else {
            return None;
        };
        let [[u_start, _], [_, v_end]] = natural_nurbs_domain(cache)?;
        Some((u_start, v_end, span))
    });
    let reversed = match reversed {
        Some((u, v, span)) => match model_surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            ir,
            geometry,
            u,
            v,
        ) {
            Ok(corner) => {
                same_point(corner.get(), span.end) && !same_point(corner.get(), span.start)
            }
            Err(EvaluationFailure::ResourceLimit(limit)) => return Err(limit.into()),
            Err(
                EvaluationFailure::NoValue
                | EvaluationFailure::NonFinite(_)
                | EvaluationFailure::WorkExhausted,
            ) => false,
        },
        None => false,
    };
    entities.push(Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: Vec::new(),
        type_code: 118,
        form,
        label: String::new(),
        status: EntityStatus::Independent,
        subscript: 0,
        parameter_body: format!(
            "{},{},{},{developable};",
            reference_marker(indices[0]),
            reference_marker(indices[1]),
            u8::from(reversed)
        )
        .into_bytes(),
        transform: None,
        transform_link: None,
    });
    Ok(entities)
}

/// The form and developability flag of the source Type 118 entity of
/// `surface`, when it names the rails `rails`.
fn source_fields(ir: &CadIr, surface: &Surface, rails: [&CurveId; 2]) -> Option<(i64, i64)> {
    let source = surface
        .source_object
        .as_ref()
        .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)?;
    let id = format!(
        "iges:entity:directory#{}",
        source.object_id.as_str().strip_prefix('D')?
    );
    let record = ir
        .native
        .namespace("iges")?
        .arenas()
        .get("entities")?
        .iter()
        .find(|record| record.id() == id)?;
    if record.field("entity_type")?.as_i64()? != 118 {
        return None;
    }
    let form = record
        .field("form")?
        .as_i64()
        .filter(|form| matches!(form, 0 | 1))?;
    let parameters = record.field("parameters")?;
    let integer = |index: usize| parameters.get(index)?.get("value")?.get("value")?.as_i64();
    for (index, rail) in [1, 2].into_iter().zip(rails) {
        let named = format!("iges:model:curve#D{}", integer(index)?);
        if named != rail.as_str() {
            return None;
        }
    }
    let developable = integer(4).filter(|flag| matches!(flag, 0 | 1))?;
    Some((form, developable))
}
