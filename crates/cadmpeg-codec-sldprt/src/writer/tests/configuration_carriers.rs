// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::EditableDecodeResult;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
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

const EPS_COLOR_COMPONENT: f32 = 1.0e-6;

#[test]
fn geometry_edit_replaces_each_generated_configuration_partition() {
    use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};
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
    ir.model.bodies.iter_mut().for_each(|body| body.name = None);
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    let body = ir.model.bodies[0].id.clone();
    ir.model.configurations = (0..2)
        .map(|index| DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:probe:configuration#{index}")).unwrap(),
            ordinal: index,
            active: index == 1,
            source_index: Some(index),
            name: Some(format!("Config {index}")),
            material: None,
            properties: BTreeMap::new(),
            bodies: Some(
                cadmpeg_ir::features::DistinctMembers::try_from(vec![body.clone()]).unwrap(),
            ),
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        })
        .collect();
    let mut source = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut source))
    .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let point = decoded.ir().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
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

    assert!(container::first_solidworks_envelope([payload.as_slice()]).is_some());
    let patched = super::super::patch_active_configuration_xml(&payload, "New", true, 7)
        .unwrap()
        .expect("the UTF-16 envelope is recognized");
    let envelope = container::first_solidworks_envelope([patched.as_slice()])
        .expect("the rewritten envelope remains recognizable");
    assert_eq!(envelope.sw_version.as_deref(), Some("34000"));
    assert_eq!(envelope.configuration_name.as_deref(), Some("New"));
    assert_eq!(
        envelope
            .configuration_attributes
            .get("sw_configuration_7_needs_update")
            .map(String::as_str),
        Some("YES")
    );
}

