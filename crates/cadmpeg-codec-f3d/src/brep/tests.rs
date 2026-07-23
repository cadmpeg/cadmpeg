// SPDX-License-Identifier: Apache-2.0
//! Unit tests for B-rep topology decode, geometry recognition, and
//! procedural carrier classification.
use super::geometry::{
    analytic_procedural_surface, edge_pcurve_parameter_ranges, is_asm_stream_delimiter,
    is_known_record_head, pcurve_ranges_on_domain, point_vector, rational_four_arc_circle,
    select_face_pcurve,
};
use super::topology::{shell_faces, shell_wire_roots, subshell_ancestor_shells};
use super::*;
use crate::nurbs;
use crate::records::BodyNativeKey;
use crate::sab::{Record, Token};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::{BodyId, FaceId, LoopId, RegionId, ShellId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Loop, LoopBoundaryRole, Shell};
use std::collections::{HashMap, HashSet};

fn exact_circle_directrix() -> cadmpeg_ir::geometry::NurbsCurve {
    let center = Point3::new(2.0, 3.0, 4.0);
    let point = |x, y| Point3::new(center.x + x, center.y + y, center.z);
    cadmpeg_ir::geometry::NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
        control_points: vec![
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
        weights: Some(vec![
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
        periodic: false,
    }
}

#[test]
fn exact_circle_extrusion_reduces_to_cylinder_only_along_normal() {
    let definition =
        |direction| nurbs::proc_surface::DecodedProceduralSurfaceDefinition::Extrusion {
            directrix: exact_circle_directrix(),
            parameter_interval: [0.0, 4.0],
            direction,
            native_position: Point3::new(0.0, 0.0, 0.0),
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
    approximate.control_points[3].x += 1.0e-5;
    assert!(rational_four_arc_circle(&approximate).is_none());
}

fn degree_elevated_circle() -> cadmpeg_ir::geometry::NurbsCurve {
    let quadratic = exact_circle_directrix();
    let weights = quadratic.weights.as_deref().unwrap();
    let homogeneous = |index: usize| {
        let point = quadratic.control_points[index];
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
    cadmpeg_ir::geometry::NurbsCurve {
        degree: 3,
        knots: vec![
            0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 4.0, 4.0, 4.0, 4.0,
        ],
        control_points,
        weights: Some(weights),
        periodic: false,
    }
}

#[test]
fn exact_circle_recognition_is_projective_and_degree_invariant() {
    let mut scaled = exact_circle_directrix();
    for weight in scaled.weights.as_mut().unwrap() {
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
            }
        ),
        Some(SurfaceGeometry::Cylinder { .. })
    ));
    elevated.control_points[5].x += 1.0e-5;
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
    cadmpeg_ir::geometry::NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: points,
        weights: None,
        periodic: false,
    }
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
    spine.control_points[1].x = 2.1;
    assert!(analytic_procedural_surface(&definition).is_none());
}

