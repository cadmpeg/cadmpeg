// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::CadIr;

#[test]
fn topology_subtype_any_preserves_equality_refusal() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=FACE();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner).unwrap();
    let record = exchange.records().get(&1).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One dispatch-chain visit and one partial visit fit; equality refuses.
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
    let error = super::super::most_specific(&ctx, record, &["FACE"]).unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("subtype comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP most specific equality");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn procedural_surface_owner_lookup_preserves_refusal() {
    let mut ir = CadIr::empty();
    let id = SurfaceId::try_from("step:data:surface#1").unwrap();
    let construction = ProceduralSurfaceId::try_from("step:data:construction#2").unwrap();
    ir.model.surfaces.push(Surface {
        id: id.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction,
        ProceduralSurfaceDefinition::CurveBounded {
            support: id.clone(),
            boundaries: Vec::new(),
            boundary_pcurves: Vec::new(),
            implicit_outer: false,
        },
        None,
    ));
    let index_ctx = cadmpeg_test_support::service_decode_context();
    let index = super::super::PcurveSelectionIndex::build(&ir, &index_ctx).unwrap();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP procedural surface owner lookup",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            super::super::surface_selection_parameter_domains(
                &index,
                &id,
                &ir.model.surfaces[0].geometry,
                &ctx,
            )
        },
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("owner lookup must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "STEP procedural surface owner lookup");
}

#[test]
fn procedural_surface_owner_index_excludes_shared_constructions() {
    let mut ir = CadIr::empty();
    let construction = ProceduralSurfaceId::try_from("step:data:construction#2").unwrap();
    let first = SurfaceId::try_from("step:data:surface#1").unwrap();
    for id in [first.clone(), SurfaceId::try_from("step:data:surface#3").unwrap()] {
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Procedural { construction: construction.clone(), cache: None },
            source_object: None,
        });
    }
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction,
        ProceduralSurfaceDefinition::CurveBounded {
            support: first,
            boundaries: Vec::new(),
            boundary_pcurves: Vec::new(),
            implicit_outer: false,
        },
        None,
    ));
    let ctx = cadmpeg_test_support::service_decode_context();
    let index = super::super::PcurveSelectionIndex::build(&ir, &ctx).unwrap();
    assert!(index.owned_procedurals.is_empty());
}
