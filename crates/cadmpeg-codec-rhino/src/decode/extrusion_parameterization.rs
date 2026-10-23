// SPDX-License-Identifier: Apache-2.0
//! Keep the sweep parameter interval independent of its endpoint displacement.

use super::{
    CandidateError, Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry,
};
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, RecordBounds};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::{CadIr, SourceObjectAssociation};

pub(super) fn construction(
    ir: &mut CadIr,
    profile: &NurbsCurve,
    lateral: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    directrix: &CurveId,
    id: ProceduralSurfaceId,
    displacement: Vector3,
    source: &SourceObjectAssociation,
) -> Result<ProceduralSurface, CandidateError> {
    let u_lower = lateral.u_knots()
        [usize::try_from(lateral.u_degree()).map_err(|error| error.to_string())?];
    let u_upper = lateral.u_knots()[lateral.u_count()];
    let v_lower = lateral.v_knots()
        [usize::try_from(lateral.v_degree()).map_err(|error| error.to_string())?];
    let v_upper = lateral.v_knots()[lateral.v_count()];
    let direction = displacement.scale(1.0 / (v_upper - v_lower));
    // CADIR's extrusion is C(u) + v D. The source's bounded path is
    // C_start(u) + (v-v_lower) D, so its construction directrix is at v=0.
    let directrix = if v_lower == 0.0 {
        directrix.clone()
    } else {
        let mut curve = profile.clone();
        curve.try_map_control_points(|_, point| {
            cadmpeg_ir::features::FinitePoint3::new(point.get().translated(direction, -v_lower))
                .ok_or_else(|| {
                    "extrusion construction directrix translation is not finite".to_owned()
                })
        })?;
        let id = CurveId::mint(format!("{}.sweep-origin", directrix.as_str()))
            .map_err(|error| error.to_string())?;
        ir.model.curves.push(Curve {
            native_ref: None,
            id: id.clone(),
            parameter_range: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            source_object: Some(source.clone()),
        });
        id
    };
    let definition = cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
        directrix,
        None,
        direction,
        None,
        cadmpeg_ir::geometry::CacheContract::from_form(None),
    )
    .map_err(|error| error.to_string())?;
    let bounds =
        RecordBounds::try_new([Some(u_lower), Some(v_lower), Some(u_upper), Some(v_upper)])
            .map_err(|error| error.to_string())?;
    Ok(ProceduralSurface::new(
        id,
        ProceduralSurfaceDefinition::Extrusion(definition),
        Some(bounds),
    ))
}
