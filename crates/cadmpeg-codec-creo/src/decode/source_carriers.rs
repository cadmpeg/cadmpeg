// SPDX-License-Identifier: Apache-2.0
//! Source-unit geometry retained for analyses during IR construction.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    pcurve::Pcurve, Curve, CurveGeometry, ProceduralCurve, ProceduralSurface, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CurveId, EdgeId, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId,
};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::topology::{Edge, EdgeCarrier, Point};

#[derive(Default)]
pub(super) struct SourceUnitCarriers {
    length_scale_mm: Option<PositiveReal>,
    surfaces: BTreeMap<SurfaceId, SurfaceGeometry>,
    curves: BTreeMap<CurveId, CurveGeometry>,
    procedural_surfaces: BTreeSet<ProceduralSurfaceId>,
    procedural_curves: BTreeSet<ProceduralCurveId>,
    points: BTreeSet<PointId>,
    edges: BTreeSet<EdgeId>,
    edge_parameter_ranges: BTreeMap<EdgeId, [f64; 2]>,
    pcurves: BTreeSet<PcurveId>,
}

impl SourceUnitCarriers {
    pub(super) fn new(length_scale_mm: Option<PositiveReal>) -> Self {
        Self {
            length_scale_mm: length_scale_mm.filter(|scale| scale.get() != 1.0),
            surfaces: BTreeMap::new(),
            curves: BTreeMap::new(),
            procedural_surfaces: BTreeSet::new(),
            procedural_curves: BTreeSet::new(),
            points: BTreeSet::new(),
            edges: BTreeSet::new(),
            edge_parameter_ranges: BTreeMap::new(),
            pcurves: BTreeSet::new(),
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

    pub(super) fn replace_curve_geometry(
        &mut self,
        curve: &mut Curve,
        mut geometry: CurveGeometry,
    ) -> Result<(), CodecError> {
        let source_geometry = geometry.clone();
        if let (Some(scale), CurveGeometry::Solved(solved)) = (self.length_scale_mm, &mut geometry)
        {
            crate::decode::build::units::scale_curve_geometry(solved, scale).map_err(|error| {
                match error {
                    CodecError::Malformed(message) => CodecError::NotImplemented(message),
                    other => other,
                }
            })?;
        }
        self.curves.insert(curve.id.clone(), source_geometry);
        curve.geometry = geometry;
        Ok(())
    }

    pub(super) fn admit_point(
        &mut self,
        ir: &mut CadIr,
        mut point: Point,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            let position = point.position().scaled(scale).ok_or_else(|| {
                CodecError::NotImplemented("Creo scaled model point must be finite".into())
            })?;
            point.set_position(position);
        }
        self.points.insert(point.id.clone());
        ir.model.points.push(point);
        Ok(())
    }

    pub(super) fn contains_point(&self, id: &PointId) -> bool {
        self.points.contains(id)
    }

    pub(super) fn admit_edge(&mut self, ir: &mut CadIr, mut edge: Edge) -> Result<(), CodecError> {
        let source_range = edge.param_range().map(cadmpeg_ir::units::FiniteVector::get);
        if let (Some(scale), EdgeCarrier::Bounded(curve_id, interval)) =
            (self.length_scale_mm, &mut edge.carrier)
        {
            let curve = ir.model.curves.iter().find(|curve| curve.id == *curve_id);
            let parameter_scale = curve
                .and_then(|curve| self.curve_geometry(curve).solved())
                .and_then(|geometry| {
                    crate::decode::build::units::curve_parameter_scale(geometry, scale)
                });
            if let Some(parameter_scale) = parameter_scale {
                *interval = interval.scaled(parameter_scale).ok_or_else(|| {
                    CodecError::NotImplemented("edge param_range must be finite and ordered".into())
                })?;
            }
        }
        self.edges.insert(edge.id.clone());
        if let Some(source_range) = source_range {
            self.edge_parameter_ranges
                .insert(edge.id.clone(), source_range);
        }
        ir.model.edges.push(edge);
        Ok(())
    }

    pub(super) fn source_edge_parameter_range(&self, edge: &Edge) -> Option<[f64; 2]> {
        self.edge_parameter_ranges
            .get(&edge.id)
            .copied()
            .or_else(|| edge.param_range().map(cadmpeg_ir::units::FiniteVector::get))
    }

    pub(super) fn contains_edge(&self, id: &EdgeId) -> bool {
        self.edges.contains(id)
    }

    pub(super) fn admit_pcurve(
        &mut self,
        ir: &mut CadIr,
        pcurve: Pcurve,
        surface_id: &SurfaceId,
    ) -> Result<(), CodecError> {
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|surface| &surface.id == surface_id)
            .ok_or_else(|| CodecError::malformed("Creo pcurve has no owning surface"))?;
        let source_geometry = self.surface_geometry(surface).clone();
        self.admit_pcurve_with_source_surface(ir, pcurve, &source_geometry)
    }

    pub(super) fn admit_pcurve_with_source_surface(
        &mut self,
        ir: &mut CadIr,
        mut pcurve: Pcurve,
        source_surface: &SurfaceGeometry,
    ) -> Result<(), CodecError> {
        if let (Some(scale), Some(geometry)) = (self.length_scale_mm, source_surface.solved()) {
            let scales =
                crate::decode::build::units::surface_parameter_scales(geometry, scale.get());
            pcurve.geometry.try_scale_coordinates(scales).map_err(|_| {
                CodecError::NotImplemented(format!(
                    "Creo pcurve cannot be represented after unit normalization with scales {scales:?}"
                ))
            })?;
        }
        self.pcurves.insert(pcurve.id.clone());
        ir.model.pcurves.push(pcurve);
        Ok(())
    }

