// SPDX-License-Identifier: Apache-2.0
//! Input-sized BREP parser allocation tests.

use super::super::{
    parse_binary_prefix, parse_reference_suffix, parse_shape_kind, parse_shape_use, parse_text,
    TokenCursor,
};
use crate::native::{EntryRecord, PropertyBody, PropertyFamily, PropertyRecord, RetainedXml};
use crate::test_support::assert_retained_refusal_at;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn text_brep_token_index_refuses_at_materialized_limit() {
    let bytes = b"CASCADE Topology V1, (c) Matra-Datavision Locations 0 Curve2ds 0 Curves 0 Polygon3D 0 PolygonOnTriangulations 0 Surfaces 0 Triangulations 0 TShapes 0 *";
    let token_count = bytes.split(|byte| byte.is_ascii_whitespace())
        .filter(|token| !token.is_empty()).count();
    let required = u64::try_from(token_count * std::mem::size_of::<&str>())
        .expect("test token index size fits u64");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = required - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(parse_text(&ctx, bytes), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "FreeCAD text B-rep tokens"
            && limit.additional == required));
    policy.limits.max_materialized_bytes = required;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(parse_text(&ctx, bytes).is_ok());
}

#[derive(Clone, Copy)]
enum BinaryLane {
    PolygonNodes,
    PolygonParameters,
    IndexedPolygonNodes,
    IndexedPolygonParameters,
    TriangulationNodes,
    TriangulationUv,
    TriangulationTriangles,
    TriangulationNormals,
}

