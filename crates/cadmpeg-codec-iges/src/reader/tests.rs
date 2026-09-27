// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::wire;

use std::io::Cursor;

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Vertex;
use cadmpeg_ir::{CadIr, SourceProvenance};

use crate::loss::IgesLossCode;
use crate::test_support::test_curves_and_surfaces::{
    direction_file, point_file, point_file_with_global,
};
use crate::test_support::test_drawing_and_trimming::test_surface_domains::transform_chain_overflow_file;
use crate::IgesCodec;

fn directory_fixture() -> (
    Vec<crate::directory::DirectoryEntry>,
    crate::global::GlobalTable,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = point_file();
    let scan = crate::card::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _) = crate::global::parse(&scan, &ctx).unwrap();
    let table = global.global_table();
    let (directory, _) = crate::directory::parse(&scan, table, Some(&ctx)).unwrap();
    (directory, table)
}

#[test]
fn reader_occurrence_loss_refuses_slot_and_message_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (directory, _) = directory_fixture();
    let sequence = directory[0].sequence;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut losses = Vec::new();
    assert!(matches!(
        super::push_occurrence_loss(&ctx, &mut losses, IgesLossCode::OccurrenceRootInferenceBlocked,
            format_args!("loss"), sequence, &directory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "iges occurrence loss slots"
    ));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::push_occurrence_loss(&ctx, &mut losses, IgesLossCode::OccurrenceRootInferenceBlocked,
            format_args!("loss"), sequence, &directory),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "iges occurrence loss message"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    super::push_occurrence_loss(
        &ctx,
        &mut losses,
        IgesLossCode::OccurrenceRootInferenceBlocked,
        format_args!("loss"),
        sequence,
        &directory,
    )
    .unwrap();
    assert_eq!(losses[0].message, "loss");
    assert_eq!(
        losses[0].provenance.as_ref().unwrap().tag.as_deref(),
        Some("directory_entry:D1")
    );
}

#[test]
fn reader_generic_loss_refuses_slot_and_message_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (directory, table) = directory_fixture();
    let projection = crate::entities::geometry::Projection::default();
    let attributed = std::collections::BTreeSet::new();
    let mut losses = Vec::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::append_generic_losses(&ctx, &mut losses, &directory, &projection, &attributed, table),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "iges generic loss slots"
    ));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::append_generic_losses(&ctx, &mut losses, &directory, &projection, &attributed, table),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "iges generic loss message"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    super::append_generic_losses(
        &ctx,
        &mut losses,
        &directory,
        &projection,
        &attributed,
        table,
    )
    .unwrap();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        IgesLossCode::EntityRetainedUnprojected.kind()
    );
    assert_eq!(
        losses[0].provenance.as_ref().unwrap().tag.as_deref(),
        Some("directory_entry:D1")
    );
}

#[test]
fn directory_loss_provenance_refuses_format_and_tag_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (directory, _) = directory_fixture();
    for (cap, operation) in [
        (0, "iges loss source format"),
        (4, "iges loss directory tag"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            directory[0].admitted_loss_provenance(&ctx),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == operation
        ));
    }

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        directory[0].admitted_loss_provenance(&ctx).unwrap(),
        directory[0].loss_provenance()
    );
}

#[test]
fn source_fidelity_refuses_id_owner_and_record_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    for (cap, operation) in [
        (0, "iges source fidelity id"),
        (
            crate::SOURCE_IMAGE_ID.len() as u64 + 3,
            "iges source fidelity stream owner",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            super::source_fidelity(&[], &ctx),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == operation
        ));
    }

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::source_fidelity(&[], &ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "iges source fidelity record node"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let fidelity = super::source_fidelity(b"IGES", &ctx).unwrap();
    let record = fidelity.retained_record(crate::SOURCE_IMAGE_ID).unwrap();
    assert_eq!(record.stream(), "iges");
    assert_eq!(record.data(), Some(b"IGES".as_slice()));
}

