// SPDX-License-Identifier: Apache-2.0
use super::*;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::io::Cursor;

use crate::test_support::{parametrically_bounded_plane_file, trimmed_plane_file};
use crate::writer::Entity;
use crate::{IgesCodec, IgesEncoder, IgesVersion};

mod encode;
mod roundtrip;

#[test]
fn generation_timestamp_uses_utc_calendar_fields() {
    assert_eq!(
        generation_timestamp(UNIX_EPOCH).expect("Unix epoch is representable"),
        "19700101.000000"
    );
    assert_eq!(
        generation_timestamp(UNIX_EPOCH + std::time::Duration::from_secs(951_827_696))
            .expect("leap-day timestamp is representable"),
        "20000229.123456"
    );
}

#[test]
fn number_collapses_libm_near_zeros_and_near_ones() {
    // cos(π/2)-class near-zeros and 1-ε near-ones must share one Fixed ASCII
    // spelling so parameter cards do not reflow across platforms.
    assert_eq!(number(6.123_233_995_736_766e-17), "0");
    assert_eq!(number(9.999_999_999_999_998e-1), number(1.0));
    assert_eq!(
        number(1.802_581_857_082_682),
        number(1.802_581_857_082_681_5)
    );
}

#[test]
fn generated_resolution_covers_large_coordinate_endpoint_admission() {
    let point_start = PointId("point#start".into());
    let point_end = PointId("point#end".into());
    let vertex_start = VertexId("vertex#start".into());
    let vertex_end = VertexId("vertex#end".into());
    let curve_id = CurveId("curve#line".into());
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.extend([
        Point {
            id: point_start.clone(),
            source_object: None,
            position: Point3::new(2_000_000.0, 0.0, 0.0),
        },
        Point {
            id: point_end.clone(),
            source_object: None,
            position: Point3::new(2_000_001.0, 0.0, 0.0),
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: vertex_start.clone(),
            point: point_start,
            tolerance: None,
        },
        Vertex {
            id: vertex_end.clone(),
            point: point_end,
            tolerance: None,
        },
    ]);
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(2_000_000.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("edge#line".into()),
        curve: Some(curve_id),
        start: vertex_start,
        end: vertex_end,
        param_range: Some([0.0, 1.0]),
        tolerance: None,
    });

    let expected = 2_000_001.0 * WRITER_ENDPOINT_RELATIVE_TOLERANCE;
    assert!((generated_minimum_resolution(&ir) - expected).abs() <= f64::EPSILON * 64.0);
}

#[test]
fn generated_global_uses_fixed_profile_and_emitted_coordinate_bound() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.push(Point {
        id: PointId("point#global-profile".into()),
        source_object: None,
        position: Point3::new(123.0, -4.0, 5.0),
    });

    let plan = crate::IgesEncoder::default()
        .plan(EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .expect("NURBS curve has a supported semantic writer profile");
    let mut written = Vec::new();
    plan.write_to(&mut written)
        .expect("generated IGES bytes are writable");
    let scan = crate::card::scan(&written).expect("generated IGES cards scan");
    let global = crate::global::parse(&scan).expect("generated Global record parses");
    assert_eq!(
        global.sender_product().as_deref(),
        Some(WRITER_SENDER_PRODUCT)
    );
    assert_eq!(
        global.native_file_name().as_deref(),
        Some(WRITER_NATIVE_FILE_NAME)
    );
    assert_eq!(global.units_flag(), WRITER_UNITS_FLAG);
    assert_eq!(global.units_name().as_deref(), Some(WRITER_UNITS_NAME));
    assert_eq!(global.version(), "5.3");
    assert!((global.minimum_resolution_mm() - 0.01).abs() <= f64::EPSILON * 64.0);
    assert!((global.maximum_coordinate_mm() - 123.0).abs() <= f64::EPSILON * 64.0);

    let global_text = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(crate::card::Section::Global))
        .flat_map(|line| line.payload.iter().take(72).copied())
        .collect::<Vec<_>>();
    let global_text = String::from_utf8(global_text).expect("generated Global record is ASCII");
    assert!(global_text.starts_with(
        "1H,,1H;,7Hcadmpeg,13Hgenerated.igs,7Hcadmpeg,3H0.1,32,38,6,308,17,0H,1.0,2,2HMM,1,1.0,15H"
    ));
    assert!(global_text.contains(",6Hauthor,7Hcadmpeg,11,0,0H,0H;"));
}

