// SPDX-License-Identifier: Apache-2.0
//! Input-sized BREP parser allocation tests.

use super::super::{
    append_text_curve, append_text_surface, parse_binary_prefix, parse_reference_suffix,
    parse_shape_kind, parse_shape_use, parse_text, transfer_text_curves, transfer_text_surfaces,
    CurveTransfer, NestedCurve, NestedSurface, ShapePayload, ShapePayloadRecord, ShapeSet,
    SurfaceTransfer, TextCurve, TextSurface, TextTShapes, TextTopologyVersion, TokenCursor,
};
use crate::native::{EntryRecord, PropertyBody, PropertyFamily, PropertyRecord, RetainedXml};
use crate::test_support::assert_retained_refusal_at;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::SourceObjectAssociation;

#[test]
fn pcurve_pair_continuity_refuses_at_matching_retained_limit() {
    let counts = BTreeMap::from([
        ("Curve2ds".to_owned(), 2),
        ("Surfaces".to_owned(), 1),
        ("Locations".to_owned(), 0),
    ]);
    assert_retained_refusal_at(&[], "FreeCAD B-rep edge continuity", |ctx| {
        let tokens = ["1", "2", "CONTINUITY", "1", "0", "0", "10"];
        let mut cursor = TokenCursor::new(ctx, &tokens);
        super::super::parse_edge_representation(3, &mut cursor, &counts, 1)
    });
}

#[test]
fn edge_regularity_continuity_refuses_at_matching_retained_limit() {
    let counts = BTreeMap::from([("Surfaces".to_owned(), 1), ("Locations".to_owned(), 0)]);
    assert_retained_refusal_at(&[], "FreeCAD B-rep edge continuity", |ctx| {
        let tokens = ["CONTINUITY", "1", "0", "1", "0"];
        let mut cursor = TokenCursor::new(ctx, &tokens);
        super::super::parse_edge_representation(4, &mut cursor, &counts, 1)
    });
}

#[test]
fn binary_section_diagnostic_refuses_at_matching_retained_limit() {
    assert_retained_refusal_at(&[], "FreeCAD binary section diagnostic", |ctx| {
        let mut cursor = super::super::BinaryCursor::new(ctx, b"Wrong 0\n");
        cursor.section_count("Locations")
    });
}

#[test]
fn tshape_flags_diagnostic_refuses_at_matching_retained_limit() {
    assert_retained_refusal_at(&[], "FreeCAD TShape flag diagnostic", |ctx| {
        super::super::parse_shape_flags(ctx, "invalid-input-flags", 1)
    });
}

#[test]
fn text_brep_token_index_refuses_at_materialized_limit() {
    let bytes = b"CASCADE Topology V1, (c) Matra-Datavision Locations 0 Curve2ds 0 Curves 0 Polygon3D 0 PolygonOnTriangulations 0 Surfaces 0 Triangulations 0 TShapes 0 *";
    let token_count = bytes
        .split(u8::is_ascii_whitespace)
        .filter(|token| !token.is_empty())
        .count();
    let required = u64::try_from(token_count * std::mem::size_of::<&str>())
        .expect("test token index size fits u64");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = required - 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    assert!(
        matches!(parse_text(&ctx, bytes), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "FreeCAD text B-rep tokens"
            && limit.additional == required)
    );
    policy.limits.max_materialized_bytes = required;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
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
    let version = if matches!(lane, BinaryLane::TriangulationNormals) {
        4
    } else {
        3
    };
    let mut bytes =
        format!("\nOpen CASCADE Topology V{version} (c)\nLocations 0\nCurve2ds 0\nCurves 0\n")
            .into_bytes();
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
            bytes.push(u8::from(matches!(
                lane,
                BinaryLane::IndexedPolygonParameters
            )));
            if matches!(lane, BinaryLane::IndexedPolygonParameters) {
                bytes.extend_from_slice(&0.2_f64.to_le_bytes());
            }
        }
        _ => {
            bytes.extend_from_slice(
                b"Polygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 1\n",
            );
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
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    assert!(
        matches!(parse_binary_prefix(&ctx, &bytes), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == operation
            && limit.used == admitted_items
            && limit.additional == 1)
    );
}

#[test]
fn binary_polygon_nodes_refuse_at_collection_limit() {
    assert_binary_lane_refusal(BinaryLane::PolygonNodes, 1, "FreeCAD binary polygon nodes");
}

#[test]
fn binary_polygon_parameters_refuse_at_collection_limit() {
    assert_binary_lane_refusal(
        BinaryLane::PolygonParameters,
        2,
        "FreeCAD binary polygon parameters",
    );
}

#[test]
fn binary_indexed_polygon_nodes_refuse_at_collection_limit() {
    assert_binary_lane_refusal(
        BinaryLane::IndexedPolygonNodes,
        1,
        "FreeCAD binary indexed polygon nodes",
    );
}

#[test]
fn binary_indexed_polygon_parameters_refuse_at_collection_limit() {
    assert_binary_lane_refusal(
        BinaryLane::IndexedPolygonParameters,
        2,
        "FreeCAD binary indexed polygon parameters",
    );
}

