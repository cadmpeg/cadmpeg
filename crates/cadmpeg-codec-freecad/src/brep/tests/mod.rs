// SPDX-License-Identifier: Apache-2.0
//! BREP parser and transfer unit tests.

mod allocation_tests;
mod nesting;

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;

use super::{
    normalize_periodic_surface, parse_analytic_surface, parse_binary_edge_representation,
    parse_binary_prefix, parse_edge_representation, parse_payloads, parse_text,
    AnalyticSurfaceKind, BinaryCursor, LocationRef, ShapePayload, ShapePayloadRecord, TableRef,
    TextCurve, TextCurve2d, TextEdgeRepresentation, TextShapeKind, TextSurface, TextTShape,
    TextTShapeGeometry, TextTShapeWire, TextTShapes, TokenCursor,
};
use crate::native::{EntryRecord, PropertyRecord};
use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn in_decode_context<T>(f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within the input limit");
    f(&ctx)
}

fn with_collection_limit<T>(
    bytes: &[u8],
    limit: u64,
    f: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("input is within the root limit");
    f(&ctx)
}

#[test]
fn bezier_knot_vector_refuses_at_caller_limit() {
    let result = with_collection_limit(&[], 3, |ctx| super::clamped_bezier_knots(ctx, 1));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD Bezier knots"));
}

#[test]
fn surface_grid_rows_refuse_at_caller_limit() {
    let result = with_collection_limit(&[], 3, |ctx| super::grid_rows(ctx, vec![1_u8, 2, 3, 4], 2));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD B-rep surface row values"));
}

#[test]
fn periodic_curve_pole_padding_refuses_at_caller_limit() {
    let result = with_collection_limit(&[], 0, |ctx| {
        let mut poles = vec![1_u8];
        super::append_periodic_curve_poles(ctx, &mut poles, None, 1)
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD periodic B-rep curve poles"));
}

#[test]
fn binary_brep_location_capacity_refuses_on_collection_limit() {
    let mut bytes = b"Open CASCADE Topology V1\nLocations 1\n".to_vec();
    bytes.push(1);
    let result = with_collection_limit(&bytes, 0, |ctx| parse_binary_prefix(ctx, &bytes));
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD B-rep parse_binary_prefix"
    ));
}

#[test]
fn text_brep_location_capacity_refuses_on_collection_limit() {
    let bytes = b"CASCADE Topology V1, (c) Matra-Datavision Locations 1 1 Curve2ds 0 Curves 0 Polygon3D 0 PolygonOnTriangulations 0 Surfaces 0 Triangulations 0 TShapes 0";
    let result = with_collection_limit(bytes, 0, |ctx| parse_text(ctx, bytes));
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD B-rep parse_locations"
    ));
}

#[test]
fn periodic_brep_knot_capacity_refuses_on_collection_limit() {
    let knots = [0.0, 0.0, 0.5, 1.0, 1.0]
        .into_iter()
        .map(|value| FiniteReal::new(value).expect("finite knot"))
        .collect();
    let result = with_collection_limit(&[], 0, |ctx| {
        super::normalize_periodic_knots(ctx, knots, 2, true)
    });
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD periodic B-rep knots"
    ));
}

fn one_curve_payload() -> ShapePayloadRecord {
    ShapePayloadRecord {
        id: "fcstd:test:shape-payload#one".to_owned(),
        property: "fcstd:test:property#one".to_owned(),
        entry: "fcstd:test:entry#one".to_owned(),
        payload: ShapePayload::Text {
            version: super::TextTopologyVersion::V1,
            facts: super::ShapeSet {
                locations: Vec::new(),
                curve2ds: Vec::new(),
                curves: vec![TextCurve::Line {
                    origin: FinitePoint3::ZERO,
                    direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0))
                        .expect("finite direction"),
                }],
                polygons3d: Vec::new(),
                polygons_on_triangulations: Vec::new(),
                surfaces: Vec::new(),
                triangulations: Vec::new(),
                tshapes: Vec::new().into(),
                roots: Vec::new(),
            },
        },
    }
}

#[test]
fn carrier_census_record_capacity_refuses_on_collection_limit() {
    let payload = one_curve_payload();
    let result = with_collection_limit(&[], 0, |ctx| super::carrier_census(ctx, &[payload]));
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD carrier census records"
    ));
}

#[test]
fn carrier_census_family_insert_refuses_on_collection_limit() {
    let payload = one_curve_payload();
    let result = with_collection_limit(&[], 1, |ctx| super::carrier_census(ctx, &[payload]));
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD carrier census families"
    ));
}

#[test]
fn carrier_census_identity_refuses_at_retained_limit() {
    let payload = one_curve_payload();
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD native child identity", |ctx| {
        super::carrier_census(ctx, std::slice::from_ref(&payload))
    });
}

#[test]
fn shape_payload_identity_refuses_at_retained_limit() {
    let property = PropertyRecord {
        id: crate::native::native_id("property", "Shape"),
        owner: crate::native::native_id("object", "Shape"),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Part file=\"empty.brp\"/></Property>".into(),
            0,
        )
        .expect("valid test XML"),
    };
    let entry = EntryRecord {
        id: crate::native::native_id("entry", "empty.brp"),
        name: "empty.brp".into(),
        role: cadmpeg_core::container::ContainerRole::Brep,
        referenced_by: Vec::new(),
        data: Vec::new(),
    };
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD native child identity", |ctx| {
        parse_payloads(
            ctx,
            std::slice::from_ref(&property),
            std::slice::from_ref(&entry),
        )
    });
}

#[test]
fn shape_payload_record_refuses_on_collection_limit() {
    let property = PropertyRecord {
        id: crate::native::native_id("property", "Shape"),
        owner: crate::native::native_id("object", "Shape"),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["empty.brp".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Part file=\"empty.brp\"/></Property>".into(),
            0,
        )
        .expect("valid test XML"),
    };
    let entry = EntryRecord {
        id: crate::native::native_id("entry", "empty.brp"),
        name: "empty.brp".into(),
        role: cadmpeg_core::container::ContainerRole::Brep,
        referenced_by: vec![property.id.clone()],
        data: Vec::new(),
    };
    let result = with_collection_limit(&[], 1, |ctx| parse_payloads(ctx, &[property], &[entry]));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD shape payload records"));
}

#[test]
fn shape_entry_index_refuses_on_collection_limit() {
    let entry = EntryRecord {
        id: crate::native::native_id("entry", "empty.brp"),
        name: "empty.brp".into(),
        role: cadmpeg_core::container::ContainerRole::Brep,
        referenced_by: Vec::new(),
        data: Vec::new(),
    };
    let result = with_collection_limit(&[], 0, |ctx| parse_payloads(ctx, &[], &[entry]));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD shape entry index"));
}