#[test]
fn constant_circular_plane_cylinder_blend_reduces_to_tangent_torus() {
    let mut circle = exact_circle_directrix();
    for point in &mut circle.control_points {
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
fn brep_qualification_rewrites_owned_ids_and_cross_references() {
    use cadmpeg_ir::ids::{BodyId, RegionId};
    use cadmpeg_ir::topology::{Body, Region};

    let body = BodyId("f3d:brep:entity#1".into());
    let region = RegionId("f3d:brep:entity#2".into());
    let mut brep = Brep {
        bodies: vec![Body {
            id: body.clone(),
            kind: Default::default(),
            regions: vec![region.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        }],
        regions: vec![Region {
            id: region,
            body: body.clone(),
            shells: Vec::new(),
        }],
        body_keys: HashMap::from([(body.clone(), 7)]),
        body_native_keys: vec![BodyNativeKey {
            id: "f3d:asm:body-native-key#1".into(),
            body,
            record_index: 1,
            body_ordinal: 0,
            source_brep: Some("BREP.source.smbh".into()),
            asm_body_key: Some(7),
        }],
        annotation_records: vec![AnnotationRecord {
            id: "f3d:brep:entity#1".into(),
            stream: "asset/BREP.source.smbh".into(),
            offset: 10,
            tag: "body".into(),
            derived_fields: Vec::new(),
        }],
        ..Brep::default()
    };

    brep.qualify_ids("source").expect("qualify BREP");

    let qualified = BodyId("f3d:brep/source/brep:entity#1".into());
    assert_eq!(brep.bodies[0].id, qualified);
    assert_eq!(brep.regions[0].body, qualified);
    assert_eq!(brep.body_native_keys[0].body, qualified);
    assert_eq!(brep.body_keys.get(&qualified), Some(&7));
    assert_eq!(brep.annotation_records[0].id, qualified.0);
    assert_eq!(
        brep.body_native_keys[0].source_brep.as_deref(),
        Some("BREP.source.smbh")
    );
}

#[test]
fn body_key_retention_keeps_only_the_selected_connected_graph() {
    use cadmpeg_ir::ids::{BodyId, RegionId};
    use cadmpeg_ir::topology::{Body, Region};

    let body = |index, region| Body {
        id: BodyId(format!("f3d:brep:entity#{index}")),
        kind: Default::default(),
        regions: vec![RegionId(format!("f3d:brep:entity#{region}"))],
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let native_key = |index, key| BodyNativeKey {
        id: format!("f3d:asm:body-native-key#{index}"),
        body: BodyId(format!("f3d:brep:entity#{index}")),
        record_index: index,
        body_ordinal: index - 1,
        source_brep: Some("BREP.source.smbh".into()),
        asm_body_key: Some(key),
    };
    let mut brep = Brep {
        bodies: vec![body(1, 2), body(3, 4)],
        regions: vec![
            Region {
                id: RegionId("f3d:brep:entity#2".into()),
                body: BodyId("f3d:brep:entity#1".into()),
                shells: Vec::new(),
            },
            Region {
                id: RegionId("f3d:brep:entity#4".into()),
                body: BodyId("f3d:brep:entity#3".into()),
                shells: Vec::new(),
            },
        ],
        body_keys: HashMap::from([
            (BodyId("f3d:brep:entity#1".into()), 10),
            (BodyId("f3d:brep:entity#3".into()), 20),
        ]),
        body_native_keys: vec![native_key(1, 10), native_key(3, 20)],
        ..Brep::default()
    };

    brep.retain_body_keys(&HashSet::from([20]))
        .expect("retain body graph");

    assert_eq!(brep.bodies.len(), 1);
    assert_eq!(brep.bodies[0].id.0, "f3d:brep:entity#3");
    assert_eq!(brep.regions.len(), 1);
    assert_eq!(brep.regions[0].id.0, "f3d:brep:entity#4");
    assert_eq!(brep.body_native_keys.len(), 1);
    assert_eq!(brep.body_keys.len(), 1);
}

#[test]
fn body_selectors_use_ordinals_only_for_an_all_null_key_lane() {
    let native_key = |ordinal, key| BodyNativeKey {
        id: format!("f3d:asm:body-native-key#{ordinal}"),
        body: BodyId(format!("f3d:brep:entity#{ordinal}")),
        record_index: ordinal,
        body_ordinal: ordinal,
        source_brep: Some("BREP.source.smb".into()),
        asm_body_key: key,
    };
    let mut brep = Brep {
        body_native_keys: vec![native_key(0, None), native_key(1, None)],
        ..Brep::default()
    };

    assert_eq!(brep.body_selectors().len(), 2);
    assert_eq!(
        brep.body_selectors()[&BodyId("f3d:brep:entity#1".into())],
        1
    );

    brep.body_native_keys[1].asm_body_key = Some(7);
    assert_eq!(
        brep.body_selectors(),
        HashMap::from([(BodyId("f3d:brep:entity#1".into()), 7)])
    );
}

#[test]
fn nested_attributes_inherit_their_topology_owner() {
    use cadmpeg_ir::attributes::AttributeTarget;
    use cadmpeg_ir::ids::EdgeId;

    let attribute = |index, owner| Record {
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
    let parent = attribute(7, 3);
    let child = attribute(8, 7);
    let records = HashMap::from([(7, &parent), (8, &child)]);
    let expected = AttributeTarget::Edge(EdgeId("edge".into()));
    let targets = HashMap::from([(3, expected.clone())]);

    assert_eq!(
        inherited_attribute_target(7, &records, &targets),
        Some(expected.clone())
    );
    assert_eq!(
        inherited_attribute_target(8, &records, &targets),
        Some(expected)
    );

    let cycle_left = attribute(9, 10);
    let cycle_right = attribute(10, 9);
    let cycle = HashMap::from([(9, &cycle_left), (10, &cycle_right)]);
    assert_eq!(inherited_attribute_target(9, &cycle, &targets), None);
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
    let mut brep = Brep {
        shells: vec![Shell {
            id: ShellId(id(3)),
            region: RegionId("region".into()),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        }],
        loops: vec![Loop {
            id: LoopId(id(4)),
            face: FaceId("face".into()),
            boundary_role: LoopBoundaryRole::Unspecified,
            coedges: Vec::new(),
            vertex_uses: Vec::new(),
        }],
        ..Brep::default()
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
        emit_attributes(&mut brep, &records, &by_index, &reach),
        HashSet::from([1, 2])
    );
    assert!(brep
        .attributes
        .iter()
        .any(|attribute| attribute.target == AttributeTarget::Shell(ShellId(id(3)))));
    assert!(brep
        .attributes
        .iter()
        .any(|attribute| attribute.target == AttributeTarget::Loop(LoopId(id(4)))));
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

    let records =
        crate::sab::frame(&bytes, 0, bytes.len(), 8).expect("generated subshell bytes must frame");
    let by_index = records
        .iter()
        .map(|record| (record.index as i64, record))
        .collect::<HashMap<_, _>>();
    let kept = [4, 5].into_iter().collect::<HashSet<_>>();

    assert_eq!(
        shell_faces(&records[1], &by_index, &kept),
        vec![
            FaceId("f3d:brep:entity#4".into()),
            FaceId("f3d:brep:entity#5".into())
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

    let records = crate::sab::frame(&bytes, 0, bytes.len(), 8)
        .expect("generated subshell-wire bytes must frame");
    let by_index = records
        .iter()
        .map(|record| (record.index as i64, record))
        .collect::<HashMap<_, _>>();
    assert_eq!(shell_wire_roots(&records[1], &by_index), [4, 5, 6]);
}

#[test]
fn exact_procedural_pcurve_bypasses_nurbs_cache_parameterization() {
    let records = [
        Record {
            index: 1,
            name: "point".into(),
            head: "point".into(),
            tokens: vec![Token::Position([0.0, 0.0, 0.0])].into(),
            offset: 0,
            len: 0,
        },
        Record {
            index: 2,
            name: "point".into(),
            head: "point".into(),
            tokens: vec![Token::Position([1.0, 0.0, 0.0])].into(),
            offset: 0,
            len: 0,
        },
        Record {
            index: 3,
            name: "vertex".into(),
            head: "vertex".into(),
            tokens: vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Long(0),
                Token::Ref(1),
            ]
            .into(),
            offset: 0,
            len: 0,
        },
        Record {
            index: 4,
            name: "vertex".into(),
            head: "vertex".into(),
            tokens: vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Long(1),
                Token::Ref(2),
            ]
            .into(),
            offset: 0,
            len: 0,
        },
        Record {
            index: 5,
            name: "edge".into(),
            head: "edge".into(),
            tokens: vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(3),
                Token::Double(0.0),
                Token::Ref(4),
            ]
            .into(),
            offset: 0,
            len: 0,
        },
    ];
    let by_index = records
        .iter()
        .map(|record| (record.index as i64, record))
        .collect::<HashMap<_, _>>();
    let cache = SurfaceGeometry::Nurbs(cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        weights: None,
        u_periodic: false,
        v_periodic: false,
    });
    let candidate = || nurbs::pcurve::NurbsPcurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![
            cadmpeg_ir::math::Point2::new(10.0, 10.0),
            cadmpeg_ir::math::Point2::new(11.0, 10.0),
        ],
        weights: None,
        periodic: false,
    };

    assert!(select_face_pcurve(
        vec![candidate()],
        Some(&cache),
        false,
        Some(&records[4]),
        &by_index,
    )
    .is_none());
    assert!(select_face_pcurve(
        vec![candidate()],
        Some(&cache),
        true,
        Some(&records[4]),
        &by_index,
    )
    .is_some());
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
    let candidate = nurbs::pcurve::NurbsPcurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    assert_eq!(
        pcurve_ranges_on_domain(&candidate, Some(&edge)),
        Some(vec![[0.55, 0.60], [0.0, 1.0]])
    );
}
