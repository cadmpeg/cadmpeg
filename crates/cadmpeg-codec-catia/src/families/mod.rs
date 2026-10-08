//! Per-family CATIA record decoders.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, CadIr};

use crate::container::ContainerScan;
use crate::variant::Variant;

pub(crate) mod a5a8;
pub(crate) mod b2;
pub(crate) mod b5;
pub(crate) mod consolidated;
pub(crate) mod e5;
mod freeform;
pub(crate) mod standard;
pub(crate) mod zero_entity;

/// Model layers a family route emits for one decoded storage stream.
pub(crate) struct FamilyOutput {
    pub(crate) ir: CadIr,
    pub(crate) report: DecodeBody,
    pub(crate) annotations: Annotations,
    pub(crate) unknowns: Vec<UnknownRecord>,
    pub(crate) admitted_model_entities: u64,
}

pub(crate) struct FamilyEntityAdmission<'a, 'b> {
    ctx: &'a DecodeContext<'b>,
    admitted: u64,
}

impl<'a, 'b> FamilyEntityAdmission<'a, 'b> {
    pub(crate) fn new(ctx: &'a DecodeContext<'b>) -> Self {
        Self { ctx, admitted: 0 }
    }

    pub(crate) fn context(&self) -> &'a DecodeContext<'b> {
        self.ctx
    }

    pub(crate) fn charge(&mut self) -> Result<(), CodecError> {
        self.ctx
            .charge_entities(1, "admit CATIA family model entity")?;
        self.admitted += 1;
        Ok(())
    }

    pub(crate) fn reserve_entity<T>(
        &mut self,
        values: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.ctx
            .charge_entities(1, "admit CATIA family model entity")?;
        self.ctx.reserve_vec(values, 1, operation)?;
        self.admitted += 1;
        Ok(())
    }

    pub(crate) fn admitted(&self) -> u64 {
        self.admitted
    }
}

/// Positions of the model curves and procedural constructions, keyed by
/// identity, so each wire member finds its source carrier by one lookup.
pub(crate) struct ModelCurvePositions<'ctx> {
    curves: std::collections::BTreeMap<cadmpeg_ir::ids::CurveId, usize>,
    procedurals: std::collections::BTreeMap<cadmpeg_ir::ids::ProceduralCurveId, usize>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> ModelCurvePositions<'ctx> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        ir: &CadIr,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut positions = Self {
            curves: std::collections::BTreeMap::new(),
            procedurals: std::collections::BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "catia_zero_wire_curve_positions")?,
        };
        for (position, curve) in ctx
            .admit_iter(&ir.model.curves, "catia_zero_wire_curve_positions")?
            .enumerate()
        {
            positions.add_curve(ctx, &curve.id, position)?;
        }
        for (position, procedural) in ctx
            .admit_iter(
                &ir.model.procedural_curves,
                "catia_zero_wire_procedural_positions",
            )?
            .enumerate()
        {
            positions.add_procedural(ctx, &procedural.id, position)?;
        }
        Ok(positions)
    }

    /// Records the first position of a curve identity.
    pub(crate) fn add_curve(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &cadmpeg_ir::ids::CurveId,
        position: usize,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let curves = &mut self.curves;
        self.storage.with_storage(|| {
            if ctx.contains_key_btree_map(curves, id, "catia_zero_wire_curve_positions")? {
                return Ok(());
            }
            let key = id.try_clone_for_decode(ctx, "catia_zero_wire_curve_positions")?;
            ctx.insert_btree_map(curves, key, position, "catia_zero_wire_curve_positions")?;
            Ok(())
        })
    }

    /// Records the first position of a procedural construction identity.
    pub(crate) fn add_procedural(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &cadmpeg_ir::ids::ProceduralCurveId,
        position: usize,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let procedurals = &mut self.procedurals;
        self.storage.with_storage(|| {
            if ctx.contains_key_btree_map(
                procedurals,
                id,
                "catia_zero_wire_procedural_positions",
            )? {
                return Ok(());
            }
            let key = id.try_clone_for_decode(ctx, "catia_zero_wire_procedural_positions")?;
            ctx.insert_btree_map(
                procedurals,
                key,
                position,
                "catia_zero_wire_procedural_positions",
            )?;
            Ok(())
        })
    }

    pub(crate) fn curve(
        &self,
        ctx: &DecodeContext<'_>,
        id: &cadmpeg_ir::ids::CurveId,
    ) -> Result<Option<usize>, cadmpeg_core::CodecError> {
        Ok(ctx
            .get_btree_map(&self.curves, id, "catia_zero_wire_curve_lookup")?
            .copied())
    }

    pub(crate) fn procedural(
        &self,
        ctx: &DecodeContext<'_>,
        id: &cadmpeg_ir::ids::ProceduralCurveId,
    ) -> Result<Option<usize>, cadmpeg_core::CodecError> {
        Ok(ctx
            .get_btree_map(&self.procedurals, id, "catia_zero_wire_procedural_lookup")?
            .copied())
    }
}

