// SPDX-License-Identifier: Apache-2.0
use super::*;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::topology::{Edge, PcurveUse, Point, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::io::Cursor;

use crate::loss::IgesLossCode;
use crate::test_support::{
    fixed_ascii_with_global, parametrically_bounded_plane_file, point_file_with_global,
    trimmed_plane_file, trimmed_plane_with_inner_loop_file,
};
use crate::writer::Entity;
use crate::{IgesCodec, IgesEncoder, IgesVersion};

mod encode;
mod quarantine;
mod roundtrip;

fn accepts_procedural_reduction_loss(taxonomy: cadmpeg_ir::LossTaxonomy) -> bool {
    matches!(
        taxonomy,
        cadmpeg_ir::LossTaxonomy::PassthroughRecordOmitted
            | cadmpeg_ir::LossTaxonomy::PreservedSourceUnavailable
            | cadmpeg_ir::LossTaxonomy::ProceduralReduced
            | cadmpeg_ir::LossTaxonomy::MetadataNotTransferred
    )
}

fn accepts_non_manifold_write_loss(taxonomy: cadmpeg_ir::LossTaxonomy) -> bool {
    matches!(
        taxonomy,
        cadmpeg_ir::LossTaxonomy::PassthroughRecordOmitted
            | cadmpeg_ir::LossTaxonomy::PreservedSourceUnavailable
            | cadmpeg_ir::LossTaxonomy::MetadataNotTransferred
    )
}

#[test]
fn rejects_mixed_unclassified_bounded_surface_representation() {
    let mut decoded = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_with_inner_loop_file()),
            &DecodeOptions::default(),
        )
        .expect("synthetic mixed-loop fixture decodes");
    let model_only_loop_id = decoded.ir().model.faces[0].loops[0].clone();
    {
        let mut ir = decoded.ir_mut();
        for loop_ in &mut ir.model.loops {
            loop_.boundary_role = LoopBoundaryRole::Unspecified;
        }
        let coedge_ids = ir
            .model
            .loops
            .iter()
            .find(|loop_| loop_.id == model_only_loop_id)
            .expect("the first face loop resolves")
            .coedges
            .clone();
        for coedge_id in coedge_ids {
            ir.model
                .coedges
                .iter_mut()
                .find(|coedge| coedge.id == coedge_id)
                .expect("the loop coedge resolves")
                .pcurves
                .clear();
        }
        let used_pcurves = ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| coedge.pcurves.iter())
            .map(|pcurve| pcurve.pcurve.clone())
            .collect::<std::collections::BTreeSet<_>>();
        ir.model
            .pcurves
            .retain(|pcurve| used_pcurves.contains(&pcurve.id));
    }

    let result = IgesEncoder::default().plan(EncodeInput {
        ir: decoded.ir(),
        fidelity: None,
    });
    assert!(matches!(result, Err(CodecError::NotImplemented(_))));
}

#[test]
fn type_508_requires_an_explicit_isoparametric_flag() {
    let pcurve = PcurveUse {
        pcurve: "pcurve#type-508".into(),
        isoparametric: Some(false),
        parameter_range: None,
    };
    assert_eq!(
        isoparametric_flag(&pcurve, "loop").expect("explicit false is supported"),
        0
    );

    assert_eq!(
        isoparametric_flag(
            &PcurveUse {
                isoparametric: Some(true),
                ..pcurve.clone()
            },
            "loop"
        )
        .expect("explicit true is supported"),
        1
    );

    let pcurve = PcurveUse {
        isoparametric: None,
        ..pcurve
    };
    assert!(matches!(
        isoparametric_flag(&pcurve, "loop"),
        Err(CodecError::NotImplemented(_))
    ));
}

#[test]
fn generation_timestamp_uses_utc_calendar_fields() {
    assert_eq!(
        generation_timestamp(UNIX_EPOCH, IgesVersion::V5_3).expect("Unix epoch is representable"),
        "19700101.000000"
    );
    assert_eq!(
        generation_timestamp(
            UNIX_EPOCH + std::time::Duration::from_secs(951_827_696),
            IgesVersion::V5_3,
        )
        .expect("leap-day timestamp is representable"),
        "20000229.123456"
    );
}

