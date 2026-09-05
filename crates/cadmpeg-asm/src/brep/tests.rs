// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::default_trait_access)]
//! Unit tests for B-rep topology decode, geometry recognition, and
//! procedural carrier classification.
use super::geometry::{
    analytic_procedural_surface, edge_pcurve_parameter_ranges, is_asm_stream_delimiter,
    is_known_record_head, pcurve_ranges_on_domain, point_vector, rational_four_arc_circle,
};
use super::topology::{shell_faces, shell_wire_roots, subshell_ancestor_shells};
use super::*;
use crate::kernel_header::RefWidth;
use crate::nurbs;
use crate::sab::{Record, Token};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::{EdgeId, FaceId, LoopId, RegionId, ShellId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Loop, Shell};
use std::collections::{HashMap, HashSet};

const FORMAT: IdFormat<'static> = IdFormat("f3d");

fn exact_circle_directrix() -> cadmpeg_ir::geometry::NurbsCurve {
    let center = Point3::new(2.0, 3.0, 4.0);
    let point = |x, y| Point3::new(center.x + x, center.y + y, center.z);
    cadmpeg_ir::geometry::NurbsCurve::new(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
        vec![
            point(5.0, 0.0),
            point(5.0, 5.0),
            point(0.0, 5.0),
            point(-5.0, 5.0),
            point(-5.0, 0.0),
            point(-5.0, -5.0),
            point(0.0, -5.0),
            point(5.0, -5.0),
            point(5.0, 0.0),
        ],
        Some(vec![
            1.0,
            std::f64::consts::FRAC_1_SQRT_2,
            1.0,
            std::f64::consts::FRAC_1_SQRT_2,
            1.0,
            std::f64::consts::FRAC_1_SQRT_2,
            1.0,
            std::f64::consts::FRAC_1_SQRT_2,
            1.0,
        ]),
        false,
    )
    .unwrap()
}

#[test]
fn exact_circle_extrusion_reduces_to_cylinder_only_along_normal() {
    let definition =
        |direction| nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Extrusion {
            directrix: exact_circle_directrix(),
            parameter_interval: [0.0, 4.0],
            direction,
            native_position: Point3::new(0.0, 0.0, 0.0),
            revision_form: None,
        };
    let Some(SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    }) = analytic_procedural_surface(&definition(Vector3::new(0.0, 0.0, -8.0)))
    else {
        panic!("exact circle extrusion did not reduce")
    };
    assert!(point_vector(Point3::new(2.0, 3.0, 4.0), origin).norm() < 1.0e-12);
    assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
    assert!((ref_direction.x - 1.0).abs() < 1.0e-12);
    assert!(ref_direction.y.abs() < 1.0e-12);
    assert!(ref_direction.z.abs() < 1.0e-12);
    assert!((radius - 5.0).abs() < 1.0e-12);
    assert!(analytic_procedural_surface(&definition(Vector3::new(1.0, 0.0, 8.0))).is_none());
    let mut approximate = exact_circle_directrix();
    approximate.control_points_mut()[3].x += 1.0e-5;
    assert!(rational_four_arc_circle(&approximate).is_none());
}

fn degree_elevated_circle() -> cadmpeg_ir::geometry::NurbsCurve {
    let quadratic = exact_circle_directrix();
    let weights = quadratic.weights().unwrap();
    let homogeneous = |index: usize| {
        let point = quadratic.control_points()[index];
        let weight = weights[index] * 7.0;
        [point.x * weight, point.y * weight, point.z * weight, weight]
    };
    let combine = |first: [f64; 4], first_scale: f64, second: [f64; 4], second_scale: f64| {
        std::array::from_fn(|coordinate| {
            first_scale * first[coordinate] + second_scale * second[coordinate]
        })
    };
    let mut elevated = Vec::new();
    for span in 0..4 {
        let [first, middle, last] = [
            homogeneous(span * 2),
            homogeneous(span * 2 + 1),
            homogeneous(span * 2 + 2),
        ];
        let span = [
            first,
            combine(first, 1.0 / 3.0, middle, 2.0 / 3.0),
            combine(middle, 2.0 / 3.0, last, 1.0 / 3.0),
            last,
        ];
        elevated.extend_from_slice(if elevated.is_empty() {
            &span
        } else {
            &span[1..]
        });
    }
    let (control_points, weights): (Vec<_>, Vec<_>) = elevated
        .into_iter()
        .map(|point| {
            (
                Point3::new(
                    point[0] / point[3],
                    point[1] / point[3],
                    point[2] / point[3],
                ),
                point[3],
            )
        })
        .unzip();
    cadmpeg_ir::geometry::NurbsCurve::new(
        3,
        vec![
            0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 4.0, 4.0, 4.0, 4.0,
        ],
        control_points,
        Some(weights),
        false,
    )
    .unwrap()
}