#[test]
fn shape_entry_name_refuses_on_retained_limit() {
    let property = PropertyRecord {
        id: crate::native::native_id("property", "Shape"),
        owner: crate::native::native_id("object", "Shape"),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Part file=\"empty.brp\"/></Property>".into(),
            0,
        )
        .expect("valid test XML"),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    assert!(matches!(super::direct_shape_entry(&ctx, &property),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD shape entry name"));
}

#[test]
fn transferred_curve_refuses_at_caller_limit() {
    let curve = TextCurve::Line {
        origin: FinitePoint3::ZERO,
        direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0))
            .expect("finite direction"),
    };
    let association = cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new("object").expect("nonblank object"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let result = with_collection_limit(&[], 0, |ctx| {
        let mut transfer = super::CurveTransfer::default();
        super::append_text_curve(
            ctx,
            &curve,
            cadmpeg_ir::ids::CurveId::mint("fcstd:test:curve#limit").expect("identity grammar"),
            &association,
            &mut transfer,
        )
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD transferred curves"));
}

#[test]
fn transferred_surface_refuses_at_caller_limit() {
    let surface = TextSurface::Plane {
        origin: FinitePoint3::ZERO,
        axis: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))
            .expect("finite normal"),
        u_axis: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0))
            .expect("finite u axis"),
        v_reversed: false,
    };
    let association = cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new("object").expect("nonblank object"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let result = with_collection_limit(&[], 0, |ctx| {
        let mut curves = super::CurveTransfer::default();
        let mut surfaces = super::SurfaceTransfer::default();
        super::append_text_surface(
            ctx,
            &surface,
            cadmpeg_ir::ids::SurfaceId::mint("fcstd:test:surface#limit").expect("identity grammar"),
            &association,
            &mut curves,
            &mut surfaces,
        )
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD transferred surfaces"));
}

#[test]
fn nurbs_curve_copy_refuses_at_caller_limit() {
    let knots = vec![
        FiniteReal::ZERO,
        FiniteReal::ZERO,
        FiniteReal::ONE,
        FiniteReal::ONE,
    ];
    let points = vec![FinitePoint3::ZERO, FinitePoint3::ZERO];
    let nurbs =
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_finite_lanes(1, knots, points, None, false)
            .expect("valid curve lanes");
    let result = with_collection_limit(&[], 5, |ctx| super::clone_nurbs_curve(ctx, &nurbs));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD NURBS curve copy"));
}

#[test]
fn nurbs_surface_copy_refuses_at_caller_limit() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let knots = vec![
        FiniteReal::ZERO,
        FiniteReal::ZERO,
        FiniteReal::ONE,
        FiniteReal::ONE,
    ];
    let axis = || NurbsSurfaceAxis::new(1, knots.clone(), false);
    let nurbs = NurbsSurface::from_finite_lanes(
        axis(),
        axis(),
        NurbsSurfaceLanes::new(vec![vec![FinitePoint3::ZERO; 2]; 2], None),
        false,
    )
    .expect("valid surface lanes");
    let result = with_collection_limit(&[], 13, |ctx| super::clone_nurbs_surface(ctx, &nurbs));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD NURBS surface copy"));
}

fn test_parse_text(
    bytes: &[u8],
) -> Result<(super::ShapeSet, super::TextTopologyVersion), CodecError> {
    in_decode_context(|ctx| parse_text(ctx, bytes))
}

fn test_parse_binary_prefix(
    bytes: &[u8],
) -> Result<(super::ShapeSet, super::BinaryTopologyVersion), CodecError> {
    in_decode_context(|ctx| parse_binary_prefix(ctx, bytes))
}

fn test_parse_payloads(
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<Vec<ShapePayloadRecord>, CodecError> {
    in_decode_context(|ctx| parse_payloads(ctx, properties, entries))
}

#[test]
fn indexed_polygon_admits_only_aligned_parameters() {
    let node = cadmpeg_ir::features::FinitePoint3::ZERO;
    let mut facts = super::ShapeSet {
        locations: Vec::new(),
        curve2ds: Vec::new(),
        curves: Vec::new(),
        polygons3d: vec![super::TextPolygon3d {
            deflection: cadmpeg_ir::scalar::NonNegativeReal::ZERO,
            nodes: vec![node],
            parameters: Some(vec![]),
        }],
        polygons_on_triangulations: Vec::new(),
        surfaces: Vec::new(),
        triangulations: Vec::new(),
        tshapes: Vec::new().into(),
        roots: Vec::new(),
    };
    assert!(facts.validate().is_err());
    facts.polygons3d[0].parameters = Some(vec![
        cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite")
    ]);
    assert!(facts.validate().is_ok());
    facts.polygons3d[0].parameters = None;
    assert!(facts.validate().is_ok());
}

#[test]
fn polygon_deflection_is_admitted_on_source_and_native_wire() {
    assert!(super::admit_polygon_deflection(FiniteReal::ZERO).is_ok());
    assert!(super::admit_polygon_deflection(FiniteReal::new(-1.0).unwrap()).is_err());
    assert!(FiniteReal::new(f64::INFINITY).is_none());

    for (value, expected) in [
        ("nan", "non-finite 3D polygon deflection"),
        ("-1.0", "polygon deflection must be finite and non-negative"),
    ] {
        let text = format!(
                "CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 1\n1 0 {value} 0 0 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*"
            );
        let error = test_parse_text(text.as_bytes()).expect_err("invalid source deflection");
        assert!(error.to_string().contains(expected), "{error}");
    }

    let polygon = serde_json::json!({
        "deflection": 0.5,
        "nodes": [{"x": 0.0, "y": 0.0, "z": 0.0}],
        "parameters": null,
    });
    let admitted: super::TextPolygon3d = serde_json::from_value(polygon.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), polygon);
    let mut invalid = polygon;
    invalid["deflection"] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<super::TextPolygon3d>(invalid).is_err());

    let polygon = serde_json::json!({
        "nodes": [1],
        "deflection": 0.5,
        "parameters": null,
    });
    let admitted: super::TextPolygonOnTriangulation =
        serde_json::from_value(polygon.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), polygon);
    let mut invalid = polygon;
    invalid["deflection"] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<super::TextPolygonOnTriangulation>(invalid).is_err());
}

#[test]
fn tshape_collection_checks_indices_at_direct_wire_admission() {
    let wire = serde_json::json!([
        {"index": 1, "kind": "wire", "geometry": {"kind": "empty"},
         "flags": [false, false, false, false, false, false, false], "children": []},
        {"index": 2, "kind": "compound", "geometry": {"kind": "empty"},
         "flags": [false, false, false, false, false, false, false], "children": []}
    ]);
    let shapes: TextTShapes = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&shapes).unwrap(), wire);
    assert!(shapes.resolve(0).is_err());
    assert_eq!(shapes.resolve(1).unwrap().kind(), TextShapeKind::Wire);
    assert!(shapes.resolve(3).is_err());
    for index in [0, 1, 3] {
        let mut invalid = wire.clone();
        invalid[1]["index"] = serde_json::json!(index);
        assert!(serde_json::from_value::<TextTShapes>(invalid).is_err());
    }
}

#[test]
fn tshape_geometry_arms_write_their_representation_tables() {
    let wire = serde_json::json!([
        {"index": 1, "kind": "vertex",
         "geometry": {"kind": "vertex", "tolerance": 0.25,
             "point": {"x": 1.0, "y": 2.0, "z": 3.0},
             "representations": [{"parameter": 0.5, "second_parameter": null, "kind": 1,
                 "curve": 1, "surface": null, "location": 0}]},
         "flags": [true, false, true, false, true, false, true],
         "children": []},
        {"index": 2, "kind": "edge",
         "geometry": {"kind": "edge", "tolerance": 0.125, "same_parameter": true,
             "same_range": false, "degenerated": false,
             "representations": [{"kind": 1, "primary": 1, "secondary": null,
                 "surface": null, "second_surface": null, "location": 0,
                 "second_location": null, "parameter_range": [0.0, 1.0],
                 "continuity": null, "uv_endpoints": null}]},
         "flags": [false, false, false, false, false, false, false],
         "children": [{"shape": 1, "orientation": "forward", "location": 0}]}
    ]);
    let shapes: TextTShapes = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&shapes).unwrap(), wire);
}

#[test]
fn an_edge_representation_writes_its_continuity_token() {
    for (representation, expected) in [
        (
            TextEdgeRepresentation::PcurvePair {
                curves: [5, 6],
                continuity: "CN".to_owned(),
                surface: 7,
                location: 8,
                parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
                uv_endpoints: None,
            },
            serde_json::json!({
                "kind": 3, "primary": 5, "secondary": 6, "surface": 7,
                "second_surface": null, "location": 8, "second_location": null,
                "parameter_range": [0.0, 1.0], "continuity": "CN", "uv_endpoints": null
            }),
        ),
        (
            TextEdgeRepresentation::Regularity {
                continuity: "C1".to_owned(),
                surfaces: [2, 3],
                locations: [0, 4],
            },
            serde_json::json!({
                "kind": 4, "primary": 0, "secondary": null, "surface": 2,
                "second_surface": 3, "location": 0, "second_location": 4,
                "parameter_range": null, "continuity": "C1", "uv_endpoints": null
            }),
        ),
    ] {
        assert_eq!(serde_json::to_value(&representation).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<TextEdgeRepresentation>(expected).unwrap(),
            representation
        );
    }
}

