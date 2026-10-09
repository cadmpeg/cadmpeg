// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::test_support::parameter_record;
use crate::records::parameters::{DesignParameter, DesignParameterCompanion};

pub(super) const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

/// A dimension parameter record of `record_index` in [`STREAM`] that opens at
/// `byte_offset`.
pub(super) fn parameter_at(record_index: u32, byte_offset: u64) -> DesignParameter {
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(300),
        "40 mm",
        "Linear Dimension-3",
        Some("mm"),
        "d3",
        4.0,
    ))
    .unwrap();
    parameter.id = format!(
        "{}:design-parameter#{byte_offset}",
        crate::test_support::with_decode_context(|ctx| crate::ids::native_scope(
            ctx,
            STREAM,
            "retain F3D native scope"
        )
        .expect("test F3D native identity"))
    );
    parameter.record_index = record_index;
    let mut wire = serde_json::to_value(&parameter).unwrap();
    for field in [
        "byte_offset",
        "family_discriminator_offset",
        "expression_offset",
        "source_kind_offset",
        "unit_offset",
        "name_offset",
        "evaluated_value_offset",
    ] {
        if let Some(offset) = wire.get(field).and_then(serde_json::Value::as_u64) {
            wire[field] = (offset + byte_offset).into();
        }
    }
    serde_json::from_value(wire).unwrap()
}

#[test]
fn companion_intervals_end_at_every_parameter_record() {
    // Two parameter records of one stream share a record index. Each record
    // ends the companion's owned interval, as in the companion payload, so the
    // interval ends at the first of them.
    let parameters = [parameter_at(301, 100), parameter_at(301, 200)];
    let companion = DesignParameterCompanion::unbound(
        format!(
            "{}:parameter-companion#0",
            crate::test_support::with_decode_context(|ctx| crate::ids::native_scope(
                ctx,
                STREAM,
                "retain F3D native scope"
            )
            .expect("test F3D native identity"))
        ),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        11,
        10,
        std::num::NonZeroU64::MIN,
        42,
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let scope = crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_scope(ctx, STREAM, "retain F3D native scope")
            .expect("test F3D native identity")
    });
    let mut intervals =
        super::super::CompanionIntervals::new(&ctx, &parameters, &[], &[], &[]).unwrap();
    assert_eq!(
        intervals.interval(&scope, &companion, 300).unwrap(),
        Some((58, 100))
    );
}

/// A scope of record `record_index` at `byte_offset` that references `members`.
fn scope_at(
    record_index: u32,
    byte_offset: u64,
    members: Vec<u32>,
) -> crate::records::feature::scope::DesignParameterScope {
    let offsets = members.iter().map(|_| 0).collect();
    crate::records::feature::scope::DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: format!("f3d:native:parameter-scope#{record_index}"),
            byte_offset,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index,
            frame_length: 200,
            kind_offset: 0,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: byte_offset + 9,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                members,
                offsets,
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::Extrude
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

fn header_at(record_index: u32, byte_offset: u64) -> crate::records::decal::DesignRecordHeader {
    crate::records::decal::DesignRecordHeader {
        id: format!("f3d:native:record-header#{byte_offset}"),
        record_index,
        class_tag: crate::records::references::DesignClassTag::try_from("302".to_owned()).unwrap(),
        byte_offset,
    }
}

#[test]
fn companion_interval_ends_at_first_header_another_scope_references() {
    let companion = DesignParameterCompanion::unbound(
        "f3d:native:parameter-companion#302".into(),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        302,
        300,
        std::num::NonZeroU64::MIN,
        42,
    );
    let owner = crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: "f3d:native:design-parameter-owner#300".into(),
            byte_offset: 900,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 300,
            scope_record_index: 12,
            local_ordinal: 0,
            evaluated_value: 4.0,
            evaluated_value_offset: 940,
            parameter_record_index: 301,
            owned_ordinal: 3,
            variant: Some(0),
            companion_record_index: 302,
        },
    )
    .unwrap();
    // Scope 12 owns the companion. Records 55 and 56 are its own; scope 13
    // references record 57, and both scopes reference record 58.
    let scopes = [
        scope_at(12, 400, vec![55, 56, 58]),
        scope_at(13, 600, vec![57, 58]),
    ];
    let ctx = cadmpeg_test_support::service_decode_context();
    let interval = |owners: &[crate::records::parameters::DesignParameterOwner],
                    headers: &[crate::records::decal::DesignRecordHeader]| {
        super::super::CompanionIntervals::new(&ctx, &[], owners, &scopes, headers)
            .unwrap()
            .interval("f3d:native", &companion, 1000)
            .unwrap()
    };
    let own_then_foreign = [header_at(57, 90), header_at(56, 75), header_at(55, 70)];
    // The owning scope's headers do not end the interval; the next header
    // that another scope references does.
    assert_eq!(
        interval(std::slice::from_ref(&owner), &own_then_foreign),
        Some((58, 90))
    );
    // Without an owner, every scope is foreign.
    assert_eq!(interval(&[], &own_then_foreign), Some((58, 70)));
    // A header that two scopes reference is foreign to each of them.
    let shared = [header_at(55, 70), header_at(58, 72), header_at(57, 90)];
    assert_eq!(
        interval(std::slice::from_ref(&owner), &shared),
        Some((58, 72))
    );
    // Headers of the owning scope alone leave the interval to the next
    // boundary, the owning scope's record.
    let own = [header_at(55, 70), header_at(56, 75)];
    assert_eq!(
        interval(std::slice::from_ref(&owner), &own),
        Some((58, 400))
    );
}

