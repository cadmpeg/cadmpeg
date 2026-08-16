// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn occurrence_keys_canonicalize_equivalent_transform_roundoff() {
    let mut positive = Transform::identity();
    positive.rows[0][3] = 0.5e-12;
    let mut negative = Transform::identity();
    negative.rows[0][3] = -0.5e-12;

    assert_eq!(
        OccurrenceKey::new(7, positive),
        OccurrenceKey::new(7, negative)
    );
    assert_eq!(
        OccurrenceKey::new(7, positive).0,
        occurrence_label(7, positive)
    );
    assert_ne!(
        SourceOccurrenceKey::new(7, positive),
        SourceOccurrenceKey::new(7, negative)
    );

    let mut composed = Transform::identity();
    composed.rows[0][3] = 258.75;
    composed.rows[2][3] = -1.4e-14;
    let mut direct = Transform::identity();
    direct.rows[0][3] = 258.75;

    assert_eq!(
        OccurrenceKey::new(14, composed),
        OccurrenceKey::new(14, direct)
    );
    assert_ne!(
        SourceOccurrenceKey::new(14, composed),
        SourceOccurrenceKey::new(14, direct)
    );

    direct.rows[2][3] = 1.0e-8;
    assert_ne!(
        OccurrenceKey::new(14, composed),
        OccurrenceKey::new(14, direct)
    );
}

#[test]
fn source_indices_span_root_order_and_deduplicate_repeated_placements() {
    let mut translated = Transform::identity();
    translated.rows[0][3] = 10.0;
    let locations = [TextLocation {
        factors: Vec::new(),
        transform: translated,
    }];
    let tshapes = [TextTShape {
        index: 1,
        kind: TextShapeKind::Edge,
        geometry: TextTShapeGeometry::Empty,
        flags: [false; 7],
        children: Vec::new(),
    }];
    let roots = [
        TextShapeUse {
            shape: 1,
            orientation: TextOrientation::Forward,
            location: 0,
        },
        TextShapeUse {
            shape: 1,
            orientation: TextOrientation::Reversed,
            location: 0,
        },
        TextShapeUse {
            shape: 1,
            orientation: TextOrientation::Forward,
            location: 1,
        },
    ];
    let tables = Tables {
        locations: &locations,
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &tshapes,
        triangulations: &[],
        roots: &roots,
    };

    let indices = source_topology_indices(tables);

    assert_eq!(
        indices.get(&(
            TextShapeKind::Edge,
            SourceOccurrenceKey::new(1, Transform::identity()),
        )),
        Some(&1)
    );
    assert_eq!(
        indices.get(&(TextShapeKind::Edge, SourceOccurrenceKey::new(1, translated),)),
        Some(&2)
    );
}