#[test]
fn shape_set_wire_indices_match_position_for_both_carriers() {
    for form in ["text", "binary"] {
        let mut facts = serde_json::json!({
            "topology_version": 1,
            "locations": [], "curve2ds": [], "curves": [],
            "polygons3d": [], "polygons_on_triangulations": [],
            "surfaces": [], "triangulations": [], "roots": [],
            "tshapes": [
                {"index": 1, "kind": "wire", "geometry": {"kind": "empty"},
                 "flags": [false, false, false, false, false, false, false], "children": []},
                {"index": 2, "kind": "compound", "geometry": {"kind": "empty"},
                 "flags": [false, false, false, false, false, false, false], "children": []}
            ]
        });
        if form == "text" {
            facts["shape_types"] = serde_json::json!({"wire": 1, "compound": 1});
            facts["section_counts"] = serde_json::json!({
                "Locations": 0, "Curve2ds": 0, "Curves": 0, "Polygon3D": 0,
                "PolygonOnTriangulations": 0, "Surfaces": 0, "Triangulations": 0, "TShapes": 2
            });
        }
        let mut wire = serde_json::json!({
            "id": "shape", "property": "property", "entry": "Shape.brp",
            "form": form, "text": null, "binary": null
        });
        wire[form] = facts;
        let mut payload: ShapePayloadRecord = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&payload).unwrap(), wire);
        let (ShapePayload::Text { facts, .. } | ShapePayload::Binary { facts, .. }) =
            &mut payload.payload
        else {
            panic!("expected shape set");
        };
        let mut shapes = facts.tshapes.to_vec();
        shapes.swap(0, 1);
        facts.tshapes = shapes.into();
        let reordered = serde_json::to_value(payload).unwrap();
        assert_eq!(reordered[form]["tshapes"][0]["index"], 1);
        assert_eq!(reordered[form]["tshapes"][1]["index"], 2);
        assert_eq!(reordered[form]["tshapes"][0]["kind"], "compound");
        assert_eq!(reordered[form]["tshapes"][1]["kind"], "wire");

        wire[form]["tshapes"][1]["index"] = serde_json::json!(1);
        let error = serde_json::from_value::<ShapePayloadRecord>(wire).unwrap_err();
        assert!(error.to_string().contains("tshapes[1].index must equal 2"));
    }
}

#[test]
fn shape_payload_direct_wire_checks_cross_table_references() {
    for form in ["text", "binary"] {
        let mut facts = serde_json::json!({
            "topology_version": 1,
            "locations": [], "curve2ds": [], "curves": [],
            "polygons3d": [], "polygons_on_triangulations": [],
            "surfaces": [], "triangulations": [], "roots": [],
            "tshapes": [
                {"index": 1, "kind": "wire", "geometry": {"kind": "empty"},
                 "flags": [false, false, false, false, false, false, false], "children": []},
                {"index": 2, "kind": "compound", "geometry": {"kind": "empty"},
                 "flags": [false, false, false, false, false, false, false], "children": []}
            ]
        });
        if form == "text" {
            facts["shape_types"] = serde_json::json!({"wire": 1, "compound": 1});
            facts["section_counts"] = serde_json::json!({
                "Locations": 0, "Curve2ds": 0, "Curves": 0, "Polygon3D": 0,
                "PolygonOnTriangulations": 0, "Surfaces": 0, "Triangulations": 0, "TShapes": 2
            });
        }
        let mut wire = serde_json::json!({
            "id": "shape", "property": "property", "entry": "Shape.brp",
            "form": form, "text": null, "binary": null
        });
        wire[form] = facts;

        wire[form]["tshapes"][1]["children"] = serde_json::json!([
            {"shape": 2, "orientation": "forward", "location": 0}
        ]);
        let error = serde_json::from_value::<ShapePayloadRecord>(wire.clone()).unwrap_err();
        assert!(error.to_string().contains("non-prior"), "{form}: {error}");

        wire[form]["tshapes"][1]["children"] = serde_json::json!([]);
        wire[form]["roots"] = serde_json::json!([
            {"shape": 3, "orientation": "forward", "location": 0}
        ]);
        let error = serde_json::from_value::<ShapePayloadRecord>(wire).unwrap_err();
        assert!(error.to_string().contains("root TShape"), "{form}: {error}");
    }
}

#[test]
fn shape_payload_wire_admits_only_versions_of_its_carrier() {
    for (form, maximum) in [("text", 3), ("binary", 4)] {
        for version in [0, 1, maximum, maximum + 1, u8::MAX] {
            let facts = serde_json::json!({
                "topology_version": version,
                "locations": [], "curve2ds": [], "curves": [],
                "polygons3d": [], "polygons_on_triangulations": [],
                "surfaces": [], "triangulations": [], "tshapes": [], "roots": [],
                "shape_types": {},
                "section_counts": {"Locations": 0, "Curve2ds": 0, "Curves": 0,
                    "Polygon3D": 0, "PolygonOnTriangulations": 0, "Surfaces": 0,
                    "Triangulations": 0, "TShapes": 0}
            });
            let mut wire = serde_json::json!({
                "id": "shape", "property": "property", "entry": "Shape.brp",
                "form": form, "text": null, "binary": null
            });
            wire[form] = facts;
            let result = serde_json::from_value::<ShapePayloadRecord>(wire);
            if (1..=maximum).contains(&version) {
                let payload = result.unwrap();
                assert_eq!(payload.payload.topology_version(), Some(version));
                assert_eq!(
                    serde_json::to_value(payload).unwrap()[form]["topology_version"],
                    version
                );
            } else {
                assert!(result.unwrap_err().to_string().contains("topology_version"));
            }
        }
    }
}

#[test]
fn face_table_references_preserve_wire_absence_and_reject_zero_triangulation() {
    let mut wire = serde_json::json!({
        "index": 1, "kind": "face",
        "geometry": { "kind": "face", "natural_restriction": false,
            "tolerance": 0.0, "surface": 0, "location": 0, "triangulation": null },
        "flags": [false, false, false, false, false, false, false], "children": []
    });
    let shape =
        TextTShape::try_from(serde_json::from_value::<TextTShapeWire>(wire.clone()).unwrap())
            .unwrap();
    assert_eq!(
        serde_json::to_value(TextTShapes::from(vec![shape])).unwrap(),
        serde_json::Value::Array(vec![wire.clone()])
    );
    wire["geometry"]["triangulation"] = serde_json::json!(0);
    assert!(
        TextTShape::try_from(serde_json::from_value::<TextTShapeWire>(wire).unwrap())
            .unwrap_err()
            .contains("triangulation")
    );
    assert!(TableRef::<TextSurface>::new(0).is_err());
    assert!(TableRef::<TextSurface>::new(1)
        .unwrap()
        .resolve(&[])
        .is_err());
    assert_eq!(
        LocationRef::Identity.resolve(&[]).unwrap(),
        Transform::identity()
    );
    assert!(LocationRef::from(1).resolve(&[]).is_err());
}