#[test]
fn exact_circle_recognition_is_projective_and_degree_invariant() {
    let mut scaled = exact_circle_directrix();
    for weight in scaled.weights_mut().unwrap() {
        *weight *= 7.0;
    }
    assert!(rational_four_arc_circle(&scaled).is_some());

    let mut elevated = degree_elevated_circle();
    assert!(rational_four_arc_circle(&elevated).is_some());
    assert!(matches!(
        analytic_procedural_surface(
            &nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Extrusion {
                directrix: elevated.clone(),
                parameter_interval: [0.0, 4.0],
                direction: Vector3::new(0.0, 0.0, 3.0),
                native_position: Point3::new(0.0, 0.0, 0.0),
                revision_form: None,
            }
        ),
        Some(SurfaceGeometry::Cylinder { .. })
    ));
    elevated.control_points_mut()[5].x += 1.0e-5;
    assert!(rational_four_arc_circle(&elevated).is_none());
}

fn plane(origin: Point3, normal: Vector3, u_axis: Vector3) -> SurfaceGeometry {
    SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    }
}

fn cylinder(origin: Point3, axis: Vector3, radius: f64) -> SurfaceGeometry {
    SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    }
}

fn linear_spine(points: Vec<Point3>) -> cadmpeg_ir::geometry::NurbsCurve {
    cadmpeg_ir::geometry::NurbsCurve::new(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        points,
        None,
        false,
    )
    .unwrap()
}

#[test]
fn constant_circular_plane_plane_blend_reduces_to_tangent_cylinder() {
    let mut definition = nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Blend {
        supports: Box::new([
            Some(plane(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            )),
            Some(plane(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )),
        ]),
        spine: Some(linear_spine(vec![
            Point3::new(2.0, 2.0, -4.0),
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(2.0, 2.0, 7.0),
        ])),
        radius: cadmpeg_ir::geometry::BlendRadiusLaw::Constant {
            signed_radius: -2.0,
        },
        cross_section: cadmpeg_ir::geometry::BlendCrossSection::Circular,
        native: None,
    };
    assert!(matches!(
        analytic_procedural_surface(&definition),
        Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        }) if origin == Point3::new(2.0, 2.0, -4.0)
            && axis == Vector3::new(0.0, 0.0, 1.0)
            && radius == 2.0
    ));

    let nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Blend {
        spine: Some(spine), ..
    } = &mut definition
    else {
        unreachable!()
    };
    spine.control_points_mut()[1].x = 2.1;
    assert!(analytic_procedural_surface(&definition).is_none());
}