#[test]
fn binary_triangulation_nodes_refuse_at_collection_limit() {
    assert_binary_lane_refusal(
        BinaryLane::TriangulationNodes,
        1,
        "FreeCAD binary triangulation nodes",
    );
}

#[test]
fn binary_triangulation_uv_refuses_at_collection_limit() {
    assert_binary_lane_refusal(
        BinaryLane::TriangulationUv,
        2,
        "FreeCAD binary triangulation UV nodes",
    );
}

#[test]
fn binary_triangulation_triangles_refuse_at_collection_limit() {
    assert_binary_lane_refusal(
        BinaryLane::TriangulationTriangles,
        2,
        "FreeCAD binary triangulation triangles",
    );
}

#[test]
fn binary_triangulation_normals_refuse_at_collection_limit() {
    assert_binary_lane_refusal(
        BinaryLane::TriangulationNormals,
        2,
        "FreeCAD binary triangulation normals",
    );
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
        super::super::parse_payloads(ctx, std::slice::from_ref(&property), &[])
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
    let property =
        shape_property("<Property><Part file=\"a.brp\"/><Part file=\"b.brp\"/></Property>");
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
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    let error = super::super::parse_payloads(&ctx, &[property], &[entry])
        .expect_err("malformed binary payload");
    assert!(error
        .to_string()
        .contains("unsupported binary B-rep header"));
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

fn line_curve() -> TextCurve {
    TextCurve::Line {
        origin: FinitePoint3::ZERO,
        direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0))
            .expect("finite direction"),
    }
}

fn plane_surface() -> TextSurface {
    TextSurface::Plane {
        origin: FinitePoint3::ZERO,
        axis: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))
            .expect("finite axis"),
        u_axis: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0))
            .expect("finite u axis"),
        v_reversed: false,
    }
}

fn source_association() -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new("Owner").expect("nonblank object"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    }
}

fn shape_payload() -> ShapePayloadRecord {
    ShapePayloadRecord {
        id: "fcstd:native:shape-payload#Payload".into(),
        property: "fcstd:native:property#Owner:Shape".into(),
        entry: "fcstd:native:entry#Shape.brp".into(),
        payload: ShapePayload::Text {
            facts: ShapeSet {
                locations: Vec::new(),
                curve2ds: Vec::new(),
                curves: vec![line_curve()],
                polygons3d: Vec::new(),
                polygons_on_triangulations: Vec::new(),
                surfaces: vec![plane_surface()],
                triangulations: Vec::new(),
                tshapes: TextTShapes::from(Vec::new()),
                roots: Vec::new(),
            },
            version: TextTopologyVersion::V1,
        },
    }
}

fn trimmed_curve() -> TextCurve {
    TextCurve::Trimmed {
        parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
        basis: NestedCurve::try_new(line_curve()).expect("nested line"),
    }
}

fn offset_curve() -> TextCurve {
    TextCurve::Offset {
        distance: FiniteReal::ONE,
        direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
            .expect("finite offset direction"),
        basis: NestedCurve::try_new(line_curve()).expect("nested line"),
    }
}

fn extrusion_surface() -> TextSurface {
    TextSurface::Extrusion {
        direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))
            .expect("finite extrusion direction"),
        directrix: NestedCurve::try_new(line_curve()).expect("nested directrix"),
    }
}

fn revolution_surface() -> TextSurface {
    TextSurface::Revolution {
        axis_origin: FinitePoint3::ZERO,
        axis_direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))
            .expect("finite revolution axis"),
        directrix: NestedCurve::try_new(line_curve()).expect("nested directrix"),
    }
}

fn trimmed_surface() -> TextSurface {
    TextSurface::Trimmed {
        parameter_ranges: [[FiniteReal::ZERO, FiniteReal::ONE]; 2],
        basis: NestedSurface::try_new(plane_surface()).expect("nested plane"),
    }
}

fn offset_surface() -> TextSurface {
    TextSurface::Offset {
        distance: FiniteReal::ONE,
        basis: NestedSurface::try_new(plane_surface()).expect("nested plane"),
    }
}

fn assert_curve_identity_refusal(curve: &TextCurve, operation: &str) {
    let id = CurveId::mint("fcstd:model:curve#Payload:1").expect("curve identity");
    let association = source_association();
    assert_retained_refusal_at(&[], operation, |ctx| {
        append_text_curve(
            ctx,
            curve,
            id.clone(),
            &association,
            &mut CurveTransfer::default(),
        )
    });
}

fn assert_surface_identity_refusal(surface: &TextSurface, operation: &str) {
    let id = SurfaceId::mint("fcstd:model:surface#Payload:1").expect("surface identity");
    let association = source_association();
    assert_retained_refusal_at(&[], operation, |ctx| {
        append_text_surface(
            ctx,
            surface,
            id.clone(),
            &association,
            &mut CurveTransfer::default(),
            &mut SurfaceTransfer::default(),
        )
    });
}