#[test]
fn expands_occt_periodic_knots_and_cyclic_surface_poles() {
    in_decode_context(|ctx| {
        let normalized = normalize_periodic_surface(
            ctx,
            [3, 1],
            [
                vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0]
                    .into_iter()
                    .map(|value| FiniteReal::new(value).unwrap())
                    .collect(),
                vec![0.0, 0.0, 1.0, 1.0]
                    .into_iter()
                    .map(|value| FiniteReal::new(value).unwrap())
                    .collect(),
            ],
            [6, 2],
            (0..6)
                .flat_map(|u| {
                    [
                        Point3::new(f64::from(u), 0.0, 0.0),
                        Point3::new(f64::from(u), 1.0, 0.0),
                    ]
                })
                .map(|point| FinitePoint3::new(point).unwrap())
                .collect(),
            None,
            [true, false],
        )
        .expect("valid periodic surface");

        assert_eq!(normalized.u_count(), 7);
        assert_eq!(
            normalized.u_knots().as_slice(),
            [-0.5, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.5]
        );
        let poles = normalized.poles();
        assert_eq!(poles.len(), 14);
        assert_eq!(poles[12], poles[0]);
        assert_eq!(poles[13], poles[1]);
        let start = cadmpeg_ir::eval::nurbs_surface_point(&normalized, 0.0, 0.5)
            .expect("periodic start point");
        let end = cadmpeg_ir::eval::nurbs_surface_point(&normalized, 1.0, 0.5)
            .expect("periodic end point");
        assert!((start.x - end.x).abs() <= 1.0e-12);
        assert!((start.y - end.y).abs() <= 1.0e-12);
        assert!((start.z - end.z).abs() <= 1.0e-12);
    });
}

fn text_brep(curves: &str, curve_count: usize, surfaces: &str, surface_count: usize) -> String {
    format!(
            "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves {curve_count}\n{curves}\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces {surface_count}\n{surfaces}\nTriangulations 0\nTShapes 0\n*"
        )
}

#[test]
fn binary_edge_continuity_retains_decimal_byte_spelling() {
    in_decode_context(|ctx| {
        for (byte, spelling) in [(0, "0"), (2, "2")] {
            for kind in [3, 4] {
                let mut bytes = Vec::new();
                if kind == 3 {
                    bytes.extend_from_slice(&1_i32.to_le_bytes());
                    bytes.extend_from_slice(&2_i32.to_le_bytes());
                }
                bytes.push(byte);
                bytes.extend_from_slice(&1_i32.to_le_bytes());
                bytes.extend_from_slice(&0_i32.to_le_bytes());
                if kind == 3 {
                    bytes.extend_from_slice(&0_f64.to_le_bytes());
                    bytes.extend_from_slice(&1_f64.to_le_bytes());
                } else {
                    bytes.extend_from_slice(&1_i32.to_le_bytes());
                    bytes.extend_from_slice(&0_i32.to_le_bytes());
                }
                let mut cursor = BinaryCursor::new(ctx, &bytes);
                let record =
                    parse_binary_edge_representation(&mut cursor, 1, kind, 0, 2, 1, 0, 0, 0, 0)
                        .unwrap();
                let (TextEdgeRepresentation::PcurvePair { continuity, .. }
                | TextEdgeRepresentation::Regularity { continuity, .. }) = record
                else {
                    panic!("expected continuity representation");
                };
                assert_eq!(continuity, spelling);
                assert_eq!(cursor.remaining(), 0);
            }
        }
    });
}

#[test]
fn parses_joined_seam_pcurve_continuity_token() {
    in_decode_context(|ctx| {
        let tokens = ["1", "2CN", "1", "0", "0", "10"];
        let mut cursor = TokenCursor::new(ctx, &tokens);
        let counts = BTreeMap::from([
            ("Curve2ds".to_owned(), 2),
            ("Surfaces".to_owned(), 1),
            ("Locations".to_owned(), 0),
        ]);
        let record = parse_edge_representation(3, &mut cursor, &counts, 1)
            .expect("joined pcurve continuity");
        let TextEdgeRepresentation::PcurvePair {
            curves, continuity, ..
        } = record
        else {
            panic!("expected pcurve pair");
        };
        assert_eq!(curves, [1, 2]);
        assert_eq!(continuity, "CN");
        assert!(cursor.is_empty());
    });
}

#[test]
fn retains_indirect_analytic_surface_parameter_frames() {
    in_decode_context(|ctx| {
        let tokens = [
            "0", "0", "0", "0", "0", "1", "1", "0", "0", "0", "-1", "0", "2", "0.5",
        ];
        let mut cursor = TokenCursor::new(ctx, &tokens);
        let cone =
            parse_analytic_surface(AnalyticSurfaceKind::Cone, &mut cursor).expect("indirect cone");
        assert!(matches!(
            cone,
            TextSurface::Cone {
                radius,
                half_angle,
                u_reversed: true,
                ..
            } if radius.get() == 2.0 && half_angle.get() == 0.5
        ));

        let tokens = [
            "0", "0", "0", "0", "0", "1", "1", "0", "0", "0", "-1", "0", "2",
        ];
        let mut cursor = TokenCursor::new(ctx, &tokens);
        let sphere = parse_analytic_surface(AnalyticSurfaceKind::Sphere, &mut cursor)
            .expect("indirect sphere");
        assert!(matches!(
            sphere,
            TextSurface::Sphere {
                radius,
                u_reversed: true,
                ..
            } if radius.get() == 2.0
        ));

        let tokens = [
            "0", "0", "0", "0", "0", "1", "1", "0", "0", "0", "-1", "0", "4", "1",
        ];
        let mut cursor = TokenCursor::new(ctx, &tokens);
        let torus = parse_analytic_surface(AnalyticSurfaceKind::Torus, &mut cursor)
            .expect("indirect torus");
        assert!(matches!(
            torus,
            TextSurface::Torus {
                major_radius,
                minor_radius,
                u_reversed: true,
                ..
            } if major_radius.get() == 4.0 && minor_radius.get() == 1.0
        ));
    });
}

#[test]
fn binds_only_the_direct_shape_entry_as_typed_payload() {
    let property = PropertyRecord {
        id: crate::native::native_id("property", "Shape:SuppressedShape"),
        owner: crate::native::native_id("object", "Shape"),
        name: "SuppressedShape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["empty.brp".into(), "empty-2.brp".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            r#"<Property><Part file="empty.brp"/><Extra file="empty-2.brp"/></Property>"#.into(),
            0,
        )
        .unwrap(),
    };
    let entry = EntryRecord {
        id: crate::native::native_id("entry", "empty.brp"),
        name: "empty.brp".into(),
        role: cadmpeg_core::container::ContainerRole::Brep,
        referenced_by: vec![property.id.clone()],
        data: Vec::new(),
    };
    let second_entry = EntryRecord {
        id: crate::native::native_id("entry", "empty-2.brp"),
        name: "empty-2.brp".into(),
        role: cadmpeg_core::container::ContainerRole::Brep,
        referenced_by: vec![property.id.clone()],
        data: Vec::new(),
    };
    let payloads =
        test_parse_payloads(&[property], &[entry, second_entry]).expect("empty shape payload");
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0].entry, "fcstd:native:entry#empty.brp");
    assert!(matches!(payloads[0].payload, ShapePayload::Empty));
}

#[test]
fn ignores_nested_shape_part_carriers() {
    let property = PropertyRecord {
        id: crate::native::native_id("property", "Shape:Nested"),
        owner: crate::native::native_id("object", "Shape"),
        name: "Nested".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["nested.brp".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            r#"<Property><Wrapper><Part file="nested.brp"/></Wrapper></Property>"#.into(),
            0,
        )
        .unwrap(),
    };
    let payloads = test_parse_payloads(&[property], &[]).expect("nested carrier is ignored");
    assert!(payloads.is_empty());
}

