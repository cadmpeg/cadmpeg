// SPDX-License-Identifier: Apache-2.0
//! Author parametric surface-curve carriers and their two-dimensional splines.

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::geometry::{
    pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles},
    Curve, CurveGeometry, ProceduralCurveDefinition, SolvedSurfaceGeometry, SurfaceCurveFamily,
};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::CadIr;
use std::collections::HashMap;

use super::write_error::WriteError;

const FIT_TOLERANCE_MM: f64 = 1.0e-5;

fn canonical_tail() -> Option<
    cadmpeg_ir::geometry::SurfaceCurveCacheFirst<cadmpeg_ir::geometry::ParametricSurfaceCurveFlags>,
> {
    use cadmpeg_ir::geometry::{
        FitTolerance, ParametricSurfaceCurveFlags, RevisionCacheForm, SurfaceCurveCacheFirst,
        SurfaceCurveTail,
    };
    let form = SurfaceCurveTail::try_new(
        0,
        cadmpeg_ir::scalar::PositiveI64::new(1)?,
        RevisionCacheForm::SolvedCache {
            fit_tolerance: FitTolerance::try_new(FIT_TOLERANCE_MM).ok()?,
        },
        [[None; 4]; 2],
        [None, None],
    )
    .ok()?;
    Some(SurfaceCurveCacheFirst {
        form,
        flags: ParametricSurfaceCurveFlags {
            flag: false,
            second_flag: None,
        },
    })
}

pub(super) fn supported<'a>(
    ir: &'a CadIr,
    curve: &'a Curve,
) -> Option<(&'a SurfaceId, &'a PcurveNurbs, Option<[f64; 2]>)> {
    let CurveGeometry::Procedural {
        construction,
        cache,
    } = &curve.geometry
    else {
        return None;
    };
    let definition = ir
        .model
        .procedural_curves
        .iter()
        .find(|item| &item.id == construction)?
        .definition();
    let ProceduralCurveDefinition::SurfaceCurve {
        family: SurfaceCurveFamily::Parametric { context, tail },
    } = definition
    else {
        return None;
    };
    if (cache.is_some() && tail.as_ref() != canonical_tail().as_ref())
        || (cache.is_none() && tail.is_some())
        || context
            .discontinuities()
            .iter()
            .any(|lane| !lane.is_empty())
    {
        return None;
    }
    let [first, second] = context.sides();
    if second.surface.is_some() || second.pcurve.is_some() {
        return None;
    }
    let surface = first.surface.as_ref()?;
    let support = first.pcurve.as_ref()?;
    if support.parameter_range.is_some() {
        return None;
    }
    let PcurveGeometry::Nurbs { nurbs } = &support.geometry else {
        return None;
    };
    let degree = usize::try_from(nurbs.degree()).ok()?;
    let domain = [
        *nurbs.knots().get(degree)?,
        *nurbs
            .knots()
            .get(nurbs.knots().len().checked_sub(degree.checked_add(1)?)?)?,
    ];
    let interval = context.parameter_range().endpoints();
    if domain[0] >= domain[1] || interval[0] < domain[0] || interval[1] > domain[1] {
        return None;
    }
    if ir
        .model
        .edges
        .iter()
        .filter(|edge| edge.curve() == Some(&curve.id))
        .any(|edge| {
            edge.param_range()
                .is_some_and(|range| range.get() != interval)
        })
    {
        return None;
    }
    let geometry = ir
        .model
        .surfaces
        .iter()
        .find(|item| &item.id == surface)?
        .geometry
        .solved()?;
    parameter_scales(geometry)?;
    Some((surface, nurbs, (interval != domain).then_some(interval)))
}

pub(super) fn bounded_interval(ir: &CadIr, edge: &cadmpeg_ir::topology::Edge) -> Option<[f64; 2]> {
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve())?;
    supported(ir, curve)?.2
}

pub(super) fn point_at(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    curve: &Curve,
    parameter: f64,
) -> Result<Option<cadmpeg_ir::math::Point3>, ResourceLimit> {
    let Some((surface, _, _)) = supported(ir, curve) else {
        return Ok(None);
    };
    let CurveGeometry::Procedural { construction, .. } = &curve.geometry else {
        return Ok(None);
    };
    let Some(ProceduralCurveDefinition::SurfaceCurve {
        family: SurfaceCurveFamily::Parametric { context, .. },
    }) = ir
        .model
        .procedural_curves
        .iter()
        .find(|item| &item.id == construction)
        .map(cadmpeg_ir::geometry::ProceduralCurve::definition)
    else {
        return Ok(None);
    };
    let Some(pcurve) = context.sides()[0]
        .pcurve
        .as_ref()
        .map(|pcurve| &pcurve.geometry)
    else {
        return Ok(None);
    };
    let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
        ctx, pcurve, parameter,
    ))?
    else {
        return Ok(None);
    };
    let Some(surface) = ir.model.surfaces.iter().find(|item| &item.id == surface) else {
        return Ok(None);
    };
    let uv = uv.get();
    Ok(
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::surface_point(
            ctx,
            &surface.geometry,
            uv.u,
            uv.v,
        ))?
        .map(cadmpeg_ir::features::FinitePoint3::get),
    )
}

