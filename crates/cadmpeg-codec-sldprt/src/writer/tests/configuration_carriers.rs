// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.

use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::parameters::{DesignConfiguration, ParameterValue};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    ConfigurationId, Feature, FeatureDefinition, FeatureId, FeatureOperation,
};

use cadmpeg_test_support::EditableDecodeResult;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::appearance::sldprt_with_body_and_material;
use crate::test_support::container::make_block;
use crate::test_support::container::outer_header;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::history::resolved_features_payload;
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::history::sldprt_with_body_and_resolved_features;
use crate::test_support::ir::translate_model_x;
use crate::test_support::native::sldprt_native;
use crate::test_support::native::update_sldprt_native;
use crate::test_support::parasolid::closed_cylinder_body;
use crate::test_support::parasolid::count_entity51_family;
use crate::test_support::parasolid::entity51;
use crate::test_support::parasolid::entity53_color;
use crate::test_support::parasolid::face_color_definition;
use crate::test_support::parasolid::owned_triangle;
use crate::test_support::parasolid::owned_triangle_with_kind;
use crate::test_support::parasolid::parasolid_with_body;
use crate::test_support::parasolid::sphere_patch_body;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::parasolid::FACE_COLOR_DEFINITION_ID;
use crate::SldprtCodec;
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};

const EPS_COLOR_COMPONENT: f64 = 1.0e-6;

fn strip_names(topology: &mut cadmpeg_ir::document::Topology) {
    topology.bodies.iter_mut().for_each(|body| body.name = None);
    topology.faces.iter_mut().for_each(|face| face.name = None);
}

fn unnamed_cube() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    strip_names(&mut ir.geometry);
    ir.geometry
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    ir
}

#[test]
fn geometry_edit_replaces_each_generated_configuration_partition() {
    use std::collections::BTreeMap;

    let mut ir = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap()
        .into_parts()
        .0;
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    strip_names(&mut ir.geometry);
    let body = ir.geometry.bodies[0].id.clone();
    ir.model.configurations = (0..2)
        .map(|index| DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:probe:configuration#{index}")).unwrap(),
            ordinal: index,
            active: index == 1,
            source_index: Some(index),
            name: Some(format!("Config {index}")),
            material: None,
            properties: BTreeMap::new(),
            bodies: Some(DistinctMembers::try_from(vec![body.clone()]).unwrap()),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        })
        .collect();
    let mut source = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Unnamed,
    )
    .and_then(|plan| plan.write_to(&mut source))
    .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let point = decoded.ir().geometry.points[0].position().get();
    decoded.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            point.x,
            point.y,
            point.z + 1.0,
        ))
        .unwrap(),
    );
    let mut output = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut output,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&output);
    for index in 0..2 {
        let section = format!("Contents/Config-{index}-Partition");
        assert_eq!(
            scan.blocks
                .iter()
                .filter(|block| block.section.name() == Some(section.as_str()))
                .count(),
            1,
            "{section} must have one partition"
        );
    }
    let selected = container::select_active_parasolid_site(&scan).expect("unique active solid");
    assert_eq!(selected.name(), "Contents/Config-1-Partition");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        regenerated
            .ir()
            .model
            .configurations
            .iter()
            .filter(|configuration| configuration.active)
            .count(),
        1
    );
}

#[test]
fn retained_utf16_document_envelope_uses_the_shared_recognizer_and_patcher() {
    let xml =
        r#"<swSolidWorks swVersion="34000"><swModel swConfigurationName="Old"/></swSolidWorks>"#;
    let mut payload = vec![0xff, 0xfe];
    for unit in xml.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let read = |bytes: &[u8]| container::first_solidworks_envelope(&ctx, [bytes]).unwrap();
    assert!(read(&payload).is_some());
    let patched =
        with_ctx(|ctx| super::super::patch_active_configuration_xml(ctx, &payload, "New"))
            .unwrap()
            .unwrap()
            .expect("the UTF-16 envelope is recognized");
    let envelope = read(&patched).expect("the rewritten envelope remains recognizable");
    assert_eq!(envelope.sw_version.as_deref(), Some("34000"));
    assert_eq!(envelope.configuration_name.as_deref(), Some("New"));
}

#[test]
fn encoder_writes_authored_with_retained_native_envelope_datum_features() {
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = unnamed_cube();
    let definitions = [
        FeatureDefinition::Operation(FeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::geometry::FeatureDatumPlaneFrame::new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        }),
        FeatureDefinition::Operation(FeatureOperation::datum_axis(
            FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).unwrap(),
            cadmpeg_ir::features::geometry::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0))
                .unwrap(),
        )),
        FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            position: FinitePoint3::new(Point3::new(7.0, 8.0, 9.0)).unwrap(),
            construction: None,
        }),
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            product_definition: None,
            id: FeatureId::mint(format!("synthetic:test:feature#datum-{ordinal}"))
                .expect("identity grammar"),
            ordinal: cadmpeg_core::decode::u64_from_index(ordinal),
            name: Some(format!("Datum {ordinal}")),
            suppressed: Some(false),
            dependencies: DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Unnamed,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { .. })
    ));
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumAxis { .. })
    ));
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPoint { .. })
    ));
}