#[test]
fn rejects_duplicate_direct_shape_part_carriers() {
    let property = PropertyRecord {
        id: crate::native::native_id("property", "Shape:Duplicate"),
        owner: crate::native::native_id("object", "Shape"),
        name: "Duplicate".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["first.brp".into(), "second.brp".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            r#"<Property><Part file="first.brp"/><Part file="second.brp"/></Property>"#.into(),
            0,
        )
        .unwrap(),
    };
    assert!(test_parse_payloads(&[property], &[]).is_err());
}

#[test]
fn retains_transient_shape_without_a_carrier() {
    let property = PropertyRecord {
        id: crate::native::native_id("property", "Shape:PreviewShape"),
        owner: crate::native::native_id("object", "Shape"),
        name: "PreviewShape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Geometry,
        status: Some(152),
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            r#"<_Property name="PreviewShape" type="Part::PropertyPartShape" status="152"/>"#
                .into(),
            0,
        )
        .unwrap(),
    };
    let payloads = test_parse_payloads(&[property], &[]).expect("transient shape is retained");
    assert!(payloads.is_empty());
}

#[test]
fn ignores_non_shape_runtime_names_when_framing_payloads() {
    let property = PropertyRecord {
        id: crate::native::native_id("property", "Shape:Custom"),
        owner: crate::native::native_id("object", "Shape"),
        name: "Custom".into(),
        type_name: "Custom::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["custom.brp".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).unwrap(),
    };

    let payloads = test_parse_payloads(&[property], &[]).expect("unknown type is retained");
    assert!(payloads.is_empty());
}

#[test]
fn normalizes_rational_bezier_curve_to_nurbs() {
    let input = text_brep("6 1 2 0 0 0 1 5 0 0 2 10 0 0 1", 1, "", 0);
    let facts = test_parse_text(input.as_bytes())
        .expect("valid Bezier curve")
        .0;
    let TextCurve::Nurbs(curve) = &facts.curves[0] else {
        panic!("Bezier curve was not normalized to NURBS")
    };
    assert_eq!(curve.degree(), 2);
    assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(curve.pole_rows().weights(), Some(vec![1.0, 2.0, 1.0]));
}