/// An owner record of `record_index` in [`STREAM`] at `byte_offset` that
/// binds the parameter `parameter_record_index` of the scope
/// `scope_record_index` to the companion `companion_record_index`.
pub(super) fn owner_at(
    record_index: u32,
    scope_record_index: u32,
    parameter_record_index: u32,
    companion_record_index: u32,
    byte_offset: u64,
) -> crate::records::parameters::DesignParameterOwner {
    crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: format!(
                "{}:design-parameter-owner#{record_index}",
                crate::test_support::with_decode_context(|ctx| crate::ids::native_scope(
                    ctx,
                    STREAM,
                    "retain F3D native scope"
                )
                .expect("test F3D native identity"))
            ),
            byte_offset,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index,
            scope_record_index,
            local_ordinal: 0,
            evaluated_value: 1.0,
            evaluated_value_offset: byte_offset + 40,
            parameter_record_index,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index,
        },
    )
    .unwrap()
}

#[test]
fn companion_boundaries_commit_only_surviving_offsets() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let scopes = [scope_at(13, 80, vec![55])];
    // The only surviving allocation is the four-slot offset vector.
    let output_bytes = u64::try_from(4 * std::mem::size_of::<u64>()).unwrap();
    for retained_limit in [output_bytes - 1, output_bytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let storage;
        let (boundaries, held_storage) = ctx.with_scoped_storage("test companion boundaries", || {
            super::super::CompanionBoundaries::build(&ctx, &[], &[], &scopes, &[])
        }).unwrap();
        storage = held_storage;
        assert_eq!(boundaries.offsets, [80]);
        assert!(boundaries.owner_scopes.is_empty());
        assert!(boundaries.referenced_headers.is_empty());
        assert!(boundaries.run_starts.is_empty());
        assert_eq!(boundaries.offsets.capacity(), 4);
        let result = storage.commit_value(boundaries);
        if retained_limit == output_bytes {
            let boundaries = result.unwrap();
            assert_eq!(boundaries.offsets, [80]);
            assert!(ctx.resource_refusal().is_none());
        } else {
            let failure = match result {
                Err(CodecError::ResourceLimit(failure)) => failure,
                _ => panic!("expected the original retained output refusal"),
            };
            assert_eq!(failure.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(failure.limit, output_bytes - 1);
            assert_eq!(failure.used, 0);
            assert_eq!(failure.additional, output_bytes);
            assert_eq!(failure.operation, "test companion boundaries");
            assert_eq!(ctx.resource_refusal(), Some(failure));
        }
    }
}

