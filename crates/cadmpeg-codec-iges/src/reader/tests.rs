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

use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

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

#[test]
fn container_only_retains_unknown_flag_three_units_without_projection() {
    let global = b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,3,3HFOO,1,1.0,1Hd,0.001,1,1Ha,1Ho,11,0,0H,0H;";
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };

    let result = IgesCodec
        .decode(&mut Cursor::new(fixed_ascii_with_global(global)), &options)
        .unwrap();

    assert!(result.report().container_only);
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("native_units"))
            .map(String::as_str),
        Some("FOO")
    );
    assert_eq!(result.ir().model.entity_count(), 0);
}

#[test]
fn type316_property_does_not_supply_a_global_flag_three_factor() {
    let mut bytes = units_data_file();
    let old = b",1.0,2,2HMG";
    let new = b",1.0,3,2HFG";
    let position = bytes
        .windows(old.len())
        .position(|window| window == old)
        .expect("Global units fields");
    bytes[position..position + old.len()].copy_from_slice(new);
    let old = b"\nM,1,1.0";
    let new = b"\nO,1,1.0";
    let position = bytes
        .windows(old.len())
        .position(|window| window == old)
        .expect("Global continuation");
    bytes[position..position + old.len()].copy_from_slice(new);

    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &options)
        .unwrap();
    let units = &result.ir().native.namespace("iges").unwrap().arenas["units_data"][0];
    assert_eq!(units.fields()["owners"][0], "iges:entity:directory#1");
    assert_eq!(units.fields()["units"][0]["scale_factor"], 1852.0);

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_err();
    assert!(matches!(error, CodecError::NotImplemented(_)));
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