#[test]
fn normalizes_bezier_surface_to_nurbs() {
    let input = text_brep("", 0, "8 0 0 1 1 0 0 0 0 1 0 1 0 0 1 1 0", 1);
    let facts = test_parse_text(input.as_bytes())
        .expect("valid Bezier surface")
        .0;
    let TextSurface::Nurbs(surface) = &facts.surfaces[0] else {
        panic!("Bezier surface was not normalized to NURBS")
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (1, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
    assert_eq!(surface.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert!(surface.weights().is_none());
}

#[test]
fn rejects_invalid_recursive_curve_domains() {
    let reversed = text_brep("8 2 1 1 0 0 0 1 0 0", 1, "", 0);
    let error = test_parse_text(reversed.as_bytes()).expect_err("reversed trim must fail");
    assert!(error.to_string().contains("parameter range is reversed"));

    let zero_normal = text_brep("9 2 0 0 0 1 0 0 0 1 0 0", 1, "", 0);
    let error = test_parse_text(zero_normal.as_bytes()).expect_err("zero normal must fail");
    assert!(error.to_string().contains("direction is zero"));
}

#[test]
fn parses_recursive_surface_constructions() {
    let input = text_brep(
            "",
            0,
            "6 0 0 2 1 0 0 0 1 0 0\n7 0 0 0 0 0 1 1 0 0 0 1 0 0\n10 0 1 2 3 11 4 1 0 0 0 0 0 1 1 0 0 0 1 0",
            3,
        );
    let facts = test_parse_text(input.as_bytes())
        .expect("recursive surfaces")
        .0;
    let TextSurface::Extrusion {
        direction,
        directrix,
    } = &facts.surfaces[0]
    else {
        panic!("expected extrusion")
    };
    assert_eq!([direction.x, direction.y, direction.z], [0.0, 0.0, 2.0]);
    assert!(matches!(directrix.curve(), TextCurve::Line { .. }));

    let TextSurface::Revolution { directrix, .. } = &facts.surfaces[1] else {
        panic!("expected revolution")
    };
    assert!(matches!(directrix.curve(), TextCurve::Line { .. }));

    let TextSurface::Trimmed {
        parameter_ranges,
        basis,
    } = &facts.surfaces[2]
    else {
        panic!("expected trimmed surface")
    };
    assert_eq!(
        parameter_ranges.map(|range| range.map(FiniteReal::get)),
        [[0.0, 1.0], [2.0, 3.0]]
    );
    assert!(matches!(basis.surface(), TextSurface::Offset { .. }));
}

#[test]
fn recursive_brep_carriers_retain_checked_fields_across_native_json() {
    for curve_text in ["8 0 1 1 0 0 0 1 0 0", "9 2 0 0 1 1 0 0 0 1 0 0"] {
        let input = text_brep(curve_text, 1, "", 0);
        let facts = test_parse_text(input.as_bytes())
            .expect("recursive curve")
            .0;
        let wire = serde_json::to_value(&facts.curves[0]).unwrap();
        let admitted: TextCurve = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    }

    let input = text_brep(
            "",
            0,
            "6 0 0 2 1 0 0 0 1 0 0\n7 0 0 0 0 0 1 1 0 0 0 1 0 0\n10 0 1 2 3 11 4 1 0 0 0 0 0 1 1 0 0 0 1 0",
            3,
        );
    let facts = test_parse_text(input.as_bytes())
        .expect("recursive surfaces")
        .0;
    for surface in facts.surfaces {
        let wire = serde_json::to_value(&surface).unwrap();
        let admitted: TextSurface = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    }
}

#[test]
fn resolves_elementary_and_compound_locations_in_source_order() {
    let input = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 3\n1 1 0 0 5 0 1 0 0 0 0 1 0\n2 1 2 0\n2 1 -1 2 1 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
    let facts = test_parse_text(input.as_bytes()).expect("location table").0;
    assert_eq!(facts.locations.len(), 3);
    assert_eq!(facts.locations[0].transform.rows()[0][3], 5.0);
    assert_eq!(facts.locations[1].transform.rows()[0][3], 10.0);
    assert_eq!(facts.locations[2].transform.rows()[0][3], 5.0);
    assert_eq!(facts.locations[2].factors[0].power, -1);
}

#[test]
fn checked_location_rows_keep_signed_zero_and_source_refusals() {
    let text = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 1\n1 1 -0 0 5 0 1 0 0 0 0 1 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
    let facts = test_parse_text(text.as_bytes())
        .expect("finite text location")
        .0;
    assert_eq!(
        facts.locations[0].transform.affine_rows()[0][1].to_bits(),
        (-0.0_f64).to_bits()
    );
    let invalid = text.replace("1 -0 0 5", "1 NaN 0 5");
    let error = test_parse_text(invalid.as_bytes()).expect_err("non-finite text location");
    assert!(error
        .to_string()
        .contains("non-finite location transform value"));

    let binary = |first: f64| {
        let mut bytes = b"\nOpen CASCADE Topology V3 (c)\nLocations 1\n".to_vec();
        bytes.push(1);
        for value in [
            first, -0.0, 0.0, 5.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(b"Curve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n");
        bytes
    };
    let facts = test_parse_binary_prefix(&binary(1.0))
        .expect("finite binary location")
        .0;
    assert_eq!(
        facts.locations[0].transform.affine_rows()[0][1].to_bits(),
        (-0.0_f64).to_bits()
    );
    let error =
        test_parse_binary_prefix(&binary(f64::NAN)).expect_err("non-finite binary location");
    assert!(error
        .to_string()
        .contains("non-finite binary location transform"));
}

#[test]
fn parses_binary_locations_and_recursive_parameter_curves() {
    fn real(bytes: &mut Vec<u8>, value: f64) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let mut bytes = b"\nOpen CASCADE Topology V3 (c)\nLocations 1\n".to_vec();
    bytes.push(1);
    for value in [1.0, 0.0, 0.0, 5.0, 0.0, 1.0, 0.0, 6.0, 0.0, 0.0, 1.0, 7.0] {
        real(&mut bytes, value);
    }
    bytes.extend_from_slice(b"Curve2ds 2\n");
    bytes.push(1);
    for value in [0.0, 0.0, 1.0, 0.0] {
        real(&mut bytes, value);
    }
    bytes.push(8);
    real(&mut bytes, 0.0);
    real(&mut bytes, std::f64::consts::PI);
    bytes.push(2);
    for value in [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 2.0] {
        real(&mut bytes, value);
    }
    bytes.extend_from_slice(b"Curves 2\n");
    bytes.push(1);
    for value in [0.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
        real(&mut bytes, value);
    }
    bytes.push(9);
    for value in [0.5, 0.0, 0.0, 1.0] {
        real(&mut bytes, value);
    }
    bytes.push(1);
    for value in [1.0, 2.0, 3.0, 0.0, 1.0, 0.0] {
        real(&mut bytes, value);
    }
    bytes.extend_from_slice(b"Polygon3D 1\n");
    bytes.extend_from_slice(&2_i32.to_le_bytes());
    bytes.push(1);
    real(&mut bytes, 0.01);
    for value in [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
        real(&mut bytes, value);
    }
    bytes.extend_from_slice(b"PolygonOnTriangulations 1\n");
    bytes.extend_from_slice(&2_i32.to_le_bytes());
    bytes.extend_from_slice(&1_i32.to_le_bytes());
    bytes.extend_from_slice(&2_i32.to_le_bytes());
    real(&mut bytes, 0.02);
    bytes.push(0);
    bytes.extend_from_slice(b"Surfaces 2\n");
    bytes.push(1);
    for value in [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        real(&mut bytes, value);
    }
    bytes.push(11);
    real(&mut bytes, 0.25);
    bytes.push(4);
    for value in [
        0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0,
    ] {
        real(&mut bytes, value);
    }
    bytes.extend_from_slice(b"Triangulations 1\n");
    bytes.extend_from_slice(&3_i32.to_le_bytes());
    bytes.extend_from_slice(&1_i32.to_le_bytes());
    bytes.push(1);
    real(&mut bytes, 0.03);
    for value in [
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
    ] {
        real(&mut bytes, value);
    }
    for node in [1_i32, 2, 3] {
        bytes.extend_from_slice(&node.to_le_bytes());
    }
    bytes.extend_from_slice(b"TShapes 0\n");

    let (facts, version) = test_parse_binary_prefix(&bytes).expect("binary prefix");
    assert_eq!(version.number(), 3);
    assert_eq!(facts.locations[0].transform.rows()[0][3], 5.0);
    assert!(matches!(facts.curve2ds[0], TextCurve2d::Line { .. }));
    assert!(matches!(facts.curve2ds[1], TextCurve2d::Trimmed { .. }));
    assert!(matches!(facts.curves[0], TextCurve::Line { .. }));
    assert!(matches!(facts.curves[1], TextCurve::Offset { .. }));
    assert_eq!(facts.polygons3d[0].nodes.len(), 2);
    assert_eq!(
        facts.polygons3d[0].parameters.as_deref(),
        Some(
            &[
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE
            ][..]
        )
    );
    assert_eq!(facts.polygons_on_triangulations[0].nodes, [1, 2]);
    assert!(matches!(facts.surfaces[0], TextSurface::Plane { .. }));
    assert!(matches!(facts.surfaces[1], TextSurface::Offset { .. }));
    assert_eq!(facts.triangulations[0].triangles(), [[0, 1, 2]]);
    assert!(facts.tshapes.is_empty());
    assert!(facts.roots.is_empty());
}

#[test]
fn parses_analytic_spline_and_recursive_parameter_curves() {
    let input = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 3\n1 0 0 1 0\n6 1 2 0 0 1 5 0 2 10 0 1\n8 0 6.28 9 2 2 0 0 1 0 0 1 3\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
    let facts = test_parse_text(input.as_bytes()).expect("2D curve table").0;
    assert!(matches!(facts.curve2ds[0], TextCurve2d::Line { .. }));
    let TextCurve2d::Nurbs(nurbs) = &facts.curve2ds[1] else {
        panic!("expected normalized 2D Bezier")
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(
        nurbs
            .knots
            .iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    assert_eq!(
        nurbs
            .weights
            .as_ref()
            .map(|values| values.iter().map(|value| value.get()).collect::<Vec<_>>()),
        Some(vec![1.0, 2.0, 1.0])
    );
    let TextCurve2d::Trimmed {
        parameter_range,
        basis,
    } = &facts.curve2ds[2]
    else {
        panic!("expected trimmed 2D curve")
    };
    assert_eq!(parameter_range.map(FiniteReal::get), [0.0, 314.0 / 50.0]);
    assert!(matches!(basis.curve(), TextCurve2d::Offset { .. }));
}

#[test]
fn expands_periodic_parameter_curve_knots_and_poles() {
    let input = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 1\n7 1 1 6 6 2 0 0 1 1 0 1 1 1 1 0 1 1 -1 0 1 -1 -1 1 0 6 6.283185307179586 6\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
    let facts = test_parse_text(input.as_bytes())
        .expect("periodic parameter curve")
        .0;
    let TextCurve2d::Nurbs(nurbs) = &facts.curve2ds[0] else {
        panic!("expected periodic NURBS")
    };

    assert_eq!(nurbs.control_points.len(), 7);
    assert_eq!(nurbs.weights.as_ref().map(Vec::len), Some(7));
    assert_eq!(nurbs.knots.len(), 14);
    assert_eq!(nurbs.control_points.first(), nurbs.control_points.last());
    assert_eq!(
        nurbs.weights.as_ref().map(|weights| weights[0].get()),
        Some(1.0)
    );
    assert_eq!(
        nurbs.weights.as_ref().map(|weights| weights[6].get()),
        Some(1.0)
    );
}

#[test]
fn parses_polygonal_carriers_and_version_three_normals() {
    let input = "CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 1\n2 1 0.1 0 0 0 1 0 0 0 1\nPolygonOnTriangulations 1\n2 1 2 p 0.2 1 0 1\nSurfaces 0\nTriangulations 1\n3 1 1 1 0.01 0 0 0 1 0 0 0 1 0 0 0 1 0 0 1 1 2 3 0 0 1 0 0 1 0 0 1\nTShapes 0\n*";
    let facts = test_parse_text(input.as_bytes())
        .expect("polygonal carriers")
        .0;
    assert_eq!(facts.polygons3d[0].nodes.len(), 2);
    assert_eq!(
        facts.polygons3d[0].parameters.as_deref(),
        Some(
            &[
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE
            ][..]
        )
    );
    assert_eq!(facts.polygons_on_triangulations[0].nodes, [1, 2]);
    let triangulation = &facts.triangulations[0];
    assert_eq!(triangulation.nodes().len(), 3);
    assert_eq!(triangulation.triangles(), [[0, 1, 2]]);
    assert_eq!(triangulation.uv_nodes().map(<[_]>::len), Some(3));
    assert_eq!(triangulation.normals().map(<[_]>::len), Some(3));
}

#[test]
fn polygon_source_readers_refuse_nonfinite_nodes_and_parameters() {
    for (polygon_section, expected) in [
        (
            "Polygon3D 1\n1 0 0.1 NaN 0 0\nPolygonOnTriangulations 0",
            "non-finite 3D polygon node",
        ),
        (
            "Polygon3D 1\n1 1 0.1 0 0 0 NaN\nPolygonOnTriangulations 0",
            "non-finite 3D polygon parameter",
        ),
        (
            "Polygon3D 0\nPolygonOnTriangulations 1\n1 1 p 0.1 1 NaN",
            "non-finite polygon-on-triangulation parameter",
        ),
    ] {
        let input = format!(
                "CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\n{polygon_section}\nSurfaces 0\nTriangulations 0\nTShapes 0\n*"
            );
        let error = test_parse_text(input.as_bytes()).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn parses_subshape_first_topology_and_reverse_references() {
    let input = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 1\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 8\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 1 0 0 1 0 1001000 +8 0 +7 0 *\nWi 1001000 +6 0 *\nFa 0 0.001 1 0 1001000 +5 0 *\nSh 1001000 +4 0 *\nSo 1001000 +3 0 *\nCo 1001000 +2 0 *\n+1 0 *";
    let facts = test_parse_text(input.as_bytes()).expect("topology table").0;
    assert_eq!(facts.tshapes.len(), 8);
    assert_eq!(facts.tshapes[2].kind(), TextShapeKind::Edge);
    assert_eq!(facts.tshapes[2].children[0].shape, 1);
    assert_eq!(facts.tshapes[2].children[1].shape, 2);
    let TextTShapeGeometry::Edge {
        representations, ..
    } = &facts.tshapes[2].geometry
    else {
        panic!("expected edge geometry")
    };
    assert_eq!(
        representations[0]
            .parameter_range()
            .map(|range| range.map(FiniteReal::get)),
        Some([0.0, 1.0])
    );
    assert_eq!(facts.roots.len(), 1);
    assert_eq!(facts.roots[0].shape, 8);
}

#[test]
fn a_text_vertex_holds_its_admitted_point() {
    let table = |point: &str| {
        format!(
                "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nVe 0.001 {point} 0 0 1001000 *\n+1 0 *"
            )
    };
    let facts = test_parse_text(table("1 -2.5 3").as_bytes())
        .expect("vertex table")
        .0;
    let TextTShapeGeometry::Vertex { point, .. } = facts.tshapes[0].geometry else {
        panic!("expected vertex geometry")
    };
    assert_eq!(point, Point3::new(1.0, -2.5, 3.0));
    let error = test_parse_text(table("1 inf 3").as_bytes()).expect_err("non-finite vertex");
    assert!(
        error
            .to_string()
            .contains("non-finite vertex point in text B-rep Curves table"),
        "{error}"
    );
}

#[test]
fn rejects_oversized_and_out_of_order_text_tables() {
    let oversized = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 1000001\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
    assert!(test_parse_text(oversized)
        .expect_err("oversized table")
        .to_string()
        .contains("count limit"));

    let out_of_order = b"CASCADE Topology V1, (c) Matra-Datavision\nCurve2ds 0\nLocations 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
    assert!(test_parse_text(out_of_order)
        .expect_err("out-of-order table")
        .to_string()
        .contains("out of order"));
}

#[test]
fn rejects_duplicate_text_headers_and_section_markers() {
    let duplicate_header = b"CASCADE Topology V1, (c) Matra-Datavision\nCASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
    assert!(matches!(
        test_parse_text(duplicate_header),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let duplicate_section = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
    assert!(matches!(
        test_parse_text(duplicate_section),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
pub(crate) fn transfers_recursive_exact_parameter_curve_geometry() {
    let source = crate::brep::TextCurve2d::Offset {
        distance: FiniteReal::new(0.25).unwrap(),
        basis: super::NestedCurve2d::try_new(crate::brep::TextCurve2d::Trimmed {
            parameter_range: [
                FiniteReal::new(0.0).unwrap(),
                FiniteReal::new(std::f64::consts::PI).unwrap(),
            ],
            basis: super::NestedCurve2d::try_new(crate::brep::TextCurve2d::Circle {
                center: cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(
                    1.0, 2.0,
                ))
                .unwrap(),
                x_axis: cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(
                    1.0, 0.0,
                ))
                .unwrap(),
                y_axis: cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(
                    0.0, 1.0,
                ))
                .unwrap(),
                radius: FiniteReal::new(3.0).unwrap(),
            })
            .expect("one inline basis is admitted"),
        })
        .expect("two inline bases are admitted"),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Offset(offset_pcurve) =
        crate::topology_transfer::pcurve_geometry(&ctx, &source)
            .expect("recursive pcurve lanes pair")
            .expect("valid recursive pcurve")
    else {
        panic!("expected offset pcurve");
    };
    let distance = offset_pcurve.distance();
    let basis = offset_pcurve.basis();
    assert_eq!(distance.get(), 0.25);
    assert!(
        matches!(basis, cadmpeg_ir::geometry::pcurve::PcurveGeometry::Trimmed(trimmed_pcurve)
        if {
            let basis = trimmed_pcurve.basis();
            matches!(basis, cadmpeg_ir::geometry::pcurve::PcurveGeometry::Circle(circle_pcurve)
                    if { circle_pcurve.radius().get() == 3.0 })
        })
    );
}

#[test]
pub(crate) fn transfers_binary_exact_curve_and_surface_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.bin"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let mut brep = b"\nOpen CASCADE Topology V3 (c)\nLocations 0\nCurve2ds 0\nCurves 1\n".to_vec();
    brep.push(1);
    for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.extend_from_slice(b"Polygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n");
    brep.push(1);
    for value in [
        0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
    ] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.extend_from_slice(b"Triangulations 1\n");
    brep.extend_from_slice(&3_i32.to_le_bytes());
    brep.extend_from_slice(&1_i32.to_le_bytes());
    brep.push(0);
    for value in [0.01_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    for node in [1_i32, 2, 3] {
        brep.extend_from_slice(&node.to_le_bytes());
    }
    brep.extend_from_slice(b"TShapes 7\n");
    let flags = |brep: &mut Vec<u8>| brep.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0]);
    let child = |brep: &mut Vec<u8>, orientation: u8, reverse_index: i32| {
        brep.push(orientation);
        brep.extend_from_slice(&reverse_index.to_le_bytes());
        brep.extend_from_slice(&0_i32.to_le_bytes());
    };
    brep.push(7);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.push(0);
    flags(&mut brep);
    brep.push(b'*');
    brep.push(6);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    brep.extend_from_slice(&[1, 1, 1, 0]);
    flags(&mut brep);
    child(&mut brep, 0, 7);
    child(&mut brep, 1, 7);
    brep.push(b'*');
    brep.push(5);
    flags(&mut brep);
    child(&mut brep, 0, 6);
    brep.push(b'*');
    brep.push(4);
    brep.push(0);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    brep.extend_from_slice(&1_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    brep.push(2);
    brep.extend_from_slice(&1_i32.to_le_bytes());
    flags(&mut brep);
    child(&mut brep, 0, 5);
    brep.push(b'*');
    for (kind, reverse_index) in [(3_u8, 4_i32), (2, 3), (0, 2)] {
        brep.push(kind);
        flags(&mut brep);
        child(&mut brep, 0, reverse_index);
        brep.push(b'*');
    }
    brep.extend_from_slice(&7_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.bin", &brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("binary curve carrier");
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(matches!(
        result.ir().model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
    ));
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
    ));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].triangles(), [[0, 1, 2]]);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(
        result.ir().model.tessellations[0].body.as_ref(),
        Some(&result.ir().model.bodies[0].id)
    );
    assert_eq!(
        result.ir().model.tessellations[0].faces,
        [result.ir().model.faces[0].id.clone()]
    );
    assert_eq!(result.ir().model.coedges.len(), 1);
    assert!(result.report().geometry_transferred());
}

#[test]
fn transfers_zero_radius_brep_circles_as_degenerate_curves() {
    let center = cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0);
    let curve = crate::brep::TextCurve::Circle {
        center: FinitePoint3::new(center).unwrap(),
        axis: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        ref_direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        radius: FiniteReal::ZERO,
    };
    let association = cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new("object")
            .expect("nonempty source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let mut transfer = crate::brep::CurveTransfer::default();

    let geometry = in_decode_context(|ctx| {
        crate::brep::append_text_curve(
            ctx,
            &curve,
            cadmpeg_ir::ids::CurveId::mint("fcstd:test:curve#1").expect("identity grammar"),
            &association,
            &mut transfer,
        )
    })
    .unwrap();

    assert_eq!(
        geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
            cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(center).unwrap()
        ))
    );
}

#[test]
fn native_analytic_curve_fields_remain_checked_and_wire_identical() {
    let point = serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0});
    let axis = serde_json::json!({"x": 0.0, "y": 0.0, "z": 1.0});
    let reference = serde_json::json!({"x": 1.0, "y": 0.0, "z": 0.0});
    let wires = [
        serde_json::json!({"kind": "line", "origin": point, "direction": axis}),
        serde_json::json!({"kind": "circle", "center": point, "axis": axis, "ref_direction": reference, "radius": 2.0}),
        serde_json::json!({"kind": "ellipse", "center": point, "axis": axis, "major_direction": reference, "major_radius": 3.0, "minor_radius": 2.0}),
        serde_json::json!({"kind": "parabola", "vertex": point, "axis": axis, "major_direction": reference, "focal_distance": 2.0}),
        serde_json::json!({"kind": "hyperbola", "center": point, "axis": axis, "major_direction": reference, "major_radius": 2.0, "minor_radius": 3.0}),
    ];
    for wire in wires {
        let curve: TextCurve = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&curve).unwrap(), wire);
        if let TextCurve::Line { origin, direction } = curve {
            let _: FinitePoint3 = origin;
            let _: FiniteVector3 = direction;
        }
    }
}

#[test]
fn native_parameter_curve_fields_remain_checked_through_transfer() {
    let center = serde_json::json!({"u": 1.0, "v": 2.0});
    let x_axis = serde_json::json!({"u": 1.0, "v": 0.0});
    let y_axis = serde_json::json!({"u": 0.0, "v": 1.0});
    let wires = [
        serde_json::json!({"kind": "line", "origin": center, "direction": x_axis}),
        serde_json::json!({"kind": "circle", "center": center, "x_axis": x_axis, "y_axis": y_axis, "radius": 3.0}),
        serde_json::json!({"kind": "ellipse", "center": center, "x_axis": x_axis, "y_axis": y_axis, "major_radius": 3.0, "minor_radius": 2.0}),
        serde_json::json!({"kind": "parabola", "vertex": center, "x_axis": x_axis, "y_axis": y_axis, "focal_distance": 2.0}),
        serde_json::json!({"kind": "hyperbola", "center": center, "x_axis": x_axis, "y_axis": y_axis, "major_radius": 3.0, "minor_radius": 2.0}),
    ];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    for wire in wires {
        let curve: TextCurve2d = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&curve).unwrap(), wire);
        assert!(crate::topology_transfer::pcurve_geometry(&ctx, &curve)
            .unwrap()
            .is_some());
    }
}

