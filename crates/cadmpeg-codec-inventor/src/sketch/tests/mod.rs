// SPDX-License-Identifier: Apache-2.0
//! Unit tests for Inventor sketch records and projection.

use super::{
    inventory, parse_constraint, parse_direction, parse_entity, parse_sketch, parse_transform,
    project, PmDcSketchConstraintKind, PmDcSketchEntityKind, SketchConstraintTag,
    SketchEntityTag, SketchInventory,
    COINCIDENT_TYPE, DIAMETER_TYPE, DIRECTION_TYPE, HORIZONTAL_DISTANCE_TYPE,
    HORIZONTAL_TYPE, LINE_TYPE, POINT_TYPE, RADIUS_TYPE, SKETCH_TYPE, TRANSFORM_TYPE,
    VERTICAL_DISTANCE_TYPE,
};
use crate::container::InventorContainer;
use crate::pmdc::{PmDcReference, PmDcReferenceList};
use crate::record_identity::Located;
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::test_support::test_fixtures::{content, parse, primary_envelope_fixture};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::sketches::SketchPlacement;

fn fixture_record_type_id(value: [u8; 16]) -> crate::record_identity::RecordTypeId {
    crate::record_identity::RecordTypeId::from_bytes(
        &cadmpeg_test_support::service_decode_context(),
        value,
        "retain Inventor sketch fixture record type id",
    )
    .expect("service context admits fixture record type id")
}

fn list(marker: u16, references: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&marker.to_le_bytes());
    bytes.extend_from_slice(&0x3000u16.to_le_bytes());
    bytes.extend_from_slice(
        &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    if !references.is_empty() {
        if marker == 8 {
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
        } else {
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        for reference in references {
            bytes.extend_from_slice(&reference.to_le_bytes());
        }
    }
    bytes
}

fn entity_prefix(index: u32, sketch: u32, flags: u32) -> Vec<u8> {
    let mut bytes = content(index);
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&sketch.to_le_bytes());
    bytes
}

fn point_bytes(index: u32, sketch: u32, position: [f64; 2]) -> Vec<u8> {
    let mut bytes = entity_prefix(index, sketch, 0);
    for value in position {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend(list(2, &[]));
    bytes.extend(list(2, &[]));
    bytes
}

fn line_bytes(index: u32, sketch: u32, points: [u32; 2]) -> Vec<u8> {
    let mut bytes = entity_prefix(index, sketch, 0);
    bytes.extend(list(2, &points));
    bytes.extend(list(2, &[]));
    bytes.extend_from_slice(&0.0f64.to_le_bytes());
    bytes.extend_from_slice(&0.0f64.to_le_bytes());
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&0.0f64.to_le_bytes());
    bytes
}

fn constraint_header(index: u32, parameter: u32) -> Vec<u8> {
    let mut bytes = content(index);
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    bytes.extend_from_slice(&0x8000_000cu32.to_le_bytes());
    bytes.extend(list(6, &[]));
    bytes.extend(list(6, &[]));
    bytes.extend_from_slice(&parameter.to_le_bytes());
    bytes
}

mod admission;
mod parsing;
mod projection;
mod serialization;