#[test]
fn encoder_writes_authored_with_retained_native_envelope_neutral_configurations() {
    use std::collections::BTreeMap;

    let mut ir = unnamed_cube();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("sldprt:model:configuration#generated:z")
            .expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: None,
        name: Some("Metric".to_string()),
        material: Some("Steel".into()),
        properties: BTreeMap::from([(cadmpeg_core::nonblank_literal!("Finish"), "Ground".into())]),
        bodies: Some(DistinctMembers::try_from(vec![ir.geometry.bodies[0].id.clone()]).unwrap()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("sldprt:model:configuration#generated:a")
            .expect("identity grammar"),
        ordinal: 1,
        active: false,
        source_index: None,
        name: Some("Empty".to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: Some(DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.finalize();

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Unnamed,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let scan = crate::test_support::container::scan(&encoded);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-0-Partition") }));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-1-Partition") }));
    let empty = scan
        .blocks
        .iter()
        .find(|block| block.section.name() == Some("Contents/Config-1-Partition"))
        .unwrap();
    assert_eq!(empty.ps_streams.len(), 2);
    assert_eq!(empty.ps_streams[0].header.description, "partition body");
    assert_eq!(empty.ps_streams[1].header.description, "deltas body");
    assert!(empty
        .ps_streams
        .iter()
        .all(|stream| stream.header.body_offset == stream.payload.len()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded
            .ir()
            .model
            .configurations
            .iter()
            .filter_map(|configuration| configuration.name.as_deref())
            .collect::<Vec<_>>(),
        vec!["Metric", "Empty"]
    );
    assert_eq!(
        sldprt_native(decoded.ir()).feature_histories[0]
            .configurations
            .iter()
            .map(|configuration| configuration.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(decoded.ir().model.configurations[0].source_index, Some(0));
    assert_eq!(decoded.ir().model.configurations[1].source_index, Some(1));
    assert!(sldprt_native(decoded.ir()).feature_histories[0]
        .configurations
        .iter()
        .all(|configuration| !configuration.properties.contains_key("SourceIndex")));
    let configuration = &decoded.ir().model.configurations[0];
    assert_eq!(configuration.name.as_deref(), Some("Metric"));
    assert_eq!(configuration.material.as_deref(), Some("Steel"));
    assert_eq!(configuration.properties["Finish"], "Ground");
    assert!(configuration.active);
    assert_eq!(
        configuration.bodies.as_deref(),
        Some(
            decoded
                .ir()
                .geometry
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>()
                .as_slice()
        )
    );
    assert!(decoded.ir().model.configurations[1]
        .bodies
        .as_deref()
        .is_some_and(<[_]>::is_empty));

    let (mut inactive, _, fidelity) = decoded.into_parts();
    inactive
        .model
        .configurations
        .iter_mut()
        .for_each(|configuration| configuration.active = false);
    let report = SldprtCodec
        .plan(EncodeInput::new(&inactive, Some(&fidelity)))
        .unwrap()
        .report()
        .clone();
    let assumed = report
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "writer.active-configuration-assumed")
        .unwrap();
    assert!(assumed
        .message
        .contains(inactive.model.configurations[0].id.as_str()));
}

#[test]
fn semantic_writer_round_trips_active_configuration() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Manufacturing &amp; QA"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swName="Part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    assert!(decoded.ir().model.configurations[0].active);
    assert!(!decoded.ir().model.configurations[1].active);

    decoded.ir_mut().model.configurations[0].active = false;
    decoded.ir_mut().model.configurations[1].active = true;
    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(!regenerated.ir().model.configurations[0].active);
    assert!(regenerated.ir().model.configurations[1].active);
    assert_eq!(
        regenerated.ir().source.as_ref().unwrap().attributes["sw_configuration_name"],
        "Manufacturing & QA"
    );
}

