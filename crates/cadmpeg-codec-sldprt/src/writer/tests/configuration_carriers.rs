// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn encoder_writes_source_less_datum_features() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let definitions = [
        FeatureDefinition::DatumPlane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        FeatureDefinition::DatumAxis {
            origin: Point3::new(4.0, 5.0, 6.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        },
        FeatureDefinition::DatumPoint {
            position: Point3::new(7.0, 8.0, 9.0),
            construction: None,
        },
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#datum-{ordinal}")),
            ordinal: ordinal as u64,
            name: Some(format!("Datum {ordinal}")),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumPlane { .. }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::DatumAxis { .. }
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::DatumPoint { .. }
    ));
}

#[test]
fn encoder_writes_source_less_neutral_configurations() {
    use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("sldprt:model:configuration#generated:z".into()),
        ordinal: 0,
        active: true.into(),
        source_index: None,
        name: "Metric".into(),
        material: Some("Steel".into()),
        properties: BTreeMap::from([("Finish".into(), "Ground".into())]),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(vec![ir.model.bodies[0].id.clone()]),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("sldprt:model:configuration#generated:a".into()),
        ordinal: 1,
        active: false.into(),
        source_index: None,
        name: "Empty".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.finalize();

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let scan = container::scan_bytes(&encoded);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-0-Partition") }));
    assert!(!scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-1-Partition") }));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded
            .ir()
            .model
            .configurations
            .iter()
            .filter_map(|configuration| configuration.name.resolved())
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
    assert_eq!(configuration.name, "Metric");
    assert_eq!(configuration.material.as_deref(), Some("Steel"));
    assert_eq!(configuration.properties["Finish"], "Ground");
    assert!(configuration.active.is_active());
    assert_eq!(
        configuration.bodies,
        decoded
            .ir()
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
    assert!(decoded.ir().model.configurations[1].bodies.is_empty());

    let (mut inactive, _, fidelity) = decoded.into_parts();
    inactive
        .model
        .configurations
        .iter_mut()
        .for_each(|configuration| configuration.active = false.into());
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&inactive, &fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("requires exactly one active configuration"));
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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.configurations[0].active.is_active());
    assert!(decoded.ir().model.configurations[1].active.is_inactive());

    decoded.ir_mut().model.configurations[0].active = false.into();
    decoded.ir_mut().model.configurations[1].active = true.into();
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(regenerated.ir().model.configurations[0]
        .active
        .is_inactive());
    assert!(regenerated.ir().model.configurations[1].active.is_active());
    assert_eq!(
        regenerated.ir().source.as_ref().unwrap().attributes["sw_configuration_name"],
        "Manufacturing & QA"
    );
}