fn binary_lane(lane: BinaryLane) -> Vec<u8> {
    let version = if matches!(lane, BinaryLane::TriangulationNormals) { 4 } else { 3 };
    let mut bytes = format!("\nOpen CASCADE Topology V{version} (c)\nLocations 0\nCurve2ds 0\nCurves 0\n").into_bytes();
    match lane {
        BinaryLane::PolygonNodes | BinaryLane::PolygonParameters => {
            bytes.extend_from_slice(b"Polygon3D 1\n");
            bytes.extend_from_slice(&1_i32.to_le_bytes());
            bytes.push(u8::from(matches!(lane, BinaryLane::PolygonParameters)));
            bytes.extend_from_slice(&0.1_f64.to_le_bytes());
            for value in [0.0_f64; 3] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            if matches!(lane, BinaryLane::PolygonParameters) {
                bytes.extend_from_slice(&0.2_f64.to_le_bytes());
            }
        }
        BinaryLane::IndexedPolygonNodes | BinaryLane::IndexedPolygonParameters => {
            bytes.extend_from_slice(b"Polygon3D 0\nPolygonOnTriangulations 1\n");
            bytes.extend_from_slice(&1_i32.to_le_bytes());
            bytes.extend_from_slice(&1_i32.to_le_bytes());
            bytes.extend_from_slice(&0.1_f64.to_le_bytes());
            bytes.push(u8::from(matches!(lane, BinaryLane::IndexedPolygonParameters)));
            if matches!(lane, BinaryLane::IndexedPolygonParameters) {
                bytes.extend_from_slice(&0.2_f64.to_le_bytes());
            }
        }
        _ => {
            bytes.extend_from_slice(b"Polygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 1\n");
            bytes.extend_from_slice(&1_i32.to_le_bytes());
            let has_triangle = matches!(lane, BinaryLane::TriangulationTriangles);
            bytes.extend_from_slice(&i32::from(has_triangle).to_le_bytes());
            bytes.push(u8::from(matches!(lane, BinaryLane::TriangulationUv)));
            if version == 4 {
                bytes.push(1);
            }
            bytes.extend_from_slice(&0.1_f64.to_le_bytes());
            for value in [0.0_f64; 3] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            if matches!(lane, BinaryLane::TriangulationUv) {
                for value in [0.0_f64; 2] {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            if has_triangle {
                for index in [1_i32; 3] {
                    bytes.extend_from_slice(&index.to_le_bytes());
                }
            }
            if matches!(lane, BinaryLane::TriangulationNormals) {
                for value in [0.0_f32; 3] {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    bytes
}

fn assert_binary_lane_refusal(lane: BinaryLane, admitted_items: u64, operation: &str) {
    let bytes = binary_lane(lane);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = admitted_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(parse_binary_prefix(&ctx, &bytes), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == operation
            && limit.used == admitted_items
            && limit.additional == 1));
}

#[test]
fn binary_polygon_nodes_refuse_at_collection_limit() {
    assert_binary_lane_refusal(BinaryLane::PolygonNodes, 1, "FreeCAD binary polygon nodes");
}

#[test]
fn binary_polygon_parameters_refuse_at_collection_limit() {
    assert_binary_lane_refusal(BinaryLane::PolygonParameters, 2, "FreeCAD binary polygon parameters");
}

#[test]
fn binary_indexed_polygon_nodes_refuse_at_collection_limit() {
    assert_binary_lane_refusal(BinaryLane::IndexedPolygonNodes, 1, "FreeCAD binary indexed polygon nodes");
}

#[test]
fn binary_indexed_polygon_parameters_refuse_at_collection_limit() {
    assert_binary_lane_refusal(BinaryLane::IndexedPolygonParameters, 2, "FreeCAD binary indexed polygon parameters");
}

#[test]
fn binary_triangulation_nodes_refuse_at_collection_limit() {
    assert_binary_lane_refusal(BinaryLane::TriangulationNodes, 1, "FreeCAD binary triangulation nodes");
}

#[test]
fn binary_triangulation_uv_refuses_at_collection_limit() {
    assert_binary_lane_refusal(BinaryLane::TriangulationUv, 2, "FreeCAD binary triangulation UV nodes");
}

#[test]
fn binary_triangulation_triangles_refuse_at_collection_limit() {
    assert_binary_lane_refusal(BinaryLane::TriangulationTriangles, 2, "FreeCAD binary triangulation triangles");
}

#[test]
fn binary_triangulation_normals_refuse_at_collection_limit() {
    assert_binary_lane_refusal(BinaryLane::TriangulationNormals, 2, "FreeCAD binary triangulation normals");
}

fn shape_property(xml: &str) -> PropertyRecord {
    PropertyRecord {
        id: "fcstd:native:property#Owner:Shape".into(),
        owner: "fcstd:native:object#Owner".into(),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: PropertyFamily::Geometry,
        status: None,
        body: PropertyBody::Transient,
        order: 0,
        xml: RetainedXml::from_text(xml.into(), 0).expect("test XML span"),
    }
}

#[test]
fn missing_shape_entry_diagnostic_refuses_at_retained_limit() {
    let property = shape_property("<Property><Part file=\"missing.brp\"/></Property>");
    assert_retained_refusal_at(&[], "FreeCAD missing shape entry", |ctx| {
        super::super::parse_payloads(ctx, &[property.clone()], &[])
    });
}

#[test]
fn shape_property_xml_diagnostic_refuses_at_retained_limit() {
    let property = shape_property("<Property>");
    assert_retained_refusal_at(&[], "FreeCAD shape property XML diagnostic", |ctx| {
        super::super::direct_shape_entry(ctx, &property)
    });
}

#[test]
fn shape_property_root_diagnostic_refuses_at_retained_limit() {
    let property = shape_property("<Wrong/>");
    assert_retained_refusal_at(&[], "FreeCAD shape property root diagnostic", |ctx| {
        super::super::direct_shape_entry(ctx, &property)
    });
}

#[test]
fn shape_property_carrier_diagnostic_refuses_at_retained_limit() {
    let property = shape_property("<Property><Part file=\"a.brp\"/><Part file=\"b.brp\"/></Property>");
    assert_retained_refusal_at(&[], "FreeCAD shape property carrier diagnostic", |ctx| {
        super::super::direct_shape_entry(ctx, &property)
    });
}

#[test]
fn uppercase_binary_shape_extension_selects_binary_reader() {
    let property = shape_property("<Property><Part file=\"Body.BIN\"/></Property>");
    let entry = EntryRecord {
        id: "fcstd:native:entry#Body.BIN".into(),
        name: "Body.BIN".into(),
        role: cadmpeg_core::container::ContainerRole::Brep,
        referenced_by: Vec::new(),
        data: b"garbage\n".to_vec(),
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let error = super::super::parse_payloads(&ctx, &[property], &[entry])
        .expect_err("malformed binary payload");
    assert!(error.to_string().contains("unsupported binary B-rep header"));
}

#[test]
fn invalid_shape_kind_refuses_before_diagnostic_allocation() {
    assert_retained_refusal_at(&[], "FreeCAD invalid shape kind", |ctx| {
        parse_shape_kind(ctx, "UnexpectedShapeKind")
    });
}

#[test]
fn invalid_shape_use_prefix_refuses_before_diagnostic_allocation() {
    let tokens = ["?VeryLongShapeUse"];
    assert_retained_refusal_at(&[], "FreeCAD invalid shape use", |ctx| {
        let mut cursor = TokenCursor::new(ctx, &tokens);
        parse_shape_use(&mut cursor, 1, &BTreeMap::new())
    });
}

#[test]
fn invalid_shape_use_index_refuses_before_diagnostic_allocation() {
    let tokens = ["+VeryLongShapeIndex"];
    assert_retained_refusal_at(&[], "FreeCAD invalid shape use", |ctx| {
        let mut cursor = TokenCursor::new(ctx, &tokens);
        parse_shape_use(&mut cursor, 1, &BTreeMap::new())
    });
}

#[test]
fn reference_suffix_copy_refuses_at_retained_limit() {
    let tokens = ["1FaceOfLongSource"];
    assert_retained_refusal_at(&[], "FreeCAD B-rep reference suffix", |ctx| {
        let mut cursor = TokenCursor::new(ctx, &tokens);
        parse_reference_suffix(&mut cursor, "test suffix", 1)
    });
}