#[test]
fn encoder_partitions_authored_with_retained_native_envelope_bodies_by_configuration() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::tessellation::Tessellation;
    use cadmpeg_ir::transform::Transform;
    use std::collections::BTreeMap;

    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let mut ir = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap()
        .into_parts()
        .0;
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    strip_names(&mut ir.geometry);
    let body_ids = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    for (index, body) in ir.geometry.bodies.iter_mut().enumerate() {
        body.transform = Some(
            Transform::affine([
                [
                    1.0,
                    0.0,
                    0.0,
                    (cadmpeg_core::convert::f64_from_index(index).expect("index is exact") + 1.0)
                        * 10.0,
                ],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        );
    }
    ir.model.tessellations = body_ids
        .iter()
        .enumerate()
        .map(|(index, body)| {
            Tessellation::new(
                cadmpeg_ir::tessellation::TessellationId::mint(format!(
                    "synthetic:test:tessellation#{index}"
                ))
                .expect("valid identity"),
                cadmpeg_ir::tessellation::TessellationMesh::from_strip_lanes(
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 0.0, 0.0),
                        Point3::new(0.0, 1.0, 0.0),
                    ],
                    Some(vec![Vector3::new(0.0, 0.0, 1.0); 3]),
                    &[3],
                )
                .expect("strip lanes line up"),
                Vec::new(),
            )
            .expect("valid tessellation")
            .with_of(cadmpeg_ir::tessellation::Meshed::Body {
                body: body.clone(),
                faces: Vec::new(),
            })
        })
        .collect();
    ir.model.configurations = body_ids
        .iter()
        .enumerate()
        .map(|(index, body)| DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:configuration#config-{index}"))
                .expect("identity grammar"),
            ordinal: u32::try_from(index).expect("index fits u32"),
            active: false,
            source_index: None,
            name: format!("Config {index}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: Some(DistinctMembers::try_from(vec![body.clone()]).unwrap()),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        })
        .collect();
    ir.model.configurations[1].active = true;

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Unnamed,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let scan = crate::test_support::container::scan(&encoded);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-0-Partition") }));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-1-Partition") }));
    assert_eq!(container::active_configuration_index(&scan), Some(1));
    assert_eq!(
        container::select_active_parasolid_site(&scan)
            .unwrap()
            .name(),
        "Contents/Config-1-Partition"
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().geometry.bodies.len(), 2);
    assert_eq!(
        decoded.ir().model.configurations[0]
            .bodies
            .as_deref()
            .map_or(0, <[_]>::len),
        1
    );
    assert_eq!(
        decoded.ir().model.configurations[1]
            .bodies
            .as_deref()
            .map_or(0, <[_]>::len),
        1
    );
    assert!(decoded.ir().model.configurations[1].active);
    assert_ne!(
        decoded.ir().model.configurations[0].bodies,
        decoded.ir().model.configurations[1].bodies
    );
    let mesh_x = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .flat_map(|mesh| mesh.vertices().into_iter().map(|point| point.x))
        .collect::<Vec<_>>();
    assert!(mesh_x.iter().any(|value| (*value - 10.0).abs() < 1.0e-6));
    assert!(mesh_x.iter().any(|value| (*value - 20.0).abs() < 1.0e-6));
}

#[test]
fn semantic_writer_remaps_partition_without_remapping_resolved_features() {
    let mut source = outer_header();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks><swModel swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    source.extend(make_block(
        0x20,
        "Contents/Config-3-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-3-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    assert_eq!(decoded.ir().model.configurations[0].source_index, Some(3));
    assert!(decoded.ir().model.configurations[0].active);

    decoded.ir_mut().model.configurations[0].source_index = Some(5);
    let mut written = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-5-Partition") }));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-3-ResolvedFeatures") }));
    assert_eq!(container::active_configuration_index(&scan), Some(5));
    assert_eq!(
        container::select_active_parasolid_site(&scan)
            .unwrap()
            .name(),
        "Contents/Config-5-Partition"
    );
    let stale = scan
        .blocks
        .iter()
        .filter_map(|block| block.section.name())
        .filter(|section| {
            *section == "Contents/Config-3-Partition"
                || *section == "Contents/Config-5-ResolvedFeatures"
        })
        .collect::<Vec<_>>();
    assert!(stale.is_empty(), "stale sections: {stale:?}");
    assert!(!scan.blocks.iter().any(|block| {
        block.section.name().is_some_and(|section| {
            section == "Contents/Config-3-Partition"
                || section == "Contents/Config-5-ResolvedFeatures"
        })
    }));
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir().model.configurations[0].source_index,
        Some(5)
    );
}

#[test]
fn semantic_writer_allocates_partition_index_without_remapping_resolved_features() {
    let mut source = outer_header();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks><swModel swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    source.extend(make_block(
        0x20,
        "Contents/Config-3-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-3-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.configurations[0].source_index = None;

    let mut written = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.name() == Some("Contents/Config-0-Partition")));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-3-ResolvedFeatures") }));
    assert!(!scan.blocks.iter().any(|block| {
        matches!(
            block.section.name(),
            Some(
                "Contents/ResolvedFeatures"
                    | "Contents/Config-3-Partition"
                    | "Contents/Config-0-ResolvedFeatures"
            )
        )
    }));
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir().model.configurations[0].source_index,
        Some(0)
    );
}

#[test]
fn semantic_writer_reassigns_a_repeated_configuration_source_index() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let mut duplicate = decoded.ir().model.configurations[0].clone();
    duplicate.id =
        cadmpeg_ir::features::ConfigurationId::mint(format!("{}-duplicate", duplicate.id))
            .expect("identity grammar");
    duplicate.ordinal += 1;
    duplicate
        .name
        .as_mut()
        .expect("resolved configuration name")
        .push_str(" Duplicate");
    duplicate.native_ref = None;
    decoded.ir_mut().model.configurations.push(duplicate);

    let report = SldprtCodec
        .plan(EncodeInput::new(
            decoded.ir(),
            Some(decoded.source_fidelity()),
        ))
        .unwrap()
        .report()
        .clone();
    assert!(report.losses.iter().any(|loss| {
        loss.code.local_code() == "writer.configuration-identity-assigned"
            && loss.message.contains("-duplicate source index 0 repeats")
    }));
}

