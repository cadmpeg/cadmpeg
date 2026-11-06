// SPDX-License-Identifier: Apache-2.0
//! Procedural carriers: bounded supports, directly evaluated extrusions,
//! and solved caches with an admitted fit tolerance.

use cadmpeg_ir::geometry::{
    CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry, Surface,
    SurfaceGeometry,
};

use super::placement::Placement;
use super::surface::{Chart, ExtrusionChart};
use super::{Carrier, Shape};
use crate::failure::{Measure, MeasureFailure};
use crate::owners::Owners;
use crate::reason::UnavailableReason;
use crate::select::SurfaceKind;

impl<'ir> Carrier<'ir> {
    /// The carrier of `surface` placed by `body`, with procedural
    /// constructions resolved through `owners`.
    pub(crate) fn resolve_in(
        owners: &Owners<'ir>,
        surface: &'ir Surface,
        body: Placement,
    ) -> Measure<Self> {
        Self::resolve_procedural(owners, surface, body, true)
    }

    /// `resolve_in`; a curve-bounded construction is followed to its support
    /// only when `follow_support` is set, so a support chain stops after one
    /// step.
    fn resolve_procedural(
        owners: &Owners<'ir>,
        surface: &'ir Surface,
        body: Placement,
        follow_support: bool,
    ) -> Measure<Self> {
        let SurfaceGeometry::Procedural {
            construction,
            cache,
        } = &surface.geometry
        else {
            return Self::resolve(surface, body);
        };
        let procedural = owners.procedural_surface(construction.as_str());
        let definition = procedural.map(ProceduralSurface::definition);
        if let Some(ProceduralSurfaceDefinition::CurveBounded {
            support,
            implicit_outer,
            ..
        }) = definition
        {
            if !follow_support {
                return Self::resolve(surface, body);
            }
            let Some(support_surface) = owners.surface(support.as_str()) else {
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::CarrierUnresolved,
                    Some(support.as_str()),
                    "the curve-bounded construction's support does not resolve",
                ));
            };
            let mut carrier = Self::resolve_procedural(owners, support_surface, body, false)?;
            carrier
                .chain
                .insert(0, "procedural:curve_bounded".to_owned());
            carrier.face_surface = surface;
            carrier.implicit_outer = *implicit_outer;
            return Ok(carrier);
        }
        if let Some(ProceduralSurfaceDefinition::Extrusion(extrusion)) = definition {
            if extrusion.parameter_interval().is_none()
                && procedural.is_some_and(|item| item.record_bounds().is_none())
                && extrusion.revision_form().is_none()
            {
                if let Some(curve) = owners.curve(extrusion.directrix().as_str()) {
                    if let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(directrix)) =
                        &curve.geometry
                    {
                        if cadmpeg_ir::eval::nurbs_curve_parameter_domain(directrix).is_some() {
                            let mut carrier = Self::resolve(surface, body)?;
                            carrier.chain.push("extrusion".to_owned());
                            carrier.shape =
                                Shape::Charted(Box::new(Chart::Extrusion(ExtrusionChart {
                                    surface: &surface.id,
                                    directrix,
                                    placement: body,
                                })));
                            return Ok(carrier);
                        }
                    }
                }
            }
        }
        let fit = match definition {
            Some(ProceduralSurfaceDefinition::Exact(_)) => Some(0.0),
            _ => procedural
                .and_then(ProceduralSurface::cache_fit_tolerance)
                .map(cadmpeg_ir::geometry::FitTolerance::get),
        };
        let (Some(cache), Some(fit)) = (cache, fit) else {
            return Self::resolve(surface, body);
        };
        let mut carrier = Self::resolve_geometry(surface, cache, body)?;
        let leaf = carrier.chain.last().cloned().unwrap_or_default();
        carrier.chain = vec!["procedural".to_owned(), format!("cache:{leaf}")];
        carrier.kind = SurfaceKind::Procedural;
        carrier.leaf = None;
        carrier.fit = fit;
        Ok(carrier)
    }
}
