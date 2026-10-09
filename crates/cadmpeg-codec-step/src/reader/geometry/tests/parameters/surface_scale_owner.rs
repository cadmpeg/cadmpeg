// SPDX-License-Identifier: Apache-2.0
//! Surface-scale maps and memo storage belong to the constructor's session.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::LineCurve;

use super::{procedural_surface_parameter_scales, BTreeMap, CadIr, Curve, CurveGeometry,
    CurveId, Point3, ProceduralSurface, ProceduralSurfaceDefinition, ProceduralSurfaceId,
    SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry, SurfaceId,
    SurfaceScaleIndex, Vector3};

fn fixture() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("test:model:curve#directrix").expect("curve identity"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
                .expect("finite line"),
        )),
        source_object: None,
    });
    let owner = SurfaceId::mint("test:model:surface#owner").expect("surface identity");
    ir.model.surfaces.push(Surface {
        id: owner.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    });
    ir.model.add_procedural_surface(
        &cadmpeg_ir::document::admission::StandardAdmission,
        &owner,
        ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:procedural-surface#torus")
                .expect("construction identity"),
            ProceduralSurfaceDefinition::DegenerateTorus { select_outer: true },
            None,
        ),
    ).expect("construction admission").expect("owned construction");
    ir
}

fn scale(ir: &CadIr, index: &mut SurfaceScaleIndex<'_, '_>)
    -> Result<Option<[f64; 2]>, CodecError>
{
    let surface = &ir.model.surfaces[0];
    procedural_surface_parameter_scales(ir, index, &surface.id, &surface.geometry,
        [10.0, 0.25], &BTreeMap::new())
}

fn preserves_refusal(populated: bool, query: impl FnOnce(&CadIr, &mut SurfaceScaleIndex<'_, '_>)
    -> Result<(), CodecError>)
{
    let ir = fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("empty root fits policy");
    let mut index = SurfaceScaleIndex::build(&ir, &ctx).expect("scale index");
    if populated {
        assert_eq!(scale(&ir, &mut index).expect("initial scale"), Some([0.25, 0.25]));
    }
    let before = (index.curves.len(), index.surfaces.len(), index.owners.len(),
        index.procedurals.len(), index.owned_procedurals.len(), index.terminals.len(),
        index.generation);
    let CodecError::ResourceLimit(original) = ctx
        .charge_work(u64::MAX, "test original surface-scale refusal")
        .expect_err("original context refuses") else { panic!("resource refusal"); };
    assert!(matches!(query(&ir, &mut index),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert_eq!((index.curves.len(), index.surfaces.len(), index.owners.len(),
        index.procedurals.len(), index.owned_procedurals.len(), index.terminals.len(),
        index.generation), before);
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(index);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}

#[test]
fn surface_scale_memo_miss_preserves_original_session_refusal() {
    preserves_refusal(false, |ir, index| scale(ir, index).map(|_| ()));
}

#[test]
fn surface_scale_memo_hit_preserves_original_session_refusal() {
    preserves_refusal(true, |ir, index| scale(ir, index).map(|_| ()));
}

#[test]
fn surface_scale_curve_lookup_preserves_original_session_refusal() {
    preserves_refusal(false, |ir, index| index.curve(ir, &ir.model.curves[0].id).map(|_| ()));
}

#[test]
fn surface_scale_surface_lookup_preserves_original_session_refusal() {
    preserves_refusal(false, |ir, index| index.surface(ir, &ir.model.surfaces[0].id).map(|_| ()));
}

#[test]
fn surface_scale_procedure_lookup_preserves_original_session_refusal() {
    preserves_refusal(false, |ir, index| index.owned_procedural(ir, &ir.model.surfaces[0].id).map(|_| ()));
}

#[test]
fn surface_scale_surface_append_preserves_original_session_refusal() {
    preserves_refusal(true, |ir, index| index.add_surface(&ir.model.surfaces[0], 1));
}

#[test]
fn surface_scale_procedure_append_preserves_original_session_refusal() {
    preserves_refusal(true, |ir, index| index.add_procedural(&ir.model.procedural_surfaces[0], 1));
}

fn lease_lifetime(append: bool, release: bool) {
    let ir = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let empty = CadIr::empty();
    let (mut index, outer) = ctx.with_scoped_storage("test outer scale scope", ||
        SurfaceScaleIndex::build(if append { &empty } else { &ir }, &ctx))
        .expect("scale index inside outer scope");
    drop(outer);
    if append {
        index.add_surface(&ir.model.surfaces[0], 0).expect("append surface");
        index.add_procedural(&ir.model.procedural_surfaces[0], 0).expect("append procedure");
        assert_eq!(scale(&ir, &mut index).expect("memo scale"), Some([0.25, 0.25]));
        assert_eq!(index.terminals.len(), 1);
    } else {
        assert_eq!(index.curves.len(), 1);
    }
    let mut index = Some(index);
    if release { drop(index.take()); }
    let CodecError::ResourceLimit(original) = ctx
        .reserve_scoped(u64::MAX, "test live scale storage")
        .err().expect("materialized probe refuses") else { panic!("resource refusal"); };
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    if release { assert_eq!(original.used, 0); } else { assert!(original.used > 0); }
    drop(index);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}

#[test]
fn surface_scale_constructor_storage_survives_outer_scope_until_owner_drop() {
    lease_lifetime(false, false);
    lease_lifetime(false, true);
}

#[test]
fn surface_scale_append_and_memo_storage_survive_until_owner_drop() {
    lease_lifetime(true, false);
    lease_lifetime(true, true);
}