#[test]
fn generation_timestamp_uses_the_declared_version_width() {
    let instant = UNIX_EPOCH + std::time::Duration::from_secs(951_827_696);
    assert_eq!(
        generation_timestamp(instant, IgesVersion::V4_0)
            .expect("IGES 4.0 timestamp is representable"),
        "000229.123456"
    );
    assert_eq!(
        generation_timestamp(instant, IgesVersion::V5_0)
            .expect("IGES 5.0 timestamp is representable"),
        "000229.123456"
    );
}

#[test]
fn number_preserves_distinct_finite_values() {
    for value in [
        6.123_233_995_736_766e-17,
        5.0e-13,
        9.999_999_999_999_998e-1,
        1.802_581_857_082_682,
        1.802_581_857_082_681_5,
    ] {
        let encoded = number(value);
        let decoded = encoded
            .replace('D', "E")
            .parse::<f64>()
            .expect("generated real must parse");
        assert_eq!(decoded.to_bits(), value.to_bits(), "{value}: {encoded}");
    }
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
    let (global, _) = crate::global::parse(&scan).expect("generated Global record parses");
    assert_eq!(
        global.sender_product().as_deref(),
        Some(WRITER_SENDER_PRODUCT)
    );
    assert_eq!(
        global.native_file_name().as_deref(),
        Some(WRITER_NATIVE_FILE_NAME)
    );
    assert_eq!(global.units_flag(), Some(WRITER_UNITS_FLAG));
    assert_eq!(global.units_name().as_deref(), Some(WRITER_UNITS_NAME));
    assert_eq!(global.version(), "5.3");
    assert!(
        (global
            .length_context()
            .expect("generated Global resolves a millimetre length factor")
            .minimum_resolution_mm()
            - 0.01)
            .abs()
            <= f64::EPSILON * 64.0
    );
    assert!(
        (global
            .maximum_coordinate_mm()
            .expect("generated Global declares a maximum coordinate")
            - 123.0)
            .abs()
            <= f64::EPSILON * 64.0
    );

    let global_text = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(crate::card::Section::Global))
        .flat_map(|line| line.payload.iter().take(72).copied())
        .collect::<Vec<_>>();
    let global_text = String::from_utf8(global_text)
        .expect("generated Global record is ASCII")
        .replace(' ', "");
    assert!(global_text.starts_with(
        "1H,,1H;,7Hcadmpeg,13Hgenerated.igs,7Hcadmpeg,3H0.1,32,38,6,308,17,0H,1.0,2,2HMM,1,1.0,15H"
    ));
    assert!(global_text.contains(",6Hauthor,7Hcadmpeg,11,0,0H,0H;"));
}

#[test]
fn generated_global_matches_the_4_0_and_5_0_field_contracts() {
    for (version, name, timestamp, tail) in [
        (IgesVersion::V4_0, "4.0", "260714.000000", ",6,0;"),
        (IgesVersion::V5_0, "5.0", "260714.000000", ",8,0,0H;"),
    ] {
        let global_bytes = generated_global(version, timestamp, 0.001, 1000.0);
        let fixture = fixed_ascii_with_global(&global_bytes);
        let scan = crate::card::scan(&fixture).expect("versioned generated Global cards scan");
        let (global, losses) = crate::global::parse(&scan).expect("versioned Global parses");
        assert_eq!(global.version(), name);
        assert!(losses.is_empty(), "{name}: {losses:#?}");

        let global_text = scan
            .lines
            .iter()
            .filter(|line| line.section == Some(crate::card::Section::Global))
            .flat_map(|line| line.payload.iter().take(72).copied())
            .collect::<Vec<_>>();
        let global_text = String::from_utf8(global_text)
            .expect("Global is ASCII")
            .replace(' ', "");
        assert!(
            global_text.contains(&format!(",{}H{timestamp}", timestamp.len())),
            "{name}: {global_text}"
        );
        assert!(
            global_text.trim_end().ends_with(tail),
            "{name}: {global_text}"
        );
    }
}

