use super::*;
use crate::records::feature::scope::DesignFeatureKind;

fn convert(
    fields: MeshOperationFields,
    values: &[(u32, f64, bool)],
) -> (MeshModification, Vec<String>, Vec<(String, String)>) {
    let mut notes = Notes {
        source: "synthetic:scope",
        properties: Vec::new(),
        losses: Vec::new(),
    };
    let value = |owner: u32| {
        values
            .iter()
            .find(|(candidate, ..)| *candidate == owner)
            .map(|(_, value, length)| OwnerValue {
                value: *value,
                length: *length,
            })
    };
    let modification = modification(&fields, &value, &mut notes).unwrap();
    let codes = notes
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect();
    (modification, codes, notes.properties)
}

fn reduce(distribution: u32, target: u32) -> MeshOperationFields {
    MeshOperationFields::Reduce {
        tolerance_owner: 12,
        distribution,
        proportion_owner: 11,
        target,
        face_count_owner: 13,
    }
}

const REDUCE_VALUES: [(u32, f64, bool); 3] = [(11, 50.0, false), (12, 0.1, true), (13, 6.0, false)];

#[test]
fn reduce_target_selector_names_the_active_bound() {
    let (deviation, codes, properties) = convert(reduce(0, 0), &REDUCE_VALUES);
    assert_eq!(
        deviation,
        MeshModification::Reduce {
            distribution: Some(MeshFaceDistribution::Adaptive),
            target: Some(MeshReduceTarget::MaximumDeviation(
                NonNegativeLength::new(1.0).unwrap()
            )),
        }
    );
    assert!(codes.is_empty());
    assert!(properties.contains(&("mesh-selector:target".into(), "0".into())));
    assert_eq!(
        convert(reduce(1, 1), &REDUCE_VALUES).0,
        MeshModification::Reduce {
            distribution: Some(MeshFaceDistribution::Uniform),
            target: Some(MeshReduceTarget::FaceProportion(
                Fraction::new(0.5).unwrap()
            )),
        }
    );
    assert_eq!(
        convert(reduce(0, 2), &REDUCE_VALUES).0,
        MeshModification::Reduce {
            distribution: Some(MeshFaceDistribution::Adaptive),
            target: Some(MeshReduceTarget::FaceCount(
                std::num::NonZeroU32::new(6).unwrap()
            )),
        }
    );
}

#[test]
fn unruled_selectors_and_unusable_values_keep_named_losses() {
    let (unruled, codes, properties) = convert(reduce(7, 5), &REDUCE_VALUES);
    assert_eq!(
        unruled,
        MeshModification::Reduce {
            distribution: None,
            target: None,
        }
    );
    assert_eq!(
        codes,
        [
            "feature.mesh-operation-selector-unruled",
            "feature.mesh-operation-selector-unruled"
        ]
    );
    assert!(properties.contains(&("mesh-selector:distribution".into(), "7".into())));
    assert!(properties.contains(&("mesh-selector:target".into(), "5".into())));
    for values in [
        vec![],
        vec![(13, 6.5, false)],
        vec![(13, 0.0, false)],
        vec![(13, 6.0, true)],
    ] {
        let (unbound, codes, _) = convert(reduce(0, 2), &values);
        assert_eq!(
            unbound,
            MeshModification::Reduce {
                distribution: Some(MeshFaceDistribution::Adaptive),
                target: None,
            }
        );
        assert_eq!(codes, ["feature.mesh-operation-parameter-unbound"]);
    }
    let (percent, codes, _) = convert(reduce(0, 1), &[(11, 150.0, false)]);
    assert_eq!(
        percent,
        MeshModification::Reduce {
            distribution: Some(MeshFaceDistribution::Adaptive),
            target: None,
        }
    );
    assert_eq!(codes, ["feature.mesh-operation-parameter-unbound"]);
}

fn repair(rebuild_mode: u32, method: u32) -> MeshOperationFields {
    MeshOperationFields::Repair {
        density_owner: 11,
        offset_owner: 12,
        rebuild_mode,
        method,
    }
}