#[test]
fn semantic_writer_names_empty_and_repeated_configuration_names() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.configurations[0]
        .name
        .as_mut()
        .expect("resolved configuration name")
        .clear();
    let assigned = |decoded: &EditableDecodeResult| {
        SldprtCodec
            .plan(EncodeInput::new(
                decoded.ir(),
                Some(decoded.source_fidelity()),
            ))
            .unwrap()
            .report()
            .losses
            .iter()
            .find(|loss| loss.code.local_code() == "writer.configuration-identity-assigned")
            .map(|loss| loss.message.clone())
            .unwrap()
    };
    assert!(assigned(&decoded).contains(r#"name "" written as "Configuration 1""#));

    decoded.ir_mut().model.configurations[0].name = Some("Default".to_string());
    let mut duplicate = decoded.ir().model.configurations[0].clone();
    duplicate.id =
        cadmpeg_ir::features::ConfigurationId::mint(format!("{}-duplicate", duplicate.id))
            .expect("identity grammar");
    duplicate.ordinal += 1;
    duplicate.source_index = None;
    duplicate.native_ref = None;
    duplicate.active = false;
    decoded.ir_mut().model.configurations.push(duplicate);
    assert!(assigned(&decoded).contains(r#"name "Default" written as "Default (2)""#));
}

#[test]
fn encoder_writes_authored_with_retained_native_envelope_neutral_parameters() {
    use cadmpeg_ir::features::parameters::DesignParameter;
    use cadmpeg_ir::features::ParameterId;
    use std::collections::BTreeMap;

    let mut ir = unnamed_cube();
    let feature_id =
        FeatureId::mint("sldprt:model:feature#generated:equation").expect("identity grammar");
    ir.model.features.push(Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("Equation".into()),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("EquationSet"),
            "Global".into(),
        )]),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: "EquationDriven".into(),
                parameters: BTreeMap::from([(
                    cadmpeg_core::nonblank_literal!("Pitch"),
                    "D1@Sketch1 * 2".into(),
                )]),
            },
        )),
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: ParameterId::mint("sldprt:model:parameter#generated:equation:0")
            .expect("identity grammar"),
        owner: Some(feature_id),
        ordinal: 0,
        name: "Pitch".into(),
        expression: "D1@Sketch1 * 2".into(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Unnamed,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.parameters.len(), 1);
    assert_eq!(
        decoded.ir().model.parameters[0].expression,
        "D1@Sketch1 * 2"
    );
    assert_eq!(
        decoded.ir().model.features[0]
            .source_properties
            .get("EquationSet")
            .map(String::as_str),
        Some("Global")
    );
}

#[test]
fn encoder_bakes_rigid_body_transform() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::transform::Transform;

    let mut ir = unnamed_cube();
    let original_point = ir.geometry.points[0].position().get();
    let original_normal = ir
        .geometry
        .surfaces
        .iter()
        .find_map(|surface| match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))
                if {
                    let normal = *plane_surface.frame().axis().as_raw();
                    normal.x == 1.0
                } =>
            {
                let normal = *plane_surface.frame().axis().as_raw();
                Some(normal)
            }
            _ => None,
        })
        .unwrap();
    ir.geometry.bodies[0].transform = Some(
        Transform::affine([
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
        ])
        .expect("affine transform"),
    );
    let expected_point = Point3::new(
        -original_point.y + 10.0,
        original_point.x + 20.0,
        original_point.z + 30.0,
    );
    let expected_normal = Vector3::new(-original_normal.y, original_normal.x, original_normal.z);

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Unnamed,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.ir().geometry.points.iter().any(|point| {
        (point.position().get().x - expected_point.x).abs() < 1.0e-9
            && (point.position().get().y - expected_point.y).abs() < 1.0e-9
            && (point.position().get().z - expected_point.z).abs() < 1.0e-9
    }));
    assert!(decoded.ir().geometry.surfaces.iter().any(|surface| {
        matches!(surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))
        if {
            let normal = plane_surface.frame().axis().as_raw();
            *normal == expected_normal
        })
    }));
    assert!(decoded
        .ir()
        .geometry
        .bodies
        .iter()
        .all(|body| body.transform.is_none()));
}

#[test]
fn semantic_writer_regenerates_modified_planar_brep() {
    let source = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(source);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let mut result = EditableDecodeResult::from(result);
    let moved = result.ir_mut().geometry.points[0].position().get();
    result.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x + 1.0,
            moved.y,
            moved.z,
        ))
        .expect("a finite position is a point"),
    );
    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        result.ir(),
        result.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let mut regenerated = Cursor::new(encoded);
    let decoded = SldprtCodec
        .decode(&mut regenerated, &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir()
        .geometry
        .points
        .iter()
        .any(|point| point.position().get().x == 1.0));
}

