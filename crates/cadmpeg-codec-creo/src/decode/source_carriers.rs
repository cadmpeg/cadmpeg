// SPDX-License-Identifier: Apache-2.0
//! Source-unit geometry retained for analyses during IR construction.

use std::collections::{btree_map::Entry, BTreeMap};

use crate::decode::build::units::{malformed_refusal, not_implemented_refusal};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
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

#[derive(Clone, Copy)]
enum SourceContext<'ctx, 'input> {
    Decode(&'ctx DecodeContext<'input>),
    #[cfg(test)]
    Fixture,
}

struct SourceValue<'ctx, T> {
    value: (T, Option<ScopedReservation<'ctx>>),
    _key_and_node_storage: Option<ScopedReservation<'ctx>>,
}

pub(super) struct SourceUnitCarriers<'ctx, 'input> {
    context: SourceContext<'ctx, 'input>,
    length_scale_mm: Option<PositiveReal>,
    surfaces: BTreeMap<SurfaceId, SourceValue<'ctx, SurfaceGeometry>>,
    curves: BTreeMap<CurveId, SourceValue<'ctx, CurveGeometry>>,
    edge_parameter_ranges: BTreeMap<EdgeId, SourceValue<'ctx, [f64; 2]>>,
    sketch_entities: BTreeMap<SketchEntityId, SourceValue<'ctx, SketchGeometry>>,
}

#[cfg(test)]
impl Default for SourceUnitCarriers<'_, '_> {
    fn default() -> Self {
        Self::new(None)
    }
}

impl<'ctx, 'input> SourceUnitCarriers<'ctx, 'input> {
    pub(super) fn for_decode(ctx: &'ctx DecodeContext<'input>, length_scale_mm: Option<PositiveReal>) -> Self {
        Self {
            context: SourceContext::Decode(ctx),
            length_scale_mm: length_scale_mm.filter(|scale| scale.get() != 1.0),
            surfaces: BTreeMap::new(),
            curves: BTreeMap::new(),
            edge_parameter_ranges: BTreeMap::new(),
            sketch_entities: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn new(length_scale_mm: Option<PositiveReal>) -> Self {
        Self {
            context: SourceContext::Fixture,
            length_scale_mm: length_scale_mm.filter(|scale| scale.get() != 1.0),
            surfaces: BTreeMap::new(),
            curves: BTreeMap::new(),
            edge_parameter_ranges: BTreeMap::new(),
            sketch_entities: BTreeMap::new(),
        }
    }

    fn scale_product_translation(
        &self,
        ctx: &DecodeContext<'_>,
        transform: &mut Transform,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            *transform = transform.scaled_translation(scale).ok_or_else(|| {
                not_implemented_refusal(
                    ctx,
                    "Creo product transform translation cannot be represented in millimeters",
                )
            })?;
        }
        Ok(())
    }

    pub(super) fn admit_body(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut body: Body,
    ) -> Result<(), CodecError> {
        if let Some(transform) = body.transform.as_mut() {
            self.scale_product_translation(ctx, transform)?;
        }
        ctx.reserve_vec(&mut ir.model.bodies, 1, "creo model bodies")?;
        ir.model.bodies.push(body);
        Ok(())
    }

    pub(super) fn admit_occurrence(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut occurrence: Occurrence,
    ) -> Result<(), CodecError> {
        self.scale_product_translation(ctx, &mut occurrence.transform)?;
        if let Some(transform) = occurrence.linked_prototype.as_mut() {
            self.scale_product_translation(ctx, transform)?;
        }
        ctx.reserve_vec(&mut ir.model.occurrences, 1, "creo model occurrences")?;
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
            let mut result = Ok(());
            feature.evaluation.edit(|definition, _| {
                result =
                    crate::decode::build::units::scale_feature_definition(ctx, definition, scale);
            });
            result.map_err(Self::unrepresentable_length)?;
        }
        ctx.reserve_vec(&mut ir.model.features, 1, "creo model features")?;
        ir.model.features.push(feature);
        Ok(())
    }

