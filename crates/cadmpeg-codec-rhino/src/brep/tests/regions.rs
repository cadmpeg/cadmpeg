// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn v5_region_topology_userdata_decodes_the_v5_array_grammar() {
    let payload = region_topology_userdata_payload();
    let descriptor = region_topology_userdata_descriptor(0..payload.len());
    let mut warnings = Diagnostics::new();
    let (sides, regions, source_range, loaded) = with_test_context(&payload, |ctx| {
        read_region_topology_userdata(
            ctx,
            &payload,
            &descriptor,
            ArchiveVersion::V5,
            1,
            &mut warnings,
        )
    })
    .expect("V5 region topology userdata");

    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert!(loaded);
    assert_eq!(source_range, Some(0..payload.len()));
    assert_eq!(sides.len(), 2);
    assert_eq!(sides[0].index, 0);
    assert_eq!(sides[0].region, 0);
    assert_eq!(sides[0].face, 0);
    assert_eq!(sides[0].direction, 1);
    assert_eq!(sides[1].direction, -1);
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].region_type, 0);
    assert_eq!(regions[0].sides, vec![0, 1]);
    assert_eq!(
        regions[0].bounds.minimum,
        crate::test_support::point3([-1.0, -1.0, 0.0])
    );
    assert_eq!(
        regions[0].bounds.maximum,
        crate::test_support::point3([2.0, 2.0, 1.0])
    );
}

#[test]
fn v6_region_topology_arrays_unwrap_polymorphic_records() {
    let payload = region_topology_v6_payload();
    let descriptor = region_topology_userdata_descriptor(0..payload.len());
    let mut warnings = Diagnostics::new();
    let (sides, regions, _, loaded) = with_test_context(&payload, |ctx| {
        read_region_topology_userdata(
            ctx,
            &payload,
            &descriptor,
            ArchiveVersion::V6,
            1,
            &mut warnings,
        )
    })
    .expect("V6 region topology userdata");

    assert!(loaded);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(
        sides.iter().map(|side| side.direction).collect::<Vec<_>>(),
        [1, -1]
    );
    assert_eq!(regions[0].sides, vec![0, 1]);
}