#[test]
fn encoder_partitions_source_less_bodies_by_configuration() {
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
        body.transform = Some(Transform {
            rows: [
                [1.0, 0.0, 0.0, (index as f64 + 1.0) * 10.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        });
    }
    ir.model.tessellations = body_ids
        .iter()
        .enumerate()
        .map(|(index, body)| Tessellation {
            id: format!("synthetic:test:tessellation#{index}"),
            body: Some(body.clone()),
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
            feature_edges: Vec::new(),
            strip_lengths: vec![3],
            normals: vec![Vector3::new(0.0, 0.0, 1.0); 3],
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: Vec::new(),
        })
        .collect();
    ir.model.configurations = body_ids
        .iter()
        .enumerate()
        .map(|(index, body)| DesignConfiguration {
            id: ConfigurationId(format!("synthetic:test:configuration#config-{index}")),
            ordinal: index as u32,
            active: false.into(),
            source_index: None,
            name: format!("Config {index}").into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(vec![body.clone()]),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        })
        .collect();
    ir.model.configurations[1].active = true.into();

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let scan = container::scan_bytes(&encoded);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-0-Partition") }));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-1-Partition") }));
    assert_eq!(container::active_configuration_index(&scan), Some(1));
    assert_eq!(
        container::select_active_parasolid(&scan)
            .unwrap()
            .0
            .section
            .as_deref(),
        Some("Contents/Config-1-Partition")
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.bodies.len(), 2);
    assert_eq!(decoded.ir().model.configurations[0].bodies.len(), 1);
    assert_eq!(decoded.ir().model.configurations[1].bodies.len(), 1);
    assert!(decoded.ir().model.configurations[1].active.is_active());
    assert_ne!(
        decoded.ir().model.configurations[0].bodies,
        decoded.ir().model.configurations[1].bodies
    );
    let mesh_x = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .flat_map(|mesh| mesh.vertices.iter().map(|point| point.x))
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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.configurations[0].source_index, Some(3));
    assert!(decoded.ir().model.configurations[0].active.is_active());

    decoded.ir_mut().model.configurations[0].source_index = Some(5);
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-5-Partition") }));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-3-ResolvedFeatures") }));
    assert_eq!(container::active_configuration_index(&scan), Some(5));
    assert_eq!(
        container::select_active_parasolid(&scan)
            .unwrap()
            .0
            .section
            .as_deref(),
        Some("Contents/Config-5-Partition")
    );
    let stale = scan
        .blocks
        .iter()
        .filter_map(|block| block.section.as_deref())
        .filter(|section| {
            *section == "Contents/Config-3-Partition"
                || *section == "Contents/Config-5-ResolvedFeatures"
        })
        .collect::<Vec<_>>();
    assert!(stale.is_empty(), "stale sections: {stale:?}");
    assert!(!scan.blocks.iter().any(|block| {
        block.section.as_deref().is_some_and(|section| {
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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded.ir_mut().model.configurations[0].source_index = None;

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.as_deref() == Some("Contents/Config-0-Partition")));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-3-ResolvedFeatures") }));
    assert!(!scan.blocks.iter().any(|block| {
        matches!(
            block.section.as_deref(),
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
fn semantic_writer_rejects_duplicate_configuration_source_indices() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut duplicate = decoded.ir().model.configurations[0].clone();
    duplicate.id.0.push_str("-duplicate");
    duplicate.ordinal += 1;
    duplicate
        .name
        .resolved_mut()
        .expect("resolved configuration name")
        .push_str(" Duplicate");
    duplicate.native_ref = None;
    decoded.ir_mut().model.configurations.push(duplicate);

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("repeats configuration source index"),
        "{error}"
    );
}

#[test]
fn semantic_writer_rejects_empty_and_duplicate_configuration_names() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.configurations[0]
        .name
        .resolved_mut()
        .expect("resolved configuration name")
        .clear();
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("empty name"), "{error}");

    decoded.ir_mut().model.configurations[0].name = "Default".into();
    let mut duplicate = decoded.ir().model.configurations[0].clone();
    duplicate.id.0.push_str("-duplicate");
    duplicate.ordinal += 1;
    duplicate.source_index = None;
    duplicate.native_ref = None;
    duplicate.active = false.into();
    decoded.ir_mut().model.configurations.push(duplicate);
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("repeats configuration name"),
        "{error}"
    );
}

#[test]
fn encoder_writes_source_less_neutral_parameters() {
    use cadmpeg_ir::features::{
        DesignParameter, Feature, FeatureDefinition, FeatureId, ParameterId,
    };
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let feature_id = FeatureId("sldprt:model:feature#generated:equation".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("Equation".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "EquationDriven".into(),
            parameters: BTreeMap::from([("Pitch".into(), "D1@Sketch1 * 2".into())]),
            properties: BTreeMap::from([("EquationSet".into(), "Global".into())]),
        },
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("sldprt:model:parameter#generated:equation:0".into()),
        owner: Some(feature_id),
        ordinal: 0,
        name: "Pitch".into(),
        expression: "D1@Sketch1 * 2".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
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
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Native { properties, .. }
            if properties.get("EquationSet").map(String::as_str) == Some("Global")
    ));
}

#[test]
fn encoder_bakes_rigid_body_transform() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::transform::Transform;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let original_point = ir.model.points[0].position;
    let original_normal = ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match surface.geometry {
            SurfaceGeometry::Plane { normal, .. } if normal.x == 1.0 => Some(normal),
            _ => None,
        })
        .unwrap();
    ir.model.bodies[0].transform = Some(Transform {
        rows: [
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    let expected_point = Point3::new(
        -original_point.y + 10.0,
        original_point.x + 20.0,
        original_point.z + 30.0,
    );
    let expected_normal = Vector3::new(-original_normal.y, original_normal.x, original_normal.z);

    let mut encoded = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.ir().model.points.iter().any(|point| {
        (point.position.x - expected_point.x).abs() < 1e-9
            && (point.position.y - expected_point.y).abs() < 1e-9
            && (point.position.z - expected_point.z).abs() < 1e-9
    }));
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        matches!(surface.geometry, SurfaceGeometry::Plane { normal, .. } if normal == expected_normal)
    }));
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.transform.is_none()));
}

#[test]
fn semantic_writer_regenerates_modified_planar_brep() {
    let source = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(source);
    let mut result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    result.ir_mut().model.points[0].position.x += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(result.ir(), result.source_fidelity(), &mut encoded)
        .unwrap();
    let mut regenerated = Cursor::new(encoded);
    let decoded = SldprtCodec
        .decode(&mut regenerated, &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.position.x == 1.0));
}