#[test]
fn source_indices_follow_depth_first_topology_order() {
    let use_shape = |shape: usize| TextShapeUse {
        shape,
        orientation: TextOrientation::Forward,
        location: 0,
    };
    let empty = |index: usize, kind: TextShapeKind, children: Vec<usize>| TextTShape {
        index,
        kind,
        geometry: TextTShapeGeometry::Empty,
        flags: [false; 7],
        children: children.into_iter().map(use_shape).collect(),
    };
    let tshapes = vec![
        empty(1, TextShapeKind::Compound, vec![2, 3]),
        empty(2, TextShapeKind::Solid, vec![4]),
        empty(3, TextShapeKind::Solid, vec![5]),
        empty(4, TextShapeKind::Shell, vec![6]),
        empty(5, TextShapeKind::Shell, vec![7]),
        empty(6, TextShapeKind::Face, vec![8]),
        empty(7, TextShapeKind::Face, vec![9]),
        empty(8, TextShapeKind::Wire, vec![10]),
        empty(9, TextShapeKind::Wire, vec![11]),
        empty(10, TextShapeKind::Edge, vec![12, 13]),
        empty(11, TextShapeKind::Edge, vec![14, 15]),
        empty(12, TextShapeKind::Vertex, Vec::new()),
        empty(13, TextShapeKind::Vertex, Vec::new()),
        empty(14, TextShapeKind::Vertex, Vec::new()),
        empty(15, TextShapeKind::Vertex, Vec::new()),
    ];
    let roots = [use_shape(1)];
    let tables = Tables {
        locations: &[],
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &tshapes,
        triangulations: &[],
        roots: &roots,
    };
    let indices = source_topology_indices(tables);
    let index =
        |kind, shape| indices.get(&(kind, SourceOccurrenceKey::new(shape, Transform::identity())));

    assert_eq!(index(TextShapeKind::Compound, 1), Some(&1));
    assert_eq!(index(TextShapeKind::Solid, 2), Some(&1));
    assert_eq!(index(TextShapeKind::Solid, 3), Some(&2));
    assert_eq!(index(TextShapeKind::Shell, 4), Some(&1));
    assert_eq!(index(TextShapeKind::Shell, 5), Some(&2));
    assert_eq!(index(TextShapeKind::Face, 6), Some(&1));
    assert_eq!(index(TextShapeKind::Face, 7), Some(&2));
    assert_eq!(index(TextShapeKind::Wire, 8), Some(&1));
    assert_eq!(index(TextShapeKind::Wire, 9), Some(&2));
    assert_eq!(index(TextShapeKind::Edge, 10), Some(&1));
    assert_eq!(index(TextShapeKind::Edge, 11), Some(&2));
    assert_eq!(index(TextShapeKind::Vertex, 12), Some(&1));
    assert_eq!(index(TextShapeKind::Vertex, 13), Some(&2));
    assert_eq!(index(TextShapeKind::Vertex, 14), Some(&3));
    assert_eq!(index(TextShapeKind::Vertex, 15), Some(&4));
}

#[test]
fn endpoint_selection_requires_unique_oriented_direct_children() {
    let children = [
        TextShapeUse {
            shape: 1,
            orientation: TextOrientation::Forward,
            location: 0,
        },
        TextShapeUse {
            shape: 2,
            orientation: TextOrientation::Internal,
            location: 0,
        },
        TextShapeUse {
            shape: 4,
            orientation: TextOrientation::Reversed,
            location: 0,
        },
    ];
    let (start, end) = edge_endpoint_uses(9, &children).expect("endpoint uses");
    assert_eq!(start.shape, 1);
    assert_eq!(end.shape, 4);

    let closed = [
        TextShapeUse {
            shape: 7,
            orientation: TextOrientation::Forward,
            location: 0,
        },
        TextShapeUse {
            shape: 7,
            orientation: TextOrientation::Reversed,
            location: 0,
        },
    ];
    let (start, end) = edge_endpoint_uses(9, &closed).expect("closed edge endpoints");
    assert_eq!(start.shape, end.shape);

    assert!(matches!(
        edge_endpoint_uses(9, &children[..2]),
        Err(CodecError::Malformed(_))
    ));
    let duplicate_forward = [
        children[0].clone(),
        TextShapeUse {
            shape: 3,
            orientation: TextOrientation::Forward,
            location: 0,
        },
        children[2].clone(),
    ];
    assert!(matches!(
        edge_endpoint_uses(9, &duplicate_forward),
        Err(CodecError::Malformed(_))
    ));
    let duplicate_reversed = [
        children[0].clone(),
        children[2].clone(),
        TextShapeUse {
            shape: 5,
            orientation: TextOrientation::Reversed,
            location: 0,
        },
    ];
    assert!(matches!(
        edge_endpoint_uses(9, &duplicate_reversed),
        Err(CodecError::Malformed(_))
    ));
}