#[test]
fn admission_loss_slots_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = point_file_with_global(&crate::test_support::global_with_version_flag("1"));
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut parse =
        super::PhysicalParse::run(&bytes, &parse_ctx, super::ParseMode::Inspect).unwrap();

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        parse.admission_losses(&ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "iges admission loss slots"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let losses = parse.admission_losses(&ctx).unwrap();
    assert!(losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::SourceDialectUnverified.kind()));
}

#[test]
fn combined_summary_refuses_collection_limit_before_append() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut notes = Vec::new();
    let result = super::append_summary_notes(&ctx, &mut notes, vec!["a".into()]);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "iges combined summary notes"
    ));
    assert!(notes.is_empty());

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    super::append_summary_notes(&ctx, &mut notes, vec!["a".into()]).unwrap();
    assert_eq!(notes, ["a"]);
}

#[test]
fn source_metadata_admits_formatted_values_before_building_attributes() {
    use crate::{card, dialect, global, representation::Representation};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = point_file();
    let scan = card::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _) = global::parse(&scan, &parse_ctx).unwrap();
    let representation = Representation::FixedAscii;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = representation.as_str().len() as u64 - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::source_meta(
        &ctx,
        &global,
        representation,
        dialect::classify(representation, &global),
    );
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 0
                && limit.additional == representation.as_str().len() as u64
                && limit.operation == "iges source representation"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let meta = super::source_meta(
        &ctx,
        &global,
        representation,
        dialect::classify(representation, &global),
    )
    .unwrap();
    assert_eq!(meta.attributes["representation"], representation.as_str());
}

#[test]
fn source_attribute_admits_key_and_map_node_before_insertion() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeMap;

    let key = "native_units";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = key.len() as u64 - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::insert_source_attribute(&ctx, &mut BTreeMap::new(), key, String::new());
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 0
                && limit.additional == key.len() as u64
                && limit.operation == "iges source attribute key"
    ));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::insert_source_attribute(&ctx, &mut BTreeMap::new(), key, String::new());
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "iges source attributes"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut attributes = BTreeMap::new();
    super::insert_source_attribute(&ctx, &mut attributes, key, "MM".into()).unwrap();
    assert_eq!(attributes[key], "MM");
}

#[test]
fn decode_refuses_a_transformation_chain_over_its_projection_limit() {
    let error = IgesCodec
        .decode(
            &mut Cursor::new(transform_chain_overflow_file(65)),
            &DecodeOptions::default(),
        )
        .unwrap_err();

    assert!(
        matches!(
            &error,
            cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Codec("iges_transform_depth")
                    && limit.limit == 64
                    && limit.used == 64
                    && limit.additional == 1
        ),
        "{error:#?}"
    );
}

#[test]
fn transfer_ledger_reports_an_unprojected_native_only_direction() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(direction_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityRetainedUnprojected.kind()));
    assert_eq!(
        wire::field_or_default::<Option<String>>(
            &(result.report().transfer_ledger.entries[0].outcome),
            "note"
        )
        .as_deref(),
        Some("native record retained; semantic projection omitted with an attributed loss")
    );
}

#[test]
fn container_and_semantic_decode_retain_an_unknown_flag_three_name_without_geometry() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,3,7Hfurlong,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let bytes = point_file_with_global(global);

    for container_only in [false, true] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(bytes.clone()),
                &DecodeOptions {
                    container_only,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        assert_eq!(
            result.ir().source.as_ref().unwrap().attributes["native_units"],
            "furlong"
        );
        assert!(result.ir().model.points.is_empty());
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code == IgesLossCode::GlobalLengthUnitUnresolved.kind())
                .count(),
            1,
            "container_only={container_only}: {:#?}",
            result.report().losses
        );
        assert_eq!(result.report().transfer_ledger.entries.len(), 1);
    }
}

#[test]
fn semantic_decode_applies_delegated_nmi_factor() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,3,3Hnmi,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(point_file_with_global(global)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    let point = &result.ir().model.points[0].position().get();
    for (actual, expected) in [
        (point.x, 1_852_000.0),
        (point.y, 3_704_000.0),
        (point.z, 5_556_000.0),
    ] {
        let tolerance = f64::EPSILON * 64.0 * expected;
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} != {expected}"
        );
    }
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["native_units"],
        "nmi"
    );
}