#[test]
fn encoder_writes_authored_with_retained_native_envelope_datum_features() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    let definitions = [
        FeatureDefinition::Operation(FeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        }),
        FeatureDefinition::Operation(FeatureOperation::DatumAxis {
            origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).unwrap(),
            direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0))
                .unwrap(),
        }),
        FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(7.0, 8.0, 9.0)).unwrap(),
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
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            needs_recompute: false,
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
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
    use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("sldprt:model:configuration#generated:z")
            .expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: None,
        name: Some("Metric".to_string()),
        material: Some("Steel".into()),
        properties: BTreeMap::from([(cadmpeg_core::nonblank_literal!("Finish"), "Ground".into())]),
        bodies: Some(
            cadmpeg_ir::features::DistinctMembers::try_from(vec![ir.model.bodies[0].id.clone()])
                .unwrap(),
        ),
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
        bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.finalize();

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let scan = crate::test_support::container::scan(&encoded);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-0-Partition") }));
    assert!(!scan
        .blocks
        .iter()
        .any(|block| { block.section.name() == Some("Contents/Config-1-Partition") }));
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
                .model
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>()
                .as_slice()
        )
    );
    assert!(decoded.ir().model.configurations[1].bodies.is_none());

    let (mut inactive, _, fidelity) = decoded.into_parts();
    inactive
        .model
        .configurations
        .iter_mut()
        .for_each(|configuration| configuration.active = false);
    // D13 (#237): with no active configuration the synthesis base acts
    // instead of refusing. The fidelity path still declines, so the plan
    // falls back to synthesis with the base selected as active.
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(&inactive, &fidelity, &mut encoded)
        .expect("a configuration-less document synthesizes with its base active");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("synthesized base decodes");
    assert_eq!(
        regenerated
            .ir()
            .model
            .configurations
            .iter()
            .filter(|configuration| configuration.active)
            .count(),
        1,
        "synthesis selects exactly one active configuration"
    );
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
    use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};
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
    ir.model.bodies.iter_mut().for_each(|body| body.name = None);
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    for (index, body) in ir.model.bodies.iter_mut().enumerate() {
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
            .with_body(Some(body.clone()))
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
            bodies: Some(
                cadmpeg_ir::features::DistinctMembers::try_from(vec![body.clone()]).unwrap(),
            ),
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
        TargetRequest::Inherit,
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
    assert_eq!(decoded.ir().model.bodies.len(), 2);
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
fn semantic_writer_repairs_duplicate_configuration_source_indices() {
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
    let duplicate_id = duplicate.id.clone();
    decoded.ir_mut().model.configurations.push(duplicate);

    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    for field in ["source index", "active state"] {
        assert!(plan.report().losses.iter().any(|loss| {
            loss.message.contains(duplicate_id.as_str()) && loss.message.contains(field)
        }));
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let configurations = &output.ir().model.configurations;
    assert_eq!(configurations.len(), 2);
    assert_eq!(
        configurations
            .iter()
            .filter(|configuration| configuration.active)
            .count(),
        1
    );
    assert_eq!(
        configurations
            .iter()
            .filter_map(|configuration| configuration.source_index)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        2
    );
}

#[test]
fn semantic_writer_repairs_empty_and_duplicate_configuration_names() {
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
    // #237 item 1, #217 §5.0: a patch failure selects synthesis, which
    // writes the configuration under its identity and names it in a loss.
    let configuration = decoded.ir().model.configurations[0].id.to_string();
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    assert!(matches!(
        plan.report().synthesis_cause(),
        Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { reason })
            if reason.contains("empty name")
    ));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&configuration)));

    decoded.ir_mut().model.configurations[0].name = Some("Default".to_string());
    let mut duplicate = decoded.ir().model.configurations[0].clone();
    duplicate.id =
        cadmpeg_ir::features::ConfigurationId::mint(format!("{}-duplicate", duplicate.id))
            .expect("identity grammar");
    duplicate.ordinal += 1;
    duplicate.source_index = None;
    duplicate.native_ref = None;
    duplicate.active = false;
    let duplicate_id = duplicate.id.clone();
    decoded.ir_mut().model.configurations.push(duplicate);
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(plan.report().losses.iter().any(|loss| {
        loss.message.contains(duplicate_id.as_str()) && loss.message.contains("name")
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let names = output
        .ir()
        .model
        .configurations
        .iter()
        .filter_map(|configuration| configuration.name.as_deref())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(names.len(), 2);
    assert!(names.contains("Default"));
}

#[test]
fn encoder_writes_authored_with_retained_native_envelope_neutral_parameters() {
    use cadmpeg_ir::features::{
        DesignParameter, Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
    };
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    let feature_id =
        FeatureId::mint("sldprt:model:feature#generated:equation").expect("identity grammar");
    ir.model.features.push(Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("Equation".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("EquationSet"),
            "Global".into(),
        )]),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "EquationDriven".into(),
                parameters: BTreeMap::from([(
                    cadmpeg_core::nonblank_literal!("Pitch"),
                    cadmpeg_ir::features::Expression::from("D1@Sketch1 * 2").into(),
                )]),
            }),
        ),
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        driving: true,
        id: ParameterId::mint("sldprt:model:parameter#generated:equation:0")
            .expect("identity grammar"),
        owner: Some(feature_id),
        ordinal: 0,
        name: "Pitch".into(),
        expression: cadmpeg_ir::features::Expression::from("D1@Sketch1 * 2"),
        display: None,
        value: None,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });

    let mut encoded = Vec::new();
    crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .and_then(|plan| plan.write_to(&mut encoded))
    .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.parameters.len(), 1);
    assert_eq!(
        decoded.ir().model.parameters[0].expression,
        "D1@Sketch1 * 2".into()
    );
    let namespace = decoded
        .ir()
        .native
        .namespace("sldprt")
        .expect("native namespace");
    let native = crate::native::SldprtNative::load(namespace).expect("typed native");
    let records = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features);
    assert!(records
        .filter_map(|record| record.properties.get("EquationSet"))
        .any(|value| value.as_str() == "Global"));
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
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
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

    assert!(regenerated.ir().model.bodies[0].name.is_none());
    assert!(regenerated.ir().model.bodies[0].color.is_none());
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
fn semantic_writer_rejects_overlong_material_names() {
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
    decoded.ir_mut().model.bodies[0].name = Some("M".repeat(256));
    decoded.ir_mut().model.bodies[0].color = Some(
        cadmpeg_ir::topology::Color::new(32.0 / 255.0, 64.0 / 255.0, 128.0 / 255.0, 1.0)
            .expect("valid color"),
    );
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        matches!(&error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity { field, limit: 255, value: 256 } if field.field == "material.name")
    );
    assert_eq!(
        error.to_string(),
        "target capacity exceeded for material.name: limit 255, value 256"
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
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
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
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
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
fn untyped_feature_framing_is_not_written_as_content() {
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

    let (ir, _, fidelity) = decoded.into_parts();
    for scoped in [true, false] {
        let mut document = ir.clone();
        if !scoped {
            update_sldprt_native(&mut document, |native| {
                native.feature_input_lanes[0].configuration = None;
            });
        }
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(&document, Some(&fidelity)),
                TargetRequest::Inherit,
            )
            .unwrap();
        assert!(matches!(
            plan.report().write_path(),
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        ));
        assert!(plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.namespace() == "export"
                && loss.code.local_code() == "native.records-omitted"));
        let mut written = Vec::new();
        plan.write_to(&mut written).unwrap();
        let output = SldprtCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(output.ir());
        assert!(native
            .feature_input_lanes
            .iter()
            .flat_map(|lane| &lane.classes)
            .any(|class| class.name == "sgPointHandle"));
        assert!(native
            .feature_input_lanes
            .iter()
            .all(|lane| lane.sketch_entities.is_empty()));
        let synthesized =
            crate::test_support::assert_source_free_synthesis_agrees(&document, output.ir());
        let native = sldprt_native(synthesized.ir());
        assert!(native
            .feature_input_lanes
            .iter()
            .flat_map(|lane| &lane.classes)
            .any(|class| class.name == "sgPointHandle"));
        assert!(native
            .feature_input_lanes
            .iter()
            .all(|lane| lane.sketch_entities.is_empty()));
    }
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
fn neutral_configuration_edits_override_native_edits() {
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

    let mut output = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut output)
        .unwrap();
    let written = SldprtCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        written.ir().model.configurations[0].name.as_deref(),
        Some("Neutral")
    );
}

#[test]
fn semantic_writer_applies_neutral_parameter_edits() {
    use cadmpeg_ir::{features::ParameterValue, scalar::Length};

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
    crate::test_support::serialize_edited_history(
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
        "20mm".into()
    );
}

mod parameter_attributes;

mod section_names;

pub(crate) mod geometry;