#[test]
fn edge_representation_selection_follows_family_rules() {
    let representation = |kind, primary| TextEdgeRepresentation {
        kind,
        primary,
        secondary: None,
        surface: None,
        second_surface: None,
        location: 0,
        second_location: None,
        parameter_range: None,
        continuity: None,
        uv_endpoints: None,
    };
    let curves = [
        TextCurve::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        TextCurve::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        TextCurve::Line {
            origin: Point3::new(0.0, 1.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
    ];
    let tables = Tables {
        locations: &[],
        curve2ds: &[],
        curves: &curves,
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &[],
        triangulations: &[],
        roots: &[],
    };
    let equivalent_exact = [representation(1, 1), representation(1, 2)];
    let selected = select_exact_curve_representation(7, &equivalent_exact, &tables)
        .expect("equivalent exact curves")
        .expect("exact curve");
    assert_eq!(selected.0, 0);

    let distinct_exact = [representation(1, 1), representation(1, 3)];
    assert!(matches!(
        select_exact_curve_representation(7, &distinct_exact, &tables),
        Err(CodecError::Malformed(_))
    ));

    let fallback = [representation(5, 1), representation(6, 1)];
    assert!(matches!(
        unique_fallback_polygon_representation(7, &fallback),
        Err(CodecError::Malformed(_))
    ));

    let matching_pcurves = [representation(2, 1), representation(2, 1)];
    let selected = first_edge_representation(&matching_pcurves, |candidate| candidate.kind == 2)
        .expect("first matching pcurve");
    assert_eq!(selected.0, 0);

    let exact_precedes_polygon = [representation(5, 1), representation(1, 1)];
    let selected = select_exact_curve_representation(7, &exact_precedes_polygon, &tables)
        .expect("exact curve after polygon")
        .expect("exact curve");
    assert_eq!(selected.0, 1);
}

#[test]
fn non_manifold_incidence_does_not_invent_a_radial_order() {
    let edge = EdgeId("edge".into());
    let mut coedges = (0..3)
        .map(|index| {
            let id = CoedgeId(format!("coedge-{index}"));
            Coedge {
                id: id.clone(),
                owner_loop: LoopId(format!("loop-{index}")),
                edge: edge.clone(),
                next: id.clone(),
                previous: id.clone(),
                radial_next: id,
                sense: Sense::Forward,
                use_curve: None,
                use_curve_parameter_range: None,
                pcurves: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    close_radial_rings(&mut coedges);
    assert!(coedges.iter().all(|coedge| coedge.radial_next == coedge.id));

    let mut four = (0..4)
        .map(|index| {
            let id = CoedgeId(format!("coedge-four-{index}"));
            Coedge {
                id: id.clone(),
                owner_loop: LoopId(format!("loop-four-{index}")),
                edge: edge.clone(),
                next: id.clone(),
                previous: id.clone(),
                radial_next: id,
                sense: Sense::Forward,
                use_curve: None,
                use_curve_parameter_range: None,
                pcurves: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    let original_ids = four
        .iter()
        .map(|coedge| coedge.radial_next.clone())
        .collect::<Vec<_>>();
    close_radial_rings(&mut four);
    assert_eq!(
        four.iter()
            .map(|coedge| &coedge.radial_next)
            .collect::<Vec<_>>(),
        original_ids.iter().collect::<Vec<_>>()
    );

    let id = CoedgeId("coedge-single".into());
    let mut singleton = vec![Coedge {
        id: id.clone(),
        owner_loop: LoopId("loop-single".into()),
        edge,
        next: id.clone(),
        previous: id.clone(),
        radial_next: id.clone(),
        sense: Sense::Forward,
        use_curve: None,
        use_curve_parameter_range: None,
        pcurves: Vec::new(),
    }];
    close_radial_rings(&mut singleton);
    assert_eq!(singleton[0].radial_next, id);
}

#[test]
fn occt_parabola_ranges_convert_to_step_parameters() {
    let geometry = CurveGeometry::Parabola {
        vertex: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        focal_distance: 4.0,
    };
    assert_eq!(
        normalize_occt_curve_range(&geometry, Some([-2.0, 4.0])),
        Some([-0.25, 0.5])
    );
    assert_eq!(normalize_occt_curve_range(&geometry, None), None);
}

#[test]
fn periodic_ranges_wrap_the_start_and_preserve_the_sweep() {
    let geometry = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    let [start, end] =
        normalize_occt_curve_range(&geometry, Some([-1.0e-15, std::f64::consts::FRAC_PI_2]))
            .expect("periodic range");
    assert_eq!(start, 0.0);
    assert!((end - start - (std::f64::consts::FRAC_PI_2 + 1.0e-15)).abs() < 1.0e-15);
}

#[test]
fn collapsed_pcurve_ranges_are_unbounded() {
    assert_eq!(bounded_pcurve_range(false, Some([2.0, 2.0])), None);
    assert_eq!(bounded_pcurve_range(true, Some([1.0, 3.0])), None);
    assert_eq!(
        bounded_pcurve_range(false, Some([1.0, 3.0])),
        Some([1.0, 3.0])
    );
}

#[test]
fn adjacent_pcurve_domain_rounding_is_canonicalized() {
    let geometry = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![2.0, 2.0, 4.0, 4.0],
        control_points: vec![
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };

    assert_eq!(
        normalize_pcurve_parameter_range(&geometry, Some([2.0 - 1.0e-11, 4.0 + 1.0e-11])),
        Some([2.0, 4.0])
    );
    assert_eq!(
        normalize_pcurve_parameter_range(&geometry, Some([1.0, 5.0])),
        Some([1.0, 5.0])
    );
}

#[test]
fn face_connectivity_partitions_transitively_without_reordering() {
    let sets = [
        HashSet::from(["edge-a".to_owned()]),
        HashSet::from(["edge-b".to_owned()]),
        HashSet::from(["edge-a".to_owned(), "edge-c".to_owned()]),
        HashSet::from(["edge-c".to_owned()]),
        HashSet::new(),
    ];

    assert_eq!(
        connected_components(&sets),
        vec![vec![0, 2, 3], vec![1], vec![4]]
    );
    assert!(connected_components(&[]).is_empty());
}

#[test]
fn indirect_analytic_frames_reverse_the_pcurve_u_parameter() {
    let surface = TextSurface::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
        u_reversed: true,
    };
    let affine = surface_parameter_affine(&surface);
    assert_eq!(affine.u_scale, -1.0);
    assert_eq!(affine.v_scale, 1.0);

    let cone = TextSurface::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
        half_angle: std::f64::consts::FRAC_PI_3,
        u_reversed: true,
    };
    let affine = surface_parameter_affine(&cone);
    assert_eq!(affine.u_scale, -1.0);
    assert!((affine.v_scale - 0.5).abs() < 1.0e-15);

    let trimmed = TextSurface::Trimmed {
        parameter_ranges: [[2.0, 3.0], [4.0, 8.0]],
        basis: Box::new(cone),
    };
    let affine = surface_parameter_affine(&trimmed);
    assert_eq!(affine.u_scale, 1.0);
    assert_eq!(affine.u_offset, -2.0);
    assert!((affine.v_scale - 0.5).abs() < 1.0e-15);
    assert!((affine.v_offset + 2.0).abs() < 1.0e-15);
}

#[test]
pub(crate) fn transfers_connected_text_brep_topology() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Shape" expanded="1"><Properties Count="8">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3368601600"/></Property>
<Property name="ShapeAppearance" type="App::PropertyMaterialList"><MaterialList file="ShapeAppearance" version="3"/></Property>
<Property name="LineColor" type="App::PropertyColor"><PropertyColor value="4278190335"/></Property>
<Property name="LineWidth" type="App::PropertyFloatConstraint"><Float value="2.5"/></Property>
<Property name="PointColor" type="App::PropertyColor"><PropertyColor value="16711935"/></Property>
<Property name="PointSize" type="App::PropertyFloatConstraint"><Float value="4"/></Property>
<Property name="Transparency" type="App::PropertyPercent"><Integer value="25"/></Property>
<Property name="Visibility" type="App::PropertyBool"><Bool value="false"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 3
1 0 0 1 0
1 1 0 -1 0
1 0 0 1 0
Curves 2
1 0 0 0 1 0 0
1 1 0 0 -1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 2 3 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
    let mut shape_appearance = Vec::new();
    shape_appearance.extend_from_slice(&1_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0x3333_33ff_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0x3366_99ff_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0x1111_11ff_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0x0000_00ff_u32.to_le_bytes());
    shape_appearance.extend_from_slice(&0.75_f32.to_le_bytes());
    shape_appearance.extend_from_slice(&0.25_f32.to_le_bytes());
    for _ in 0..3 {
        shape_appearance.extend_from_slice(&0_u32.to_le_bytes());
    }
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("ShapeAppearance", &shape_appearance),
        ("Shape.brp", brep),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("connected topology");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 2);
    assert_eq!(result.ir().model.edges.len(), 2);
    assert_eq!(result.ir().model.vertices.len(), 2);
    assert_eq!(result.ir().model.pcurves.len(), 2);
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .any(|coedge| { coedge.pcurves[0].pcurve.0.ends_with("3%3A2%3A1") }));
    assert_eq!(result.ir().model.appearances.len(), 3);
    assert_eq!(result.ir().model.appearance_bindings.len(), 5);
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(
                binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
            ))
            .count(),
        2
    );
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(
                binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Vertex(_)
            ))
            .count(),
        2
    );
    assert_eq!(
        result
            .ir()
            .model
            .appearances
            .iter()
            .find(|appearance| appearance.schema.as_deref()
                == Some("FCStd ViewProvider line style"))
            .and_then(|appearance| appearance.properties.get("line_width")),
        Some(&2.5)
    );
    assert_eq!(
        result
            .ir()
            .model
            .appearances
            .iter()
            .find(
                |appearance| appearance.schema.as_deref() == Some("FCStd ViewProvider point style")
            )
            .and_then(|appearance| appearance.properties.get("point_size")),
        Some(&4.0)
    );
    assert_eq!(result.ir().model.bodies[0].visible, Some(false));
    assert_eq!(result.ir().model.presentation_documents.len(), 1);
    assert_eq!(result.ir().model.view_presentations.len(), 1);
    let view = &result.ir().model.view_presentations[0];
    assert!(view
        .object
        .as_deref()
        .is_some_and(|id| id.ends_with("Shape")));
    assert_eq!(view.order, 0);
    assert_eq!(view.expanded, Some(true));
    assert_eq!(view.visible, Some(false));
    assert_eq!(view.line_width, Some(2.5));
    assert_eq!(view.point_size, Some(4.0));
    let color = result.ir().model.bodies[0].color.expect("shape color");
    assert!((color.r - 0x33 as f32 / 255.0).abs() < 1e-6);
    assert!((color.g - 0x66 as f32 / 255.0).abs() < 1e-6);
    assert!((color.b - 0x99 as f32 / 255.0).abs() < 1e-6);
    assert!((color.a - 0.75).abs() < 1e-6);
    let shape_material = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.schema.as_deref() == Some("FCStd ShapeAppearance"))
        .expect("shape material");
    assert_eq!(shape_material.properties.get("shininess"), Some(&0.75));
    assert_eq!(shape_material.properties.get("transparency"), Some(&0.25));
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    assert_eq!(namespace.version, 22);
    let census = namespace
        .arena_as::<crate::native::CarrierCensusRecord>("carrier_census")
        .expect("carrier census");
    assert_eq!(census.len(), 1);
    assert_eq!(census[0].topology_version, 1);
    assert_eq!(census[0].curves_2d["line"], 3);
    assert_eq!(census[0].curves_3d["line"], 2);
    assert_eq!(census[0].surfaces["plane"], 1);
    assert_eq!(census[0].topology["edge"], 2);
    assert_eq!(census[0].topology["vertex"], 2);
    let gui_providers = namespace
        .arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")
        .expect("GUI providers");
    let gui_properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(gui_providers.len(), 1);
    assert_eq!(
        gui_providers[0].object.as_deref(),
        Some("fcstd:native:object#Shape")
    );
    assert_eq!(gui_properties.len(), 8);
    assert!(gui_properties
        .iter()
        .all(|property| property.raw_xml.starts_with("<Property")));
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    corrupted.model.view_presentations[0].line_width = Some(f64::NAN);
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid view presentation reference, order, or size"));
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()));
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::Severity::Error),
        "{:#?}",
        report.findings
    );
}