#[test]
fn constant_circular_plane_cylinder_blend_reduces_to_tangent_torus() {
    let mut circle = exact_circle_directrix();
    for point in circle.control_points_mut() {
        point.x -= 2.0;
        point.y -= 3.0;
        point.z -= 3.0;
    }
    let mut definition = nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Blend {
        supports: Box::new([
            Some(plane(
                Point3::new(0.0, 0.0, -1.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )),
            Some(cylinder(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                3.0,
            )),
        ]),
        spine: Some(circle),
        radius: cadmpeg_ir::geometry::BlendRadiusLaw::Constant {
            signed_radius: -2.0,
        },
        cross_section: cadmpeg_ir::geometry::BlendCrossSection::Circular,
        native: None,
    };
    assert!(matches!(
        analytic_procedural_surface(&definition),
        Some(SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        }) if center == Point3::new(0.0, 0.0, 1.0)
            && axis == Vector3::new(0.0, 0.0, 1.0)
            && ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 5.0
            && minor_radius == -2.0
    ));

    let nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Blend { supports, .. } =
        &mut definition
    else {
        unreachable!()
    };
    supports[0] = Some(plane(
        Point3::new(0.0, 0.0, -1.0),
        Vector3::new(0.0, 1.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    ));
    assert!(analytic_procedural_surface(&definition).is_none());
}

#[test]
fn normalized_topology_heads_are_not_other_records() {
    for head in ["subshell", "wire", "tcoedge", "tedge", "tvertex"] {
        assert!(is_known_record_head(head), "{head}");
    }
}

#[test]
fn unreferenced_carrier_heads_are_not_application_records() {
    for head in ["spline", "intcurve", "pcurve"] {
        assert!(is_known_record_head(head), "{head}");
    }
    assert!(!is_known_record_head("ATTRIB_CUSTOM"));
}

#[test]
fn asm_stream_delimiters_are_not_application_records() {
    for name in ["Begin-of-ASM-History-Data", "End-of-ASM-data"] {
        assert!(is_asm_stream_delimiter(name));
    }
    assert!(!is_asm_stream_delimiter("ATTRIB_CUSTOM-attrib"));
}

#[test]
fn saved_top_level_edge_projects_as_a_wire_body() {
    let record = |index, name: &str, head: &str, tokens: Vec<Token>| Record {
        index,
        name: name.into(),
        head: head.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    };
    let records = vec![
        record(0, "asmheader", "asmheader", Vec::new()),
        record(
            1,
            "edge",
            "edge",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(2),
                Token::Double(0.0),
                Token::Ref(3),
                Token::Double(1.0),
                Token::Ref(-1),
                Token::Ref(6),
                Token::False,
            ],
        ),
        record(
            2,
            "vertex",
            "vertex",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(1),
                Token::Long(0),
                Token::Ref(4),
            ],
        ),
        record(
            3,
            "vertex",
            "vertex",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(1),
                Token::Long(1),
                Token::Ref(5),
            ],
        ),
        record(
            4,
            "point",
            "point",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position([0.0, 0.0, 0.0]),
            ],
        ),
        record(
            5,
            "point",
            "point",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position([1.0, 0.0, 0.0]),
            ],
        ),
        record(
            6,
            "straight-curve",
            "straight",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Position([0.0, 0.0, 0.0]),
                Token::Vector3([1.0, 0.0, 0.0]),
            ],
        ),
    ];
    let mut bytes = Vec::from(&b"ASM BinaryFile4"[..]);
    bytes.extend_from_slice(&22500u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let brep = decode_with_purpose(
        &records,
        &bytes,
        "BREP.saved-edge.smbh",
        FORMAT,
        DecodePurpose::Model,
    );

    assert_eq!(brep.bodies.len(), 1);
    assert_eq!(brep.bodies[0].kind, cadmpeg_ir::topology::BodyKind::Wire);
    assert_eq!(brep.regions.len(), 1);
    assert_eq!(brep.shells.len(), 1);
    assert_eq!(
        brep.shells[0].wire_edges,
        vec![EdgeId::mint(id(FORMAT, 1)).expect("identity grammar")]
    );
    assert_eq!(brep.edges.len(), 1);
    assert_eq!(brep.vertices.len(), 2);
    assert_eq!(brep.points.len(), 2);
    assert_eq!(brep.curves.len(), 1);
}