#[test]
fn repair_method_selector_names_the_method_and_rebuild_controls() {
    let values = [(11, 8.0, false), (12, 0.05, true)];
    assert_eq!(
        convert(repair(0, 0), &values).0,
        MeshModification::Repair {
            method: Some(MeshRepair::CloseHoles)
        }
    );
    assert_eq!(
        convert(repair(0, 1), &values).0,
        MeshModification::Repair {
            method: Some(MeshRepair::StitchAndRemove)
        }
    );
    let (rebuild, codes, _) = convert(repair(2, 3), &values);
    assert_eq!(
        rebuild,
        MeshModification::Repair {
            method: Some(MeshRepair::Rebuild {
                mode: Some(MeshRebuildMode::Accurate),
                density: Some(PositiveReal::new(8.0).unwrap()),
                offset: Some(Length::new(0.5).unwrap()),
            })
        }
    );
    assert!(codes.is_empty());
    assert_eq!(
        convert(repair(0, 3), &values).0,
        MeshModification::Repair {
            method: Some(MeshRepair::Rebuild {
                mode: Some(MeshRebuildMode::Fast),
                density: Some(PositiveReal::new(8.0).unwrap()),
                offset: Some(Length::new(0.5).unwrap()),
            })
        }
    );
    let (unruled, codes, properties) = convert(repair(1, 2), &values);
    assert_eq!(unruled, MeshModification::Repair { method: None });
    assert_eq!(codes, ["feature.mesh-operation-selector-unruled"]);
    assert!(properties.contains(&("mesh-selector:method".into(), "2".into())));
    assert!(properties.contains(&("mesh-selector:rebuild".into(), "1".into())));
    let (mode, codes, _) = convert(repair(1, 3), &values);
    assert_eq!(
        mode,
        MeshModification::Repair {
            method: Some(MeshRepair::Rebuild {
                mode: None,
                density: Some(PositiveReal::new(8.0).unwrap()),
                offset: Some(Length::new(0.5).unwrap()),
            })
        }
    );
    assert_eq!(codes, ["feature.mesh-operation-selector-unruled"]);
}

#[test]
fn remesh_smooth_and_shell_values_use_their_owner_units() {
    assert_eq!(
        convert(
            MeshOperationFields::Remesh {
                distribution: 0,
                shape_preservation_owner: Some(11),
                edge_length_owner: 12,
            },
            &[(11, 0.5, false), (12, 1.5, true)],
        )
        .0,
        MeshModification::Remesh {
            distribution: Some(MeshFaceDistribution::Adaptive),
            edge_length: Some(PositiveLength::new(15.0).unwrap()),
            shape_preservation: Some(Fraction::new(0.5).unwrap()),
        }
    );
    assert_eq!(
        convert(
            MeshOperationFields::Remesh {
                distribution: 1,
                shape_preservation_owner: None,
                edge_length_owner: 12,
            },
            &[(12, 2.0, true)],
        )
        .0,
        MeshModification::Remesh {
            distribution: Some(MeshFaceDistribution::Uniform),
            edge_length: Some(PositiveLength::new(20.0).unwrap()),
            shape_preservation: None,
        }
    );
    assert_eq!(
        convert(
            MeshOperationFields::Smooth {
                smoothness_owner: 11
            },
            &[(11, 0.25, false)],
        )
        .0,
        MeshModification::Smooth {
            smoothness: Some(Fraction::new(0.25).unwrap())
        }
    );
    assert_eq!(
        convert(
            MeshOperationFields::Shell {
                thickness_owner: 11
            },
            &[(11, 0.2, true)],
        )
        .0,
        MeshModification::Shell {
            thickness: Some(PositiveLength::new(2.0).unwrap())
        }
    );
    let (shell, codes, _) = convert(
        MeshOperationFields::Shell {
            thickness_owner: 11,
        },
        &[(11, 0.0, true)],
    );
    assert_eq!(shell, MeshModification::Shell { thickness: None });
    assert_eq!(codes, ["feature.mesh-operation-parameter-unbound"]);
}

fn reference(target: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[0] = 1;
    bytes[1..5].copy_from_slice(&target.to_le_bytes());
    bytes
}