#[test]
fn target_profiles_cover_every_emitted_entity_form() {
    let emitted_forms = [
        (100, 0),
        (102, 0),
        (104, 0),
        (104, 2),
        (104, 3),
        (110, 0),
        (116, 0),
        (123, 0),
        (124, 0),
        (126, 0),
        (128, 0),
        (141, 0),
        (142, 0),
        (143, 0),
        (144, 0),
        (186, 0),
        (190, 1),
        (192, 1),
        (194, 1),
        (196, 1),
        (198, 1),
        (502, 1),
        (504, 1),
        (508, 1),
        (510, 1),
        (514, 1),
    ];
    let entity = |(type_code, form)| Entity {
        type_code,
        form,
        label: "TEST",
        status: "00000000",
        parameters: Vec::new(),
        transform: None,
    };
    for version in [IgesVersion::V5_1, IgesVersion::V5_2, IgesVersion::V5_3] {
        let entities = emitted_forms
            .iter()
            .copied()
            .map(entity)
            .collect::<Vec<_>>();
        assert!(ensure_version_support(&entities, version).is_ok());
    }

    let open_shell = entity((514, 2));
    for version in [IgesVersion::V5_1, IgesVersion::V5_2] {
        assert!(matches!(
            ensure_version_support(std::slice::from_ref(&open_shell), version),
            Err(CodecError::NotImplemented(_))
        ));
    }
    assert!(ensure_version_support(std::slice::from_ref(&open_shell), IgesVersion::V5_3).is_ok());
    for unsupported in [(514, 3), (999, 0)] {
        assert!(matches!(
            ensure_version_support(&[entity(unsupported)], IgesVersion::V5_3),
            Err(CodecError::NotImplemented(_))
        ));
    }
}

#[test]
fn generated_boundary_records_use_the_declared_dependent_status() {
    let regenerate = |bytes| {
        let decoded = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("fixture decodes");
        let plan = IgesEncoder::default()
            .plan(EncodeInput {
                ir: decoded.ir(),
                fidelity: None,
            })
            .expect("fixture has a semantic writer profile");
        let mut written = Vec::new();
        plan.write_to(&mut written).expect("writer succeeds");
        IgesCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .expect("generated IGES decodes")
    };
    let status = |ir: &CadIr, entity_type: i64| {
        ir.native
            .namespace("iges")
            .expect("generated document has the IGES namespace")
            .arenas["entities"]
            .iter()
            .find(|entity| entity.field("entity_type") == Some(entity_type.into()))
            .map(|entity| {
                (
                    entity.field("subordinate_status"),
                    entity.field("use_flag"),
                    entity.field("hierarchy_status"),
                )
            })
            .expect("generated entity status exists")
    };

    assert_eq!(
        status(regenerate(parametrically_bounded_plane_file()).ir(), 141),
        (Some(1.into()), Some(0.into()), Some(0.into()))
    );
    assert_eq!(
        status(regenerate(trimmed_plane_file()).ir(), 142),
        (Some(1.into()), Some(0.into()), Some(0.into()))
    );
}

#[test]
fn analytic_surface_family_uses_pointer_defined_iges_carriers() {
    let cases = [
        (
            SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            190,
        ),
        (
            SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 1.0,
            },
            192,
        ),
        (
            SurfaceGeometry::Cone {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 1.0,
                ratio: 1.0,
                half_angle: std::f64::consts::FRAC_PI_6,
            },
            194,
        ),
        (
            SurfaceGeometry::Sphere {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 1.0,
            },
            196,
        ),
        (
            SurfaceGeometry::Torus {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 2.0,
                minor_radius: 1.0,
            },
            198,
        ),
    ];
    for (geometry, expected_type) in cases {
        let entities = surface_entities(&geometry, 0).expect("analytic surface has a carrier");
        let surface = entities
            .last()
            .expect("analytic surface emits a surface entity");
        assert_eq!(surface.type_code, expected_type);
        assert_eq!(surface.form, 1);
    }
}