#[test]
fn nested_attributes_inherit_their_topology_owner() {
    use cadmpeg_ir::attributes::AttributeTarget;
    use cadmpeg_ir::ids::EdgeId;

    let current_attribute = |index, owner| Record {
        index,
        name: "ATTRIB_CUSTOM-attrib".into(),
        head: "ATTRIB_CUSTOM".into(),
        tokens: vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(owner),
        ]
        .into(),
        offset: 0,
        len: 0,
    };
    let legacy_attribute = |index, owner| Record {
        index,
        name: "ATTRIB_CUSTOM-attrib".into(),
        head: "ATTRIB_CUSTOM".into(),
        tokens: vec![
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(owner),
        ]
        .into(),
        offset: 0,
        len: 0,
    };
    let parent = current_attribute(7, 3);
    let child = legacy_attribute(8, 7);
    let records = HashMap::from([(7, &parent), (8, &child)]);
    let expected = AttributeTarget::Edge(EdgeId::mint("edge").expect("identity grammar"));
    let targets = HashMap::from([(3, expected.clone())]);

    assert_eq!(
        inherited_attribute_target(7, &records, &targets),
        Some(expected.clone())
    );
    assert_eq!(
        inherited_attribute_target(8, &records, &targets),
        Some(expected)
    );

    let cycle_left = current_attribute(9, 10);
    let cycle_right = legacy_attribute(10, 9);
    let cycle = HashMap::from([(9, &cycle_left), (10, &cycle_right)]);
    assert_eq!(inherited_attribute_target(9, &cycle, &targets), None);
}

#[test]
fn standard_attribute_chain_uses_forward_links_and_first_exact_color() {
    use super::attributes::{
        attribute_chain_color_carrier, collect_attributes, DirectColorCarrier,
    };
    use cadmpeg_ir::attributes::AttributeTarget;

    let record = |index, name: &str, next, payload: Vec<Token>| {
        let mut tokens = vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(next),
            Token::Ref(-1),
            Token::Ref(0),
        ];
        tokens.extend(payload);
        Record {
            index,
            name: name.into(),
            head: name.split('-').next().unwrap().into(),
            tokens: tokens.into(),
            offset: 0,
            len: 0,
        }
    };
    let entity = Record {
        index: 0,
        name: "face".into(),
        head: "face".into(),
        tokens: vec![Token::Ref(1)].into(),
        offset: 0,
        len: 0,
    };
    let attributes = [
        record(1, "color-adesk-attrib", 2, vec![Token::Long(5)]),
        record(
            2,
            "material-adesk-attrib",
            3,
            vec![Token::Long(7), Token::Long(11)],
        ),
        record(
            3,
            "truecolor-adesk-attrib",
            4,
            vec![Token::Int64(i64::from(0xc3_40_80_c0_u32))],
        ),
        record(
            4,
            "rgb_color-st-attrib",
            5,
            // A four-channel binary record must carry terminal f64 1.
            vec![
                Token::Double(0.25),
                Token::Double(0.5),
                Token::Double(0.75),
                Token::Double(0.5),
            ],
        ),
        record(
            5,
            "truecolor-adesk-attrib",
            6,
            vec![Token::Int64(i64::from(0xc2_40_80_c0_u32))],
        ),
        record(
            6,
            "entatt_color-bt-attrib",
            -1,
            vec![Token::Str("16711680".into())],
        ),
    ];
    let by_index = attributes
        .iter()
        .map(|attribute| (attribute.index as i64, attribute))
        .collect::<HashMap<_, _>>();

    let (carrier, decoded) =
        attribute_chain_color_carrier(&entity, |index| by_index.get(&index).copied()).unwrap();
    assert_eq!(carrier.index, 5);
    assert_eq!(
        decoded.carrier,
        DirectColorCarrier::AutodeskTrueColor { field: 5 }
    );
    assert_eq!(
        (
            decoded.color.r,
            decoded.color.g,
            decoded.color.b,
            decoded.color.a,
        ),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0)
    );

    let mut emitted = HashSet::new();
    let mut source = Vec::new();
    collect_attributes(
        &entity,
        &AttributeTarget::Face(FaceId::mint("face").expect("identity grammar")),
        &by_index,
        &mut emitted,
        &mut source,
        FORMAT,
    );
    assert_eq!(
        source
            .iter()
            .map(|attribute| attribute.name.as_str())
            .collect::<Vec<_>>(),
        [
            "color-adesk-attrib",
            "material-adesk-attrib",
            "truecolor-adesk-attrib",
            "rgb_color-st-attrib",
            "truecolor-adesk-attrib",
            "entatt_color-bt-attrib",
        ]
    );
}