#[test]
fn semantic_writer_uses_schema_specific_face_families() {
    let mut solid = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    solid.ir_mut().model.points[0].position.z += 1.0;
    let mut solid_bytes = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(solid.ir(), solid.source_fidelity(), &mut solid_bytes)
        .unwrap();
    let solid_scan = container::scan_bytes(&solid_bytes);
    let solid_payload = &solid_scan.blocks[0].payload;
    assert!(count_entity51_family(solid_payload, 2, 0x0013) >= 1);
    assert!(count_entity51_family(solid_payload, 1, 0x0015) >= 1);

    let sheet_body = owned_triangle_with_kind(0, 701, 0.0, 3);
    let mut sheet = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&sheet_body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    sheet.ir_mut().model.points[0].position.z += 1.0;
    let mut sheet_bytes = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(sheet.ir(), sheet.source_fidelity(), &mut sheet_bytes)
        .unwrap();
    let sheet_scan = container::scan_bytes(&sheet_bytes);
    let sheet_payload = &sheet_scan.blocks[0].payload;
    assert!(count_entity51_family(sheet_payload, 2, 0x0015) >= 1);
    assert!(count_entity51_family(sheet_payload, 1, 0x001f) >= 1);
}

#[test]
fn semantic_writer_emits_typed_body_ownership_nodes() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = super::super::brep_body(decoded.ir(), 0.001, false).unwrap();
    let facts = crate::brep::typed::scan(&body);

    assert!(facts.has_valid_ownership());
    assert_eq!(facts.bodies.len(), 1);
    assert!(!facts.shells.is_empty());
    assert!(!facts.regions.is_empty());
    assert!(!facts.faces.is_empty());
}

#[test]
fn semantic_writer_preserves_outer_header() {
    let mut source = sldprt_with_body(&triangle_body());
    source[..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    source[4..8].copy_from_slice(&7u32.to_be_bytes());
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded.ir_mut().model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
        let mut result = SldprtCodec
            .decode(&mut cur, &DecodeOptions::default())
            .unwrap();
        translate_model_x(&mut result.ir_mut(), 1.0);

        let mut encoded = Vec::new();
        SldprtCodec
            .write_preserved_with_source_fidelity(
                result.ir(),
                result.source_fidelity(),
                &mut encoded,
            )
            .unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();

        assert_eq!(
            decoded.ir().model.faces.len(),
            result.ir().model.faces.len()
        );
        assert_eq!(
            decoded.ir().model.curves.len(),
            result.ir().model.curves.len()
        );
        assert_eq!(
            decoded
                .ir()
                .model
                .surfaces
                .iter()
                .map(|surface| &surface.geometry)
                .collect::<Vec<_>>(),
            result
                .ir()
                .model
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
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.points[0].position.z += 1.0;
    let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir().model.bodies.len(), 1);
    assert_eq!(
        regenerated.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(regenerated.ir().model.faces.len(), 1);
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
fn semantic_writer_rejects_invalid_ir_without_panicking() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.faces[0].surface = cadmpeg_ir::ids::SurfaceId("missing".into());
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn semantic_writer_rejects_unrepresented_typed_fields() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.edges[0].param_range = Some([0.0, 1.0]);
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
pub(crate) fn semantic_writer_rejects_subds() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.subds.push(cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("test:sldprt:subd#0".into()),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    });

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::NotImplemented(message)
            if message.contains("does not support SubD surfaces")
    ));
}

#[test]
fn semantic_writer_rejects_unsupported_conic_curves() {
    let axis = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let major_direction = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    for geometry in [
        cadmpeg_ir::geometry::CurveGeometry::Parabola {
            vertex: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis,
            major_direction,
            focal_distance: 1.0,
        },
        cadmpeg_ir::geometry::CurveGeometry::Hyperbola {
            center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis,
            major_direction,
            major_radius: 2.0,
            minor_radius: 1.0,
        },
    ] {
        assert!(matches!(
            crate::writer::curve_values(&geometry, 0.001),
            Err(cadmpeg_core::CodecError::NotImplemented(_))
        ));
    }
}

#[test]
fn semantic_writer_rejects_noncanonical_ellipse_radius_order() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&closed_cylinder_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.curves[0].geometry = cadmpeg_ir::geometry::CurveGeometry::Ellipse {
        center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        major_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 1.0,
        minor_radius: 2.0,
    };

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("ellipse major radius is smaller than its minor radius")
    ));
}