#[test]
fn semantic_writer_uses_schema_specific_face_families() {
    let solid = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut solid = EditableDecodeResult::from(solid);
    let moved = solid.ir_mut().geometry.points[0].position().get();
    solid.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let mut solid_bytes = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        solid.ir(),
        solid.source_fidelity(),
        &mut solid_bytes,
    )
    .unwrap();
    let solid_scan = crate::test_support::container::scan(&solid_bytes);
    let solid_payload = &solid_scan.blocks[0].payload;
    assert!(count_entity51_family(solid_payload, 2, 0x0013) >= 1);
    assert!(count_entity51_family(solid_payload, 1, 0x0015) >= 1);

    let sheet_body = owned_triangle_with_kind(0, 701, 0.0, 3);
    let sheet = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&sheet_body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut sheet = EditableDecodeResult::from(sheet);
    let moved = sheet.ir_mut().geometry.points[0].position().get();
    sheet.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let mut sheet_bytes = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        sheet.ir(),
        sheet.source_fidelity(),
        &mut sheet_bytes,
    )
    .unwrap();
    let sheet_scan = crate::test_support::container::scan(&sheet_bytes);
    let sheet_payload = &sheet_scan.blocks[0].payload;
    assert!(count_entity51_family(sheet_payload, 2, 0x0015) >= 1);
    assert!(count_entity51_family(sheet_payload, 1, 0x001f) >= 1);
}

#[test]
fn semantic_writer_preserves_outer_header() {
    let mut source = sldprt_with_body(&triangle_body());
    source[..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    source[4..8].copy_from_slice(&7u32.to_be_bytes());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().geometry.points[0].position().get();
    decoded.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();

    assert_eq!(
        u32::from_le_bytes(encoded[..4].try_into().unwrap()),
        0x1234_5678
    );
    assert_eq!(u32::from_be_bytes(encoded[4..8].try_into().unwrap()), 7);
}

#[test]
fn semantic_writer_regenerates_modified_analytic_breps() {
    for body in [closed_cylinder_body(), sphere_patch_body()] {
        let source = sldprt_with_body(&body);
        let mut cur = Cursor::new(source);
        let result = SldprtCodec
            .decode(&mut cur, &DecodeOptions::default())
            .unwrap();
        let mut result = EditableDecodeResult::from(result);
        translate_model_x(&mut result.ir_mut(), 1.0);

        let mut encoded = Vec::new();
        crate::test_support::plan_fixture_write_with_native_envelope(
            result.ir(),
            result.source_fidelity(),
            &mut encoded,
        )
        .unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();

        assert_eq!(
            decoded.ir().geometry.faces.len(),
            result.ir().geometry.faces.len()
        );
        assert_eq!(
            decoded.ir().geometry.curves.len(),
            result.ir().geometry.curves.len()
        );
        assert_eq!(
            decoded
                .ir()
                .geometry
                .surfaces
                .iter()
                .map(|surface| &surface.geometry)
                .collect::<Vec<_>>(),
            result
                .ir()
                .geometry
                .surfaces
                .iter()
                .map(|surface| &surface.geometry)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn semantic_writer_preserves_sheet_body_classification() {
    let body = owned_triangle_with_kind(0, 701, 0.0, 3);
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().geometry.points[0].position().get();
    decoded.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir().geometry.bodies.len(), 1);
    assert_eq!(
        regenerated.ir().geometry.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(regenerated.ir().geometry.faces.len(), 1);
    assert_eq!(
        regenerated
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("parasolid_schema"))
            .map(String::as_str),
        Some("SCH_SW_32001_11000")
    );
}

#[test]
fn semantic_writer_omits_a_body_whose_face_names_a_missing_surface() {
    let mut ir = crate::test_support::ir::source_less_cube();
    ir.geometry.faces[0].surface =
        cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#missing").expect("identity grammar");
    let report = SldprtCodec
        .plan(EncodeInput::new(&ir, None))
        .unwrap()
        .report()
        .clone();
    assert!(report.losses.iter().any(|loss| {
        loss.code.local_code() == "writer.bodies-omitted"
            && loss
                .message
                .contains("missing surface test:model:entity#missing")
    }));
}

#[test]
pub(crate) fn semantic_writer_omits_subds_and_names_them() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let cage = cadmpeg_test_support::cages::push_cage_body(&mut decoded.ir_mut(), "sldprt");

    let mut bytes = Vec::new();
    let report = SldprtCodec
        .plan(EncodeInput::new(
            decoded.ir(),
            Some(decoded.source_fidelity()),
        ))
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.code.local_code() == "writer.subd-omitted" && loss.message.contains(cage.subd.as_str())
    }));
    cadmpeg_test_support::cages::assert_cage_body_dropped(&report, &cage, "bodies", 1);
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(written.ir().geometry.bodies.len(), 1);
}

#[test]
fn semantic_writer_rejects_unsupported_conic_curves() {
    let axis = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let major_direction = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    for geometry in [
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
            cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis,
                major_direction,
                1.0,
            )
            .unwrap(),
        )),
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
            cadmpeg_ir::geometry::analytic::HyperbolaCurve::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis,
                major_direction,
                2.0,
                1.0,
            )
            .unwrap(),
        )),
    ] {
        assert!(matches!(
            crate::writer::curve_values(&geometry, 0.001),
            Err(cadmpeg_core::CodecError::NotImplemented(_))
        ));
    }
}