#[test]
pub(crate) fn transfers_triangulation_only_face_and_indexed_edge_polygon() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="MeshShape" id="1"/></Objects>
<ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade
Locations 1
1 1 0 0 10 0 1 0 0 0 0 1 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 1
2 1 2 p 0.01 1 0 1
Surfaces 0
Triangulations 1
3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3
TShapes 7
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 6 1 1 0 0 1001000 +7 0 -6 0 *
Wi 1001000 +5 0 *
Fa 0 0.001 0 1 2 1 1001000 +4 0 *
Sh 1001000 +3 0 *
So 1001000 +2 0 *
+1 0 *";
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("Shape.brp", brep),
            ])),
            &DecodeOptions::default(),
        )
        .expect("triangulation-only topology");
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].vertices[0].x, 0.0);
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Polygonal {
            chordal_deflection: 0.02,
            ..
        }
    ));
    assert!(matches!(
        result.ir().model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Polyline {
            chordal_deflection: 0.01,
            ..
        }
    ));
    assert_eq!(result.ir().model.edges[0].param_range, Some([0.0, 1.0]));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(
        validation.findings.iter().all(|finding| {
            finding.severity < cadmpeg_ir::Severity::Error
                || finding.check == cadmpeg_ir::Check::Identity
        }),
        "{:#?}",
        validation.findings
    );
}

