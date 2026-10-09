// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};
use cadmpeg_ir::CadIr;

use crate::directory::DirectoryEntry;
use crate::entities::geometry::SourceSequences;
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use crate::test_support::test_surface_fixtures::{
    line_surface_of_revolution_file, placed_hyperbola_surface_of_revolution_file,
    placed_surface_of_revolution_file, surface_of_revolution_file,
};
use crate::IgesCodec;

fn prepared(bytes: &[u8]) -> (CadIr, Vec<DirectoryEntry>, Vec<ParameterRecord>, ProjectedGlobal) {
    let (directory, parameters, global) = crate::test_support::with_service_context(bytes, |ctx| {
        let scan = crate::card::scan_with_context(bytes, ctx).expect("cards");
        let (global, _, _storage) = crate::global::parse(&scan, ctx).expect("global");
        let (directory, quarantined) =
            crate::directory::parse(&scan, global.global_table(), ctx).expect("directory");
        assert!(quarantined.is_empty());
        let parameters = crate::parameter::assemble_with_context(
            &scan, &directory, &quarantined, &global, ctx,
        ).expect("parameters").records;
        (directory, parameters, global.length_context().expect("length context"))
    });
    // Preserve decoded curve domains and prebuild all output arena backing.
    // Only the surface owner runs inside the measured session.
    let (mut ir, _, _) = IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("fixture decode").into_parts();
    ir.model.surfaces.clear();
    ir.model.procedural_surfaces.clear();
    ir.model.curves.retain(|curve| !curve.id.as_str().ends_with("-placed-generatrix"));
    (ir, directory, parameters, global)
}

fn exact_curve_identity(bytes: &[u8], placed: bool) {
    let (fixture, directory, parameters, global) = prepared(bytes);
    let (surface, construction, directrix, sequence) = if placed {
        ("iges:model:surface#D7", "iges:model:procedural-surface#D7",
            "iges:model:curve#D7-placed-generatrix", 7)
    } else {
        ("iges:model:surface#D5", "iges:model:procedural-surface#D5",
            "iges:model:curve#D3", 5)
    };
    // The source carrier is minted once. The surface and construction each
    // have two live factory/copy results. Each output source association has
    // its D key, REVOLVE label and layer 0. Placed geometry has two curve IDs.
    let key_bytes = u64::try_from("iges:model:curve#D3".len()).expect("key bytes");
    let source_bytes = 2 + 7 + 1;
    let output_bytes = key_bytes
        + 2 * u64::try_from(surface.len()).expect("surface bytes")
        + 2 * u64::try_from(construction.len()).expect("construction bytes")
        + if placed {
            2 * u64::try_from(directrix.len()).expect("directrix bytes") + 2 * source_bytes
        } else {
            source_bytes
        };
    for refuse_source_object in [true, false] {
        let mut ir = fixture.clone();
        ir.model.surfaces.reserve_exact(1);
        ir.model.procedural_surfaces.reserve_exact(1);
        ir.model.curves.reserve_exact(usize::from(placed));
        let before = ir.model.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The surface source layer is the last retained allocation.
        policy.limits.max_retained_bytes = output_bytes - u64::from(refuse_source_object);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut sequences = SourceSequences::new(&ctx).expect("sequence storage");
        let result = super::super::project(
            &mut ir, &directory, &parameters, &global, &ctx, &mut sequences,
        );
        if refuse_source_object {
            let first = match result.err().expect("source layer refusal") {
                CodecError::ResourceLimit(first) => first,
                other => panic!("expected resource refusal: {other:?}"),
            };
            assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(first.operation, "iges source object layer");
            assert_eq!((first.used, first.additional, first.limit),
                (output_bytes - 1, 1, output_bytes - 1));
            assert!(ir.model.surfaces.is_empty());
            assert!(ir.model.procedural_surfaces.is_empty());
            let refused_model = ir.model.clone();
            for _ in 0..64 {
                assert!(matches!(super::super::project(
                    &mut ir, &directory, &parameters, &global, &ctx, &mut sequences,
                ), Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(ir.model, refused_model);
            }
            drop(sequences);
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let outcome = result.expect("exact retained strings fit");
            assert!(outcome.losses.is_empty());
            assert_eq!(outcome.decoded.iter().copied().collect::<Vec<_>>(), [sequence]);
            assert_eq!(ir.model.surfaces.len(), 1);
            assert_eq!(ir.model.procedural_surfaces.len(), 1);
            assert_eq!(ir.model.curves.len(), before.curves.len() + usize::from(placed));
            assert_eq!(ir.model.surfaces[0].id.as_str(), surface);
            assert!(matches!(&ir.model.surfaces[0].geometry,
                SurfaceGeometry::Procedural { construction: id, cache: None }
                    if id.as_str() == construction));
            let procedural = &ir.model.procedural_surfaces[0];
            assert_eq!(procedural.id.as_str(), construction);
            let ProceduralSurfaceDefinition::Revolution(payload) = procedural.definition() else {
                panic!("expected revolution");
            };
            assert_eq!(payload.directrix().as_str(), directrix);
            assert_eq!(sequences.surface(&ir.model.surfaces[0].id, &ctx).expect("sequence"),
                Some(sequence));
            drop(outcome);
            drop(sequences);
            ctx.finish_session().expect("unfused session");
            // Retained identity survives release of lookup and sequence storage.
            assert_eq!(ir.model.procedural_surfaces[0].id.as_str(), construction);
        }
    }
}

#[test]
fn exact_revolution_moves_the_unplaced_generatrix_identity() {
    exact_curve_identity(&line_surface_of_revolution_file(), false);
}

#[test]
fn exact_revolution_mints_only_the_placed_generatrix_identity() {
    exact_curve_identity(&placed_hyperbola_surface_of_revolution_file(), true);
}

fn nurbs_carrier_identity(bytes: &[u8], directrix: &str, curve_count: usize) {
    let (mut ir, directory, parameters, global) = prepared(bytes);
    crate::test_support::with_service_context(&[], |ctx| {
        let mut sequences = SourceSequences::new(ctx).expect("sequence storage");
        let outcome = super::super::project(
            &mut ir, &directory, &parameters, &global, ctx, &mut sequences,
        ).expect("NURBS revolution");
        assert!(outcome.losses.is_empty());
        assert_eq!(ir.model.procedural_surfaces.len(), 1);
        let ProceduralSurfaceDefinition::Revolution(payload) =
            ir.model.procedural_surfaces[0].definition() else {
                panic!("expected revolution");
            };
        assert_eq!(payload.directrix().as_str(), directrix);
        assert_eq!(ir.model.curves.len(), curve_count);
    });
}

#[test]
fn nurbs_revolution_keeps_placed_carrier_identity() {
    nurbs_carrier_identity(&placed_surface_of_revolution_file(),
        "iges:model:curve#D7-placed-generatrix", 3);
}

#[test]
fn nurbs_revolution_keeps_unplaced_carrier_identity() {
    nurbs_carrier_identity(&surface_of_revolution_file(), "iges:model:curve#D3", 2);
}