fn record(index: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"714");
    bytes[7..11].copy_from_slice(&index.to_le_bytes());
    bytes
}

fn feature(id: &str, kind: &str, native_ref: Option<String>) -> Feature {
    Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint(id).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some(kind.into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: kind.into(),
                parameters: [(cadmpeg_core::nonblank_literal!("d1"), "1.00 mm".to_owned())].into(),
            }),
        ),
        native_ref,
    }
}

const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

fn bind(stream: &[u8], features: &mut [Feature], scopes: &[DesignParameterScope]) -> Vec<String> {
    use std::io::{Cursor, Write};

    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(stream).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            bind_features(ctx, scan, features, scopes, &[], &[], &mut losses)
        })
        .unwrap();
    });
    losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect()
}

fn scope(kind: &str, record_index: u32) -> (String, DesignParameterScope) {
    let id = format!("f3d:{ENTRY}:design-parameter-scope#0");
    let kind = DesignFeatureKind::try_from(kind.to_owned()).unwrap();
    let scope = DesignParameterScope::empty(&id, kind, record_index);
    (id, scope)
}

fn with_source_records(mut stream: Vec<u8>, bodies: &[u32]) -> Vec<u8> {
    let mut group = record(8, 25 + 11 * bodies.len());
    group[21..25].copy_from_slice(&u32::try_from(bodies.len()).unwrap().to_le_bytes());
    for (ordinal, body) in bodies.iter().enumerate() {
        let member = 40 + u32::try_from(ordinal).unwrap();
        group[25 + 11 * ordinal..36 + 11 * ordinal].copy_from_slice(&reference(member));
        let mut bytes = record(member, 40);
        bytes[21..32].copy_from_slice(&reference(*body));
        stream.extend_from_slice(&bytes);
    }
    stream.extend_from_slice(&group);
    stream
}

#[test]
fn reverse_normals_scope_projects_its_source_bodies_and_geometry_records() {
    let mut stream = record(7, 128);
    stream[21..32].copy_from_slice(&reference(30));
    stream[32..43].copy_from_slice(&reference(20));
    stream[47..58].copy_from_slice(&reference(8));
    let stream = with_source_records(stream, &[9, 3]);
    let (scope_id, scope) = scope("MeshReverseNormals", 7);
    let mut features = [feature(
        "synthetic:model:feature#reverse",
        "MeshReverseNormals",
        Some(scope_id.clone()),
    )];
    let codes = bind(&stream, &mut features, &[scope]);
    let FeatureDefinition::Operation(FeatureOperation::MeshModify {
        source: MeshSelection::Native { native, members },
        modification: MeshModification::ReverseNormals,
    }) = features[0].evaluation.definition()
    else {
        panic!("mesh modification");
    };
    assert_eq!(native.as_str(), scope_id);
    assert_eq!(members.as_ref().unwrap().as_slice(), ["9", "3"]);
    assert_eq!(codes, ["feature.mesh-operation-source-unresolved"]);
    let properties = &features[0].source_properties;
    assert_eq!(properties["mesh-source-group"], "8");
    assert_eq!(properties["mesh-input-geometry"], "20");
    assert_eq!(properties["mesh-output-geometry"], "30");
    assert_eq!(properties["mesh-body-reference:0"], "9");
    assert_eq!(properties["mesh-body-reference:1"], "3");
    assert_eq!(properties["parameter:d1"], "1.00 mm");
}

#[test]
fn unresolved_source_group_keeps_the_scope_as_mesh_source() {
    let mut stream = record(7, 128);
    stream[21..32].copy_from_slice(&reference(30));
    stream[32..43].copy_from_slice(&reference(20));
    stream[47..58].copy_from_slice(&reference(8));
    for bodies in [vec![], vec![9, 9]] {
        let stream = with_source_records(stream.clone(), &bodies);
        let (scope_id, scope) = scope("MeshReverseNormals", 7);
        let mut features = [feature(
            "synthetic:model:feature#reverse",
            "MeshReverseNormals",
            Some(scope_id.clone()),
        )];
        let codes = bind(&stream, &mut features, &[scope]);
        let FeatureDefinition::Operation(FeatureOperation::MeshModify {
            source: MeshSelection::Native { native, members },
            ..
        }) = features[0].evaluation.definition()
        else {
            panic!("mesh modification");
        };
        assert_eq!(native.as_str(), scope_id);
        assert!(members.is_none());
        assert_eq!(codes, ["feature.mesh-operation-scope-unparsed"]);
        assert_eq!(features[0].source_properties["mesh-source-group"], "8");
    }
}