#[test]
fn encode_uses_neutral_linear_tolerance_as_global_floor() {
    let mut ir = CadIr::empty(Units::default());
    ir.tolerances.linear = 2.5;
    ir.model.points.push(Point {
        id: PointId("point#resolution-floor".into()),
        source_object: None,
        position: Point3::new(1.0, 2.0, 3.0),
    });

    let plan = crate::IgesEncoder::default()
        .plan(EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .expect("neutral tolerance floor is writable");
    let mut written = Vec::new();
    let report = plan
        .write_to(&mut written)
        .expect("neutral tolerance floor output is writable");
    let scan = crate::card::scan(&written).expect("neutral tolerance floor output scans");
    let (global, _) = crate::global::parse(&scan).expect("neutral tolerance floor Global parses");

    assert_eq!(
        global
            .length_context()
            .expect("generated Global resolves a millimetre length factor")
            .minimum_resolution_mm(),
        2.5
    );
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
}

#[test]
fn encode_reports_when_source_resolution_is_raised_for_geometry() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(point_file_with_global(global)),
            &DecodeOptions::default(),
        )
        .expect("source resolution witness decodes");
    assert_eq!(decoded.ir().tolerances.linear, 0.001);

    let plan = crate::IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .expect("source resolution witness is writable");
    let mut written = Vec::new();
    let report = plan
        .write_to(&mut written)
        .expect("source resolution witness output is writable");
    let scan = crate::card::scan(&written).expect("source resolution output scans");
    let (global, _) = crate::global::parse(&scan).expect("source resolution output Global parses");

    assert_eq!(
        global
            .length_context()
            .expect("generated Global resolves a millimetre length factor")
            .minimum_resolution_mm(),
        0.01
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::WriterMinimumResolutionAdjusted.kind() }));
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
        (120, 0),
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

    for unsupported in [(123, 0), (141, 0), (143, 0), (186, 0), (190, 1), (502, 1)] {
        assert!(matches!(
            ensure_version_support(&[entity(unsupported)], IgesVersion::V4_0),
            Err(CodecError::NotImplemented(_))
        ));
    }
    for supported in [(120, 0), (141, 0), (143, 0)] {
        assert!(ensure_version_support(&[entity(supported)], IgesVersion::V5_0).is_ok());
    }
    assert!(ensure_version_support(&[entity((120, 0))], IgesVersion::V4_0).is_ok());
    for unsupported in [(104, 0), (123, 0), (186, 0), (190, 1)] {
        assert!(matches!(
            ensure_version_support(&[entity(unsupported)], IgesVersion::V5_0),
            Err(CodecError::NotImplemented(_))
        ));
    }
    let single_constituent = Entity {
        type_code: 102,
        form: 0,
        label: "TEST",
        status: "00000000",
        parameters: b"102,1,@R0@;".to_vec(),
        transform: None,
    };
    assert!(matches!(
        ensure_version_support(std::slice::from_ref(&single_constituent), IgesVersion::V4_0),
        Err(CodecError::NotImplemented(_))
    ));
    assert!(
        ensure_version_support(std::slice::from_ref(&single_constituent), IgesVersion::V5_0)
            .is_ok()
    );
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
    assert_eq!(
        status(regenerate(parametrically_bounded_plane_file()).ir(), 126),
        (Some(1.into()), Some(5.into()), Some(0.into()))
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
fn generated_reals_round_trip_without_writer_quantization() {
    for value in [
        -std::f64::consts::PI,
        1.0,
        f64::MAX,
        1.234_567_890_123_456_7e50,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        1.0e-20,
        5.0e-13,
    ] {
        let encoded = number(value);
        assert!(encoded.contains('D'), "{value}: {encoded}");
        let decoded = encoded
            .replace('D', "E")
            .parse::<f64>()
            .expect("generated real must parse");
        assert_eq!(decoded.to_bits(), value.to_bits(), "{value}: {encoded}");
    }
    assert_eq!(number(0.0), "0");
    assert_eq!(number(-0.0), "0");
}

#[test]
fn generated_parameter_cards_preserve_field_boundaries() {
    let token = number(f64::MAX);
    let parameters = format!("128,{token},{token},{token},{token};");
    let fragments = crate::parameter::layout_parameter_cards(parameters.as_bytes())
        .expect("ordinary generated real tokens fit one card");
    assert!(fragments.len() > 1);
    assert!(fragments.iter().all(|fragment| fragment.len() <= 64));
    let compact = fragments
        .concat()
        .into_iter()
        .filter(|byte| *byte != b' ')
        .collect::<Vec<_>>();
    assert_eq!(compact, parameters.as_bytes());
}

#[test]
fn generated_parameter_field_wider_than_a_card_is_refused() {
    let parameters = format!("{};", "1".repeat(65));
    let error = crate::parameter::layout_parameter_cards(parameters.as_bytes())
        .expect_err("a field wider than the data area must fail");
    assert!(error.to_string().contains("field exceeds one card"));
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
