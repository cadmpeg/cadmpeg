// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn annotation(text: &str, fallback: &str) -> super::super::V2Annotation {
    let bytes = v2_payload(7, &[], text, fallback, false, None);
    with_test_context(&bytes, |ctx| {
        v2_annotation_direct(
            ctx,
            &mut BoundedReader::new(&bytes, 0, bytes.len()).unwrap(),
            MillimeterScale::IDENTITY,
        )
    })
    .expect("valid V2 text")
}

#[test]
fn v2_text_trimming_admits_discarded_characters_and_preserves_unicode() {
    for (text, fallback, expected) in [
        ("\0\t\u{2003}  ", "fallback", ""),
        ("\u{2003}\0é中\n\u{2003}", "fallback", "é中"),
        ("", "\n défaut \0", "défaut"),
        ("", "", ""),
    ] {
        let annotation = annotation(text, fallback);
        assert_eq!(
            v2_effective_text(&cadmpeg_test_support::service_decode_context(), &annotation)
                .unwrap(),
            expected
        );
    }
    for (text, operation) in [
        ("\0\t\u{2003}  ", "Rhino V2 effective text leading boundary"),
        (" é\n\u{2003}", "Rhino V2 effective text trailing boundary"),
    ] {
        let annotation = annotation(text, "fallback");
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let result = v2_effective_text(&ctx, &annotation);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            },
        );
    }
}

#[test]
fn v2_coordinate_checks_do_not_charge_fixed_arrays() {
    for count in [0, 1, 32, 257] {
        let points = vec![[1.0, 2.0]; count];
        let bytes = v2_payload(7, &points, "", "", false, None);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(count).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let annotation = v2_annotation_direct(
            &ctx, &mut BoundedReader::new(&bytes, 0, bytes.len()).unwrap(),
            MillimeterScale::IDENTITY,
        ).expect("only the input-sized point loop visits need admission");
        assert_eq!(annotation.points.len(), count);
        assert!(annotation.points.iter().all(|point| point.get() == [1.0, 2.0]));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn parsed_dimension_fields_release_storage_after_projection_input_drops() {
    for count in [3, 32, 257] {
        let points = vec![[1.0, 2.0]; count];
        let bytes = v2_payload(5, &points, "user", "default", false, None);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        // Point storage plus both UTF-8 strings and the effective rich-text copy.
        let field_bytes = count * std::mem::size_of::<cadmpeg_ir::units::FiniteVector<2>>()
            + "user".len() * 2 + "default".len();
        policy.limits.max_materialized_bytes = u64::try_from(field_bytes).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        for _ in 0..3 {
            let mut storage = ctx.reserve_scoped(0, "test dimension fields").unwrap();
            let dimension = super::super::decode(&ctx, &mut storage, &bytes,
                V2_RADIAL, 0..bytes.len(), MillimeterScale::IDENTITY, ArchiveVersion::V4,
            ).expect("parsed fields consume no retained bytes");
            assert_eq!(dimension.rich_text, "user");
            let DimensionFamily::V2 { points: parsed_points, default_text, .. } = &dimension.family else { panic!("V2 family"); };
            assert_eq!(parsed_points.len(), count);
            assert_eq!(dimension.user_text, "user");
            assert_eq!(default_text, "default");
            drop(dimension);
            drop(storage);
            let reusable = ctx.reserve_scoped(u64::try_from(field_bytes).unwrap(), "next dimension fields").expect("all temporary fields released");
            drop(reusable);
        }
        ctx.finish_session().unwrap();
    }
}

fn dimension_with_unresolved_references() -> super::super::Dimension {
    let family = [3.0_f64, 4.0, 8.0, 9.0].into_iter().flat_map(f64::to_le_bytes).collect::<Vec<_>>();
    let bytes = payload(1, &family);
    let mut dimension = test_decode(&bytes, LINEAR, 0..bytes.len(), MillimeterScale::IDENTITY, ArchiveVersion::V8).unwrap();
    let DimensionFamily::Modern { dimstyle_id } = &mut dimension.family else { panic!("modern family"); };
    *dimstyle_id = Uuid::from_wire([1; 16]);
    dimension.detail_measured = Uuid::from_wire([2; 16]);
    dimension
}

#[test]
fn unresolved_dimension_reference_backing_is_scoped_through_consumption() {
    let dimension = dimension_with_unresolved_references();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test unresolved references").unwrap();
    let error = super::super::project(&ctx, &mut storage, &dimension, "test", None, "", 0).expect_err("reference backing requires temporary storage");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else { panic!("temporary storage refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "Rhino unresolved dimension references");
    drop(storage);
    assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit));

    const SCRATCH_BYTES: u64 = 1 << 20;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test unresolved references").unwrap();
    let (annotation, unresolved) = super::super::project(&ctx, &mut storage, &dimension, "test", None, "", 0).unwrap();
    assert!(matches!(unresolved.as_slice(), [crate::loss::RhinoLossCode::DimensionStyleUnresolved, crate::loss::RhinoLossCode::DimensionDetailReferenceUnresolved]));
    drop(unresolved);
    drop(storage);
    let reusable = ctx.reserve_scoped(SCRATCH_BYTES, "next dimension references").expect("reference backing released while output remains live");
    assert_eq!(annotation.runtime_type, "linear_dimension");
    drop(reusable);
    drop(annotation);
    ctx.finish_session().unwrap();
}

#[test]
fn dimension_semantic_json_retains_only_the_output_text() {
    let dimension = dimension_with_unresolved_references();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::semantic_json(&ctx, &dimension).expect_err("only final JSON needs retained storage");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else { panic!("output storage refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "Rhino dimension semantic JSON");
    assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit));
}
