use super::{DesignDimensionLocusPairs, DesignDimensionNullLocusPairs};
use crate::native::F3dNative;
use crate::records::dimensions::DesignDimensionLocusPair;

fn pair(null_first: bool) -> DesignDimensionLocusPair {
    let shared = r#""id":"f3d:test:dimension-locus-pair#0","companion_record_index":1,"governing_companion_record_index":2,"byte_offset":10,"class_tag":"274","record_index":3,"frame_length":80"#;
    let (opaque, first) = if null_first {
        ("", 0)
    } else {
        (r#","opaque_index":4,"opaque_index_offset":45"#, 40)
    };
    let first_offset = if null_first { 35 } else { 50 };
    let first_role_offset = first_offset + 10;
    let second_offset = first_offset + 15;
    let second_role_offset = second_offset + 10;
    let wire = format!(
        r#"{{{shared}{opaque},"first_geometry_record_index":{first},"first_geometry_reference_offset":{first_offset},"first_role":0,"first_role_offset":{first_role_offset},"second_geometry_record_index":41,"second_geometry_reference_offset":{second_offset},"second_role":1,"second_role_offset":{second_role_offset},"paired_class_tag":"273","paired_byte_offset":90}}"#
    );
    serde_json::from_str(&wire).unwrap()
}

#[test]
fn null_locus_entry_conversion_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let entry = crate::records::dimension_null_locus_wire::Entry::from_null_pair_for_test(pair(true));
    let error = DesignDimensionNullLocusPairs::from_entries_charged(&ctx, vec![entry]).unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "load F3D null locus pairs"
    ));
}

#[test]
fn null_locus_entry_conversion_refuses_source_work_after_valid_output() {
    use cadmpeg_core::decode::ResourceDimension;

    let expected = pair(true);
    let entries = vec![crate::records::dimension_null_locus_wire::Entry::from_null_pair_for_test(pair(true))];
    let result = crate::test_support::with_decode_context(|ctx| {
        DesignDimensionNullLocusPairs::from_entries_charged(ctx, entries)
    })
    .expect("valid null-locus entry converts");
    assert_eq!(&*result, &[expected]);

    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "load F3D null locus pairs",
        0,
        |ctx| {
            DesignDimensionNullLocusPairs::from_entries_charged(
                ctx,
                vec![crate::records::dimension_null_locus_wire::Entry::from_null_pair_for_test(pair(true))],
            )
            .map(|_| ())
            .map_err(cadmpeg_core::CodecError::from)
        },
    );
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "load F3D null locus pairs"
    ));
}

#[test]
fn nonnull_arena_rejects_null_form_at_construction_and_deserialization() {
    let null_pair = pair(true);
    assert!(DesignDimensionLocusPairs::try_from(vec![null_pair.clone()]).is_err());
    let entry = serde_json::to_string(&null_pair).unwrap();
    let wire = format!(r#"{{"design_dimension_locus_pairs":[{entry}]}}"#);
    let error = serde_json::from_str::<F3dNative>(&wire).unwrap_err();
    assert!(error.to_string().contains("design_dimension_locus_pairs"));
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_dimension_locus_pairs",
            &[null_pair],
        )
        .unwrap();
    assert!(F3dNative::load(&namespace).is_err());
}

#[test]
fn null_arena_rejects_nonnull_form_at_construction_and_deserialization() {
    let nonnull_pair = pair(false);
    assert!(DesignDimensionNullLocusPairs::try_from(vec![nonnull_pair.clone()]).is_err());
    let entry = serde_json::to_string(&nonnull_pair).unwrap();
    let wire = format!(r#"{{"design_dimension_null_locus_pairs":[{entry}]}}"#);
    assert!(serde_json::from_str::<F3dNative>(&wire).is_err());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_dimension_null_locus_pairs",
            &[nonnull_pair],
        )
        .unwrap();
    assert!(F3dNative::load(&namespace).is_err());
}