#[test]
fn legacy_attribute_chain_uses_second_field_forward_link() {
    use super::attributes::{
        attribute_chain_color_carrier, attribute_chain_name, collect_attributes,
    };
    use cadmpeg_ir::attributes::AttributeTarget;

    let entity = Record {
        index: 0,
        name: "face".into(),
        head: "face".into(),
        tokens: vec![Token::Ref(1)].into(),
        offset: 0,
        len: 0,
    };
    let color = Record {
        index: 1,
        name: "rgb_color-st-attrib".into(),
        head: "rgb_color".into(),
        tokens: vec![
            Token::Ref(-1),
            Token::Ref(2),
            Token::Ref(-1),
            Token::Ref(0),
            Token::Double(0.25),
            Token::Double(0.5),
            Token::Double(0.75),
        ]
        .into(),
        offset: 0,
        len: 0,
    };
    let name = Record {
        index: 2,
        name: "string_attrib-name_attrib-gen-attrib".into(),
        head: "string_attrib".into(),
        tokens: vec![
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(1),
            Token::Ref(0),
            Token::Str("name".into()),
            Token::Str("legacy face".into()),
        ]
        .into(),
        offset: 0,
        len: 0,
    };
    let by_index = HashMap::from([(1, &color), (2, &name)]);

    let (carrier, decoded) =
        attribute_chain_color_carrier(&entity, |index| by_index.get(&index).copied()).unwrap();
    assert_eq!(carrier.index, 1);
    assert_eq!(
        decoded.carrier,
        super::attributes::DirectColorCarrier::NormalizedRgb { fields: [4, 5, 6] }
    );
    assert_eq!(
        attribute_chain_name(&entity, &by_index).as_deref(),
        Some("legacy face")
    );

    let mut emitted = HashSet::new();
    let mut source = Vec::new();
    collect_attributes(
        &entity,
        &AttributeTarget::Face(FaceId::mint("face").expect("identity grammar")),
        &by_index,
        &mut emitted,
        &mut source,
        FORMAT,
    );
    assert_eq!(
        source
            .iter()
            .map(|attribute| attribute.name.as_str())
            .collect::<Vec<_>>(),
        [
            "rgb_color-st-attrib",
            "string_attrib-name_attrib-gen-attrib"
        ]
    );
}

#[test]
fn shell_and_loop_attribute_chains_retain_their_native_owners() {
    use cadmpeg_ir::attributes::AttributeTarget;

    let record = |index, name: &str, head: &str, tokens: Vec<Token>| Record {
        index,
        name: name.into(),
        head: head.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    };
    let records = vec![
        record(0, "asmheader", "asmheader", vec![]),
        record(
            1,
            "ATTRIB_CUSTOM-attrib",
            "ATTRIB_CUSTOM",
            vec![Token::Ref(-1)],
        ),
        record(
            2,
            "ATTRIB_CUSTOM-attrib",
            "ATTRIB_CUSTOM",
            vec![Token::Ref(-1)],
        ),
        record(3, "shell", "shell", vec![Token::Ref(1)]),
        record(4, "loop", "loop", vec![Token::Ref(2)]),
    ];
    let mut brep = AsmBrep {
        shells: vec![Shell {
            id: ShellId::mint(id(FORMAT, 3)).expect("identity grammar"),
            region: RegionId::mint("region").expect("identity grammar"),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        }],
        loops: vec![Loop {
            id: LoopId::mint(id(FORMAT, 4)).expect("identity grammar"),
            face: FaceId::mint("face").expect("identity grammar"),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
                coedges: Vec::new(),
                vertex_uses: Vec::new(),
            },
        }],
        ..AsmBrep::default()
    };
    let by_index = records
        .iter()
        .map(|record| (record.index as i64, record))
        .collect();
    let reach = Reachable {
        loops: HashSet::from([4]),
        ..Reachable::default()
    };

    assert_eq!(
        emit_attributes(&mut brep, &records, &by_index, &reach, FORMAT),
        HashSet::from([1, 2])
    );
    assert!(brep.attributes.iter().any(|attribute| attribute.target
        == AttributeTarget::Shell(ShellId::mint(id(FORMAT, 3)).expect("identity grammar"))));
    assert!(brep.attributes.iter().any(|attribute| attribute.target
        == AttributeTarget::Loop(LoopId::mint(id(FORMAT, 4)).expect("identity grammar"))));
}