#[test]
fn native_analytic_surface_fields_remain_checked_and_wire_identical() {
    let point = serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0});
    let axis = serde_json::json!({"x": 0.0, "y": 0.0, "z": 1.0});
    let reference = serde_json::json!({"x": 1.0, "y": 0.0, "z": 0.0});
    let wires = [
        serde_json::json!({"kind": "plane", "origin": point, "axis": axis, "u_axis": reference, "v_reversed": false}),
        serde_json::json!({"kind": "cylinder", "origin": point, "axis": axis, "ref_direction": reference, "radius": 2.0, "u_reversed": false}),
        serde_json::json!({"kind": "cone", "origin": point, "axis": axis, "ref_direction": reference, "radius": 2.0, "half_angle": -0.5, "u_reversed": false}),
        serde_json::json!({"kind": "sphere", "center": point, "axis": axis, "ref_direction": reference, "radius": -2.0, "u_reversed": false}),
        serde_json::json!({"kind": "torus", "center": point, "axis": axis, "ref_direction": reference, "major_radius": 4.0, "minor_radius": 1.0, "u_reversed": false}),
    ];
    for wire in wires {
        let surface: TextSurface = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&surface).unwrap(), wire);
        if let TextSurface::Plane {
            origin,
            axis,
            u_axis,
            ..
        } = surface
        {
            let _: FinitePoint3 = origin;
            let _: FiniteVector3 = axis;
            let _: FiniteVector3 = u_axis;
        }
    }
}

