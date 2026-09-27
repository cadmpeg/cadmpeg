// SPDX-License-Identifier: Apache-2.0
//! Source-unit geometry retained for analyses during IR construction.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{DesignParameter, Feature, FeatureDefinition, ParameterValue};
use cadmpeg_ir::geometry::{
    pcurve::Pcurve, Curve, CurveGeometry, ProceduralCurve, ProceduralSurface, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, SurfaceId};
use cadmpeg_ir::products::Occurrence;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchEntity, SketchEntityId, SketchGeometry,
};
use cadmpeg_ir::topology::{Body, Coedge, Edge, EdgeCarrier, Face, Point, Vertex};
use cadmpeg_ir::transform::Transform;

#[derive(Default)]
pub(super) struct SourceUnitCarriers {
    length_scale_mm: Option<PositiveReal>,
    surfaces: BTreeMap<SurfaceId, SurfaceGeometry>,
    curves: BTreeMap<CurveId, CurveGeometry>,
    edge_parameter_ranges: BTreeMap<EdgeId, [f64; 2]>,
    sketch_entities: BTreeMap<SketchEntityId, SketchGeometry>,
}

impl SourceUnitCarriers {
    pub(super) fn new(length_scale_mm: Option<PositiveReal>) -> Self {
        Self {
            length_scale_mm: length_scale_mm.filter(|scale| scale.get() != 1.0),
            surfaces: BTreeMap::new(),
            curves: BTreeMap::new(),
            edge_parameter_ranges: BTreeMap::new(),
            sketch_entities: BTreeMap::new(),
        }
    }