#[test]
fn semantic_writer_omits_bodies_on_cones_compact_records_cannot_hold() {
    let cube = crate::test_support::ir::source_less_cube();
    let origin = cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0);
    let axis = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let reference = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    let cases = [
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    origin,
                    axis,
                    reference,
                    2.0,
                    0.5,
                    std::f64::consts::FRAC_PI_4,
                )
                .unwrap(),
            )),
            "elliptical cone ratio 0.5",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    origin,
                    axis,
                    reference,
                    2.0,
                    1.0,
                    -std::f64::consts::FRAC_PI_4,
                )
                .unwrap(),
            )),
            "cone half-angle -0.7853981633974483",
        ),
    ];

    for (geometry, expected) in cases {
        let mut ir = cube.clone();
        let surface_id = ir.geometry.surfaces[0].id.as_str().to_owned();
        ir.geometry.surfaces[0].geometry = geometry;

        let face = ir
            .geometry
            .faces
            .iter()
            .find(|face| face.surface.as_str() == surface_id)
            .unwrap()
            .id
            .as_str()
            .to_owned();
        let report = SldprtCodec
            .plan(EncodeInput::new(&ir, None))
            .unwrap()
            .report()
            .clone();
        assert!(
            report.losses.iter().any(|loss| {
                loss.code.local_code() == "writer.bodies-omitted"
                    && loss.message.contains(&format!("face {face} ({expected})"))
            }),
            "{:#?}",
            report.losses
        );
    }
}

#[test]
fn semantic_writer_converts_millimetres_to_native_metres() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().geometry.points[0].position().get();
    decoded.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(50.8, moved.y, moved.z))
            .expect("a finite position is a point"),
    );

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated
        .ir()
        .geometry
        .points
        .iter()
        .any(|point| (point.position().get().x - 50.8).abs() < 1e-5));
}

#[test]
fn semantic_writer_preserves_multiple_body_ownership() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 0.0));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().geometry.points[0].position().get();
    decoded.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap(),
    );

    assert_eq!(regenerated.ir().geometry.bodies.len(), 2);
    assert_eq!(regenerated.ir().geometry.regions.len(), 2);
    assert_eq!(regenerated.ir().geometry.shells.len(), 2);
    assert!(regenerated
        .ir()
        .geometry
        .shells
        .iter()
        .all(|shell| regenerated
            .ir()
            .geometry
            .stated_shell_faces(&shell.id)
            .count()
            == 1));
    assert!(regenerated.ir().geometry.regions.iter().all(|region| {
        regenerated.source_fidelity().annotations.provenance[region.id.as_str()]
            .tag
            .as_deref()
            == Some("00_51_region")
    }));
    assert!(regenerated.ir().geometry.shells.iter().all(|shell| {
        regenerated.source_fidelity().annotations.provenance[shell.id.as_str()]
            .tag
            .as_deref()
            == Some("00_51_shell")
    }));
}

#[test]
fn semantic_writer_regenerates_modified_nurbs_carriers() {
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };

    let body = crate::test_support::parasolid::nurbs_triangle_body();
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let (expected_curve, expected_surface) = {
        let mut ir_edit = decoded.ir_mut();
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) =
            &mut ir_edit.geometry.curves[0].geometry
        else {
            panic!("expected NURBS curve");
        };
        curve
            .try_map_control_points(|index, point| {
                let mut point = point.get();
                if index == 1 {
                    point.y += 250.0;
                }
                FinitePoint3::new(point).ok_or_else(|| {
                    cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                        "control_points contains a non-finite point".into(),
                    )
                })
            })
            .unwrap();
        let expected_curve = curve.clone();
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &mut ir_edit.geometry.surfaces[0].geometry
        else {
            panic!("expected NURBS surface");
        };
        let target = surface.v_count() + 1;
        surface
            .try_map_control_points(|index, pole| {
                let mut pole = pole.get();
                if index == target {
                    pole.z += 500.0;
                }
                FinitePoint3::new(pole).ok_or_else(|| {
                    cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                        "control_points contains a non-finite point".into(),
                    )
                })
            })
            .unwrap();
        let expected_surface = surface.clone();
        (expected_curve, expected_surface)
    };

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated.ir().geometry.curves.iter().any(
        |curve| matches!(&curve.geometry, CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(value)) if value == &expected_curve)
    ), "curves: {:?}; expected: {expected_curve:?}; losses: {:?}", regenerated.ir().geometry.curves, regenerated.report().losses);
    assert!(regenerated.ir().geometry.surfaces.iter().any(
        |surface| matches!(&surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(value)) if value == &expected_surface)
    ));
}

#[test]
fn semantic_writer_preserves_unbound_material_definition() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_material(
                &triangle_body(),
                "Steel",
                [32, 64, 128],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().geometry.points[0].position().get();
    decoded.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );
    let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated.ir().geometry.bodies[0].name.is_none());
    assert!(regenerated.ir().geometry.bodies[0].color.is_none());
    let appearance = regenerated
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.name.as_deref() == Some("Steel"))
        .unwrap();
    let color = appearance.base_color.unwrap();
    assert!((color.r() - 32.0 / 255.0).abs() < EPS_COLOR_COMPONENT);
    assert!((color.g() - 64.0 / 255.0).abs() < EPS_COLOR_COMPONENT);
    assert!((color.b() - 128.0 / 255.0).abs() < EPS_COLOR_COMPONENT);
}