#[test]
fn section_scope_names_the_feature_of_its_sketch_scope() {
    let mut stream = record(7, 128);
    stream[20] = 1;
    stream[21..25].copy_from_slice(&6u32.to_le_bytes());
    stream[25..36].copy_from_slice(&reference(8));
    stream[36..47].copy_from_slice(&reference(5));
    stream[48..59].copy_from_slice(&reference(20));
    stream[63..74].copy_from_slice(&reference(8));
    let stream = with_source_records(stream, &[9]);
    let (scope_id, section) = scope("MeshSection", 7);
    let sketch_id = format!("f3d:{ENTRY}:design-parameter-scope#4096");
    let sketch = DesignParameterScope::empty(
        &sketch_id,
        DesignFeatureKind::try_from("Sketch".to_owned()).unwrap(),
        5,
    );
    for sketch_feature in [true, false] {
        let mut features = vec![feature(
            "synthetic:model:feature#section",
            "MeshSection",
            Some(scope_id.clone()),
        )];
        if sketch_feature {
            features.push(feature(
                "synthetic:model:feature#sketch",
                "Sketch",
                Some(sketch_id.clone()),
            ));
        }
        let codes = bind(&stream, &mut features, &[section.clone(), sketch.clone()]);
        let FeatureDefinition::Operation(FeatureOperation::MeshSection {
            source: MeshSelection::Native { members, .. },
            sketch: bound,
        }) = features[0].evaluation.definition()
        else {
            panic!("mesh section");
        };
        assert_eq!(members.as_ref().unwrap().as_slice(), ["9"]);
        assert_eq!(
            bound.as_ref().map(|id| id.as_str()),
            sketch_feature.then_some("synthetic:model:feature#sketch")
        );
        assert_eq!(
            codes,
            [
                "mesh.scope-prefix-uninterpreted",
                "feature.mesh-section-curves-unresolved",
                "feature.mesh-operation-source-unresolved"
            ]
        );
        let properties = &features[0].source_properties;
        assert_eq!(properties["mesh-scope-prefix-word"], "6");
        assert_eq!(properties["mesh-section-sketch-scope"], "5");
        assert_eq!(properties["mesh-section-group"], "8");
        assert!(!properties.contains_key("mesh-output-geometry"));
    }
}

#[test]
fn unreadable_scopes_keep_typed_originating_features() {
    let stream = record(7, 128);
    for (kind, native_ref) in [
        ("MeshSmooth", None),
        ("MeshSmooth", Some(scope("MeshSmooth", 7).0)),
        ("MeshSection", Some(scope("MeshSection", 7).0)),
    ] {
        let scopes = native_ref
            .iter()
            .map(|_| scope(kind, 7).1)
            .collect::<Vec<_>>();
        let mut features = [feature(
            "synthetic:model:feature#operation",
            kind,
            native_ref,
        )];
        let codes = bind(&stream, &mut features, &scopes);
        assert_eq!(codes, ["feature.mesh-operation-scope-unparsed"]);
        let unresolved = MeshSelection::Unresolved {
            feature: features[0].id.clone(),
        };
        match features[0].evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::MeshModify {
                source,
                modification,
            }) => {
                assert_eq!(kind, "MeshSmooth");
                assert_eq!(source, &unresolved);
                assert_eq!(modification, &MeshModification::Smooth { smoothness: None });
            }
            FeatureDefinition::Operation(FeatureOperation::MeshSection { source, sketch }) => {
                assert_eq!(kind, "MeshSection");
                assert_eq!(source, &unresolved);
                assert!(sketch.is_none());
            }
            _ => panic!("mesh operation"),
        }
    }
}