fn parameter_scales(surface: &SolvedSurfaceGeometry) -> Option<[f64; 2]> {
    match surface {
        SolvedSurfaceGeometry::Plane(_) => Some([0.001, 0.001]),
        SolvedSurfaceGeometry::Cylinder(_) | SolvedSurfaceGeometry::Cone(_) => Some([1.0, 0.001]),
        SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_)
        | SolvedSurfaceGeometry::Nurbs(_) => Some([1.0, 1.0]),
        SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
}

pub(super) fn write(
    out: &mut Vec<u8>,
    ir: &CadIr,
    curve: &Curve,
    identity: u32,
    surfaces: &HashMap<SurfaceId, u32>,
    next: &mut u32,
) -> Result<(), WriteError> {
    let (surface, nurbs, _) = supported(ir, curve).ok_or_else(|| {
        WriteError::Decline("surface-curve construction has no supported native form".into())
    })?;
    let surface_attr = surfaces
        .get(surface)
        .copied()
        .ok_or_else(|| WriteError::Decline("surface-curve support is not emitted".into()))?;
    let surface_geometry = ir
        .model
        .surfaces
        .iter()
        .find(|item| &item.id == surface)
        .and_then(|item| item.geometry.solved())
        .ok_or_else(|| WriteError::Decline("surface-curve support is missing".into()))?;
    let scales = parameter_scales(surface_geometry).ok_or_else(|| {
        WriteError::Decline("surface-curve support parameters are unsupported".into())
    })?;
    let wrapper = super::take_attr(next)?;
    let descriptor = super::take_attr(next)?;
    let poles = super::take_attr(next)?;
    let multiplicity = super::take_attr(next)?;
    let knots = super::take_attr(next)?;
    super::tag(out, 0x89);
    super::typed_ref(out, identity)?;
    super::be32(out, 0);
    for _ in 0..5 {
        super::typed_ref(out, 1_u32)?;
    }
    out.push(0x2b);
    for value in [surface_attr, wrapper, 1] {
        super::typed_ref(out, value)?;
    }
    super::bef64(out, 0.0);
    super::tag(out, 0x86);
    super::typed_ref(out, wrapper)?;
    super::be32(out, 0);
    for _ in 0..5 {
        super::typed_ref(out, 1_u32)?;
    }
    out.push(0x2b);
    super::typed_ref(out, descriptor)?;
    super::typed_ref(out, 1_u32)?;
    let degree = super::field_u16(&super::CAPACITY_FIELDS[17], u64::from(nurbs.degree()))?;
    let count = super::field_u32(&super::CAPACITY_FIELDS[18], nurbs.pole_rows().count())?;
    let unique = super::unique_knots(nurbs.knots())?;
    let knot_count = super::field_u32(&super::CAPACITY_FIELDS[19], unique.len())?;
    let rational = matches!(nurbs.pole_rows(), PcurveNurbsPoles::Rational { .. });
    super::tag(out, 0x88);
    super::typed_ref(out, descriptor)?;
    super::be16(out, degree);
    super::be32(out, count);
    super::be16(out, if rational { 3 } else { 2 });
    super::be32(out, knot_count);
    out.push(0);
    out.extend_from_slice(&[u8::from(nurbs.periodic()), 0, u8::from(rational), 0]);
    for attr in [poles, multiplicity, knots] {
        super::typed_ref(out, attr)?;
    }
    let mut values = Vec::new();
    match nurbs.pole_rows() {
        PcurveNurbsPoles::Polynomial { points } => {
            for point in points {
                let point = point.get();
                values.extend([point.u * scales[0], point.v * scales[1]]);
            }
        }
        PcurveNurbsPoles::Rational { points } => {
            for pole in points {
                let point = pole.point.get();
                let weight = pole.weight.get();
                values.extend([
                    point.u * scales[0] * weight,
                    point.v * scales[1] * weight,
                    weight,
                ]);
            }
        }
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(WriteError::Decline(
            "surface-curve poles overflow native coordinates".into(),
        ));
    }
    super::f64_array(out, 0x2d, poles, values.into_iter())?;
    super::u16_array(
        out,
        multiplicity,
        unique.iter().map(|(_, count)| count.get()),
    )?;
    super::f64_array(out, 0x80, knots, unique.iter().map(|(knot, _)| *knot))?;
    Ok(())
}