#[test]
fn lump_named_attributes_bind_to_their_owning_body() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind, Region};

    let record = |index, name: &str, head: &str, tokens: Vec<Token>| Record {
        index,
        name: name.into(),
        head: head.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    };
    let records = vec![
        record(1, "body", "body", vec![Token::Ref(-1)]),
        record(
            2,
            "lump",
            "lump",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(1),
            ],
        ),
        record(
            3,
            "name_attrib-gen-attrib",
            "name",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(2),
                Token::Enum(1),
                Token::Enum(3),
                Token::Enum(1),
                Token::Enum(1),
                Token::Str("MBRD_ST_SHEETMETAL_LUMP".into()),
            ],
        ),
    ];
    let by_index = records
        .iter()
        .map(|record| (record.index as i64, record))
        .collect();
    let body_id = BodyId::mint(id(FORMAT, 1)).expect("identity grammar");
    let mut brep = AsmBrep {
        bodies: vec![Body {
            id: body_id.clone(),
            kind: BodyKind::Sheet,
            regions: vec![RegionId::mint(id(FORMAT, 2)).expect("identity grammar")],
            transform: None,
            name: None,
            color: None,
            visible: None,
        }],
        regions: vec![Region {
            id: RegionId::mint(id(FORMAT, 2)).expect("identity grammar"),
            body: body_id.clone(),
            shells: Vec::new(),
        }],
        ..AsmBrep::default()
    };

    let emitted = emit_attributes(
        &mut brep,
        &records,
        &by_index,
        &Reachable::default(),
        FORMAT,
    );

    assert_eq!(emitted, HashSet::from([3]));
    assert_eq!(
        brep.attributes,
        vec![SourceAttribute {
            id: cadmpeg_ir::ids::AttributeId::mint("f3d:brep:attribute#3")
                .expect("identity grammar"),
            target: AttributeTarget::Body(body_id),
            name: "name_attrib-gen-attrib".into(),
            values: vec![
                AttributeValue::Reference("f3d:brep:entity#-1".into()),
                AttributeValue::Integer(-1),
                AttributeValue::Reference("f3d:brep:entity#-1".into()),
                AttributeValue::Reference("f3d:brep:entity#-1".into()),
                AttributeValue::Reference("f3d:brep:entity#2".into()),
                AttributeValue::Integer(1),
                AttributeValue::Integer(3),
                AttributeValue::Integer(1),
                AttributeValue::Integer(1),
                AttributeValue::String("MBRD_ST_SHEETMETAL_LUMP".into()),
            ],
        }]
    );
}

fn ident(bytes: &mut Vec<u8>, name: &str) {
    bytes.push(0x0d);
    bytes.push(name.len() as u8);
    bytes.extend_from_slice(name.as_bytes());
}

fn reference(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x0c);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn record(bytes: &mut Vec<u8>, name: &str, fields: &[i64]) {
    ident(bytes, name);
    for field in fields {
        reference(bytes, *field);
    }
    bytes.push(0x11);
}