#[test]
fn v5_0_receiver_product_default_is_retained_in_inspection_summary() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(point_file_with_global(global)),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"receiver_product=product".into()));
}

#[test]
fn post_terminate_records_follow_the_declared_dialect() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,11,0,0H,0H;";

    let mut v4_bytes = point_file_with_global(global_v4);
    v4_bytes.extend_from_slice(b"transport padding\r\n");
    let v4 = IgesCodec
        .decode(&mut Cursor::new(v4_bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        v4.report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::GlobalNoncanonicalFraming.kind())
            .count(),
        1
    );

    let mut v5_bytes = point_file_with_global(global_v5);
    v5_bytes.extend_from_slice(b"transport padding\r\n");
    let v5 = IgesCodec
        .decode(&mut Cursor::new(v5_bytes), &DecodeOptions::default())
        .unwrap();
    assert!(v5
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != IgesLossCode::GlobalNoncanonicalFraming.kind()));
}

#[test]
fn decode_publishes_global_minimum_resolution_to_neutral_tolerance() {
    for (global, expected) in [
        (
            b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;".as_slice(),
            0.001,
        ),
        (
            b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,1,2HIN,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;".as_slice(),
            0.0254,
        ),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(point_file_with_global(global)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.ir().tolerances.linear.get(), expected);
        assert_eq!(
            result.ir().tolerances.angular.get(),
            cadmpeg_ir::units::Tolerances::default().angular.get()
        );
    }
}

#[test]
fn decode_enforces_each_iges_session_resource_dimension() {
    fn assert_refusal(
        edit: impl FnOnce(&mut cadmpeg_core::decode::ResourceLimits),
        expected: ResourceDimension,
        operation: &'static str,
    ) {
        let bytes = point_file();
        let mut options = DecodeOptions::default();
        edit(&mut options.policy.limits);
        let error = IgesCodec
            .decode(&mut Cursor::new(bytes), &options)
            .unwrap_err();
        assert!(
            matches!(
                error,
                cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))
                    if limit.dimension == expected && limit.operation == operation
            ),
            "{error:#?}"
        );
    }

    assert_refusal(
        |limits| limits.max_materialized_bytes = 1,
        ResourceDimension::MaterializedBytes,
        "iges_card_storage",
    );
    assert_refusal(
        |limits| limits.max_retained_bytes = 1,
        ResourceDimension::RetainedBytes,
        "iges physical card payload",
    );
    assert_refusal(
        |limits| limits.max_entities = 0,
        ResourceDimension::Entities,
        "iges_directory_entries",
    );
    assert_refusal(
        |limits| limits.max_entities = 1,
        ResourceDimension::Entities,
        "iges_geometry_primitives",
    );
    let mut options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    options.policy.limits.max_entities = 1;
    let error = IgesCodec
        .decode(&mut Cursor::new(point_file()), &options)
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "iges_native_entities"
    ));
    assert_refusal(
        |limits| limits.max_collection_items = 0,
        ResourceDimension::CollectionItems,
        "iges_cards",
    );
    assert_refusal(
        |limits| limits.max_work_units = 1,
        ResourceDimension::WorkUnits,
        "iges_card_scan",
    );
}

#[test]
fn inspect_enforces_iges_parser_resource_limits() {
    let mut options = cadmpeg_core::decode::InspectOptions::default();
    options.limits.max_collection_items = 0;
    let error = IgesCodec
        .inspect(&mut Cursor::new(point_file()), &options)
        .unwrap_err();

    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "iges_cards"
    ));
}

#[test]
fn semantic_decode_barrier_rejects_invalid_cadir() {
    let mut ir = CadIr::empty();
    ir.model.vertices.push(Vertex {
        id: VertexId::mint("iges:model:vertex#invalid").expect("identity grammar"),
        point: PointId::mint("iges:model:point#missing").expect("identity grammar"),
        tolerance: None,
    });

    let error = crate::reader::reject_invalid_semantic_ir(&ir).unwrap_err();

    assert!(error.to_string().contains("referential_integrity"));
    assert!(error.to_string().contains("iges:model:vertex#invalid"));
    assert!(error.to_string().contains("iges:model:point#missing"));
}

