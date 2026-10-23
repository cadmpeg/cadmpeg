// SPDX-License-Identifier: Apache-2.0
//! Procedural carriers: a curve-bounded construction is measured on its
//! support, whose domain bounds it; an exact construction on its solved
//! cache; another construction on its solved cache when the cache states its
//! fit tolerance.

use cadmpeg_ir::geometry::{
    ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};

use super::placement::Placement;
use super::Carrier;
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