/// Positions of the model surfaces, keyed by identity, so a binding finds a
/// surface by one lookup instead of a scan of the arena.
pub(crate) struct ModelSurfacePositions<'ctx> {
    surfaces: std::collections::BTreeMap<cadmpeg_ir::ids::SurfaceId, usize>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> ModelSurfacePositions<'ctx> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        ir: &CadIr,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut positions = Self {
            surfaces: std::collections::BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "catia_model_surface_positions")?,
        };
        for (position, surface) in ctx
            .admit_iter(&ir.model.surfaces, "catia_model_surface_positions")?
            .enumerate()
        {
            positions.add(ctx, &surface.id, position)?;
        }
        Ok(positions)
    }

    /// Records the first position of a surface identity.
    pub(crate) fn add(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &cadmpeg_ir::ids::SurfaceId,
        position: usize,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let surfaces = &mut self.surfaces;
        self.storage.with_storage(|| {
            if ctx.contains_key_btree_map(surfaces, id, "catia_model_surface_positions")? {
                return Ok(());
            }
            let key = id.try_clone_for_decode(ctx, "catia_model_surface_positions")?;
            ctx.insert_btree_map(surfaces, key, position, "catia_model_surface_positions")?;
            Ok(())
        })
    }

    pub(crate) fn get(
        &self,
        ctx: &DecodeContext<'_>,
        id: &cadmpeg_ir::ids::SurfaceId,
    ) -> Result<Option<usize>, cadmpeg_core::CodecError> {
        Ok(ctx
            .get_btree_map(&self.surfaces, id, "catia_model_surface_lookup")?
            .copied())
    }
}

/// One entry in the ordered decode route table.
///
/// `applicable` gates the route on the identified container [`Variant`].
/// `decode` returns `Ok(None)` when the stream does not yield a transferable model;
/// any carrier refusal it read is already in the caller's lane-refusal sink, so
/// the fall-through to the next route does not lose it. A resource refusal
/// returns `Err` and stops routing.
pub(crate) struct Route {
    /// Name the decode report states when this route falls through.
    pub(crate) name: &'static str,
    pub(crate) applicable: fn(Variant) -> bool,
    pub(crate) decode: fn(
        &DecodeContext<'_>,
        &ContainerScan,
        &mut crate::nurbs::LaneRefusals,
    ) -> Result<Option<FamilyOutput>, CodecError>,
    /// The route emits the standard FBB face population.
    pub(crate) standard_face_population: bool,
}

/// Ordered decode routes.
///
/// INVARIANT: slice order is the fallback order. Try each applicable route;
/// finish on the first `Some`. Only [`Variant::FbbOnly`] matches more than one
/// route (standard, then freeform). Every other variant matches exactly one.
pub(crate) const ROUTES: &[Route] = &[
    Route {
        name: "the standard route",
        applicable: |v| matches!(v, Variant::StandardNested | Variant::FbbOnly),
        decode: standard::decode::try_decode_standard,
        standard_face_population: true,
    },
    Route {
        name: "the zero-entity route",
        applicable: |v| v == Variant::ZeroEntity,
        decode: zero_entity::decode::try_decode_zero_entity,
        standard_face_population: false,
    },
    Route {
        name: "the E5 route",
        applicable: |v| v == Variant::E5Stream,
        decode: e5::decode::try_decode_e5,
        standard_face_population: false,
    },
    Route {
        name: "the freeform route",
        applicable: |v| {
            matches!(
                v,
                Variant::FloatPackedInnerNoFbb | Variant::FbbOnly | Variant::InnerNoDirectory
            )
        },
        decode: freeform::try_decode_freeform_surfaces,
        standard_face_population: false,
    },
];
