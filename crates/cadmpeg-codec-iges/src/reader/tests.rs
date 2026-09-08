// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::Vertex;
use cadmpeg_ir::{CadIr, SourceProvenance};

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

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
        result.report().transfer_ledger.entries[0].note(),
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
    let point = &result.ir().model.points[0].position;
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
        assert_eq!(result.ir().tolerances.linear, expected);
        assert_eq!(
            result.ir().tolerances.angular,
            cadmpeg_ir::units::Tolerances::default().angular
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
                    if limit.dimension == expected && limit.context.operation == operation
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
        "iges_source_image",
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
                && limit.context.operation == "iges_native_entities"
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
                && limit.context.operation == "iges_cards"
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
    let accepted = cadmpeg_ir::validate::admissibility_freeze::accepted_empty();
    assert!(crate::reader::reject_invalid_semantic_ir(&accepted).is_ok());
    let rejected = cadmpeg_ir::validate::admissibility_freeze::rejected_missing_point("iges:model");
    let error = crate::reader::reject_invalid_semantic_ir(&rejected).unwrap_err();
    assert!(error.to_string().contains("referential_integrity"));
}

fn tagged_loss(tag: &str) -> LossNote {
    IgesLossCode::EntityRetainedUnprojected
        .note("attribution fixture")
        .with_provenance(SourceProvenance::in_stream("iges", "iges", 0).with_tag(tag.to_owned()))
}

#[test]
fn attribution_indexes_a_parameter_tag_under_its_exact_sequence() {
    let index = crate::reader::attributed_sequences(&[tagged_loss("D7:parameter")]);

    assert!(index.contains(&7));
    assert!(!index.contains(&70));
    assert_eq!(index.len(), 1);
}

#[test]
fn attribution_indexes_directory_entry_and_indexed_parameter_tags() {
    let index = crate::reader::attributed_sequences(&[
        tagged_loss("directory_entry:D12"),
        tagged_loss("D3:parameter[4]"),
        tagged_loss("directory_entry:D12"),
    ]);

    assert_eq!(index.into_iter().collect::<Vec<_>>(), [3, 12]);
}

#[test]
fn attribution_ignores_tags_that_do_not_render_a_sequence() {
    let index = crate::reader::attributed_sequences(&[
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