#[test]
fn generated_subshell_hierarchy_flattens_faces_onto_shell() {
    let mut bytes = Vec::new();
    record(&mut bytes, "asmheader", &[]); // 0
    record(&mut bytes, "shell", &[-1, -1, -1, -1, 2, -1, -1, -1]); // 1
    record(&mut bytes, "subshell", &[-1, -1, -1, 1, -1, 3, 4, -1]); // 2
    record(&mut bytes, "subshell", &[-1, -1, -1, 2, -1, -1, 5, -1]); // 3
    record(&mut bytes, "face", &[-1, -1, -1, -1]); // 4
    record(&mut bytes, "face", &[-1, -1, -1, -1]); // 5

    let records = crate::sab::frame(&bytes, 0, bytes.len(), RefWidth::Eight)
        .expect("generated subshell bytes must frame");
    let by_index = records
        .iter()
        .map(|record| (record.index as i64, record))
        .collect::<HashMap<_, _>>();
    let kept = [4, 5].into_iter().collect::<HashSet<_>>();

    assert_eq!(
        shell_faces(&records[1], &by_index, &kept, FORMAT),
        vec![
            FaceId::mint("f3d:brep:entity#4").expect("identity grammar"),
            FaceId::mint("f3d:brep:entity#5").expect("identity grammar")
        ]
    );
    assert_eq!(
        subshell_ancestor_shells(&records, &by_index).get(&3),
        Some(&1)
    );
}

#[test]
fn subshell_wires_project_onto_the_nearest_shell() {
    let mut bytes = Vec::new();
    record(&mut bytes, "asmheader", &[]); // 0
    record(&mut bytes, "shell", &[-1, -1, -1, -1, 2, -1, 4, -1]); // 1
    record(&mut bytes, "subshell", &[-1, -1, -1, 1, -1, 3, -1, 5]); // 2
    record(&mut bytes, "subshell", &[-1, -1, -1, 2, -1, -1, -1, 6]); // 3
    record(&mut bytes, "wire", &[]); // 4
    record(&mut bytes, "wire", &[]); // 5
    record(&mut bytes, "wire", &[]); // 6

    let records = crate::sab::frame(&bytes, 0, bytes.len(), RefWidth::Eight)
        .expect("generated subshell-wire bytes must frame");
    let by_index = records
        .iter()
        .map(|record| (record.index as i64, record))
        .collect::<HashMap<_, _>>();
    assert_eq!(shell_wire_roots(&records[1], &by_index), [4, 5, 6]);
}

#[test]
fn reversed_edge_negates_its_pcurve_validation_interval() {
    let edge = Record {
        index: 1,
        name: "edge".into(),
        head: "edge".into(),
        tokens: vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Ref(2),
            Token::Double(0.55),
            Token::Ref(3),
            Token::Double(0.60),
            Token::Ref(-1),
            Token::Ref(4),
            Token::True,
        ]
        .into(),
        offset: 0,
        len: 0,
    };

    assert_eq!(
        edge_pcurve_parameter_ranges(&edge),
        Some([[-0.55, -0.60], [0.55, 0.60]])
    );
    let candidate = nurbs::pcurve::NurbsPcurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
    assert_eq!(
        pcurve_ranges_on_domain(&candidate, Some(&edge)),
        Some(vec![[0.55, 0.60], [0.0, 1.0]])
    );
}

#[test]
fn carrierless_edge_retains_raw_parameter_range_without_a_domain() {
    let edge = Record {
        index: 1,
        name: "edge".into(),
        head: "edge".into(),
        tokens: vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Ref(2),
            Token::Double(1.0),
            Token::Ref(3),
            Token::Double(0.0),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::False,
        ]
        .into(),
        offset: 0,
        len: 0,
    };
    let records = [edge];
    let by_index = records
        .iter()
        .map(|record| (record.index as i64, record))
        .collect::<HashMap<_, _>>();
    let mut brep = AsmBrep::default();
    let reach = Reachable {
        edges: HashSet::from([1]),
        vertices: HashSet::from([2, 3]),
        ..Reachable::default()
    };

    emit_edges(
        &mut brep,
        &records,
        &by_index,
        &reach,
        &HashSet::new(),
        &HashSet::new(),
        FORMAT,
    );

    assert_eq!(brep.edges.len(), 1);
    assert_eq!(brep.edges[0].curve, None);
    assert_eq!(brep.edges[0].param_range, Some([1.0, 0.0]));
}
