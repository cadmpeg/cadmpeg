// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::CadIr;

use crate::entities::geometry::SourceSequences;
use crate::test_support::test_curves_and_surfaces::parametric_spline_surface_file;
use crate::loss::IgesLossCode;

#[test]
fn spline_surface_retains_one_generated_identity() {
    let bytes = parametric_spline_surface_file();
    let (directory, parameters, global) = crate::test_support::with_service_context(&bytes, |ctx| {
        let scan = crate::card::scan_with_context(&bytes, ctx).expect("cards");
        let (global, _, _storage) = crate::global::parse(&scan, ctx).expect("global");
        let (directory, quarantined) =
            crate::directory::parse(&scan, global.global_table(), ctx).expect("directory");
        assert!(quarantined.is_empty());
        let parameters = crate::parameter::assemble_with_context(
            &scan, &directory, &quarantined, &global, ctx,
        ).expect("parameters").records;
        (directory, parameters, global.length_context().expect("length context"))
    });
    let identity = "iges:model:surface#D1";
    // The two eight-knot lanes, four row headers and sixteen admitted
    // positions move into the polynomial NURBS without a second allocation.
    let lane_bytes = 16 * std::mem::size_of::<f64>()
        + 4 * std::mem::size_of::<Vec<FinitePoint3>>()
        + 16 * std::mem::size_of::<FinitePoint3>();
    let identity_bytes = u64::try_from(lane_bytes + identity.len()).expect("retained length");
    let loss_message = "Type 114 curve and patch types are retained only in native parameters";
    let loss_bytes = loss_message.len() + 4
        + IgesLossCode::SplineHeaderNotTransferred.code().len()
        + "iges".len() + "directory_entry:D1".len();
    let output_bytes = identity_bytes + 2 + 7 + 1
        + u64::try_from(loss_bytes).expect("loss length");
    for refuse_source_object in [true, false] {
        // Prebuild the surface slot outside this session.
        let mut ir = CadIr::empty();
        ir.model.surfaces.reserve_exact(1);
        let before = ir.model.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = if refuse_source_object {
            identity_bytes
        } else {
            output_bytes
        };
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut sequences = SourceSequences::new(&ctx).expect("sequence storage");
        let result = super::super::project(
            &mut ir, &directory, &parameters, &global, &ctx, &mut sequences,
        );
        if refuse_source_object {
            let first = match result.err().expect("source object refusal") {
                CodecError::ResourceLimit(first) => first,
                other => panic!("expected resource refusal: {other:?}"),
            };
            assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(first.operation, "iges source object ID");
            assert_eq!((first.used, first.additional, first.limit),
                (identity_bytes, 2, identity_bytes));
            assert_eq!(ir.model, before);
            for _ in 0..64 {
                assert!(matches!(super::super::project(
                    &mut ir, &directory, &parameters, &global, &ctx, &mut sequences,
                ), Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(ir.model, before);
            }
            drop(sequences);
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let outcome = result.expect("one identity and exact source strings fit");
            assert_eq!(outcome.losses.len(), 1);
            assert_eq!(outcome.losses[0].code, IgesLossCode::SplineHeaderNotTransferred.kind());
            assert_eq!(outcome.losses[0].message, loss_message);
            assert_eq!(outcome.decoded.iter().copied().collect::<Vec<_>>(), [1]);
            assert_eq!(ir.model.surfaces.len(), 1);
            let surface = &ir.model.surfaces[0];
            assert_eq!(surface.id.as_str(), identity);
            assert_eq!(sequences.surface(&surface.id, &ctx).expect("sequence"), Some(1));
            let source = surface.source_object.as_ref().expect("source association");
            assert_eq!(source.object_id.as_str(), "D1");
            assert_eq!(source.name.as_deref(), Some("SPLSURF"));
            assert_eq!(source.layer.as_deref(), Some("0"));
            assert_eq!(ir.model.points, before.points);
            drop(outcome);
            drop(sequences);
            ctx.finish_session().expect("unfused session");
            // The output identity and source association survive scratch release.
            assert_eq!(ir.model.surfaces[0].id.as_str(), identity);
            assert_eq!(ir.model.surfaces[0].source_object.as_ref()
                .expect("retained association").object_id.as_str(), "D1");
        }
    }
}