#[test]
fn transferred_curve_identity_refuses_at_retained_limit() {
    let payload = shape_payload();
    assert_retained_refusal_at(&[], "FreeCAD transferred curve identity", |ctx| {
        transfer_text_curves(ctx, std::slice::from_ref(&payload), &[])
    });
}

#[test]
fn transferred_surface_identity_refuses_at_retained_limit() {
    let payload = shape_payload();
    assert_retained_refusal_at(&[], "FreeCAD transferred surface identity", |ctx| {
        transfer_text_surfaces(
            ctx,
            std::slice::from_ref(&payload),
            &[],
            &mut CurveTransfer::default(),
        )
    });
}

macro_rules! curve_identity_test {
    ($name:ident, $curve:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert_curve_identity_refusal(&$curve, $operation);
        }
    };
}

curve_identity_test!(
    trimmed_curve_basis_identity_refuses_at_retained_limit,
    trimmed_curve(),
    "FreeCAD curve basis identity"
);
curve_identity_test!(
    trimmed_curve_basis_copy_refuses_at_retained_limit,
    trimmed_curve(),
    "FreeCAD curve basis identity copy"
);
curve_identity_test!(
    trimmed_curve_procedural_copy_refuses_at_retained_limit,
    trimmed_curve(),
    "FreeCAD procedural curve identity copy"
);
curve_identity_test!(
    trimmed_curve_construction_identity_refuses_at_retained_limit,
    trimmed_curve(),
    "FreeCAD curve construction identity"
);
curve_identity_test!(
    offset_curve_basis_identity_refuses_at_retained_limit,
    offset_curve(),
    "FreeCAD curve basis identity"
);
curve_identity_test!(
    offset_curve_basis_copy_refuses_at_retained_limit,
    offset_curve(),
    "FreeCAD curve basis identity copy"
);
curve_identity_test!(
    offset_curve_procedural_copy_refuses_at_retained_limit,
    offset_curve(),
    "FreeCAD procedural curve identity copy"
);
curve_identity_test!(
    offset_curve_construction_identity_refuses_at_retained_limit,
    offset_curve(),
    "FreeCAD curve construction identity"
);

macro_rules! surface_identity_test {
    ($name:ident, $surface:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert_surface_identity_refusal(&$surface, $operation);
        }
    };
}

surface_identity_test!(
    extrusion_directrix_identity_refuses_at_retained_limit,
    extrusion_surface(),
    "FreeCAD surface directrix identity"
);
surface_identity_test!(
    extrusion_directrix_copy_refuses_at_retained_limit,
    extrusion_surface(),
    "FreeCAD surface directrix identity copy"
);
surface_identity_test!(
    extrusion_procedural_copy_refuses_at_retained_limit,
    extrusion_surface(),
    "FreeCAD procedural surface identity copy"
);
surface_identity_test!(
    extrusion_construction_identity_refuses_at_retained_limit,
    extrusion_surface(),
    "FreeCAD surface construction identity"
);
surface_identity_test!(
    revolution_directrix_identity_refuses_at_retained_limit,
    revolution_surface(),
    "FreeCAD surface directrix identity"
);
surface_identity_test!(
    revolution_directrix_copy_refuses_at_retained_limit,
    revolution_surface(),
    "FreeCAD surface directrix identity copy"
);
surface_identity_test!(
    revolution_procedural_copy_refuses_at_retained_limit,
    revolution_surface(),
    "FreeCAD procedural surface identity copy"
);
surface_identity_test!(
    revolution_construction_identity_refuses_at_retained_limit,
    revolution_surface(),
    "FreeCAD surface construction identity"
);
surface_identity_test!(
    trimmed_surface_basis_identity_refuses_at_retained_limit,
    trimmed_surface(),
    "FreeCAD surface basis identity"
);
surface_identity_test!(
    trimmed_surface_basis_copy_refuses_at_retained_limit,
    trimmed_surface(),
    "FreeCAD surface basis identity copy"
);
surface_identity_test!(
    trimmed_surface_procedural_copy_refuses_at_retained_limit,
    trimmed_surface(),
    "FreeCAD procedural surface identity copy"
);
surface_identity_test!(
    trimmed_surface_construction_identity_refuses_at_retained_limit,
    trimmed_surface(),
    "FreeCAD surface construction identity"
);
surface_identity_test!(
    offset_surface_basis_identity_refuses_at_retained_limit,
    offset_surface(),
    "FreeCAD surface basis identity"
);
surface_identity_test!(
    offset_surface_basis_copy_refuses_at_retained_limit,
    offset_surface(),
    "FreeCAD surface basis identity copy"
);
surface_identity_test!(
    offset_surface_procedural_copy_refuses_at_retained_limit,
    offset_surface(),
    "FreeCAD procedural surface identity copy"
);
surface_identity_test!(
    offset_surface_construction_identity_refuses_at_retained_limit,
    offset_surface(),
    "FreeCAD surface construction identity"
);