#[test]
pub(crate) fn semantic_writer_rejects_nonfinite_analytic_carriers() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&closed_cylinder_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    {
        let mut ir_edit = decoded.ir_mut();
        let cadmpeg_ir::geometry::CurveGeometry::Circle { center, .. } =
            &mut ir_edit.model.curves[0].geometry
        else {
            panic!("closed cylinder edge must use a circle carrier");
        };
        center.x = f64::INFINITY;
    }

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("circle center is not finite")
    ));
}

#[test]
fn semantic_writer_rejects_unrepresentable_analytic_surface_parameterizations() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let origin = cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0);
    let axis = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let reference = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    let cases = [
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction: reference,
                radius: 2.0,
                ratio: 0.5,
                half_angle: std::f64::consts::FRAC_PI_4,
            },
            "elliptical cone ratio 0.5",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction: reference,
                radius: 2.0,
                ratio: 1.0,
                half_angle: -std::f64::consts::FRAC_PI_4,
            },
            "cone half-angle -0.7853981633974483",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
                center: origin,
                axis,
                ref_direction: reference,
                radius: -2.0,
            },
            "signed sphere radius -2",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                center: origin,
                axis,
                ref_direction: reference,
                major_radius: 2.0,
                minor_radius: -0.5,
            },
            "torus radii (2, -0.5)",
        ),
    ];

    for (geometry, expected) in cases {
        let mut ir = decoded.ir().clone();
        let surface_id = ir.model.surfaces[0].id.0.clone();
        ir.model.surfaces[0].geometry = geometry;

        let error = SldprtCodec
            .write_preserved_with_source_fidelity(&ir, decoded.source_fidelity(), &mut Vec::new())
            .unwrap_err();

        assert!(matches!(
            error,
            cadmpeg_core::CodecError::NotImplemented(message)
                if message.contains(&surface_id) && message.contains(expected)
        ));
    }
}

#[test]
fn semantic_writer_converts_millimetres_to_native_metres() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.points[0].position.x = 50.8;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated
        .ir()
        .model
        .points
        .iter()
        .any(|point| (point.position.x - 50.8).abs() < 1e-5));
}

#[test]
fn semantic_writer_preserves_multiple_body_ownership() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 0.0));
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.points[0].position.z += 1.0;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir().model.bodies.len(), 2);
    assert_eq!(regenerated.ir().model.regions.len(), 2);
    assert_eq!(regenerated.ir().model.shells.len(), 2);
    assert!(regenerated
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    assert!(regenerated.ir().model.regions.iter().all(|region| {
        regenerated.source_fidelity().annotations.provenance[&region.id.0]
            .tag
            .as_deref()
            == Some("00_51_region")
    }));
    assert!(regenerated.ir().model.shells.iter().all(|shell| {
        regenerated.source_fidelity().annotations.provenance[&shell.id.0]
            .tag
            .as_deref()
            == Some("00_51_shell")
    }));
}

#[test]
fn semantic_writer_regenerates_modified_nurbs_carriers() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut body = triangle_body();
    let bridge_offset = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge_offset + 26..bridge_offset + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());
    body.extend(nurbs_curve_carrier(170, 171));
    body.extend(nurbs_surface_carrier(180, 181, 10));
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let (expected_curve, expected_surface) = {
        let mut ir_edit = decoded.ir_mut();
        let CurveGeometry::Nurbs(curve) = &mut ir_edit.model.curves[0].geometry else {
            panic!("expected NURBS curve");
        };
        curve.control_points[1].y += 250.0;
        let expected_curve = curve.clone();
        let SurfaceGeometry::Nurbs(surface) = &mut ir_edit.model.surfaces[0].geometry else {
            panic!("expected NURBS surface");
        };
        surface.control_points[3].z += 500.0;
        let expected_surface = surface.clone();
        (expected_curve, expected_surface)
    };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated.ir().model.curves.iter().any(
        |curve| matches!(&curve.geometry, CurveGeometry::Nurbs(value) if value == &expected_curve)
    ));
    assert!(regenerated.ir().model.surfaces.iter().any(
        |surface| matches!(&surface.geometry, SurfaceGeometry::Nurbs(value) if value == &expected_surface)
    ));
}

#[test]
fn semantic_writer_preserves_unbound_material_definition() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_material(
                &triangle_body(),
                "Steel",
                [32, 64, 128],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.points[0].position.z += 1.0;
    let validation = cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
    assert!((color.r - 32.0 / 255.0).abs() < 1e-6);
    assert!((color.g - 64.0 / 255.0).abs() < 1e-6);
    assert!((color.b - 128.0 / 255.0).abs() < 1e-6);
}