#[test]
fn reversed_hyperbola_uses_an_equivalent_reflected_conic_frame() {
    let geometry = CurveGeometry::Hyperbola {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 2.0,
        minor_radius: 3.0,
    };
    let range = [0.2, 1.1];
    let span = CurveSpan {
        range,
        start: curve_point(&geometry, range[0]).expect("start evaluates"),
        end: curve_point(&geometry, range[1]).expect("end evaluates"),
    };
    let entity = oriented_curve_entity(&geometry, &span, Sense::Reversed)
        .expect("a bounded hyperbola can be reversed exactly");
    assert_eq!((entity.type_code, entity.form), (104, 2));
    assert_eq!(
        entity.transform.expect("hyperbola has a placement").rows,
        [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, -1.0, 0.0, 2.0],
            [0.0, 0.0, -1.0, 3.0]
        ]
    );
    let start = hyperbola_point(2.0, 3.0, -range[1]).expect("reflected start evaluates");
    let end = hyperbola_point(2.0, 3.0, -range[0]).expect("reflected end evaluates");
    assert_eq!(
        String::from_utf8(entity.parameters).expect("parameters are ASCII"),
        format!(
            "104,{},0,{},0,0,-1,0,{},{},{},{};",
            number(1.0 / 4.0),
            number(-1.0 / 9.0),
            number(start[0]),
            number(start[1]),
            number(end[0]),
            number(end[1])
        )
    );
}

#[test]
fn orthonormal_pair_repairs_float32_scale_frame_noise() {
    let (axis, reference) = orthonormal_pair(
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, FRAME_REPAIR_DOT_LIMIT * 0.03),
        "test frame",
    )
    .expect("float32-scale skew is representation noise");
    assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(reference, Vector3::new(1.0, 0.0, 0.0));
}

#[test]
fn orthonormal_pair_accepts_the_declared_repair_bound() {
    orthonormal_pair(
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, FRAME_REPAIR_DOT_LIMIT),
        "test frame",
    )
    .expect("the declared frame policy bound is admissible");
}

#[test]
fn orthonormal_pair_refuses_skew_beyond_the_repair_bound() {
    let error = orthonormal_pair(
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, FRAME_REPAIR_DOT_LIMIT * 1.1),
        "test frame",
    )
    .expect_err("material skew must not be silently changed");
    assert!(error.to_string().contains("exceeds the frame repair bound"));
}

#[test]
fn generated_reals_round_trip_after_writer_stabilization() {
    for value in [
        -std::f64::consts::PI,
        1.0,
        f64::MAX,
        1.234_567_890_123_456_7e50,
    ] {
        let encoded = number(value);
        assert!(encoded.contains('D'), "{value}: {encoded}");
        let decoded = encoded
            .replace('D', "E")
            .parse::<f64>()
            .expect("generated real must parse");
        assert_eq!(
            decoded.to_bits(),
            stabilize_real(value).to_bits(),
            "{value}: {encoded}"
        );
    }
    // Sub-tolerance magnitudes collapse so Fixed ASCII layout stays portable.
    for value in [f64::MIN_POSITIVE, f64::from_bits(1), 1.0e-20] {
        assert_eq!(number(value), "0", "{value}");
    }
    assert_eq!(number(0.0), "0");
    assert_eq!(number(-0.0), "0");
}

#[test]
fn generated_parameter_cards_end_at_delimiters() {
    let token = number(f64::MAX);
    let parameters = format!("128,{token},{token},{token},{token};");
    let fragments = parameter_fragments(parameters.as_bytes())
        .expect("ordinary generated real tokens fit one card");
    assert!(fragments.len() > 1);
    assert!(fragments
        .iter()
        .all(|fragment| fragment.len() <= 64 && matches!(fragment.last(), Some(b',' | b';'))));
    assert_eq!(fragments.concat(), parameters.as_bytes());
}