#[test]
pub(crate) fn binds_both_seam_pcurves_and_closes_the_radial_pair() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 2
1 0 0 0 1
1 6.283185307179586 0 0 1
Curves 1
1 1 0 0 0 0 1
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
2 0 0 0 0 0 1 1 0 0 0 1 0 1
Triangulations 0
TShapes 8
Ve 0.001 1 0 0 0 0 1001000 *
Ve 0.001 1 0 1 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 3 1 2 C0 1 0 0 1 0 1001000 +8 0 -7 0 *
Wi 1001000 +6 0 -6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("cylindrical seam");
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 2);
    let first = &result.ir().model.coedges[0];
    let second = &result.ir().model.coedges[1];
    assert_eq!(first.radial_next, second.id);
    assert_eq!(second.radial_next, first.id);
    assert_ne!(first.pcurves, second.pcurves);
    assert!(!first.pcurves.is_empty() && !second.pcurves.is_empty());
    let errors = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .findings
        .into_iter()
        .filter(|finding| finding.severity == cadmpeg_ir::Severity::Error)
        .filter(|finding| finding.check != cadmpeg_ir::Check::Identity)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn preserves_a_free_edge_as_a_wire_body() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 3
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +3 0 -2 0 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("free edge");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.shells[0].wire_edges.len(), 1);
    assert!(result.ir().model.shells[0].faces.is_empty());
}

