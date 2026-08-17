// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

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
            CodecError::ResourceLimit(limit)
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
        result.report().transfer_ledger.entries[0].note.as_deref(),
        Some("native record retained; semantic projection omitted with an attributed loss")
    );
}

#[test]
fn container_decode_retains_unknown_flag_three_name_but_semantic_decode_refuses() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,3,6HCUSTOM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let bytes = point_file_with_global(global);

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap_err();
    assert!(matches!(
        error,
        CodecError::NotImplemented(message)
            if message == "IGES units flag 3 names a unit without a known millimetre factor"
    ));

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["native_units"],
        "CUSTOM"
    );
    assert!(result.ir().model.points.is_empty());
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
                CodecError::ResourceLimit(limit)
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
        CodecError::ResourceLimit(limit)
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
    let mut ir = CadIr::empty(Units::default());
    ir.model.vertices.push(Vertex {
        id: VertexId("iges:model:vertex#invalid".into()),
        point: PointId("iges:model:point#missing".into()),
        tolerance: None,
    });

    let error = crate::reader::reject_invalid_semantic_ir(&ir, &[]).unwrap_err();

    assert!(error.to_string().contains("referential_integrity"));
    assert!(error.to_string().contains("iges:model:vertex#invalid"));
    assert!(error.to_string().contains("iges:model:point#missing"));
}

/// Phase 5 freeze: shared builders must match the IGES rejection gate.
#[test]
fn phase5_freeze_shared_admissibility_fixtures() {
    let accepted = cadmpeg_ir::validate::admissibility_freeze::accepted_empty();
    assert!(crate::reader::reject_invalid_semantic_ir(&accepted, &[]).is_ok());
    let rejected = cadmpeg_ir::validate::admissibility_freeze::rejected_missing_point("iges:model");
    let error = crate::reader::reject_invalid_semantic_ir(&rejected, &[]).unwrap_err();
    assert!(error.to_string().contains("referential_integrity"));
}
