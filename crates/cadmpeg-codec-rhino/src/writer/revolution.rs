// SPDX-License-Identifier: Apache-2.0
//! Native revolution construction from held procedural surfaces.

use super::{
    revolution_surface_payload, CadIr, RevolutionSurface, SurfacePayload, WritableObjectCurve,
    WriteError, MAX_ARC_SPAN_EXCESS,
};

pub(super) fn held_revolution(
    ir: &CadIr,
    surface: &cadmpeg_ir::geometry::Surface,
) -> Result<Option<(cadmpeg_ir::ids::CurveId, SurfacePayload)>, WriteError> {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SolvedCurveGeometry};

    let Some(construction) = surface.geometry.procedural_construction().and_then(|id| {
        ir.model
            .procedural_surfaces
            .iter()
            .find(|row| row.id == *id)
    }) else {
        return Ok(None);
    };
    let ProceduralSurfaceDefinition::Revolution(revolution) = construction.definition() else {
        return Ok(None);
    };
    if revolution.angular_parameter_interval().is_some()
        || revolution.revision_form().is_some()
        || revolution.legacy_cache().is_some()
    {
        return Ok(None);
    }
    let Some(parameter) = revolution
        .parameter_interval()
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(None);
    };
    let angle = revolution.angular_interval().endpoints();
    if angle[1] - angle[0] > std::f64::consts::TAU + MAX_ARC_SPAN_EXCESS {
        return Ok(None);
    }
    let Some(curve) = ir
        .model
        .curves
        .iter()
        .find(|row| row.id == *revolution.directrix())
    else {
        return Ok(None);
    };
    if curve.parameter_range.is_some() || curve.geometry.procedural_construction().is_some() {
        return Ok(None);
    }
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = curve.geometry.solved() else {
        return Ok(None);
    };
    let Ok(degree) = usize::try_from(nurbs.degree()) else {
        return Ok(None);
    };
    let profile_domain = [nurbs.knots()[degree], nurbs.knots()[nurbs.pole_count()]];
    let transposed = !*revolution.transposed();
    let domain = if transposed {
        [profile_domain, parameter]
    } else {
        [parameter, profile_domain]
    };
    let expected = [
        Some(domain[0][0]),
        Some(domain[1][0]),
        Some(domain[0][1]),
        Some(domain[1][1]),
    ];
    if construction
        .record_bounds()
        .map(cadmpeg_ir::geometry::RecordBounds::get)
        != Some(expected)
    {
        return Ok(None);
    }
    let (profile, conversion) = match WritableObjectCurve::try_new(curve) {
        Ok(value) => value,
        Err(WriteError::Omit(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if conversion.is_some() {
        return Ok(None);
    }
    let origin = revolution.axis_origin().get();
    let direction = *revolution.axis_direction().as_raw();
    let native = RevolutionSurface {
        axis: [origin, origin.translated(direction, 1.0)],
        angle,
        parameter,
        bounds: [1.0, 0.0, 0.0, -1.0, 0.0, 0.0],
        profile: profile.payload()?,
        transposed,
        domain,
    };
    Ok(Some((
        curve.id.clone(),
        revolution_surface_payload(&native)?,
    )))
}