    fn scale_product_translation(&self, transform: &mut Transform) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            *transform = transform.scaled_translation(scale).ok_or_else(|| {
                CodecError::NotImplemented(
                    "Creo product transform translation cannot be represented in millimeters"
                        .into(),
                )
            })?;
        }
        Ok(())
    }

    pub(super) fn admit_body(&self, ir: &mut CadIr, mut body: Body) -> Result<(), CodecError> {
        if let Some(transform) = body.transform.as_mut() {
            self.scale_product_translation(transform)?;
        }
        ir.model.bodies.push(body);
        Ok(())
    }

    pub(super) fn admit_occurrence(
        &self,
        ir: &mut CadIr,
        mut occurrence: Occurrence,
    ) -> Result<(), CodecError> {
        self.scale_product_translation(&mut occurrence.transform)?;
        if let Some(transform) = occurrence.linked_prototype.as_mut() {
            self.scale_product_translation(transform)?;
        }
        ir.model.occurrences.push(occurrence);
        Ok(())
    }

    pub(super) fn admit_feature(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut feature: Feature,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            let mut definition = feature.evaluation.definition().clone();
            crate::decode::build::units::scale_feature_definition(&mut definition, scale)
                .map_err(Self::unrepresentable_length)?;
            feature.evaluation.set_definition(definition);
        }
        ctx.try_reserve_items(&mut ir.model.features, 1, "creo model features")?;
        ir.model.features.push(feature);
        Ok(())
    }

    pub(super) fn replace_feature_definition(
        &self,
        feature: &mut Feature,
        mut definition: FeatureDefinition,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            crate::decode::build::units::scale_feature_definition(&mut definition, scale)
                .map_err(Self::unrepresentable_length)?;
        }
        feature.evaluation.set_definition(definition);
        Ok(())
    }

    pub(super) fn admit_parameter(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut parameter: DesignParameter,
    ) -> Result<(), CodecError> {
        if let (Some(scale), Some(ParameterValue::Length(length))) =
            (self.length_scale_mm, parameter.value.as_mut())
        {
            crate::decode::build::units::scale_length(length, scale)
                .map_err(Self::unrepresentable_length)?;
        }
        ctx.try_reserve_items(&mut ir.model.parameters, 1, "creo model parameters")?;
        ir.model.parameters.push(parameter);
        Ok(())
    }

    fn unrepresentable_length(error: CodecError) -> CodecError {
        match error {
            CodecError::Malformed(message) => CodecError::NotImplemented(message),
            other => other,
        }
    }

    pub(super) fn admit_sketch(
        &self,
        ir: &mut CadIr,
        mut sketch: Sketch,
    ) -> Result<(), CodecError> {
        if let (Some(scale), Some((origin, _, _))) =
            (self.length_scale_mm, sketch.resolved_placement())
        {
            let origin = origin.scaled(scale).ok_or_else(|| {
                CodecError::NotImplemented(
                    "sketch origin cannot be represented in millimeters".into(),
                )
            })?;
            sketch.placement = sketch.placement.with_origin(origin);
        }
        ir.model.sketches.push(sketch);
        Ok(())
    }

    pub(super) fn admit_sketch_entities(
        &mut self,
        ir: &mut CadIr,
        entities: Vec<SketchEntity>,
    ) -> Result<(), CodecError> {
        for mut entity in entities {
            let source_geometry = entity.geometry.clone();
            if let Some(scale) = self.length_scale_mm {
                crate::decode::build::units::scale_sketch_geometry(&mut entity.geometry, scale)
                    .map_err(Self::unrepresentable_length)?;
            }
            self.sketch_entities
                .insert(entity.id().clone(), source_geometry);
            ir.model.sketch_entities.push(entity);
        }
        Ok(())
    }

    pub(super) fn sketch_geometry<'a>(&'a self, entity: &'a SketchEntity) -> &'a SketchGeometry {
        self.sketch_entities
            .get(entity.id())
            .unwrap_or(&entity.geometry)
    }

    pub(super) fn admit_sketch_constraints(
        &self,
        ir: &mut CadIr,
        constraints: Vec<SketchConstraint>,
    ) -> Result<(), CodecError> {
        for mut constraint in constraints {
            if let Some(scale) = self.length_scale_mm {
                constraint.definition.scale_lengths(scale).map_err(|error| match error {
                    cadmpeg_ir::sketches::scaling::SketchConstraintScaleError::LengthOverflow => {
                        CodecError::NotImplemented("Creo scaled length must be finite".into())
                    }
                    cadmpeg_ir::sketches::scaling::SketchConstraintScaleError::InvalidLocalValue => {
                        CodecError::malformed("invalid sketch constraint local arity or scalar value")
                    }
                })?;
            }
            ir.model.sketch_constraints.push(constraint);
        }
        Ok(())
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
        ir.model.points.push(point);
        Ok(())
    }

    fn scale_tolerance(&self, tolerance: &mut Option<PositiveReal>) -> Result<(), CodecError> {
        if let (Some(scale), Some(current)) = (self.length_scale_mm, tolerance) {
            *current = PositiveReal::new(current.get() * scale.get()).ok_or_else(|| {
                CodecError::NotImplemented(
                    "scaled topology tolerance must be positive and finite".into(),
                )
            })?;
        }
        Ok(())
    }

    pub(super) fn admit_vertex(
        &self,
        ir: &mut CadIr,
        mut vertex: Vertex,
    ) -> Result<(), CodecError> {
        self.scale_tolerance(&mut vertex.tolerance)?;
        ir.model.vertices.push(vertex);
        Ok(())
    }

    pub(super) fn admit_face(&self, ir: &mut CadIr, mut face: Face) -> Result<(), CodecError> {
        self.scale_tolerance(&mut face.tolerance)?;
        ir.model.faces.push(face);
        Ok(())
    }

    pub(super) fn admit_edge(&mut self, ir: &mut CadIr, mut edge: Edge) -> Result<(), CodecError> {
        self.scale_tolerance(&mut edge.tolerance)?;
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

    pub(super) fn admit_coedge(
        &self,
        ir: &mut CadIr,
        mut coedge: Coedge,
    ) -> Result<(), CodecError> {
        if let (Some(scale), Some(use_curve)) = (self.length_scale_mm, &mut coedge.use_curve) {
            let curve = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == use_curve.curve);
            let parameter_scale = curve
                .and_then(|curve| self.curve_geometry(curve).solved())
                .and_then(|geometry| {
                    crate::decode::build::units::curve_parameter_scale(geometry, scale)
                });
            if let Some(parameter_scale) = parameter_scale {
                use_curve.parameter_range = use_curve
                    .parameter_range
                    .scaled(parameter_scale)
                    .ok_or_else(|| {
                        CodecError::NotImplemented(
                            "parameter_range must be finite and ordered".into(),
                        )
                    })?;
            }
        }
        ir.model.coedges.push(coedge);
        Ok(())
    }

    pub(super) fn admit_pcurve(
        &self,
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
        let scales = self.length_scale_mm.and_then(|scale| {
            self.surface_geometry(surface).solved().map(|geometry| {
                crate::decode::build::units::surface_parameter_scales(geometry, scale.get())
            })
        });
        Self::push_pcurve(ir, pcurve, scales)
    }

    pub(super) fn admit_pcurve_with_source_surface(
        &self,
        ir: &mut CadIr,
        pcurve: Pcurve,
        source_surface: &SurfaceGeometry,
    ) -> Result<(), CodecError> {
        let scales = self.length_scale_mm.and_then(|scale| {
            source_surface.solved().map(|geometry| {
                crate::decode::build::units::surface_parameter_scales(geometry, scale.get())
            })
        });
        Self::push_pcurve(ir, pcurve, scales)
    }

    fn push_pcurve(
        ir: &mut CadIr,
        mut pcurve: Pcurve,
        scales: Option<[f64; 2]>,
    ) -> Result<(), CodecError> {
        if let Some(scales) = scales {
            pcurve.geometry.try_scale_coordinates(scales).map_err(|_| {
                CodecError::NotImplemented(format!(
                    "Creo pcurve cannot be represented after unit normalization with scales {scales:?}"
                ))
            })?;
        }
        ir.model.pcurves.push(pcurve);
        Ok(())
    }

    pub(super) fn admit_procedural_surface(
        &mut self,
        ir: &mut CadIr,
        owner: SurfaceId,
        mut procedural: ProceduralSurface,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            crate::decode::build::units::scale_procedural_surface(&mut procedural, scale)?;
        }
        ir.model
            .add_procedural_surface(owner, procedural)
            .map_err(CodecError::malformed)?;
        Ok(())
    }

    pub(super) fn admit_procedural_curve(
        &mut self,
        ir: &mut CadIr,
        owner: CurveId,
        mut procedural: ProceduralCurve,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            crate::decode::build::units::scale_procedural_curve(&mut procedural, scale)?;
        }
        ir.model
            .add_procedural_curve(owner, procedural)
            .map_err(CodecError::malformed)?;
        Ok(())
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

    pub(super) fn remove_surface(&mut self, id: &SurfaceId) {
        self.surfaces.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{
        DesignParameter, Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation,
        FuzzyTolerance, ParameterValue,
    };
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, HelixCurveConstruction, HelixFrame, ProceduralCurve,
        ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
        SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
    use cadmpeg_ir::scalar::PositiveReal;
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
        SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchLocus, SketchPlacement,
        SketchProfiles,
    };
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, CoedgeUseCurve, Edge, EdgeCarrier, Face, FaceLoops,
        ParameterInterval, Point, Sense, Vertex,
    };
    use cadmpeg_ir::transform::Transform;

    use super::SourceUnitCarriers;

    #[test]
    fn feature_admission_refuses_before_model_vector_growth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut ir = CadIr::empty();
        let error = SourceUnitCarriers::default()
            .admit_feature(&ctx, &mut ir, source_feature(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            )))
            .expect_err("one feature needs one model vector row");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo model features"));
        assert!(ir.model.features.is_empty());
    }

    #[test]
    fn parameter_admission_refuses_before_model_vector_growth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut ir = CadIr::empty();
        let error = SourceUnitCarriers::default()
            .admit_parameter(&ctx, &mut ir, source_length_parameter(2.0))
            .expect_err("one parameter needs one model vector row");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo model parameters"));
        assert!(ir.model.parameters.is_empty());
    }

    fn source_feature(definition: FeatureDefinition) -> Feature {
        Feature {
            id: cadmpeg_ir::features::FeatureId::mint("creo:test:feature#1")
                .expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(definition),
            native_ref: None,
        }
    }

    fn source_length_parameter(value: f64) -> DesignParameter {
        DesignParameter {
            id: cadmpeg_ir::features::ParameterId::mint("creo:test:parameter#1")
                .expect("identity grammar"),
            owner: None,
            ordinal: 0,
            name: "length".into(),
            expression: "length".into(),
            display: None,
            value: Some(ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(value).expect("finite source length"),
            )),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        }
    }

    fn source_sketch(origin: Point3) -> Sketch {
        Sketch {
            id: cadmpeg_ir::sketches::SketchId::mint("creo:test:sketch#1")
                .expect("identity grammar"),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(
                origin,
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("source placement"),
            profiles: SketchProfiles::default(),
            native_ref: None,
        }
    }

    fn source_sketch_line(x: f64) -> SketchEntity {
        SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint("creo:test:sketch_entity#1")
                .expect("identity grammar"),
            cadmpeg_ir::sketches::SketchId::mint("creo:test:sketch#1").expect("identity grammar"),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(x, 0.0),
                end: cadmpeg_ir::math::Point2::new(2.0, 0.0),
            })
            .expect("source line"),
        )
    }

    fn source_distance_constraint(value: f64) -> SketchConstraint {
        let entity = cadmpeg_ir::sketches::SketchEntityId::mint("creo:test:sketch_entity#1")
            .expect("identity grammar");
        SketchConstraint {
            id: cadmpeg_ir::sketches::SketchConstraintId::mint("creo:test:sketch_constraint#1")
                .expect("identity grammar"),
            sketch: cadmpeg_ir::sketches::SketchId::mint("creo:test:sketch#1")
                .expect("identity grammar"),
            definition: SketchConstraintDefinition::try_from(
                SketchConstraintDefinitionInput::DistanceLociValue {
                    first: SketchLocus::Start(entity.clone()),
                    second: SketchLocus::End(entity),
                    distance: cadmpeg_ir::scalar::Length::new(value)
                        .expect("finite source distance"),
                    parameter: None,
                },
            )
            .expect("valid source distance"),
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        }
    }

    #[test]
    fn planar_sketch_lengths_are_in_millimeters_at_admission() {
        let mut ir = CadIr::empty();
        let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        carriers
            .admit_sketch(&mut ir, source_sketch(Point3::new(1.0, 0.0, 0.0)))
            .expect("sketch admission");
        carriers
            .admit_sketch_entities(&mut ir, vec![source_sketch_line(1.0)])
            .expect("entity admission");
        carriers
            .admit_sketch_constraints(&mut ir, vec![source_distance_constraint(2.0)])
            .expect("constraint admission");
        assert_eq!(
            ir.model.sketches[0]
                .resolved_placement()
                .expect("resolved placement")
                .0
                .get()
                .x,
            25.4
        );
        let SketchGeometryDefinition::Line { start, .. } =
            ir.model.sketch_entities[0].geometry.definition()
        else {
            panic!("sketch line changed family");
        };
        assert_eq!(start.u, 25.4);
        let SketchConstraintDefinitionInput::DistanceLociValue { distance, .. } =
            ir.model.sketch_constraints[0].definition.kind()
        else {
            panic!("distance constraint changed family");
        };
        assert_eq!(distance.get(), 50.8);
        let SketchGeometryDefinition::Line { start, .. } = carriers
            .sketch_geometry(&ir.model.sketch_entities[0])
            .definition()
        else {
            panic!("source sketch line changed family");
        };
        assert_eq!(start.u, 1.0);
    }

    #[test]
    fn sketch_origin_overflow_refuses_before_admission() {
        let mut ir = CadIr::empty();
        let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = carriers
            .admit_sketch(&mut ir, source_sketch(Point3::new(f64::MAX, 0.0, 0.0)))
            .expect_err("millimeter placement cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.sketches.is_empty());
    }

    #[test]
    fn sketch_entity_overflow_refuses_before_admission() {
        let mut ir = CadIr::empty();
        let mut carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = carriers
            .admit_sketch_entities(&mut ir, vec![source_sketch_line(f64::MAX)])
            .expect_err("millimeter line cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.sketch_entities.is_empty());
    }

    #[test]
    fn sketch_constraint_overflow_refuses_before_admission() {
        let mut ir = CadIr::empty();
        let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = carriers
            .admit_sketch_constraints(&mut ir, vec![source_distance_constraint(f64::MAX)])
            .expect_err("millimeter constraint cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.sketch_constraints.is_empty());
    }

    #[test]
    fn datum_offset_distance_is_in_millimeters_at_feature_admission() {
        let mut ir = CadIr::empty();
        let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        crate::decode::with_test_decode_ctx(|ctx| carriers
            .admit_feature(
                ctx,
                &mut ir,
                source_feature(FeatureDefinition::Operation(
                    FeatureOperation::DatumOffsetPlane {
                        reference: None,
                        distance: cadmpeg_ir::scalar::Length::new(2.0).expect("finite distance"),
                    },
                )),
            ))
            .expect("feature admission");
        let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { distance, .. }) =
            ir.model.features[0].evaluation.definition()
        else {
            panic!("datum offset feature changed family");
        };
        assert_eq!(distance.get(), 50.8);
    }

    #[test]
    fn post_process_fuzzy_tolerance_is_in_millimeters_at_feature_admission() {
        let mut ir = CadIr::empty();
        let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        crate::decode::with_test_decode_ctx(|ctx| carriers
            .admit_feature(
                ctx,
                &mut ir,
                source_feature(FeatureDefinition::PostProcess {
                    operation: FeatureOperation::StoredGeometry {},
                    refine: false,
                    fuzzy_tolerance: FuzzyTolerance::Explicit(
                        cadmpeg_ir::scalar::PositiveLength::new(0.5)
                            .expect("positive source tolerance"),
                    ),
                }),
            ))
            .expect("feature admission");
        let FeatureDefinition::PostProcess {
            fuzzy_tolerance: FuzzyTolerance::Explicit(tolerance),
            ..
        } = ir.model.features[0].evaluation.definition()
        else {
            panic!("post process feature changed family");
        };
        assert_eq!(tolerance.get(), 12.7);
    }

    #[test]
    fn feature_length_overflow_refuses_before_admission() {
        let mut ir = CadIr::empty();
        let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = crate::decode::with_test_decode_ctx(|ctx| carriers
            .admit_feature(
                ctx,
                &mut ir,
                source_feature(FeatureDefinition::Operation(
                    FeatureOperation::DatumOffsetPlane {
                        reference: None,
                        distance: cadmpeg_ir::scalar::Length::new(f64::MAX)
                            .expect("finite source distance"),
                    },
                )),
            ))
            .expect_err("millimeter distance cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.features.is_empty());
    }

    #[test]
    fn parameter_length_overflow_refuses_before_admission() {
        let mut ir = CadIr::empty();
        let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = crate::decode::with_test_decode_ctx(|ctx| carriers
            .admit_parameter(ctx, &mut ir, source_length_parameter(f64::MAX)))
            .expect_err("millimeter parameter cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.parameters.is_empty());
    }

    fn translated_product_transform(x: f64) -> Transform {
        Transform::affine([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("finite source translation")
    }

    fn source_occurrence(transform: Transform, linked_prototype: Option<Transform>) -> Occurrence {
        Occurrence {
            id: cadmpeg_ir::ids::OccurrenceId::mint("creo:test:occurrence#0")
                .expect("identity grammar"),
            prototype: PrototypeReference::Local {
                definition: cadmpeg_ir::ids::ProductDefinitionId::mint("creo:test:product#0")
                    .expect("identity grammar"),
            },
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform,
            linked_prototype,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        }
    }

    #[test]
    fn product_transform_translations_are_in_millimeters_at_admission() {
        let mut ir = CadIr::empty();
        let carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        carriers
            .admit_body(
                &mut ir,
                Body {
                    id: cadmpeg_ir::ids::BodyId::mint("creo:test:body#0")
                        .expect("identity grammar"),
                    kind: BodyKind::Solid,
                    regions: Vec::new(),
                    transform: Some(translated_product_transform(1.0)),
                    name: None,
                    color: None,
                    visible: None,
                },
            )
            .expect("body admission");
        carriers
            .admit_occurrence(
                &mut ir,
                source_occurrence(
                    translated_product_transform(2.0),
                    Some(translated_product_transform(3.0)),
                ),
            )
            .expect("occurrence admission");
        assert_eq!(
            ir.model.bodies[0]
                .transform
                .expect("body transform")
                .affine_rows()[0][3],
            25.4
        );
        assert_eq!(ir.model.occurrences[0].transform.affine_rows()[0][3], 50.8);
        assert_eq!(
            ir.model.occurrences[0]
                .linked_prototype
                .expect("linked prototype")
                .affine_rows()[0][3],
            3.0 * 25.4
        );
    }

    #[test]
    fn product_transform_translation_overflow_refuses_before_admission() {
        let mut ir = CadIr::empty();
        let carriers = SourceUnitCarriers::new(PositiveReal::new(1000.0));
        let error = carriers
            .admit_occurrence(
                &mut ir,
                source_occurrence(translated_product_transform(f64::MAX), None),
            )
            .expect_err("a non-finite translation has no transform");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(
            error.to_string().contains("transform translation"),
            "{error}"
        );
        assert!(ir.model.occurrences.is_empty());
    }

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
        let ProceduralSurfaceDefinition::Extrusion(construction) =
            ir.model.procedural_surfaces[0].definition()
        else {
            panic!("procedural construction changed family");
        };
        assert_eq!(construction.direction().get(), Vector3::new(0.0, 0.0, 25.4));
    }

    #[test]
    fn procedural_surface_without_owner_is_malformed_at_attachment() {
        let mut ir = CadIr::empty();
        let mut source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = source_carriers
            .admit_procedural_surface(
                &mut ir,
                SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar"),
                ProceduralSurface::new(
                    ProceduralSurfaceId::mint("creo:visibgeom:extrusion#1")
                        .expect("identity grammar"),
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
                ),
            )
            .expect_err("missing owner must refuse attachment");
        assert!(matches!(error, CodecError::Malformed(_)), "{error}");
        assert!(ir.model.procedural_surfaces.is_empty());
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
            tolerance: PositiveReal::new(0.1),
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
        assert_eq!(
            ir.model.edges[0].tolerance.map(PositiveReal::get),
            Some(2.54)
        );
    }

    fn source_line_for_range_tests() -> (CadIr, SourceUnitCarriers, CurveId) {
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
        (ir, source_carriers, curve_id)
    }

    #[test]
    fn bounded_line_edge_range_overflow_refuses_at_admission() {
        let (mut ir, mut source_carriers, curve_id) = source_line_for_range_tests();
        let vertex =
            cadmpeg_ir::ids::VertexId::mint("creo:visibgeom:vertex#1").expect("identity grammar");
        let error = source_carriers
            .admit_edge(
                &mut ir,
                Edge {
                    id: cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#1")
                        .expect("identity grammar"),
                    carrier: EdgeCarrier::new(Some(curve_id), Some([0.0, f64::MAX]))
                        .expect("finite source range"),
                    start: vertex.clone(),
                    end: vertex,
                    tolerance: None,
                },
            )
            .expect_err("millimeter range overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.edges.is_empty());
    }

    #[test]
    fn coedge_line_use_range_overflow_refuses_at_admission() {
        let (mut ir, source_carriers, curve_id) = source_line_for_range_tests();
        let coedge_id =
            cadmpeg_ir::ids::CoedgeId::mint("creo:visibgeom:coedge#1").expect("identity grammar");
        let error = source_carriers
            .admit_coedge(
                &mut ir,
                Coedge {
                    id: coedge_id.clone(),
                    owner_loop: cadmpeg_ir::ids::LoopId::mint("creo:visibgeom:loop#1")
                        .expect("identity grammar"),
                    edge: cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#1")
                        .expect("identity grammar"),
                    radial_next: coedge_id,
                    sense: Sense::Forward,
                    pcurves: Vec::new(),
                    use_curve: Some(CoedgeUseCurve {
                        curve: curve_id,
                        parameter_range: ParameterInterval::try_from([0.0, f64::MAX])
                            .expect("finite source interval"),
                    }),
                },
            )
            .expect_err("millimeter use range overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.coedges.is_empty());
    }

    #[test]
    fn vertex_face_tolerances_and_coedge_line_range_are_in_millimeters_at_admission() {
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
        source_carriers
            .admit_vertex(
                &mut ir,
                Vertex {
                    id: cadmpeg_ir::ids::VertexId::mint("creo:visibgeom:vertex#1")
                        .expect("identity grammar"),
                    point: cadmpeg_ir::ids::PointId::mint("creo:visibgeom:point#1")
                        .expect("identity grammar"),
                    tolerance: PositiveReal::new(0.5),
                },
            )
            .expect("vertex admission");
        source_carriers
            .admit_face(
                &mut ir,
                Face {
                    id: cadmpeg_ir::ids::FaceId::mint("creo:visibgeom:face#1")
                        .expect("identity grammar"),
                    shell: cadmpeg_ir::ids::ShellId::mint("creo:visibgeom:shell#1")
                        .expect("identity grammar"),
                    surface: SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar"),
                    sense: Sense::Forward,
                    loops: FaceLoops::unspecified(Vec::new()),
                    name: None,
                    color: None,
                    tolerance: PositiveReal::new(0.25),
                },
            )
            .expect("face admission");
        let coedge_id =
            cadmpeg_ir::ids::CoedgeId::mint("creo:visibgeom:coedge#1").expect("identity grammar");
        source_carriers
            .admit_coedge(
                &mut ir,
                Coedge {
                    id: coedge_id.clone(),
                    owner_loop: cadmpeg_ir::ids::LoopId::mint("creo:visibgeom:loop#1")
                        .expect("identity grammar"),
                    edge: cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#1")
                        .expect("identity grammar"),
                    radial_next: coedge_id,
                    sense: Sense::Forward,
                    pcurves: Vec::new(),
                    use_curve: Some(CoedgeUseCurve {
                        curve: curve_id,
                        parameter_range: ParameterInterval::try_from([1.0, 2.0])
                            .expect("bounded source interval"),
                    }),
                },
            )
            .expect("coedge admission");
        assert_eq!(
            ir.model.vertices[0].tolerance.map(PositiveReal::get),
            Some(12.7)
        );
        assert_eq!(
            ir.model.faces[0].tolerance.map(PositiveReal::get),
            Some(6.35)
        );
        assert_eq!(
            ir.model.coedges[0]
                .use_curve
                .as_ref()
                .map(|use_curve| use_curve.parameter_range.endpoints()),
            Some([25.4, 50.8])
        );
    }

    #[test]
    fn topology_tolerance_overflow_refuses_at_admission() {
        let mut ir = CadIr::empty();
        let source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = source_carriers
            .admit_vertex(
                &mut ir,
                Vertex {
                    id: cadmpeg_ir::ids::VertexId::mint("creo:visibgeom:vertex#1")
                        .expect("identity grammar"),
                    point: cadmpeg_ir::ids::PointId::mint("creo:visibgeom:point#1")
                        .expect("identity grammar"),
                    tolerance: PositiveReal::new(f64::MAX),
                },
            )
            .expect_err("millimeter tolerance cannot be represented");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.vertices.is_empty());
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

    #[test]
    fn pcurve_without_owning_surface_is_malformed_at_admission() {
        let mut ir = CadIr::empty();
        let source_carriers = SourceUnitCarriers::new(PositiveReal::new(25.4));
        let error = source_carriers
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
                &SurfaceId::mint("creo:visibgeom:surface#1").expect("identity grammar"),
            )
            .expect_err("pcurve has no owning surface");
        assert!(matches!(error, CodecError::Malformed(_)), "{error}");
        assert!(ir.model.pcurves.is_empty());
    }

    #[test]
    fn plane_pcurve_coordinate_overflow_refuses_at_admission() {
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
        let error = source_carriers
            .admit_pcurve(
                &mut ir,
                cadmpeg_ir::geometry::pcurve::Pcurve {
                    id: cadmpeg_ir::ids::PcurveId::mint("creo:visibgeom:pcurve#1")
                        .expect("identity grammar"),
                    geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                            cadmpeg_ir::math::Point2::new(f64::MAX, 0.0),
                            cadmpeg_ir::math::Point2::new(0.0, 1.0),
                        )
                        .expect("finite source pcurve"),
                    ),
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                        None, None, None,
                    ),
                },
                &surface_id,
            )
            .expect_err("scaled pcurve coordinate overflows");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(ir.model.pcurves.is_empty());
    }
}