    pub(super) fn replace_feature_definition(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &mut Feature,
        mut definition: FeatureDefinition,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            crate::decode::build::units::scale_feature_definition(ctx, &mut definition, scale)
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
            crate::decode::build::units::scale_length(ctx, length, scale)
                .map_err(Self::unrepresentable_length)?;
        }
        ctx.reserve_vec(&mut ir.model.parameters, 1, "creo model parameters")?;
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
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut sketch: Sketch,
    ) -> Result<(), CodecError> {
        if let (Some(scale), Some((origin, _, _))) =
            (self.length_scale_mm, sketch.resolved_placement())
        {
            let origin = origin.scaled(scale).ok_or_else(|| {
                not_implemented_refusal(ctx, "sketch origin cannot be represented in millimeters")
            })?;
            sketch.placement = sketch.placement.with_origin(origin);
        }
        ctx.reserve_vec(&mut ir.model.sketches, 1, "creo model sketches")?;
        ir.model.sketches.push(sketch);
        Ok(())
    }

    pub(super) fn admit_sketch_entities(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        entities: Vec<SketchEntity>,
    ) -> Result<(), CodecError> {
        for mut entity in ctx.admit_iter(entities, "creo source sketch entity traversal")? {
            let copy_ctx = match self.context {
                SourceContext::Decode(ctx) => ctx,
                #[cfg(test)]
                SourceContext::Fixture => ctx,
            };
            let mut key_storage = match self.context {
                SourceContext::Decode(ctx) => Some(ctx.reserve_scoped(0, "creo source sketch entity IDs")?),
                #[cfg(test)]
                SourceContext::Fixture => None,
            };
            let copy_id = || entity.id().try_clone_for_decode(copy_ctx, "creo source sketch entity IDs");
            let source_id = match key_storage.as_mut() {
                Some(storage) => storage.with_storage(copy_id)?,
                None => copy_id()?,
            };
            let copy_geometry = || entity.geometry.try_clone_for_decode(copy_ctx, "creo source sketch geometry copy");
            let (source_geometry, geometry_storage) = match self.context {
                SourceContext::Decode(ctx) => {
                    let (geometry, storage) = ctx.with_scoped_storage("creo source sketch geometry copy", copy_geometry)?;
                    (geometry, Some(storage))
                }
                #[cfg(test)]
                SourceContext::Fixture => (copy_geometry()?, None),
            };
            let source_geometry = if let Some(scale) = self.length_scale_mm {
                let unscaled = std::mem::replace(&mut entity.geometry, source_geometry);
                let scaled =
                    crate::decode::build::units::scale_sketch_geometry(ctx, unscaled, scale)
                        .map_err(Self::unrepresentable_length)?;
                std::mem::replace(&mut entity.geometry, scaled)
            } else {
                source_geometry
            };
            let insert = || copy_ctx.entry_btree_map(&mut self.sketch_entities, source_id, "creo source sketch entity nodes");
            let entry = match key_storage.as_mut() {
                Some(storage) => storage.with_storage(insert)?,
                None => insert()?,
            };
            match entry {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().value = (source_geometry, geometry_storage);
                }
                Entry::Vacant(entry) => {
                    entry.insert(SourceValue {
                        value: (source_geometry, geometry_storage),
                        _key_and_node_storage: key_storage,
                    });
                }
            }
            ctx.reserve_vec(
                &mut ir.model.sketch_entities,
                1,
                "creo model sketch entities",
            )?;
            ir.model.sketch_entities.push(entity);
        }
        Ok(())
    }

    pub(super) fn sketch_geometry<'a>(&'a self, entity: &'a SketchEntity) -> Result<&'a SketchGeometry, CodecError> {
        let geometry = match self.context {
            SourceContext::Decode(ctx) => ctx.get_btree_map(&self.sketch_entities, entity.id(), "creo source sketch geometry lookup")?,
            #[cfg(test)]
            SourceContext::Fixture => crate::decode::with_test_decode_ctx(|ctx| ctx.get_btree_map(&self.sketch_entities, entity.id(), "creo source sketch geometry lookup"))?,
        };
        Ok(geometry.map(|source| &source.value.0).unwrap_or(&entity.geometry))
    }

    pub(super) fn admit_sketch_constraints(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        constraints: Vec<SketchConstraint>,
    ) -> Result<(), CodecError> {
        ctx.reserve_vec(
            &mut ir.model.sketch_constraints,
            constraints.len(),
            "creo model sketch constraints",
        )?;
        for mut constraint in ctx.admit_iter(constraints, "creo source sketch constraint traversal")? {
            if let Some(scale) = self.length_scale_mm {
                constraint.definition.scale_lengths(scale).map_err(|error| match error {
                    cadmpeg_ir::sketches::scaling::SketchConstraintScaleError::LengthOverflow => {
                        not_implemented_refusal(ctx, "Creo scaled length must be finite")
                    }
                    cadmpeg_ir::sketches::scaling::SketchConstraintScaleError::InvalidLocalValue => {
                        malformed_refusal(ctx, "invalid sketch constraint local arity or scalar value")
                    }
                })?;
            }
            ir.model.sketch_constraints.push(constraint);
        }
        Ok(())
    }

    pub(super) fn admit_surface(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut surface: Surface,
    ) -> Result<(), CodecError> {
        let copy_ctx = match self.context {
            SourceContext::Decode(ctx) => ctx,
            #[cfg(test)]
            SourceContext::Fixture => ctx,
        };
        let mut key_storage = match self.context {
            SourceContext::Decode(ctx) => Some(ctx.reserve_scoped(0, "creo source surface IDs")?),
            #[cfg(test)]
            SourceContext::Fixture => None,
        };
        let copy_id = || surface.id.try_clone_for_decode(copy_ctx, "creo source surface IDs");
        let source_id = match key_storage.as_mut() {
            Some(storage) => storage.with_storage(copy_id)?,
            None => copy_id()?,
        };
        let copy_geometry = || surface.geometry.try_clone_for_decode(copy_ctx, "creo source surface geometry");
        let (source_geometry, geometry_storage) = match self.context {
            SourceContext::Decode(ctx) => {
                let (geometry, storage) = ctx.with_scoped_storage("creo source surface geometry", copy_geometry)?;
                (geometry, Some(storage))
            }
            #[cfg(test)]
            SourceContext::Fixture => (copy_geometry()?, None),
        };
        if let (Some(scale), SurfaceGeometry::Solved(geometry)) =
            (self.length_scale_mm, &mut surface.geometry)
        {
            crate::decode::build::units::scale_surface_geometry(ctx, geometry, scale).map_err(
                |error| match error {
                    CodecError::Malformed(message) => CodecError::NotImplemented(message),
                    other => other,
                },
            )?;
        }
        let insert = || copy_ctx.entry_btree_map(&mut self.surfaces, source_id, "creo source surface nodes");
        let entry = match key_storage.as_mut() {
            Some(storage) => storage.with_storage(insert)?,
            None => insert()?,
        };
        match entry {
            Entry::Occupied(mut entry) => {
                entry.get_mut().value = (source_geometry, geometry_storage);
            }
            Entry::Vacant(entry) => {
                entry.insert(SourceValue {
                    value: (source_geometry, geometry_storage),
                    _key_and_node_storage: key_storage,
                });
            }
        }
        ctx.reserve_vec(&mut ir.model.surfaces, 1, "creo model surfaces")?;
        ir.model.surfaces.push(surface);
        Ok(())
    }

    pub(super) fn replace_surface_geometry(
        &mut self,
        ctx: &DecodeContext<'_>,
        surface: &mut Surface,
        mut geometry: SurfaceGeometry,
    ) -> Result<(), CodecError> {
        let copy_ctx = match self.context {
            SourceContext::Decode(ctx) => ctx,
            #[cfg(test)]
            SourceContext::Fixture => ctx,
        };
        let mut key_storage = match self.context {
            SourceContext::Decode(ctx) => Some(ctx.reserve_scoped(0, "creo replacement source surface IDs")?),
            #[cfg(test)]
            SourceContext::Fixture => None,
        };
        let copy_id = || surface.id.try_clone_for_decode(copy_ctx, "creo replacement source surface IDs");
        let source_id = match key_storage.as_mut() {
            Some(storage) => storage.with_storage(copy_id)?,
            None => copy_id()?,
        };
        let copy_geometry = || geometry.try_clone_for_decode(copy_ctx, "creo replacement source surface geometry");
        let (source_geometry, geometry_storage) = match self.context {
            SourceContext::Decode(ctx) => {
                let (geometry, storage) = ctx.with_scoped_storage("creo replacement source surface geometry", copy_geometry)?;
                (geometry, Some(storage))
            }
            #[cfg(test)]
            SourceContext::Fixture => (copy_geometry()?, None),
        };
        if let (Some(scale), SurfaceGeometry::Solved(solved)) =
            (self.length_scale_mm, &mut geometry)
        {
            crate::decode::build::units::scale_surface_geometry(ctx, solved, scale).map_err(
                |error| match error {
                    CodecError::Malformed(message) => CodecError::NotImplemented(message),
                    other => other,
                },
            )?;
        }
        let insert = || copy_ctx.entry_btree_map(&mut self.surfaces, source_id, "creo replacement source surface nodes");
        let entry = match key_storage.as_mut() {
            Some(storage) => storage.with_storage(insert)?,
            None => insert()?,
        };
        match entry {
            Entry::Occupied(mut entry) => {
                entry.get_mut().value = (source_geometry, geometry_storage);
            }
            Entry::Vacant(entry) => {
                entry.insert(SourceValue {
                    value: (source_geometry, geometry_storage),
                    _key_and_node_storage: key_storage,
                });
            }
        }
        surface.geometry = geometry;
        Ok(())
    }

    pub(super) fn admit_curve(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut curve: Curve,
    ) -> Result<(), CodecError> {
        let copy_ctx = match self.context {
            SourceContext::Decode(ctx) => ctx,
            #[cfg(test)]
            SourceContext::Fixture => ctx,
        };
        let mut key_storage = match self.context {
            SourceContext::Decode(ctx) => Some(ctx.reserve_scoped(0, "creo source curve IDs")?),
            #[cfg(test)]
            SourceContext::Fixture => None,
        };
        let copy_id = || curve.id.try_clone_for_decode(copy_ctx, "creo source curve IDs");
        let source_id = match key_storage.as_mut() {
            Some(storage) => storage.with_storage(copy_id)?,
            None => copy_id()?,
        };
        let copy_geometry = || curve.geometry.try_clone_for_decode(copy_ctx, "creo source curve geometry");
        let (source_geometry, geometry_storage) = match self.context {
            SourceContext::Decode(ctx) => {
                let (geometry, storage) = ctx.with_scoped_storage("creo source curve geometry", copy_geometry)?;
                (geometry, Some(storage))
            }
            #[cfg(test)]
            SourceContext::Fixture => (copy_geometry()?, None),
        };
        if let (Some(scale), CurveGeometry::Solved(geometry)) =
            (self.length_scale_mm, &mut curve.geometry)
        {
            crate::decode::build::units::scale_curve_geometry(ctx, geometry, scale).map_err(
                |error| match error {
                    CodecError::Malformed(message) => CodecError::NotImplemented(message),
                    other => other,
                },
            )?;
        }
        let insert = || copy_ctx.entry_btree_map(&mut self.curves, source_id, "creo source curve nodes");
        let entry = match key_storage.as_mut() {
            Some(storage) => storage.with_storage(insert)?,
            None => insert()?,
        };
        match entry {
            Entry::Occupied(mut entry) => {
                entry.get_mut().value = (source_geometry, geometry_storage);
            }
            Entry::Vacant(entry) => {
                entry.insert(SourceValue {
                    value: (source_geometry, geometry_storage),
                    _key_and_node_storage: key_storage,
                });
            }
        }
        ctx.reserve_vec(&mut ir.model.curves, 1, "creo model curves")?;
        ir.model.curves.push(curve);
        Ok(())
    }

    pub(super) fn curve_geometry<'a>(&'a self, curve: &'a Curve) -> Result<&'a CurveGeometry, CodecError> {
        let geometry = match self.context {
            SourceContext::Decode(ctx) => ctx.get_btree_map(&self.curves, &curve.id, "creo source curve geometry lookup")?,
            #[cfg(test)]
            SourceContext::Fixture => crate::decode::with_test_decode_ctx(|ctx| ctx.get_btree_map(&self.curves, &curve.id, "creo source curve geometry lookup"))?,
        };
        Ok(geometry.map(|source| &source.value.0).unwrap_or(&curve.geometry))
    }

    pub(super) fn replace_curve_geometry(
        &mut self,
        ctx: &DecodeContext<'_>,
        curve: &mut Curve,
        mut geometry: CurveGeometry,
    ) -> Result<(), CodecError> {
        let copy_ctx = match self.context {
            SourceContext::Decode(ctx) => ctx,
            #[cfg(test)]
            SourceContext::Fixture => ctx,
        };
        let mut key_storage = match self.context {
            SourceContext::Decode(ctx) => Some(ctx.reserve_scoped(0, "creo replacement source curve IDs")?),
            #[cfg(test)]
            SourceContext::Fixture => None,
        };
        let copy_id = || curve.id.try_clone_for_decode(copy_ctx, "creo replacement source curve IDs");
        let source_id = match key_storage.as_mut() {
            Some(storage) => storage.with_storage(copy_id)?,
            None => copy_id()?,
        };
        let copy_geometry = || geometry.try_clone_for_decode(copy_ctx, "creo replacement source curve geometry");
        let (source_geometry, geometry_storage) = match self.context {
            SourceContext::Decode(ctx) => {
                let (geometry, storage) = ctx.with_scoped_storage("creo replacement source curve geometry", copy_geometry)?;
                (geometry, Some(storage))
            }
            #[cfg(test)]
            SourceContext::Fixture => (copy_geometry()?, None),
        };
        if let (Some(scale), CurveGeometry::Solved(solved)) = (self.length_scale_mm, &mut geometry)
        {
            crate::decode::build::units::scale_curve_geometry(ctx, solved, scale).map_err(
                |error| match error {
                    CodecError::Malformed(message) => CodecError::NotImplemented(message),
                    other => other,
                },
            )?;
        }
        let insert = || copy_ctx.entry_btree_map(&mut self.curves, source_id, "creo replacement source curve nodes");
        let entry = match key_storage.as_mut() {
            Some(storage) => storage.with_storage(insert)?,
            None => insert()?,
        };
        match entry {
            Entry::Occupied(mut entry) => {
                entry.get_mut().value = (source_geometry, geometry_storage);
            }
            Entry::Vacant(entry) => {
                entry.insert(SourceValue {
                    value: (source_geometry, geometry_storage),
                    _key_and_node_storage: key_storage,
                });
            }
        }
        curve.geometry = geometry;
        Ok(())
    }

    pub(super) fn admit_point(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut point: Point,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            let position = point.position().scaled(scale).ok_or_else(|| {
                not_implemented_refusal(ctx, "Creo scaled model point must be finite")
            })?;
            point.set_position(position);
        }
        ctx.reserve_vec(&mut ir.model.points, 1, "creo model points")?;
        ir.model.points.push(point);
        Ok(())
    }

    fn scale_tolerance(
        &self,
        ctx: &DecodeContext<'_>,
        tolerance: &mut Option<PositiveReal>,
    ) -> Result<(), CodecError> {
        if let (Some(scale), Some(current)) = (self.length_scale_mm, tolerance) {
            *current = PositiveReal::new(current.get() * scale.get()).ok_or_else(|| {
                not_implemented_refusal(
                    ctx,
                    "scaled topology tolerance must be positive and finite",
                )
            })?;
        }
        Ok(())
    }

    pub(super) fn admit_vertex(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut vertex: Vertex,
    ) -> Result<(), CodecError> {
        self.scale_tolerance(ctx, &mut vertex.tolerance)?;
        ctx.reserve_vec(&mut ir.model.vertices, 1, "creo model vertices")?;
        ir.model.vertices.push(vertex);
        Ok(())
    }

    pub(super) fn admit_face(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut face: Face,
    ) -> Result<(), CodecError> {
        self.scale_tolerance(ctx, &mut face.tolerance)?;
        ctx.reserve_vec(&mut ir.model.faces, 1, "creo model faces")?;
        ir.model.faces.push(face);
        Ok(())
    }

    pub(super) fn admit_edge(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut edge: Edge,
    ) -> Result<(), CodecError> {
        self.scale_tolerance(ctx, &mut edge.tolerance)?;
        let source_range = edge.param_range().map(cadmpeg_ir::units::FiniteVector::get);
        if let (Some(scale), EdgeCarrier::Bounded(curve_id, interval)) =
            (self.length_scale_mm, &mut edge.carrier)
        {
            let curve = ctx.find_by(&ir.model.curves,
                |curve| ctx.equal(&curve.id, curve_id, "creo source edge curve ID comparison"),
                "creo source edge curve search")?;
            let parameter_scale = curve
                .map(|curve| self.curve_geometry(curve)).transpose()?.and_then(CurveGeometry::solved)
                .map(|geometry| {
                    crate::decode::build::units::curve_parameter_scale(ctx, geometry, scale)
                })
                .transpose()?
                .flatten();
            if let Some(parameter_scale) = parameter_scale {
                *interval = interval.scaled(parameter_scale).ok_or_else(|| {
                    not_implemented_refusal(ctx, "edge param_range must be finite and ordered")
                })?;
            }
        }
        if let Some(source_range) = source_range {
            let copy_ctx = match self.context {
                SourceContext::Decode(ctx) => ctx,
                #[cfg(test)]
                SourceContext::Fixture => ctx,
            };
            let mut key_storage = match self.context {
                SourceContext::Decode(ctx) => Some(ctx.reserve_scoped(0, "creo source edge range IDs")?),
                #[cfg(test)]
                SourceContext::Fixture => None,
            };
            let copy_id = || edge.id.try_clone_for_decode(copy_ctx, "creo source edge range IDs");
            let id = match key_storage.as_mut() {
                Some(storage) => storage.with_storage(copy_id)?,
                None => copy_id()?,
            };
            let insert = || copy_ctx.entry_btree_map(&mut self.edge_parameter_ranges, id, "creo source edge range nodes");
            let entry = match key_storage.as_mut() {
                Some(storage) => storage.with_storage(insert)?,
                None => insert()?,
            };
            match entry {
                Entry::Occupied(mut entry) => entry.get_mut().value.0 = source_range,
                Entry::Vacant(entry) => {
                    entry.insert(SourceValue {
                        value: (source_range, None),
                        _key_and_node_storage: key_storage,
                    });
                }
            }
        }
        ctx.reserve_vec(&mut ir.model.edges, 1, "creo model edges")?;
        ir.model.edges.push(edge);
        Ok(())
    }

    pub(super) fn source_edge_parameter_range(&self, edge: &Edge) -> Result<Option<[f64; 2]>, CodecError> {
        let source = match self.context {
            SourceContext::Decode(ctx) => ctx.get_btree_map(&self.edge_parameter_ranges, &edge.id, "creo source edge range lookup")?,
            #[cfg(test)]
            SourceContext::Fixture => crate::decode::with_test_decode_ctx(|ctx| ctx.get_btree_map(&self.edge_parameter_ranges, &edge.id, "creo source edge range lookup"))?,
        };
        Ok(source.map(|source| source.value.0)
            .or_else(|| edge.param_range().map(cadmpeg_ir::units::FiniteVector::get)))
    }

    pub(super) fn admit_coedge(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut coedge: Coedge,
    ) -> Result<(), CodecError> {
        if let (Some(scale), Some(use_curve)) = (self.length_scale_mm, &mut coedge.use_curve) {
            let curve = ctx.find_by(&ir.model.curves,
                |curve| ctx.equal(&curve.id, &use_curve.curve, "creo source coedge curve ID comparison"),
                "creo source coedge curve search")?;
            let parameter_scale = curve
                .map(|curve| self.curve_geometry(curve)).transpose()?.and_then(CurveGeometry::solved)
                .map(|geometry| {
                    crate::decode::build::units::curve_parameter_scale(ctx, geometry, scale)
                })
                .transpose()?
                .flatten();
            if let Some(parameter_scale) = parameter_scale {
                use_curve.parameter_range = use_curve
                    .parameter_range
                    .scaled(parameter_scale)
                    .ok_or_else(|| {
                        not_implemented_refusal(ctx, "parameter_range must be finite and ordered")
                    })?;
            }
        }
        ctx.reserve_vec(&mut ir.model.coedges, 1, "creo model coedges")?;
        ir.model.coedges.push(coedge);
        Ok(())
    }

    pub(super) fn admit_pcurve(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        pcurve: Pcurve,
        surface_id: &SurfaceId,
    ) -> Result<(), CodecError> {
        let surface = ctx.find_by(&ir.model.surfaces,
            |surface| ctx.equal(&surface.id, surface_id, "creo source pcurve surface ID comparison"),
            "creo source pcurve surface search")?
            .ok_or_else(|| malformed_refusal(ctx, "Creo pcurve has no owning surface"))?;
        let scales = match self.length_scale_mm {
            Some(scale) => self.surface_geometry(surface)?.solved().map(|geometry| {
                crate::decode::build::units::surface_parameter_scales(ctx, geometry, scale.get())
            }).transpose()?,
            None => None,
        };
        Self::push_pcurve(ctx, ir, pcurve, scales)
    }

    pub(super) fn admit_pcurve_with_source_surface(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        pcurve: Pcurve,
        source_surface: &SurfaceGeometry,
    ) -> Result<(), CodecError> {
        let scales = self
            .length_scale_mm
            .and_then(|scale| {
                source_surface.solved().map(|geometry| {
                    crate::decode::build::units::surface_parameter_scales(
                        ctx,
                        geometry,
                        scale.get(),
                    )
                })
            })
            .transpose()?;
        Self::push_pcurve(ctx, ir, pcurve, scales)
    }

    fn push_pcurve(
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        mut pcurve: Pcurve,
        scales: Option<[f64; 2]>,
    ) -> Result<(), CodecError> {
        if let Some(scales) = scales {
            pcurve.geometry = pcurve.geometry.scaled_coordinates_owned(ctx, scales)?
                .map_err(|_refusal| match ctx.format_retained(
                    format_args!("Creo pcurve cannot be represented after unit normalization with scales {scales:?}"),
                    "creo normalized pcurve refusal text",
                ) {
                    Ok(message) => CodecError::NotImplemented(message),
                    Err(error) => error,
                })?;
        }
        ctx.reserve_vec(&mut ir.model.pcurves, 1, "creo model pcurves")?;
        ir.model.pcurves.push(pcurve);
        Ok(())
    }

    pub(super) fn admit_procedural_surface(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        owner: &SurfaceId,
        mut procedural: ProceduralSurface,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            crate::decode::build::units::scale_procedural_surface(ctx, &mut procedural, scale)?;
        }

        ir.model
            .add_procedural_surface(ctx, owner, procedural)?
            .map_err(CodecError::malformed)
    }

    pub(super) fn admit_procedural_curve(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        owner: &CurveId,
        mut procedural: ProceduralCurve,
    ) -> Result<(), CodecError> {
        if let Some(scale) = self.length_scale_mm {
            crate::decode::build::units::scale_procedural_curve(ctx, &mut procedural, scale)?;
        }

        ir.model
            .add_procedural_curve(ctx, owner, procedural)?
            .map_err(CodecError::malformed)
    }

    #[cfg(test)]
    pub(super) fn record_surface(&mut self, surface: &Surface) {
        self.surfaces
            .insert(surface.id.clone(), SourceValue { value: (surface.geometry.clone(), None), _key_and_node_storage: None });
    }

    pub(super) fn surface_geometry<'a>(&'a self, surface: &'a Surface) -> Result<&'a SurfaceGeometry, CodecError> {
        let geometry = match self.context {
            SourceContext::Decode(ctx) => ctx.get_btree_map(&self.surfaces, &surface.id, "creo source surface geometry lookup")?,
            #[cfg(test)]
            SourceContext::Fixture => crate::decode::with_test_decode_ctx(|ctx| ctx.get_btree_map(&self.surfaces, &surface.id, "creo source surface geometry lookup"))?,
        };
        Ok(geometry.map(|source| &source.value.0).unwrap_or(&surface.geometry))
    }

    pub(super) fn remove_surface(&mut self, id: &SurfaceId) -> Result<(), CodecError> {
        match self.context {
            SourceContext::Decode(ctx) => drop(ctx.remove_btree_map(&mut self.surfaces, id, "creo source surface removal")?),
            #[cfg(test)]
            SourceContext::Fixture => drop(crate::decode::with_test_decode_ctx(|ctx| ctx.remove_btree_map(&mut self.surfaces, id, "creo source surface removal"))?),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