#[test]
fn annotation_intervals_commit_only_surviving_intervals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let parameters = [parameter_at(12, 50)];
    let owners = [owner_at(10, 13, 12, 11, 120)];
    let mut scopes = [scope_at(13, 0, vec![13])];
    let stream = crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_scope(ctx, STREAM, "test interval scope").unwrap()
    });
    scopes[0].id = format!("{stream}:parameter-scope#13");
    let companions = [DesignParameterCompanion::unbound(
        format!("{stream}:parameter-companion#11"),
        220,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        11,
        10,
        std::num::NonZeroU64::MIN,
        262,
    )];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(
        4 * std::mem::size_of::<super::super::AnnotationInterval>()
    ).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let _companion_storage;
    let (grouped, held_storage) = ctx.with_scoped_storage("test grouped companions", || {
        super::super::Grouped::build(
            &ctx, &companions,
            |companion| super::super::record_stream(&ctx, companion.id()),
            "test grouped companions",
        )
    }).unwrap();
    _companion_storage = held_storage;
    let mut boundaries = super::super::CompanionIntervals::new(
        &ctx, &parameters, &owners, &scopes, &[],
    ).unwrap();
    let storage;
    let (intervals, held_storage) = super::super::annotation_intervals(
        &ctx, &stream, 400, &grouped, &mut boundaries,
    ).unwrap();
    storage = held_storage;
    assert_eq!(intervals, [(278, 400, Some(11)), (0, 220, None)]);
    assert_eq!(intervals.capacity(), 4);
    let intervals = storage.commit_value(intervals).unwrap();
    assert_eq!(intervals, [(278, 400, Some(11)), (0, 220, None)]);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn presentation_stream_tables_commit_only_surviving_owner_groups() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::records::parameters::DesignParameterOwner;

    let parameters = [parameter_at(12, 50)];
    let owners = [owner_at(10, 13, 12, 11, 120)];
    // Installed core hash growth charges the four-bucket first table layout;
    // push_hash_group creates an exact one-slot child vector. This tests the
    // capture lifetime, not completeness of the shared hash implementation.
    let slot_bytes = std::mem::size_of::<(u32, Vec<&DesignParameterOwner>)>();
    let alignment = std::mem::align_of::<(u32, Vec<&DesignParameterOwner>)>().max(16);
    let output_bytes = 4 * slot_bytes + alignment - 1 + 4 + 16
        + std::mem::size_of::<&DesignParameterOwner>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(output_bytes).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let stream = super::super::record_stream(&ctx, owners[0].id()).unwrap().unwrap();
    let _parameter_storage;
    let (index, held_storage) = super::super::dimension_parameter_index(
        &ctx, &parameters, "test dimension parameters",
    ).unwrap();
    _parameter_storage = held_storage;
    let owner_storage;
    let (grouped, held_storage) = ctx.with_scoped_storage("test grouped owners", || {
        super::super::Grouped::build(
            &ctx, &owners,
            |owner| super::super::record_stream(&ctx, owner.id()),
            "test grouped owners",
        )
    }).unwrap();
    owner_storage = held_storage;
    let pass = super::super::PresentationPassTables {
        owners: grouped,
        geometry: super::super::StreamIndexRuns { ctx: &ctx, streams: Vec::new(), indices: Vec::new() },
        sketch_entities: super::super::StreamIndexRuns { ctx: &ctx, streams: Vec::new(), indices: Vec::new() },
        _storage: [
            owner_storage,
            ctx.reserve_scoped(0, "test empty geometry").unwrap(),
            ctx.reserve_scoped(0, "test empty sketch entities").unwrap(),
        ],
    };
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&ctx, &[]).unwrap();
    let storage;
    let (tables, held_storage) = ctx.with_scoped_storage("test presentation table output", || {
        super::super::PresentationStreamTables::build(&ctx, stream, &records, &pass, &index)
    }).unwrap();
    storage = held_storage;
    assert_eq!(tables.owners_by_scope.capacity(), 3);
    let table = storage.commit_value(tables).unwrap();
    let group = table.owners_by_scope.get(&13).unwrap();
    assert_eq!(group.len(), 1);
    assert_eq!(group.capacity(), 1);
    assert!(std::ptr::eq(group[0], &owners[0]));
    assert!(table.geometry_indices.is_empty());
    assert!(table.sketch_entities.is_empty());
    assert!(ctx.resource_refusal().is_none());
}

mod session;