#[test]
fn semantic_writer_rejects_overlong_material_names() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_material(
                &triangle_body(),
                "Steel",
                [32, 64, 128],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.appearances[0].name = Some("M".repeat(256));
    decoded.ir_mut().model.bodies[0].name = Some("M".repeat(256));
    decoded.ir_mut().model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 32.0 / 255.0,
        g: 64.0 / 255.0,
        b: 128.0 / 255.0,
        a: 1.0,
    });
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("material name is too long"));
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
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.points[0].position.z += 1.0;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
    assert_eq!([color.r, color.g, color.b], [0.25, 0.5, 0.75]);
}

#[test]
fn semantic_writer_derives_resolved_feature_section_names() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.source_fidelity_mut().annotations = cadmpeg_ir::Annotations::default();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].sketch_entities[0].kind =
            crate::records::SketchInputKind::Native(9);
    });

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-0-ResolvedFeatures") }));

    let (mut unscoped, _, fidelity) = decoded.into_parts();
    update_sldprt_native(&mut unscoped, |native| {
        native.feature_input_lanes[0].configuration = None;
    });
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&unscoped, &fidelity, &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.as_deref() == Some("Contents/ResolvedFeatures")));
}

#[test]
fn semantic_writer_preserves_idless_feature_tree_nodes() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Root" Type="Folder" id="1"><Folder Name="Group"><Sketch Name="Profile" Type="Sketch" id="2"/></Folder></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(decoded.ir()).feature_histories[0].features;
    assert_eq!(
        native[1].tree_parent.as_deref(),
        Some(native[0].id.as_str())
    );
    assert_eq!(
        native[2].tree_parent.as_deref(),
        Some(native[1].id.as_str())
    );
    assert_eq!(
        decoded.ir().model.features[1].parent.as_ref(),
        Some(&decoded.ir().model.features[0].id)
    );
    assert_eq!(
        decoded.ir().model.features[2].parent.as_ref(),
        Some(&decoded.ir().model.features[1].id)
    );
    decoded.ir_mut().model.features[2].name = Some("Edited Profile".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
        native[1].tree_parent.as_deref(),
        Some(native[0].id.as_str())
    );
    assert_eq!(
        native[2].tree_parent.as_deref(),
        Some(native[1].id.as_str())
    );
    assert_eq!(native[2].name, "Edited Profile");
}

#[test]
fn semantic_writer_applies_neutral_configuration_edits() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    {
        let mut ir_edit = decoded.ir_mut();
        let configuration = &mut ir_edit.model.configurations[0];
        configuration.name = "Machined".into();
        configuration.material = Some("Aluminum".into());
        configuration
            .properties
            .insert("Finish".into(), "Anodized".into());
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(regenerated.ir());
    let configuration = &native.feature_histories[0].configurations[0];
    assert_eq!(configuration.name, "Machined");
    assert_eq!(configuration.material.as_deref(), Some("Aluminum"));
    assert_eq!(configuration.properties["Finish"], "Anodized");
    assert_eq!(regenerated.ir().model.configurations[0].name, "Machined");
}

#[test]
fn semantic_writer_rejects_conflicting_configuration_edits() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.configurations[0].name = "Neutral".into();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].configurations[0].name = "Native".into();
    });

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting neutral and native SLDPRT configuration edits"));
}

#[test]
fn semantic_writer_applies_neutral_parameter_edits() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = ir_edit
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == "Depth")
            .unwrap();
        parameter.expression = "20mm".into();
        parameter.value = Some(ParameterValue::Length(Length(20.0)));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth" Driven="true" EquationId="D1@Boss">12mm</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = &mut ir_edit.model.parameters[0];
        assert_eq!(parameter.properties["Driven"], "true");
        assert_eq!(parameter.properties["EquationId"], "D1@Boss");
        parameter.expression = "20mm".into();
        parameter.value = Some(ParameterValue::Length(Length(20.0)));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth" Value="24mm" EquationId="D1@Boss">Width * 2</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    {
        let mut ir_edit = decoded.ir_mut();
        let parameter = &mut ir_edit.model.parameters[0];
        assert_eq!(parameter.expression, "Width * 2");
        assert_eq!(parameter.value, Some(ParameterValue::Length(Length(24.0))));
        assert_eq!(parameter.properties["Value"], "24mm");
        parameter.expression = "Width * 3".into();
        parameter.value = Some(ParameterValue::Length(Length(36.0)));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = &regenerated.ir().model.parameters[0];
    assert_eq!(parameter.expression, "Width * 3");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(36.0))));
    assert_eq!(parameter.properties["Value"], "36mm");
    assert_eq!(parameter.properties["EquationId"], "D1@Boss");
}
