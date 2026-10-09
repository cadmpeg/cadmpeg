// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn nurbs_surface_declared_population_fuses_the_original_caller_session() {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    // Type128 has (K1+1)*(K2+1) poles, independent of flags and lane payloads.
    for (values, requested) in [
        ([128, 1_000, 1_000, 1, 1], 1_001 * 1_001),
        ([128, i64::MAX, i64::MAX, 1, 1], u64::MAX),
    ] {
        let entity_type = 128;
        let operation = "iges_surface_poles";
        let limit = 1_000_000;
        let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), values.len(),
            values.into_iter().map(|value| Token {
                value: TokenValue::Integer(value), span: 0..0,
            }).collect(), Vec::new());
        let directory = [crate::test_support::directory_target(1, entity_type)];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let result = super::super::project(&mut ir, &directory, &[record], &global, &ctx, &mut sequences);
        let first = match result.as_ref() {
            Err(CodecError::ResourceLimit(first)) => *first,
            _ => panic!("expected the declared population ceiling before lane construction"),
        };
        assert_eq!(first.dimension, ResourceDimension::Codec(operation));
        assert_eq!(first.operation, operation);
        assert_eq!((first.limit, first.used, first.additional), (limit, limit, requested - limit));
        assert_eq!(ctx.resource_refusal(), Some(first));
        assert!(ir.model.curves.is_empty());
        assert!(ir.model.surfaces.is_empty());
        drop(result);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn revolution_control_population_fuses_the_original_caller_session() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};
    use cadmpeg_ir::math::Point3;
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110, form: 0, label: "AXIS".into(), status: "00010000",
            parameters: "110,0,0,0,0,0,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 126, form: 0, label: "PROFILE".into(), status: "00010000",
            parameters: "126,1,1,0,0,1,0,0,0,1,1,1,1,1,0,0,1,0,2,0,1;".into(),
        },
    ]);
    let source = crate::IgesCodec.decode(
        &mut std::io::Cursor::new(&bytes), &DecodeOptions::default(),
    ).unwrap();
    assert!(source.report().losses.is_empty(), "{:?}", source.report().losses);
    assert_eq!(source.ir().model.curves.len(), 2);
    assert_eq!(source.ir().model.edges.len(), 2);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    let directory = [
        crate::test_support::directory_target(1, 110),
        crate::test_support::directory_target(3, 126),
        crate::test_support::directory_target(5, 120),
    ];
    // A full sweep has nine angular controls. 111112*9 = 1000008.
    for count in [2, 111_112] {
        let mut ir = source.ir().clone();
        let denominator = f64::from(u32::try_from(count - 1).unwrap());
        let coordinate = |index| f64::from(u32::try_from(index).unwrap()) / denominator;
        let mut knots = vec![0.0, 0.0];
        knots.extend((1..count - 1).map(coordinate));
        knots.extend([1.0, 1.0]);
        let poles = (0..count).map(|index| Point3::new(1.0, 0.0, 2.0 * coordinate(index))).collect();
        let generatrix = crate::test_support::with_service_context(&bytes, |setup| {
            NurbsCurve::from_lanes(setup, 1, knots, poles, None, false).unwrap().unwrap()
        });
        let curve = ir.model.curves.iter_mut().find(|curve| curve.id.as_str() == "iges:model:curve#D3").unwrap();
        curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(generatrix));
        let values = [
            TokenValue::Integer(120), TokenValue::Integer(1), TokenValue::Integer(3),
            TokenValue::real(0.0), TokenValue::real(std::f64::consts::TAU),
        ];
        let record = ParameterRecord::from_test_tokens(5, 1..2, Vec::new(), values.len(),
            values.into_iter().map(|value| Token { value, span: 0..0 }).collect(), Vec::new());
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let result = super::super::project(&mut ir, &directory, &[record], &global, &ctx, &mut sequences);
        if count == 2 {
            let outcome = result.unwrap();
            assert!(outcome.losses.is_empty(), "{:?}", outcome.losses);
            assert!(outcome.decoded.contains(&5));
            assert_eq!(ir.model.surfaces.len(), 1);
            let Some(SolvedSurfaceGeometry::Nurbs(surface)) = ir.model.surfaces[0].geometry.solved() else {
                panic!("expected the accepted revolution carrier");
            };
            assert_eq!((surface.u_count(), surface.v_count()), (2, 9));
            assert_eq!(ir.model.surfaces[0].id.as_str(), "iges:model:surface#D5");
            drop(outcome);
            assert!(ctx.finish_session().is_ok());
        } else {
            let first = match result.as_ref() {
                Err(CodecError::ResourceLimit(first)) => *first,
                _ => panic!("expected the revolution population ceiling before surface construction"),
            };
            assert_eq!(first.dimension, ResourceDimension::Codec("iges_revolution_poles"));
            assert_eq!(first.operation, "iges_revolution_poles");
            assert_eq!((first.limit, first.used, first.additional), (1_000_000, 1_000_000, 8));
            assert_eq!(ctx.resource_refusal(), Some(first));
            assert!(ir.model.surfaces.is_empty());
            drop(result);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
}