#[test]
fn transfers_occt_revolution_surface_parameter_order() {
    let surface = crate::brep::TextSurface::Revolution {
        axis_origin: FinitePoint3::ZERO,
        axis_direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        directrix: super::NestedCurve::try_new(crate::brep::TextCurve::Circle {
            center: FinitePoint3::new(cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0)).unwrap(),
            axis: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)).unwrap(),
            ref_direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0))
                .unwrap(),
            radius: FiniteReal::ONE,
        })
        .expect("one inline directrix is admitted"),
    };
    let association = cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new("fcstd:native:object#Surface")
            .expect("nonempty source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let mut curves = crate::brep::CurveTransfer::default();
    let mut surfaces = crate::brep::SurfaceTransfer::default();
    in_decode_context(|ctx| {
        crate::brep::append_text_surface(
            ctx,
            &surface,
            cadmpeg_ir::ids::SurfaceId::mint("fcstd:model:surface#revolution")
                .expect("identity grammar"),
            &association,
            &mut curves,
            &mut surfaces,
        )
    })
    .unwrap();
    assert!(match surfaces.procedural[0].1.definition() {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(matched_payload) =>
            matches!((matched_payload.transposed(),), (true,)),
        _ => false,
    });
}

#[test]
fn transfers_a_signed_cone_half_angle_without_moving_the_frame() {
    let surface = crate::brep::TextSurface::Cone {
        origin: FinitePoint3::ZERO,
        axis: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)).unwrap(),
        ref_direction: FiniteVector3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        radius: FiniteReal::new(5.0).unwrap(),
        half_angle: FiniteReal::new(-0.715_584_993_317_674_8).unwrap(),
        u_reversed: false,
    };
    let association = cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new("fcstd:native:object#Surface")
            .expect("nonempty source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let mut curves = crate::brep::CurveTransfer::default();
    let mut surfaces = crate::brep::SurfaceTransfer::default();
    let geometry = in_decode_context(|ctx| {
        crate::brep::append_text_surface(
            ctx,
            &surface,
            cadmpeg_ir::ids::SurfaceId::mint("fcstd:model:surface#cone").expect("identity grammar"),
            &association,
            &mut curves,
            &mut surfaces,
        )
    })
    .expect("a signed half angle is a b-rep cone the reader admits");
    assert!(matches!(
        geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone))
        if {
            let axis = *cone.frame().axis().as_raw();
            cone.half_angle().get() == -0.715_584_993_317_674_8
                && cone.radius().get() == 5.0
                && (axis.x, axis.y, axis.z) == (0.0, 0.0, -1.0)
        }
    ));
}

#[test]
fn numerical_seventh_periodic_knots_keep_finite_exterior_knots() {
    in_decode_context(|ctx| {
        let (knots, padding) = super::normalize_periodic_knots(
            ctx,
            vec![-1e308, -9e307, 9e307, 1e308]
                .into_iter()
                .map(|value| FiniteReal::new(value).unwrap())
                .collect(),
            1,
            true,
        )
        .unwrap();
        assert_eq!(padding, 1);
        assert!((knots[0].get() / 1e308 + 1.1).abs() <= 4.0 * f64::EPSILON);
        assert!((knots[5].get() / 1e308 - 1.1).abs() <= 4.0 * f64::EPSILON);
        assert!(super::normalize_periodic_knots(
            ctx,
            vec![-1e308, 0.0, 1e308]
                .into_iter()
                .map(|value| FiniteReal::new(value).unwrap())
                .collect(),
            1,
            true
        )
        .is_err());
    });
}