#[test]
fn accepts_equivalent_repeated_exact_curve_records() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 2
1 0 0 0 1 0 0
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 3
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 1 2 0 0 1 0 1001000 +3 0 -2 0 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("equivalent repeated exact curve");
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.curves.len(), 2);
    assert!(result.ir().model.edges[0]
        .curve
        .as_ref()
        .is_some_and(|curve| curve.0.ends_with(":1")));
}

#[test]
fn repeated_shape_roots_have_distinct_occurrence_identity() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape">
<Part ElementMap="1.0" file="Shape.brp"/>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="4">
1 PostfixCount 0 MapCount 1
ElementMap 1 1 2
Edge ChildCount 0 NameCount 3
0
;EdgeStable.0.a 0
;DeletedEdgeStable.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 3
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +3 0 -2 0 *
+1 0 +1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("repeated roots");

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.edges.len(), 2);
    assert_eq!(result.ir().model.vertices.len(), 4);
    assert_ne!(
        result.ir().model.bodies[0].id,
        result.ir().model.bodies[1].id
    );
    let maps = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ElementMapRecord>("element_maps")
        .expect("element maps");
    let groups = &maps[0].maps[0].groups;
    assert_eq!(groups[0].names[1][0].topology_ids.len(), 2);
    assert_eq!(groups[1].names[1][0].topology_ids.len(), 2);
    assert_eq!(groups[1].names[2][0].topology_ids.len(), 2);
    assert_valid_document(result.ir());
}

