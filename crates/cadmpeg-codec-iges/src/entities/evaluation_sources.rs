// SPDX-License-Identifier: Apache-2.0
//! Position lookups for the geometry read by surface evaluation.

use super::evaluation_copy;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    CurveGeometry, CurveOffsetDistanceLaw, CurveOffsetRange, OffsetSide, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::schema::EntitySchema;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, HashSet};

const LOOKUP: &str = "iges evaluation support lookup";
const INDEX: &str = "iges evaluation source index";
const WALK: &str = "iges evaluation support traversal";

#[derive(Default)]
pub(super) struct EvaluationSources {
    surfaces: BTreeMap<String, usize>,
    curves: BTreeMap<String, usize>,
    surface_constructions: BTreeMap<String, usize>,
    curve_constructions: BTreeMap<String, usize>,
    surface_owners: BTreeMap<String, usize>,
    curve_owners: BTreeMap<String, Vec<usize>>,
    indexed_surfaces: usize,
    indexed_procedural_surfaces: usize,
}

fn index_positions<T: EntitySchema>(
    values: &[T],
    positions: &mut BTreeMap<String, usize>,
    first: bool,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (position, value) in ctx.admit_iter(values, INDEX)?.enumerate() {
        if let Some(existing) = ctx.get_mut_btree_map(positions, value.identity(), INDEX)? {
            if !first {
                *existing = position;
            }
        } else {
            let id = ctx.copy_retained_text(value.identity(), INDEX)?;
            ctx.insert_btree_map(positions, id, position, INDEX)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Dependency {
    Surface(usize),
    Curve(usize),
}

impl cadmpeg_core::decode::cost::DecodeCost for Dependency {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

impl EvaluationSources {
    pub(super) fn new(ir: &CadIr, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let mut result = Self::default();
        index_positions(&ir.model.curves, &mut result.curves, false, ctx)?;
        index_positions(
            &ir.model.procedural_curves,
            &mut result.curve_constructions,
            true,
            ctx,
        )?;
        for (position, curve) in ctx.admit_iter(&ir.model.curves, INDEX)?.enumerate() {
            if let Some(construction) = curve.geometry.procedural_construction() {
                if ctx.contains_key_btree_map(
                    &result.curve_constructions,
                    construction.as_str(),
                    INDEX,
                )? {
                    if let Some(owners) =
                        ctx.get_mut_btree_map(&mut result.curve_owners, curve.id.as_str(), INDEX)?
                    {
                        ctx.push_vec(owners, position, INDEX)?;
                    } else {
                        let id = ctx.copy_retained_text(curve.id.as_str(), INDEX)?;
                        ctx.push_btree_group(&mut result.curve_owners, id, position, INDEX, INDEX)?;
                    }
                }
            }
        }
        result.extend_surfaces(ir, ctx)?;
        Ok(result)
    }

    pub(super) fn extend_surfaces(
        &mut self,
        ir: &CadIr,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        for (position, construction) in ctx
            .admit_iter(
                &ir.model.procedural_surfaces[self.indexed_procedural_surfaces..],
                INDEX,
            )?
            .enumerate()
        {
            let position = self.indexed_procedural_surfaces + position;
            if !ctx.contains_key_btree_map(
                &self.surface_constructions,
                construction.id.as_str(),
                INDEX,
            )? {
                let id = ctx.copy_retained_text(construction.id.as_str(), INDEX)?;
                ctx.insert_btree_map(&mut self.surface_constructions, id, position, INDEX)?;
            }
        }
        for (position, surface) in ctx
            .admit_iter(&ir.model.surfaces[self.indexed_surfaces..], INDEX)?
            .enumerate()
        {
            let position = self.indexed_surfaces + position;
            if let Some(existing) =
                ctx.get_mut_btree_map(&mut self.surfaces, surface.id.as_str(), INDEX)?
            {
                *existing = position;
            } else {
                let id = ctx.copy_retained_text(surface.id.as_str(), INDEX)?;
                ctx.insert_btree_map(&mut self.surfaces, id, position, INDEX)?;
            }
            self.observe_surface(&ir.model.surfaces[position], position, ctx)?;
        }
        self.indexed_surfaces = ir.model.surfaces.len();
        self.indexed_procedural_surfaces = ir.model.procedural_surfaces.len();
        Ok(())
    }

    pub(super) fn observe_surface(
        &mut self,
        surface: &cadmpeg_ir::geometry::Surface,
        position: usize,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if let Some(construction) = surface.geometry.procedural_construction() {
            if ctx.contains_key_btree_map(
                &self.surface_constructions,
                construction.as_str(),
                INDEX,
            )? {
                if let Some(owner) =
                    ctx.get_mut_btree_map(&mut self.surface_owners, surface.id.as_str(), INDEX)?
                {
                    *owner = (*owner).max(position);
                } else {
                    let id = ctx.copy_retained_text(surface.id.as_str(), INDEX)?;
                    ctx.insert_btree_map(&mut self.surface_owners, id, position, INDEX)?;
                }
            }
        }
        Ok(())
    }

    fn enqueue_surface(
        &self,
        id: &str,
        queue: &mut Vec<Dependency>,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if let Some(position) = ctx.get_btree_map(&self.surfaces, id, LOOKUP)? {
            ctx.push_vec(queue, Dependency::Surface(*position), WALK)?;
        }
        Ok(())
    }

    fn enqueue_curve(
        &self,
        id: &str,
        queue: &mut Vec<Dependency>,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if let Some(position) = ctx.get_btree_map(&self.curves, id, LOOKUP)? {
            ctx.push_vec(queue, Dependency::Curve(*position), WALK)?;
        }
        Ok(())
    }

    fn curve_dependencies(
        &self,
        geometry: &SolvedCurveGeometry,
        queue: &mut Vec<Dependency>,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested(WALK)?;
        match geometry {
            SolvedCurveGeometry::Composite { segments, .. } => {
                for segment in ctx.admit_iter(&segments[..], WALK)? {
                    self.enqueue_curve(segment.curve.as_str(), queue, ctx)?;
                }
            }
            SolvedCurveGeometry::Transformed(placed) => {
                ctx.charge_work(1, WALK)?;
                self.curve_dependencies(placed.basis(), queue, ctx)?;
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn surface_model(
        &self,
        source: &CadIr,
        id: &str,
        ctx: &DecodeContext<'_>,
    ) -> Result<CadIr, CodecError> {
        let mut result = CadIr::empty();
        result.tolerances = source.tolerances;
        let mut queue = Vec::new();
        let mut visited = HashSet::new();
        let mut procedural_surfaces = HashSet::new();
        let mut procedural_curves = HashSet::new();
        self.enqueue_surface(id, &mut queue, ctx)?;
        while !queue.is_empty() {
            let Some(dependency) =
                ctx.next_charged(&mut std::iter::from_fn(|| queue.pop()), WALK)?
            else {
                break;
            };
            if visited.contains(&dependency) {
                continue;
            }
            ctx.insert_hash_set(&mut visited, dependency, WALK)?;
            match dependency {
                Dependency::Surface(position) => {
                    let surface = &source.model.surfaces[position];
                    if let Some(owner) =
                        ctx.get_btree_map(&self.surface_owners, surface.id.as_str(), LOOKUP)?
                    {
                        let carrier = &source.model.surfaces[*owner];
                        if *owner != position {
                            ctx.push_vec(
                                &mut result.model.surfaces,
                                evaluation_copy::surface(carrier, ctx)?,
                                WALK,
                            )?;
                        }
                        if let Some(construction) = carrier.geometry.procedural_construction() {
                            if let Some(index) = ctx.get_btree_map(
                                &self.surface_constructions,
                                construction.as_str(),
                                LOOKUP,
                            )? {
                                if !procedural_surfaces.contains(index) {
                                    ctx.insert_hash_set(&mut procedural_surfaces, *index, WALK)?;
                                    let procedural = &source.model.procedural_surfaces[*index];
                                    match procedural.definition() {
                                        ProceduralSurfaceDefinition::Offset(payload) => self
                                            .enqueue_surface(
                                                payload.support().as_str(),
                                                &mut queue,
                                                ctx,
                                            )?,
                                        ProceduralSurfaceDefinition::CurveBounded {
                                            support,
                                            ..
                                        } => {
                                            self.enqueue_surface(
                                                support.as_str(),
                                                &mut queue,
                                                ctx,
                                            )?;
                                        }
                                        ProceduralSurfaceDefinition::Extrusion(payload) => self
                                            .enqueue_curve(
                                                payload.directrix().as_str(),
                                                &mut queue,
                                                ctx,
                                            )?,
                                        ProceduralSurfaceDefinition::Revolution(payload) => self
                                            .enqueue_curve(
                                                payload.directrix().as_str(),
                                                &mut queue,
                                                ctx,
                                            )?,
                                        ProceduralSurfaceDefinition::Ruled {
                                            first,
                                            second,
                                            ..
                                        } => {
                                            self.enqueue_curve(first.as_str(), &mut queue, ctx)?;
                                            self.enqueue_curve(second.as_str(), &mut queue, ctx)?;
                                        }
                                        _ => {}
                                    }
                                    ctx.push_vec(
                                        &mut result.model.procedural_surfaces,
                                        evaluation_copy::procedural_surface(procedural, ctx)?,
                                        WALK,
                                    )?;
                                }
                            }
                        }
                    }
                    ctx.push_vec(
                        &mut result.model.surfaces,
                        evaluation_copy::surface(surface, ctx)?,
                        WALK,
                    )?;
                }
                Dependency::Curve(position) => {
                    let curve = &source.model.curves[position];
                    if let Some(geometry) = curve.geometry.solved() {
                        self.curve_dependencies(geometry, &mut queue, ctx)?;
                    }
                    if let Some(owners) =
                        ctx.get_btree_map(&self.curve_owners, curve.id.as_str(), LOOKUP)?
                    {
                        for owner in ctx.admit_iter(owners, WALK)? {
                            let carrier = &source.model.curves[*owner];
                            if *owner != position {
                                ctx.push_vec(
                                    &mut result.model.curves,
                                    evaluation_copy::curve(carrier, ctx)?,
                                    WALK,
                                )?;
                            }
                            if let CurveGeometry::Procedural { construction, .. } =
                                &carrier.geometry
                            {
                                if let Some(index) = ctx.get_btree_map(
                                    &self.curve_constructions,
                                    construction.as_str(),
                                    LOOKUP,
                                )? {
                                    if !procedural_curves.contains(index) {
                                        ctx.insert_hash_set(&mut procedural_curves, *index, WALK)?;
                                        let procedural = &source.model.procedural_curves[*index];
                                        match procedural.definition() {
                                            ProceduralCurveDefinition::Compound(payload) => {
                                                for component in
                                                    ctx.admit_iter(payload.components(), WALK)?
                                                {
                                                    self.enqueue_curve(
                                                        component.component.as_str(),
                                                        &mut queue,
                                                        ctx,
                                                    )?;
                                                }
                                            }
                                            ProceduralCurveDefinition::Offset(payload) => {
                                                self.enqueue_curve(
                                                    payload.source().as_str(),
                                                    &mut queue,
                                                    ctx,
                                                )?;
                                                if let OffsetSide::Direction {
                                                    support: Some(id),
                                                    ..
                                                } = payload.side()
                                                {
                                                    self.enqueue_surface(
                                                        id.as_str(),
                                                        &mut queue,
                                                        ctx,
                                                    )?;
                                                }
                                                if let Some(CurveOffsetRange::Variable {
                                                    distance_law:
                                                        CurveOffsetDistanceLaw::Coordinate {
                                                            function,
                                                            ..
                                                        },
                                                    ..
                                                }) = payload.range()
                                                {
                                                    self.enqueue_curve(
                                                        function.as_str(),
                                                        &mut queue,
                                                        ctx,
                                                    )?;
                                                }
                                            }
                                            _ => {}
                                        }
                                        ctx.push_vec(
                                            &mut result.model.procedural_curves,
                                            evaluation_copy::procedural_curve(procedural, ctx)?,
                                            WALK,
                                        )?;
                                    }
                                }
                            }
                        }
                    }
                    ctx.push_vec(
                        &mut result.model.curves,
                        evaluation_copy::curve(curve, ctx)?,
                        WALK,
                    )?;
                }
            }
        }
        Ok(result)
    }
}