#[test]
fn nonnull_arena_preserves_pair_wire() {
    let pair = pair(false);
    let entry = serde_json::to_string(&pair).unwrap();
    let native = F3dNative {
        design_dimension_locus_pairs: vec![pair].try_into().unwrap(),
        ..F3dNative::default()
    };
    let encoded = serde_json::to_string(&native).unwrap();
    assert!(encoded.contains(&format!(r#""design_dimension_locus_pairs":[{entry}]"#)));
    assert_eq!(serde_json::from_str::<F3dNative>(&encoded).unwrap(), native);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap();
    let arena: Vec<DesignDimensionLocusPair> =
        namespace.arena_as("design_dimension_locus_pairs").unwrap();
    assert_eq!(serde_json::to_string(&arena).unwrap(), format!("[{entry}]"));
    assert_eq!(F3dNative::load(&namespace).unwrap(), native);
}

#[test]
fn null_locus_borrowed_wire_matches_owned_wire_bytes() {
    let pair = pair(true);
    let owned = crate::records::dimension_null_locus_wire::Wire::from(&pair);
    let borrowed = crate::records::dimension_null_locus_wire::BorrowedWire::from(&pair);
    assert_eq!(
        serde_json::to_vec(&borrowed).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn nonnull_locus_conversion_admits_only_reached_validation_steps() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let expected = vec![pair(false), pair(false)];
    let required = u64::try_from(expected.len()).unwrap() + 1;
    for cap in [required, required - 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = DesignDimensionLocusPairs::from_pairs_charged(&ctx, expected.clone());
        if cap == required {
            assert_eq!(&*result.unwrap(), expected.as_slice());
            assert_eq!(ctx.resource_refusal(), None);
        } else {
            let cadmpeg_core::CodecError::ResourceLimit(limit) =
                cadmpeg_core::CodecError::from(result.unwrap_err())
            else {
                panic!("the validation end probe must refuse");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "validate F3D nonnull locus pairs");
            assert_eq!(limit.used, cap);
            assert_eq!(limit.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        }
    }
}

#[test]
fn nonnull_locus_empty_conversion_is_free_and_preserves_original_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(DesignDimensionLocusPairs::from_pairs_charged(&ctx, Vec::new())
        .unwrap()
        .is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    let cadmpeg_core::CodecError::ResourceLimit(original) =
        ctx.charge_work(1, "prior locus refusal").unwrap_err()
    else {
        panic!("zero work limit must refuse");
    };
    let error = DesignDimensionLocusPairs::from_pairs_charged(&ctx, Vec::new()).unwrap_err();
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit == original
    ));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn nonnull_locus_first_invalid_pair_leaves_the_validation_suffix_unread() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut pairs = vec![pair(true)];
    pairs.extend((0..4096).map(|_| pair(false)));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One reached predicate and one actual copy of the original diagnostic.
    policy.limits.max_work_units = 1 + u64::try_from(super::NONNULL_LOCUS_ERROR.len()).unwrap();
    policy.limits.max_retained_bytes = u64::try_from(super::NONNULL_LOCUS_ERROR.len()).unwrap();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = DesignDimensionLocusPairs::from_pairs_charged(&ctx, pairs).unwrap_err();
    let previous: serde_json::Error =
        <serde_json::Error as serde::de::Error>::custom(super::NONNULL_LOCUS_ERROR);
    assert_eq!(error.to_string(), cadmpeg_ir::NativeConvertError::Serde(previous).to_string());
    assert!(matches!(
        error,
        cadmpeg_ir::NativeConvertError::ConversionMessage(ref message)
            if message == super::NONNULL_LOCUS_ERROR
    ));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn nonnull_locus_diagnostic_refuses_before_its_owned_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(super::NONNULL_LOCUS_ERROR.len() - 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = DesignDimensionLocusPairs::from_pairs_charged(&ctx, vec![pair(true)]).unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = cadmpeg_core::CodecError::from(error)
    else {
        panic!("the original diagnostic must exceed retained storage");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "validate F3D nonnull locus pairs");
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, u64::try_from(super::NONNULL_LOCUS_ERROR.len()).unwrap());
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn null_locus_conversion_moves_validated_entries_without_a_second_validation_scan() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let expected = vec![pair(true), pair(true)];
    let entries = expected
        .iter()
        .cloned()
        .map(crate::records::dimension_null_locus_wire::Entry::from_null_pair_for_test)
        .collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Each collect source step is admitted once, including its end probe.
    // Both fixed moves fit the first four-slot allocation.
    policy.limits.max_work_units = u64::try_from(expected.len()).unwrap() + 1;
    policy.limits.max_collection_items = u64::try_from(expected.len()).unwrap();
    policy.limits.max_retained_bytes =
        4 * u64::try_from(std::mem::size_of::<DesignDimensionLocusPair>()).unwrap();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        &*DesignDimensionNullLocusPairs::from_entries_charged(&ctx, entries).unwrap(),
        expected.as_slice()
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn native_load_reaches_nonnull_locus_validation_with_the_original_context() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_dimension_locus_pairs",
            &[pair(false)],
        )
        .unwrap();
    let loaded = crate::test_support::with_decode_context(|ctx| {
        F3dNative::load_charged(ctx, &namespace)
    })
    .unwrap();
    assert_eq!(&*loaded.design_dimension_locus_pairs, &[pair(false)]);
    let refusal = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "validate F3D nonnull locus pairs",
        0,
        |ctx| F3dNative::load_charged(ctx, &namespace),
    );
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "validate F3D nonnull locus pairs"
                && limit.additional == 1
    ));
}