    pub(super) fn contains_pcurve(&self, id: &PcurveId) -> bool {
        self.pcurves.contains(id)
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

    pub(super) fn admit_procedural_curve(
        &mut self,
        ir: &mut CadIr,
        owner: CurveId,
        mut procedural: ProceduralCurve,
    ) -> Result<(), CodecError> {
        let procedural_id = procedural.id.clone();
        if let Some(scale) = self.length_scale_mm {
            crate::decode::build::units::scale_procedural_curve(&mut procedural, scale)?;
        }
        let attached = ir.model.add_procedural_curve(owner, procedural);
        if attached.is_ok() {
            self.procedural_curves.insert(procedural_id);
        }
        Ok(())
    }

    pub(super) fn contains_procedural_curve(&self, id: &ProceduralCurveId) -> bool {
        self.procedural_curves.contains(id)
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
        Curve, CurveGeometry, HelixCurveConstruction, HelixFrame, ProceduralCurve,
        ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
        SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::PositiveReal;
    use cadmpeg_ir::topology::{Edge, EdgeCarrier, Point};

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

    #[test]
    fn helix_construction_lengths_are_in_millimeters_at_attachment() {
        let curve_id = CurveId::mint("creo:depdb:curve#1").expect("identity grammar");
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        source_carriers
            .admit_curve(
                &mut ir,
                Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                    source_object: None,
                },
            )
            .expect("curve admission");
        source_carriers
            .admit_procedural_curve(
                &mut ir,
                curve_id,
                ProceduralCurve::new(
                    ProceduralCurveId::mint("creo:depdb:helix#1").expect("identity grammar"),
                    ProceduralCurveDefinition::Helix(
                        HelixCurveConstruction::try_new(
                            [0.0, 1.0],
                            HelixFrame {
                                center: Point3::new(1.0, 0.0, 0.0),
                                major: Vector3::new(1.0, 0.0, 0.0),
                                minor: Vector3::new(0.0, 1.0, 0.0),
                                pitch: Vector3::new(0.0, 0.0, 1.0),
                                axis: Vector3::new(0.0, 0.0, 1.0),
                            },
                            0.0,
                            None,
                        )
                        .expect("valid helix"),
                    ),
                ),
            )
            .expect("helix attachment");
        let ProceduralCurveDefinition::Helix(helix) = ir.model.procedural_curves[0].definition()
        else {
            panic!("helix construction changed family");
        };
        assert_eq!(helix.center().get(), Point3::new(25.4, 0.0, 0.0));
        assert_eq!(helix.pitch().get(), Vector3::new(0.0, 0.0, 25.4));
    }

    #[test]
    fn topological_point_is_in_millimeters_at_admission() {
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let point = Point::new(
            cadmpeg_ir::ids::PointId::mint("creo:visibgeom:point#1").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                .expect("finite source point"),
            None,
        );
        source_carriers
            .admit_point(&mut ir, point)
            .expect("point admission");
        assert_eq!(
            ir.model.points[0].position().get(),
            Point3::new(25.4, 0.0, 0.0)
        );
    }

    #[test]
    fn bounded_line_edge_range_is_in_millimeters_at_admission() {
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let curve_id = CurveId::mint("creo:visibgeom:curve#1").expect("identity grammar");
        source_carriers
            .admit_curve(
                &mut ir,
                Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(1.0, 0.0, 0.0),
                        )
                        .expect("source line"),
                    )),
                    source_object: None,
                },
            )
            .expect("curve admission");
        let vertex =
            cadmpeg_ir::ids::VertexId::mint("creo:visibgeom:vertex#1").expect("identity grammar");
        let edge = Edge {
            id: cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#1").expect("identity grammar"),
            carrier: EdgeCarrier::new(Some(curve_id), Some([0.0, 2.0])).expect("bounded line"),
            start: vertex.clone(),
            end: vertex,
            tolerance: None,
        };
        source_carriers
            .admit_edge(&mut ir, edge)
            .expect("edge admission");
        assert_eq!(
            ir.model.edges[0]
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([0.0, 50.8])
        );
        assert_eq!(
            source_carriers.source_edge_parameter_range(&ir.model.edges[0]),
            Some([0.0, 2.0])
        );
    }

    #[test]
    fn plane_pcurve_coordinates_are_in_millimeters_at_admission() {
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let surface_id = SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar");
        source_carriers
            .admit_surface(
                &mut ir,
                Surface {
                    id: surface_id.clone(),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                            Point3::new(0.0, 0.0, 0.0),
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(1.0, 0.0, 0.0),
                        )
                        .expect("source plane"),
                    )),
                    source_object: None,
                },
            )
            .expect("surface admission");
        source_carriers
            .admit_pcurve(
                &mut ir,
                cadmpeg_ir::geometry::pcurve::Pcurve {
                    id: cadmpeg_ir::ids::PcurveId::mint("creo:visibgeom:pcurve#1")
                        .expect("identity grammar"),
                    geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                            cadmpeg_ir::math::Point2::new(1.0, 2.0),
                            cadmpeg_ir::math::Point2::new(1.0, 0.0),
                        )
                        .expect("source pcurve"),
                    ),
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                        None, None, None,
                    ),
                },
                &surface_id,
            )
            .expect("pcurve admission");
        let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line) =
            &ir.model.pcurves[0].geometry
        else {
            panic!("pcurve changed family");
        };
        assert_eq!(
            line.origin().get(),
            cadmpeg_ir::math::Point2::new(25.4, 50.8)
        );
    }
}
