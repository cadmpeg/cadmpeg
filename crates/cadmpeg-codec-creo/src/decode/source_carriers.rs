// SPDX-License-Identifier: Apache-2.0
//! Source-unit geometry retained for analyses during IR construction.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, ProceduralSurface, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::scalar::PositiveReal;

#[derive(Default)]
pub(super) struct SourceUnitCarriers {
    length_scale_mm: Option<PositiveReal>,
    surfaces: BTreeMap<SurfaceId, SurfaceGeometry>,
    curves: BTreeMap<CurveId, CurveGeometry>,
    procedural_surfaces: BTreeSet<ProceduralSurfaceId>,
}

impl SourceUnitCarriers {
    pub(super) fn new(length_scale_mm: Option<PositiveReal>) -> Self {
        Self {
            length_scale_mm: length_scale_mm.filter(|scale| scale.get() != 1.0),
            surfaces: BTreeMap::new(),
            curves: BTreeMap::new(),
            procedural_surfaces: BTreeSet::new(),
        }
    }

    pub(super) fn admit_surface(
        &mut self,
        ir: &mut CadIr,
        mut surface: Surface,
    ) -> Result<(), CodecError> {
        let source_geometry = surface.geometry.clone();
        if let (Some(scale), SurfaceGeometry::Solved(geometry)) =
            (self.length_scale_mm, &mut surface.geometry)
        {
            crate::decode::build::units::scale_surface_geometry(geometry, scale).map_err(
                |error| match error {
                    CodecError::Malformed(message) => CodecError::NotImplemented(message),
                    other => other,
                },
            )?;
        }
        self.surfaces.insert(surface.id.clone(), source_geometry);
        ir.model.surfaces.push(surface);
        Ok(())
    }

    pub(super) fn replace_surface_geometry(
        &mut self,
        surface: &mut Surface,
        mut geometry: SurfaceGeometry,
    ) -> Result<(), CodecError> {
        let source_geometry = geometry.clone();
        if let (Some(scale), SurfaceGeometry::Solved(solved)) =
            (self.length_scale_mm, &mut geometry)
        {
            crate::decode::build::units::scale_surface_geometry(solved, scale).map_err(
                |error| match error {
                    CodecError::Malformed(message) => CodecError::NotImplemented(message),
                    other => other,
                },
            )?;
        }
        self.surfaces.insert(surface.id.clone(), source_geometry);
        surface.geometry = geometry;
        Ok(())
    }

    pub(super) fn admit_curve(
        &mut self,
        ir: &mut CadIr,
        mut curve: Curve,
    ) -> Result<(), CodecError> {
        let source_geometry = curve.geometry.clone();
        if let (Some(scale), CurveGeometry::Solved(geometry)) =
            (self.length_scale_mm, &mut curve.geometry)
        {
            crate::decode::build::units::scale_curve_geometry(geometry, scale).map_err(
                |error| match error {
                    CodecError::Malformed(message) => CodecError::NotImplemented(message),
                    other => other,
                },
            )?;
        }
        self.curves.insert(curve.id.clone(), source_geometry);
        ir.model.curves.push(curve);
        Ok(())
    }

    pub(super) fn curve_geometry<'a>(&'a self, curve: &'a Curve) -> &'a CurveGeometry {
        self.curves.get(&curve.id).unwrap_or(&curve.geometry)
    }

    pub(super) fn contains_curve(&self, id: &CurveId) -> bool {
        self.curves.contains_key(id)
    }

    pub(super) fn admit_procedural_surface(
        &mut self,
        ir: &mut CadIr,
        owner: SurfaceId,
        mut procedural: ProceduralSurface,
    ) -> Result<(), CodecError> {
        let procedural_id = procedural.id.clone();
        if let Some(scale) = self.length_scale_mm {
            crate::decode::build::units::scale_procedural_surface(&mut procedural, scale)?;
        }
        let attached = ir.model.add_procedural_surface(owner, procedural);
        if attached.is_ok() {
            self.procedural_surfaces.insert(procedural_id);
        }
        Ok(())
    }

    pub(super) fn contains_procedural_surface(&self, id: &ProceduralSurfaceId) -> bool {
        self.procedural_surfaces.contains(id)
    }

    #[cfg(test)]
    pub(super) fn record_surface(&mut self, surface: &Surface) {
        self.surfaces
            .insert(surface.id.clone(), surface.geometry.clone());
    }

    pub(super) fn surface_geometry<'a>(&'a self, surface: &'a Surface) -> &'a SurfaceGeometry {
        match self.surfaces.get(&surface.id) {
            Some(geometry) => geometry,
            None => &surface.geometry,
        }
    }

    pub(super) fn contains_surface(&self, id: &SurfaceId) -> bool {
        self.surfaces.contains_key(id)
    }

    pub(super) fn remove_surface(&mut self, id: &SurfaceId) {
        self.surfaces.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
        SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::PositiveReal;

    use super::SourceUnitCarriers;

    #[test]
    fn scaled_cylinder_radius_overflow_refuses_unrepresentable_ir() {
        let geometry = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            f64::MAX,
        )
        .expect("finite source cylinder");
        let surface = Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(geometry)),
            source_object: None,
        };
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = source_carriers
            .admit_surface(&mut ir, surface)
            .expect_err("millimeter radius cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.surfaces.is_empty());
    }

    #[test]
    fn scaled_line_origin_overflow_refuses_unrepresentable_ir() {
        let geometry = cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(f64::MAX, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("finite source line");
        let curve = Curve {
            id: CurveId::mint("creo:visibgeom:curve#1").expect("identity grammar"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(geometry)),
            source_object: None,
        };
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = source_carriers
            .admit_curve(&mut ir, curve)
            .expect_err("millimeter origin cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.curves.is_empty());
    }

    #[test]
    fn extrusion_construction_direction_is_in_millimeters_at_attachment() {
        let surface_id = SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar");
        let surface = Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid plane fixture"),
            )),
            source_object: None,
        };
        let mut ir = CadIr::empty();
        let scale = PositiveReal::new(25.4).expect("inch scale");
        let mut source_carriers = SourceUnitCarriers::new(Some(scale));
        source_carriers
            .admit_surface(&mut ir, surface)
            .expect("surface admission");
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId::mint("creo:visibgeom:extrusion#1").expect("identity grammar"),
            ProceduralSurfaceDefinition::Extrusion(
                cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                    CurveId::mint("creo:visibgeom:curve#1").expect("identity grammar"),
                    None,
                    Vector3::new(0.0, 0.0, 1.0),
                    None,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .expect("valid extrusion fixture"),
            ),
            None,
        );
        source_carriers
            .admit_procedural_surface(&mut ir, surface_id, procedural)
            .expect("procedural attachment");
        let ProceduralSurfaceDefinition::Extrusion(construction) =
            ir.model.procedural_surfaces[0].definition()
        else {
            panic!("procedural construction changed family");
        };
        assert_eq!(construction.direction().get(), Vector3::new(0.0, 0.0, 25.4));
        crate::decode::build::units::normalize_model_lengths(&mut ir, scale, &source_carriers)
            .expect("remaining unit normalization");
        let ProceduralSurfaceDefinition::Extrusion(construction) =
            ir.model.procedural_surfaces[0].definition()
        else {
            panic!("procedural construction changed family");
        };
        assert_eq!(construction.direction().get(), Vector3::new(0.0, 0.0, 25.4));
    }
}