#[test]
fn semantic_writer_keeps_retained_materials_when_a_name_outgrows_its_field() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_material(
                &triangle_body(),
                "Steel",
                [32, 64, 128],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.appearances[0].name = Some("M".repeat(256));
    decoded.ir_mut().geometry.bodies[0].name = Some("M".repeat(256));
    decoded.ir_mut().geometry.bodies[0].color = Some(
        cadmpeg_ir::topology::Color::new(32.0 / 255.0, 64.0 / 255.0, 128.0 / 255.0, 1.0)
            .expect("valid color"),
    );
    let (_, losses) = crate::test_support::write_with_unwritten_attributes(
        decoded.ir(),
        decoded.source_fidelity(),
    )
    .unwrap();
    assert!(
        losses
            .dropped
            .iter()
            .any(|message| message.contains("keeps the retained SWObjects materials")),
        "{losses:?}"
    );
}

#[test]
fn semantic_writer_preserves_face_appearance() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut body = Vec::new();
    body.extend(face_color_definition());
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(
        1,
        700,
        FACE_COLOR_DEFINITION_ID,
        &[0, 0, 0, 0, 0, 900],
    ));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(owned_triangle(0, 700, 0.0));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().geometry.points[0].position().get();
    decoded.ir_mut().geometry.points[0].set_position(
        FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let binding = regenerated
        .ir()
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(&binding.target.entity, AppearanceTarget::Face(_)))
        .expect("face binding");
    let color = regenerated
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .and_then(|appearance| appearance.base_color)
        .unwrap();
    assert_eq!([color.r(), color.g(), color.b()], [0.25, 0.5, 0.75]);
}

#[test]
fn semantic_writer_derives_resolved_feature_section_names() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.source_fidelity_mut().annotations = cadmpeg_ir::Annotations::default();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].sketch_entities[0]
            .reclassify(crate::records::SketchInputKind::from_native_code(9));
    });

    let mut written = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-0-ResolvedFeatures") }));

    let (mut unscoped, _, fidelity) = decoded.into_parts();
    update_sldprt_native(&mut unscoped, |native| {
        native.feature_input_lanes[0].configuration = None;
    });
    let mut written = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        &unscoped,
        &fidelity,
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.name() == Some("Contents/ResolvedFeatures")));
}

#[test]
fn semantic_writer_preserves_idless_feature_tree_nodes() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Root" Type="Folder" id="1"><Folder Name="Group"><Sketch Name="Profile" Type="Sketch" id="2"/></Folder></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                &(source),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let native = &sldprt_native(decoded.ir()).feature_histories[0].features;
    assert_eq!(
        native[1].tree_parent_record_id(),
        Some(native[0].id.as_str())
    );
    assert_eq!(
        native[2].tree_parent_record_id(),
        Some(native[1].id.as_str())
    );
    assert_eq!(
        decoded
            .ir()
            .model
            .feature_parent(&decoded.ir().model.features[1].id),
        Some(&decoded.ir().model.features[0].id)
    );
    assert_eq!(
        decoded
            .ir()
            .model
            .feature_parent(&decoded.ir().model.features[2].id),
        Some(&decoded.ir().model.features[1].id)
    );
    decoded.ir_mut().model.features[2].name = Some("Edited Profile".into());

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native.len(), 3);
    assert_eq!(native[0].xml_tag, "Feature");
    assert_eq!(native[1].xml_tag, "Folder");
    assert_eq!(native[2].xml_tag, "Sketch");
    assert_eq!(
        native[1].tree_parent_record_id(),
        Some(native[0].id.as_str())
    );
    assert_eq!(
        native[2].tree_parent_record_id(),
        Some(native[1].id.as_str())
    );
    assert_eq!(native[2].name, "Edited Profile");
}

#[test]
fn semantic_writer_applies_neutral_configuration_edits() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                &(sldprt_with_body_and_history(&triangle_body())),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let configuration = &mut ir_edit.model.configurations[0];
        configuration.name = Some("Machined".to_string());
        configuration.material = Some("Aluminum".into());
        configuration
            .properties
            .insert(cadmpeg_core::nonblank_literal!("Finish"), "Anodized".into());
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_write_with_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    let configuration = &native.feature_histories[0].configurations[0];
    assert_eq!(configuration.name, "Machined");
    assert_eq!(configuration.material.as_deref(), Some("Aluminum"));
    assert_eq!(configuration.properties["Finish"], "Anodized");
    assert_eq!(
        regenerated.ir().model.configurations[0].name.as_deref(),
        Some("Machined")
    );
}

#[test]
fn semantic_writer_patch_declines_conflicting_configuration_edits() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.ir_mut().model.configurations[0].name = Some("Neutral".to_string());
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].configurations[0].name = "Native".to_string();
    });

    let error =
        crate::test_support::plan_inherited_write_declined(decoded.ir(), decoded.source_fidelity());
    assert!(error.contains("conflicting neutral and native SLDPRT configuration edits"));
}