#[test]
fn region_record_index_mismatch_is_reported_at_parse() {
    let entries = [
        region_record(9, 0, &[1], [0.0; 6]),
        region_record(-1, 1, &[0], [0.0; 6]),
    ]
    .concat();
    let bytes = region_array(&entries, 2);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let regions = with_test_context(&bytes, |ctx| {
        read_region_records(ctx, &bytes, &mut reader, ArchiveVersion::V5, &mut warnings)
    })
    .expect("regions with redundant indexes");
    assert_eq!(
        regions
            .iter()
            .map(|region| region.region_type)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(regions[0].sides, [1]);
    assert_eq!(regions[1].sides, [0]);
    assert_eq!(
        crate::decode::with_expand_bytes(&[], |expand| warnings
            .messages(expand.ctx())
            .expect("diagnostic traversal fits"))
        .collect::<Vec<_>>(),
        ["redundant Brep region positional index mismatch; serialized array order used"]
    );
}

#[test]
fn brep_region_face_sides_refuse_collection_limit() {
    let bytes = region_array(&region_face_side(0, 0, 0, 1), 1);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let error = with_collection_limit(&bytes, 0, |ctx| {
        read_region_sides(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            &mut Diagnostics::new(),
        )
    })
    .expect_err("one region face side exceeds zero collection items");
    assert!(
        matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep region face sides")
    );
}

#[test]
fn brep_region_side_ranges_refuse_collection_limit() {
    let bytes = region_array(&region_face_side(0, 0, 0, 1), 1);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let error = with_collection_limit(&bytes, 1, |ctx| {
        read_region_sides(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            &mut Diagnostics::new(),
        )
    })
    .expect_err("one region side range exceeds one side item");
    assert!(
        matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep region side ranges")
    );
}

#[test]
fn brep_region_records_refuse_collection_limit() {
    let bytes = region_array(&region_record(0, 1, &[0], [0.0; 6]), 1);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let error = with_collection_limit(&bytes, 0, |ctx| {
        read_region_records(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            &mut Diagnostics::new(),
        )
    })
    .expect_err("one region record exceeds zero collection items");
    assert!(
        matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep region records")
    );
}

#[test]
fn brep_region_record_ranges_refuse_collection_limit() {
    let bytes = region_array(&region_record(0, 1, &[0], [0.0; 6]), 1);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
    let error = with_collection_limit(&bytes, 1, |ctx| {
        read_region_records(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            &mut Diagnostics::new(),
        )
    })
    .expect_err("one region record range exceeds one record item");
    assert!(
        matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep region record ranges")
    );
}

#[test]
fn region_outer_wrapper_preserves_v5_raw_element_boundaries() {
    let mut region_record = Vec::new();
    region_record.extend_from_slice(&0_i32.to_le_bytes());
    region_record.extend_from_slice(&0_i32.to_le_bytes());
    region_record.extend_from_slice(&0_i32.to_le_bytes());
    region_record.extend([0.0_f64; 6].into_iter().flat_map(f64::to_le_bytes));
    let raw_element = anonymous(&{
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend_from_slice(&0_i32.to_le_bytes());
        body.extend(region_record);
        body
    });
    let mut region_prefix = 1_i32.to_le_bytes().to_vec();
    region_prefix.extend_from_slice(&0_i32.to_le_bytes());
    region_prefix.extend_from_slice(&1_i32.to_le_bytes());
    let region_array = anonymous_mixed(&[(&region_prefix, false), (&raw_element, true)]);
    let side_array = anonymous(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut topology_prefix = 1_i32.to_le_bytes().to_vec();
    topology_prefix.extend_from_slice(&0_i32.to_le_bytes());
    let nested = anonymous_mixed(&[
        (&topology_prefix, false),
        (&side_array, true),
        (&region_array, true),
    ]);
    let mut outer_prefix = 1_i32.to_le_bytes().to_vec();
    outer_prefix.extend_from_slice(&1_i32.to_le_bytes());
    outer_prefix.push(1);
    let outer = anonymous_mixed(&[(&outer_prefix, false), (&nested, true)]);
    let mut reader = BoundedReader::new(&outer, 0, outer.len()).expect("reader");
    let mut warnings = Diagnostics::new();
    let (_, regions, _, loaded) = with_test_context(&outer, |ctx| {
        read_regions(
            ctx,
            &outer,
            &mut reader,
            ArchiveVersion::V5,
            0,
            &mut warnings,
        )
    })
    .expect("regions");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(loaded);
    assert_eq!(regions.len(), 1);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn valid_region_topology_survives_semantic_validation() {
    let mut raw = one_face_raw();
    raw.minor = 3;
    raw.face_sides = vec![
        RawBrepFaceSide {
            index: 0,
            region: 1,
            face: 0,
            direction: 1,
            source_range: 0..0,
        },
        RawBrepFaceSide {
            index: 1,
            region: 0,
            face: 0,
            direction: -1,
            source_range: 0..0,
        },
    ];
    raw.regions = vec![
        RawBrepRegion {
            region_type: 0,
            sides: vec![1],
            bounds: raw.bounds,
            source_range: 0..0,
        },
        RawBrepRegion {
            region_type: 1,
            sides: vec![0],
            bounds: raw.bounds,
            source_range: 0..0,
        },
    ];
    let validated =
        with_test_context(&[], |ctx| ValidatedRawBrep::try_new(ctx, raw)).expect("valid regions");
    assert_eq!(validated.raw().regions.len(), 2);
    assert!(validated.warnings().is_empty());
}

#[test]
fn resolved_brep_region_sides_refuse_without_degrading() {
    let mut raw = one_face_raw();
    raw.minor = 3;
    raw.face_sides = vec![
        RawBrepFaceSide {
            index: 0,
            region: 1,
            face: 0,
            direction: 1,
            source_range: 0..0,
        },
        RawBrepFaceSide {
            index: 1,
            region: 0,
            face: 0,
            direction: -1,
            source_range: 0..0,
        },
    ];
    raw.regions = vec![
        RawBrepRegion {
            region_type: 0,
            sides: vec![1],
            bounds: raw.bounds,
            source_range: 0..0,
        },
        RawBrepRegion {
            region_type: 1,
            sides: vec![0],
            bounds: raw.bounds,
            source_range: 0..0,
        },
    ];
    // Two trim/loop membership flag arrays now precede the two region sides.
    let error = GeometryError::from(cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "Rhino resolved Brep region sides",
        |limit| {
            with_collection_limit(&[], limit, |ctx| {
                ValidatedRawBrep::try_new(ctx, raw.clone())
            })
            .map_err(|error| match error {
                GeometryError::Codec(error) => error,
                other => cadmpeg_core::CodecError::malformed(other),
            })
        },
    ));
    assert!(matches!(error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino resolved Brep region sides"));
}

#[test]
fn brep_listed_region_sides_refuse_collection_limit() {
    let mut raw = one_face_raw();
    raw.minor = 3;
    raw.face_sides = vec![
        RawBrepFaceSide {
            index: 0,
            region: 1,
            face: 0,
            direction: 1,
            source_range: 0..0,
        },
        RawBrepFaceSide {
            index: 1,
            region: 0,
            face: 0,
            direction: -1,
            source_range: 0..0,
        },
    ];
    raw.regions = vec![
        RawBrepRegion {
            region_type: 0,
            sides: vec![1],
            bounds: raw.bounds,
            source_range: 0..0,
        },
        RawBrepRegion {
            region_type: 1,
            sides: vec![0],
            bounds: raw.bounds,
            source_range: 0..0,
        },
    ];
    let error = with_collection_limit(&[], 2, |ctx| validate_regions(ctx, &raw))
        .expect_err("one listed side exceeds two resolved face sides");
    assert!(matches!(error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino Brep listed region sides"));
}

#[test]
fn invalid_region_reciprocity_degrades_to_incidence_without_topology_failure() {
    let mut raw = one_face_raw();
    raw.minor = 3;
    raw.face_sides = vec![
        RawBrepFaceSide {
            index: 0,
            region: 1,
            face: 0,
            direction: 1,
            source_range: 0..0,
        },
        RawBrepFaceSide {
            index: 1,
            region: 0,
            face: 0,
            direction: -1,
            source_range: 0..0,
        },
    ];
    raw.regions = vec![
        RawBrepRegion {
            region_type: 0,
            sides: vec![0],
            bounds: raw.bounds,
            source_range: 0..0,
        },
        RawBrepRegion {
            region_type: 1,
            sides: vec![1],
            bounds: raw.bounds,
            source_range: 0..0,
        },
    ];
    let validated = with_test_context(&[], |ctx| ValidatedRawBrep::try_new(ctx, raw))
        .expect("optional regions degrade");
    assert!(validated.raw().regions.is_empty());
    assert_eq!(validated.warnings().len(), 1);
}

fn region_storage_fixture(direction: i32, region_sides: Vec<i32>) -> RawBrep {
    let mut raw = one_face_raw();
    raw.minor = 3;
    raw.face_sides = vec![
        RawBrepFaceSide {
            index: 0,
            region: 0,
            face: 0,
            direction: 1,
            source_range: 0..0,
        },
        RawBrepFaceSide {
            index: 1,
            region: 0,
            face: 0,
            direction,
            source_range: 0..0,
        },
    ];
    raw.regions = vec![RawBrepRegion {
        region_type: 0,
        sides: region_sides,
        bounds: raw.bounds,
        source_range: 0..0,
    }];
    raw
}

fn assert_rejected_region_storage_released(raw: &RawBrep, expected: &str) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // Two provisional sides and two membership flags are live together.
    let scratch_bytes = 2 * std::mem::size_of::<super::super::ResolvedFaceSide>() + 2;
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(scratch_bytes);
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty validation root");
    let error = validate_regions(&ctx, raw).expect_err("invalid optional region topology");
    assert!(matches!(error,
        GeometryError::Malformed(crate::chunks::FramingError::Structural { ref message, .. })
        if message == expected));
    ctx.charge_retained(0, "Rhino rejected region retained storage")
        .expect("rejected sides retain no storage");
    let storage = ctx
        .reserve_scoped(
            cadmpeg_core::decode::u64_from_index(scratch_bytes),
            "Rhino rejected region scratch reuse",
        )
        .expect("all provisional side and membership storage is released");
    drop(storage);
    ctx.finish_session()
        .expect("no resource refusal from optional region rejection");
}

#[test]
fn invalid_region_direction_releases_provisional_sides() {
    assert_rejected_region_storage_released(
        &region_storage_fixture(0, vec![0, 1]),
        "region side direction is invalid",
    );
}

#[test]
fn invalid_region_membership_releases_provisional_sides() {
    assert_rejected_region_storage_released(
        &region_storage_fixture(-1, vec![0]),
        "region membership is not reciprocal",
    );
}

#[test]
fn resolved_region_sides_refuse_retained_limit_at_acceptance() {
    let raw = region_storage_fixture(-1, vec![0, 1]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "Rhino resolved Brep region sides",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty validation root");
            validate_regions(&ctx, &raw).map_err(|error| match error {
                GeometryError::Codec(error) => error,
                other => panic!("unexpected region rejection: {other:?}"),
            })
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected region retention resource refusal");
    };
    assert_eq!(limit.used, 0);
    // Both admitted side records become retained only after all checks pass.
    assert_eq!(
        limit.additional,
        cadmpeg_core::decode::u64_from_index(
            2 * std::mem::size_of::<super::super::ResolvedFaceSide>(),
        )
    );
}