#[test]
fn preserves_an_unbounded_edge_as_a_free_exact_curve() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::Line" name="Axis" id="1"/></Objects>
<ObjectData Count="1"><Object name="Axis"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Axis.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 0 0 1
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 1
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Axis.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("unbounded datum axis");
    assert!(result.ir().model.bodies.is_empty());
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.ir().model.curves[0].source_object.is_some());
    assert_valid_document(result.ir());
}

#[test]
pub(crate) fn preserves_compound_ownership_and_composes_nested_mirrored_locations_once() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 3
1 1 0 0 10 0 1 0 0 0 0 1 0
1 -2 0 0 0 0 2 0 5 0 0 2 0
1 1 0 0 20 0 1 0 0 0 0 1 0
Curve2ds 2
1 0 0 1 0
1 1 0 -1 0
Curves 2
1 0 0 0 1 0 0
1 1 0 0 -1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 2 *
So 1001000 +3 0 *
Co 1001000 +2 1 +2 3 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("located topology");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(result.ir().model.bodies[0].regions.len(), 2);
    assert!(result.ir().model.bodies[0].transform.is_none());
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(result.ir().model.vertices.len(), 4);
    let mut positions = result
        .ir()
        .model
        .edges
        .iter()
        .flat_map(|edge| [&edge.start, &edge.end])
        .map(|vertex| {
            let vertex = result
                .ir()
                .model
                .vertices
                .iter()
                .find(|candidate| &candidate.id == vertex)
                .expect("required invariant");
            result
                .ir()
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .expect("required invariant")
                .position
        })
        .collect::<Vec<_>>();
    positions.sort_by(|left, right| left.x.total_cmp(&right.x));
    positions.dedup();
    assert_eq!(positions.len(), 4);
    assert_eq!([positions[0].x, positions[0].y], [8.0, 5.0]);
    assert_eq!([positions[1].x, positions[1].y], [10.0, 5.0]);
    assert_eq!([positions[2].x, positions[2].y], [18.0, 5.0]);
    assert_eq!([positions[3].x, positions[3].y], [20.0, 5.0]);
    let face = &result.ir().model.faces[0];
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .expect("required invariant");
    let cadmpeg_ir::geometry::SurfaceGeometry::Transformed { basis, transform } = &surface.geometry
    else {
        panic!("located face must retain its exact transformed basis");
    };
    assert!(matches!(
        basis.as_ref(),
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
    ));
    assert_eq!(transform.rows[0][0], -2.0);
    assert_eq!(transform.rows[1][1], 2.0);
    let origin =
        cadmpeg_ir::eval::surface_point(&surface.geometry, 0.0, 0.0).expect("required invariant");
    assert_eq!([origin.x, origin.y], [10.0, 5.0]);
    for edge in &result.ir().model.edges {
        let curve = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve.as_ref())
            .expect("required invariant");
        let range = edge.param_range.expect("located edge parameter range");
        let start =
            cadmpeg_ir::eval::curve_point(&curve.geometry, range[0]).expect("required invariant");
        let end =
            cadmpeg_ir::eval::curve_point(&curve.geometry, range[1]).expect("required invariant");
        assert_eq!((start.x - end.x).abs(), 2.0);
    }
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::Severity::Error
                || finding.check == cadmpeg_ir::Check::Identity),
        "{:#?}",
        report.findings
    );
}