#[test]
fn semantic_writer_applies_neutral_parameter_edits() {
    use cadmpeg_ir::scalar::Length;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Depth")
            .unwrap();
        parameter.expression = "20mm".into();
        parameter.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
    }

    let mut encoded = Vec::new();
    crate::test_support::write_history_over_stale_solid(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].parameters["Depth"],
        "20mm"
    );
    assert_eq!(
        regenerated
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Depth")
            .unwrap()
            .expression,
        "20mm"
    );
}

#[test]
fn semantic_writer_preserves_dimension_attributes() {
    use cadmpeg_ir::scalar::Length;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth" Driven="true" EquationId="D1@Boss">12mm</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = &mut ir_edit.model.parameters[0];
        assert_eq!(parameter.properties["Driven"], "true");
        assert_eq!(parameter.properties["EquationId"], "D1@Boss");
        parameter.expression = "20mm".into();
        parameter.value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
    }

    let mut encoded = Vec::new();
    crate::test_support::write_history_over_stale_solid(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Depth"], "20mm");
    assert_eq!(feature.dimension_properties["Depth"]["Driven"], "true");
    assert_eq!(
        feature.dimension_properties["Depth"]["EquationId"],
        "D1@Boss"
    );
}

#[test]
fn semantic_writer_preserves_evaluated_equation_values() {
    use cadmpeg_ir::scalar::Length;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth" Value="24mm" EquationId="D1@Boss">Width * 2</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = &mut ir_edit.model.parameters[0];
        assert_eq!(parameter.expression, "Width * 2");
        assert_eq!(
            parameter.value,
            Some(ParameterValue::Length(Length::new(24.0).unwrap()))
        );
        assert_eq!(parameter.properties["Value"], "24mm");
        parameter.expression = "Width * 3".into();
        parameter.value = Some(ParameterValue::Length(Length::new(36.0).unwrap()));
    }

    let mut encoded = Vec::new();
    crate::test_support::write_history_over_stale_solid(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = &regenerated.ir().model.parameters[0];
    assert_eq!(parameter.expression, "Width * 3");
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(36.0).unwrap()))
    );
    assert_eq!(parameter.properties["Value"], "36mm");
    assert_eq!(parameter.properties["EquationId"], "D1@Boss");
}

#[test]
fn active_metadata_edit_does_not_patch_only_one_field_of_a_structured_manifest() {
    for payload in [
        br#"<swSolidWorks><swModel id="m" swConfigurationName="Old" swConfigurationId="0"/><swConfiguration swName="Old" swID="0" swModelRef="m" swMostRecentConfiguration="YES" swConfigurationNeedsUpdate="NO"/></swSolidWorks>"#.as_slice(),
        br#"<swSolidWorks><swModel swConfigurationName="First"/><swModel swConfigurationName="Second"/></swSolidWorks>"#.as_slice(),
    ] {
        assert!(
            with_ctx(|ctx| super::super::patch_active_configuration_xml(ctx, payload, "New"))
                .unwrap()
                .is_err()
        );
    }
}

/// The stale mark sets `swConfigurationNeedsUpdate="YES"` on each named
/// configuration row (§3.4, States) and keeps the other rows and the stored
/// encoding.
#[test]
fn the_stale_mark_sets_needs_update_on_the_named_configuration_rows() {
    let source = r#"<swSolidWorks><swModel swConfigurationName="A"/><swConfiguration swName="A" swConfigurationNeedsUpdate="NO"/><swConfiguration swName="B" swID="1"/><swConfiguration swName="C" swConfigurationNeedsUpdate="NO"/></swSolidWorks>"#;
    let expected = r#"<swSolidWorks><swModel swConfigurationName="A"/><swConfiguration swName="A" swConfigurationNeedsUpdate="YES"/><swConfiguration swName="B" swConfigurationNeedsUpdate="YES" swID="1"/><swConfiguration swName="C" swConfigurationNeedsUpdate="NO"/></swSolidWorks>"#;
    let names = std::collections::BTreeSet::from(["A".to_owned(), "B".to_owned()]);
    let utf16 = |text: &str| {
        let mut bytes = vec![0x85, 0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    };
    for (payload, written) in [
        (source.as_bytes().to_vec(), expected.as_bytes().to_vec()),
        (utf16(source), utf16(expected)),
    ] {
        let (marked, rows) = with_ctx(|ctx| crate::writer::stale::mark(ctx, &payload, &names))
            .unwrap()
            .unwrap();
        assert_eq!(rows, ["A", "B"]);
        assert_eq!(marked, written);
    }
    assert!(
        with_ctx(|ctx| crate::writer::stale::mark(ctx, b"<swSolidWorks/>", &names))
            .unwrap()
            .is_none()
    );
}

/// Runs `run` in a decode session under the default policy.
fn with_ctx<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    run(&cadmpeg_core::decode::DecodeContext::new(
        &arena,
        &policy,
        cadmpeg_core::decode::DecodeDepth::Full,
    ))
}