#[test]
fn generated_parameter_token_wider_than_a_card_is_refused() {
    let parameters = format!("{};", "1".repeat(65));
    let error = parameter_fragments(parameters.as_bytes())
        .expect_err("a token wider than the data area must fail");
    assert!(error.to_string().contains("token exceeds 64 bytes"));
}

#[test]
fn generated_full_circle_has_lexically_identical_endpoints() {
    let geometry = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let entity = curve_entity(&geometry, None).expect("full circle is writable");
    let parameters = String::from_utf8(entity.parameters).expect("parameters are ASCII");
    let values = parameters
        .trim_end_matches(';')
        .split(',')
        .collect::<Vec<_>>();
    assert_eq!(&values[4..=5], &values[6..=7]);
}

#[test]
fn generated_circle_refuses_a_zero_length_edge_span() {
    let geometry = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let span = CurveSpan {
        range: [0.5, 0.5],
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(0.0, 0.0, 0.0),
    };
    let error = curve_entity(&geometry, Some(&span))
        .err()
        .expect("zero-length span must not become a full revolution");
    assert!(error.to_string().contains("non-zero ordered span"));
}

#[test]
fn generated_conic_sweep_uses_the_shared_angular_tolerance() {
    assert!(validate_arc_sweep([0.0, TAU + ANGULAR_TOLERANCE]).is_ok());
    assert!(validate_arc_sweep([0.0, TAU + ANGULAR_TOLERANCE * 1.01]).is_err());
}

#[test]
fn face_loop_order_places_the_explicit_outer_loop_first() {
    use cadmpeg_ir::ids::{FaceId, LoopId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::Face;
    use cadmpeg_ir::units::Units;

    let face_id = FaceId::from("face");
    let inner_id = LoopId::from("inner");
    let outer_id = LoopId::from("outer");
    let face = Face {
        id: face_id.clone(),
        shell: ShellId::from("shell"),
        surface: SurfaceId::from("surface"),
        sense: Sense::Forward,
        loops: vec![inner_id.clone(), outer_id.clone()],
        name: None,
        color: None,
        tolerance: None,
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.loops = vec![
        Loop {
            id: inner_id,
            face: face_id.clone(),
            boundary_role: LoopBoundaryRole::Inner,
            coedges: Vec::new(),
            vertex_uses: Vec::new(),
        },
        Loop {
            id: outer_id.clone(),
            face: face_id,
            boundary_role: LoopBoundaryRole::Outer,
            coedges: Vec::new(),
            vertex_uses: Vec::new(),
        },
    ];

    let ordered = face_loop_order(&ir, &face).expect("both face loops resolve");
    assert_eq!(ordered[0].id, outer_id);
}

#[test]
fn face_loop_order_does_not_promote_an_unclassified_loop() {
    use cadmpeg_ir::ids::{FaceId, LoopId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::Face;
    use cadmpeg_ir::units::Units;

    let face_id = FaceId::from("face");
    let inner_id = LoopId::from("inner");
    let unclassified_id = LoopId::from("unclassified");
    let face = Face {
        id: face_id.clone(),
        shell: ShellId::from("shell"),
        surface: SurfaceId::from("surface"),
        sense: Sense::Forward,
        loops: vec![inner_id.clone(), unclassified_id.clone()],
        name: None,
        color: None,
        tolerance: None,
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.loops = vec![
        Loop {
            id: inner_id,
            face: face_id.clone(),
            boundary_role: LoopBoundaryRole::Inner,
            coedges: Vec::new(),
            vertex_uses: Vec::new(),
        },
        Loop {
            id: unclassified_id.clone(),
            face: face_id,
            boundary_role: LoopBoundaryRole::Unspecified,
            coedges: Vec::new(),
            vertex_uses: Vec::new(),
        },
    ];

    let ordered = face_loop_order(&ir, &face).expect("both face loops resolve");
    assert_eq!(ordered[0].id, unclassified_id);
    assert!(face_outer_loop(&ordered).is_none());
}