/// Phase 5 freeze: shared builders must match the IGES rejection gate.
#[test]
fn phase5_freeze_shared_admissibility_fixtures() {
    let accepted = cadmpeg_test_support::admissibility::accepted_empty();
    assert!(crate::reader::reject_invalid_semantic_ir(&accepted).is_ok());
    let rejected = cadmpeg_test_support::admissibility::rejected_missing_point("iges:model")
        .expect("fixture identities are valid");
    let error = crate::reader::reject_invalid_semantic_ir(&rejected).unwrap_err();
    assert!(error.to_string().contains("referential_integrity"));
}

fn tagged_loss(tag: &str) -> LossNote {
    IgesLossCode::EntityRetainedUnprojected
        .note("attribution fixture")
        .with_provenance(
            SourceProvenance::in_stream("iges", cadmpeg_ir::stream_name!("iges"), 0)
                .with_tag(tag.to_owned()),
        )
}

fn attributed_index(losses: &[LossNote]) -> std::collections::BTreeSet<u32> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    crate::reader::attributed_sequences(losses, &ctx).unwrap()
}

#[test]
fn attributed_loss_index_refuses_node_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::attributed_sequences(&[tagged_loss("D7:parameter")], &ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "iges attributed loss sequences"
    ));
    assert_eq!(attributed_index(&[tagged_loss("D7:parameter")]).len(), 1);
}

#[test]
fn projected_directory_refuses_entry_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = point_file();
    let scan = crate::card::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _) = crate::global::parse(&scan, &parse_ctx).unwrap();
    let (directory, _) =
        crate::directory::parse(&scan, global.global_table(), Some(&parse_ctx)).unwrap();
    let quarantined = std::collections::BTreeSet::from([99]);

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::projection_directory(&directory, &quarantined, &ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "iges projected directory entries"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let projected = super::projection_directory(&directory, &quarantined, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(projected.len(), directory.len());
    assert_eq!(projected[0].sequence, directory[0].sequence);
}

#[test]
fn quarantined_parameter_sequence_index_refuses_node_limit() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00000000",
        parameters: "116,1,2,3x4,0;".into(),
    }]);
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let parse = super::PhysicalParse::run(&bytes, &parse_ctx, super::ParseMode::Decode).unwrap();
    assert_eq!(parse.quarantined_parameters.len(), 1);

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::quarantined_parameter_sequences(&parse.quarantined_parameters, &ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "iges quarantined parameter sequence index"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let sequences =
        super::quarantined_parameter_sequences(&parse.quarantined_parameters, &ctx).unwrap();
    assert!(sequences.contains(&1));
}

#[test]
fn attribution_indexes_a_parameter_tag_under_its_exact_sequence() {
    let index = attributed_index(&[tagged_loss("D7:parameter")]);

    assert!(index.contains(&7));
    assert!(!index.contains(&70));
    assert_eq!(index.len(), 1);
}

#[test]
fn attribution_indexes_directory_entry_and_indexed_parameter_tags() {
    let index = attributed_index(&[
        tagged_loss("directory_entry:D12"),
        tagged_loss("D3:parameter[4]"),
        tagged_loss("directory_entry:D12"),
    ]);

    assert_eq!(index.into_iter().collect::<Vec<_>>(), [3, 12]);
}

#[test]
fn attribution_ignores_tags_that_do_not_render_a_sequence() {
    let index = attributed_index(&[
        tagged_loss("D007:parameter"),
        tagged_loss("directory_entry:D12:extra"),
        tagged_loss("directory_entry:D007"),
        tagged_loss("D5"),
        tagged_loss("D:parameter"),
        tagged_loss("D+5:parameter"),
        tagged_loss("directory-entry:framing"),
        IgesLossCode::EntityRetainedUnprojected.note("no provenance"),
    ]);

    assert!(index.is_empty(), "{index:?}");
}
