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
            target: Some(MeshReduceTarget::MaximumDeviation {
                value: NonNegativeLength::new(1.0).unwrap(),
            }),
        }
    );
    assert!(codes.is_empty());
    assert!(properties.contains(&("mesh-selector:target".into(), "0".into())));
    assert_eq!(
        convert(reduce(1, 1), &REDUCE_VALUES).0,
        MeshModification::Reduce {
            distribution: Some(MeshFaceDistribution::Uniform),
            target: Some(MeshReduceTarget::FaceProportion {
                value: Fraction::new(0.5).unwrap(),
            }),
        }
    );
    assert_eq!(
        convert(reduce(0, 2), &REDUCE_VALUES).0,
        MeshModification::Reduce {
            distribution: Some(MeshFaceDistribution::Adaptive),
            target: Some(MeshReduceTarget::FaceCount {
                value: std::num::NonZeroU32::new(6).unwrap(),
            }),
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
            method: Some(MeshRepair::CloseHoles {})
        }
    );
    assert_eq!(
        convert(repair(0, 1), &values).0,
        MeshModification::Repair {
            method: Some(MeshRepair::StitchAndRemove {})
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
fn remesh_and_smooth_values_use_their_owner_units() {
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
            FeatureDefinition::Operation(if kind == "Sketch" {
                FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                }
            } else {
                FeatureOperation::Native {
                    kind: kind.into(),
                    parameters: [(cadmpeg_core::nonblank_literal!("d1"), "1.00 mm".to_owned())]
                        .into(),
                }
            }),
        ),
        native_ref,
    }
}

const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

struct Fixture {
    stream: Vec<u8>,
    metadata: Option<Vec<u8>>,
}

fn bind(
    fixture: &Fixture,
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
) -> Vec<String> {
    bind_inputs(
        fixture,
        features,
        MeshOperationInputs {
            scopes,
            owners: &[],
            parameters: &[],
            placements: &[],
            sketches: &[],
        },
    )
}

fn bind_inputs(
    fixture: &Fixture,
    features: &mut [Feature],
    inputs: MeshOperationInputs<'_>,
) -> Vec<String> {
    use std::io::{Cursor, Write};

    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(&fixture.stream).unwrap();
    if let Some(meta) = &fixture.metadata {
        zip.start_file(ENTRY.replace("BulkStream.dat", "MetaStream.dat"), stored)
            .unwrap();
        zip.write_all(meta).unwrap();
    }
    let archive = zip.finish().unwrap().into_inner();
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            bind_features(ctx, scan, features, inputs, &mut losses)
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

fn with_source_records(mut stream: Vec<u8>, kind: &str, bodies: &[u32]) -> Fixture {
    let mut records = vec![(7, 0)];
    stream[4..7].copy_from_slice(b"256");
    let mut group = record(8, 25 + 11 * bodies.len());
    group[4..7].copy_from_slice(b"257");
    group[21..25].copy_from_slice(&u32::try_from(bodies.len()).unwrap().to_le_bytes());
    let mut member_ids = Vec::new();
    for (ordinal, body) in bodies.iter().enumerate() {
        let member = 40 + u32::try_from(ordinal).unwrap();
        member_ids.push(u64::from(member));
        group[25 + 11 * ordinal..36 + 11 * ordinal].copy_from_slice(&reference(member));
        let mut bytes = record(member, 40);
        bytes[4..7].copy_from_slice(b"258");
        bytes[21..32].copy_from_slice(&reference(*body));
        records.push((u64::from(member), u64::try_from(stream.len()).unwrap()));
        stream.extend_from_slice(&bytes);
    }
    records.push((8, u64::try_from(stream.len()).unwrap()));
    stream.extend_from_slice(&group);
    let mut body_ids = Vec::new();
    for body in bodies {
        if body_ids.contains(&u64::from(*body)) {
            continue;
        }
        body_ids.push(u64::from(*body));
        records.push((u64::from(*body), u64::try_from(stream.len()).unwrap()));
        let mut bytes = record(*body, 11);
        bytes[4..7].copy_from_slice(b"259");
        stream.extend_from_slice(&bytes);
    }
    let (guid, version) = match kind {
        "MeshSection" => ("8FFFCBD2-7102-4A62-BCA0-8229A3C3B9DB", 1),
        "MeshReduce" => ("BE693D4C-741E-4794-A93E-5BFED71E9E13", 2),
        "MeshRepair" => ("68691143-5EE3-428E-9CD3-F728E4718559", 4),
        "MeshRemesh" => ("828DDA62-2750-447D-8728-06C9B7C2B117", 2),
        "MeshSmooth" => ("F84CD988-9696-458D-8AF3-795D12E6B9EF", 2),
        "MeshShell" => ("1514CDB8-BB81-4895-A953-7B12F4504F99", 2),
        "MeshReverseNormals" => ("4AB7A6BD-C40C-4C44-B7CA-0D415B6E4412", 0),
        "MeshRemove" => ("57A2E341-A23A-469D-B41D-59B57A9AD8B8", 0),
        _ => panic!("mesh operation kind"),
    };
    let base = if kind == "MeshRemove" {
        "63A5A106-D239-4D6B-A96B-319CA9B9BA6A"
    } else {
        "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF"
    };
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (guid, base, version, "Fusion", &[7]),
            (
                "22975A91-16AD-4CC9-A5E4-B4F8B4C27881",
                "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
                0,
                "Fusion",
                &[8],
            ),
            (
                "1AA9CC7B-5C34-45DA-AE14-0444E7F966F2",
                "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                0,
                "Fusion",
                &member_ids,
            ),
            (
                "EA90DA22-556C-4C61-89BB-20C2681B7A9D",
                "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
                u32::MAX,
                "ParaMesh",
                &body_ids,
            ),
        ],
        &records,
    );
    Fixture {
        stream,
        metadata: Some(metadata),
    }
}

fn reverse_scope() -> Vec<u8> {
    let mut stream = record(7, 128);
    stream[21..32].copy_from_slice(&reference(30));
    stream[32..43].copy_from_slice(&reference(20));
    stream[47..58].copy_from_slice(&reference(8));
    stream
}

fn removal_scope() -> Vec<u8> {
    let mut stream = record(7, 32);
    stream[21..32].copy_from_slice(&reference(8));
    stream
}

#[test]
fn removal_scope_projects_ordered_input_local_body_deletions() {
    let fixture = with_source_records(removal_scope(), "MeshRemove", &[9, 3]);
    let (native, scope) = scope("MeshRemove", 7);
    let mut features = [feature(
        "synthetic:model:feature#remove",
        "MeshRemove",
        Some(native.clone()),
    )];
    let codes = bind(&fixture, &mut features, &[scope]);
    let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: BodySelection::Local {
            bodies,
            native: source,
        },
        mode,
    }) = features[0].evaluation.definition()
    else {
        panic!("delete selected mesh bodies");
    };
    assert_eq!(source.as_str(), native);
    assert_eq!(bodies.as_slice(), ["9", "3"]);
    assert_eq!(
        *mode,
        cadmpeg_ir::features::BodyRetentionMode::DeleteSelected
    );
    assert_eq!(
        codes,
        [
            "feature.mesh-body-selection-assumed",
            "feature.mesh-operation-source-unresolved"
        ]
    );
    let properties = &features[0].source_properties;
    assert_eq!(properties["mesh-source-group"], "8");
    assert_eq!(properties["mesh-source-member-count"], "2");
    assert_eq!(properties["mesh-member-reference:0"], "40");
    assert_eq!(properties["mesh-member-reference:1"], "41");
    assert_eq!(properties["mesh-body-reference:0"], "9");
    assert_eq!(properties["mesh-body-reference:1"], "3");
    assert_eq!(properties["parameter:d1"], "1.00 mm");
    assert!(!properties.contains_key("mesh-input-geometry"));
    assert!(!properties.contains_key("mesh-output-geometry"));
}

#[test]
fn removal_missing_member_keeps_its_reference_and_later_body() {
    let mut fixture = with_source_records(removal_scope(), "MeshRemove", &[9, 3]);
    fixture.stream[39..43].copy_from_slice(&777u32.to_le_bytes());
    let (native, scope) = scope("MeshRemove", 7);
    let mut features = [feature(
        "synthetic:model:feature#remove",
        "MeshRemove",
        Some(native.clone()),
    )];
    let codes = bind(&fixture, &mut features, &[scope]);
    assert!(matches!(features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(source),
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        }) if source == &native));
    let properties = &features[0].source_properties;
    assert_eq!(properties["mesh-member-reference:0"], "40");
    assert_eq!(properties["mesh-member-reference:1"], "41");
    assert!(!properties.contains_key("mesh-body-reference:0"));
    assert_eq!(properties["mesh-body-reference:1"], "3");
    assert_eq!(codes, ["feature.mesh-operation-scope-unparsed"]);
}

#[test]
fn removal_missing_body_definition_keeps_both_body_references() {
    let mut fixture = with_source_records(removal_scope(), "MeshRemove", &[9, 3]);
    fixture.stream[166..170].copy_from_slice(&777u32.to_le_bytes());
    let (native, scope) = scope("MeshRemove", 7);
    let mut features = [feature(
        "synthetic:model:feature#remove",
        "MeshRemove",
        Some(native.clone()),
    )];
    let codes = bind(&fixture, &mut features, &[scope]);
    assert!(matches!(features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(source),
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        }) if source == &native));
    let properties = &features[0].source_properties;
    assert_eq!(properties["mesh-body-reference:0"], "9");
    assert_eq!(properties["mesh-body-reference:1"], "3");
    assert_eq!(codes, ["feature.mesh-operation-scope-unparsed"]);
}

#[test]
fn removal_truncated_source_keeps_its_kind_without_a_placeholder_group() {
    let mut source = removal_scope();
    source.truncate(31);
    let fixture = with_source_records(source, "MeshRemove", &[9]);
    let (native, scope) = scope("MeshRemove", 7);
    let mut features = [feature(
        "synthetic:model:feature#remove",
        "MeshRemove",
        Some(native.clone()),
    )];
    let codes = bind(&fixture, &mut features, &[scope]);
    assert!(matches!(features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(source),
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        }) if source == &native));
    assert!(!features[0]
        .source_properties
        .contains_key("mesh-source-group"));
    assert_eq!(
        codes,
        [
            "feature.mesh-operation-tail-partial",
            "feature.mesh-operation-scope-unparsed"
        ]
    );
}

#[test]
fn removal_without_a_scope_still_projects_a_typed_deletion() {
    let fixture = with_source_records(removal_scope(), "MeshRemove", &[]);
    let mut features = [feature(
        "synthetic:model:feature#remove",
        "MeshRemove",
        None,
    )];
    let codes = bind(&fixture, &mut features, &[]);
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        })
    ));
    assert_eq!(codes, ["feature.mesh-operation-scope-unparsed"]);
}

#[test]
fn reverse_normals_scope_projects_its_source_bodies_and_geometry_records() {
    let fixture = with_source_records(reverse_scope(), "MeshReverseNormals", &[9, 3]);
    let (scope_id, scope) = scope("MeshReverseNormals", 7);
    let mut features = [feature(
        "synthetic:model:feature#reverse",
        "MeshReverseNormals",
        Some(scope_id.clone()),
    )];
    let codes = bind(&fixture, &mut features, &[scope]);
    let FeatureDefinition::Operation(FeatureOperation::ReverseShape {
        source: BodySelection::Local { native, bodies },
    }) = features[0].evaluation.definition()
    else {
        panic!("reverse shape");
    };
    assert_eq!(native.as_str(), scope_id);
    assert_eq!(bodies.as_slice(), ["9", "3"]);
    assert_eq!(
        codes,
        [
            "feature.mesh-body-selection-assumed",
            "feature.mesh-operation-source-unresolved"
        ]
    );
    let properties = &features[0].source_properties;
    assert_eq!(properties["mesh-source-group"], "8");
    assert_eq!(properties["mesh-input-geometry"], "20");
    assert_eq!(properties["mesh-output-geometry"], "30");
    assert_eq!(properties["mesh-body-reference:0"], "9");
    assert_eq!(properties["mesh-body-reference:1"], "3");
    assert_eq!(properties["parameter:d1"], "1.00 mm");
}

#[test]
fn unresolved_source_group_keeps_the_scope_as_native_body_source() {
    for bodies in [vec![], vec![9, 9]] {
        let fixture = with_source_records(reverse_scope(), "MeshReverseNormals", &bodies);
        let (scope_id, scope) = scope("MeshReverseNormals", 7);
        let mut features = [feature(
            "synthetic:model:feature#reverse",
            "MeshReverseNormals",
            Some(scope_id.clone()),
        )];
        let codes = bind(&fixture, &mut features, &[scope]);
        let FeatureDefinition::Operation(FeatureOperation::ReverseShape {
            source: BodySelection::Native(native),
        }) = features[0].evaluation.definition()
        else {
            panic!("reverse shape");
        };
        assert_eq!(native, &scope_id);
        assert_eq!(codes, ["feature.mesh-operation-scope-unparsed"]);
        assert_eq!(features[0].source_properties["mesh-source-group"], "8");
    }
}

fn section_scope() -> Vec<u8> {
    let mut stream = record(7, 128);
    stream[20] = 1;
    stream[21..25].copy_from_slice(&6u32.to_le_bytes());
    stream[25..36].copy_from_slice(&reference(8));
    stream[36..47].copy_from_slice(&reference(5));
    stream[48..59].copy_from_slice(&reference(20));
    stream[63..74].copy_from_slice(&reference(8));
    stream
}

#[test]
fn section_scope_names_an_actual_sketch_feature_of_its_sketch_scope() {
    let fixture = with_source_records(section_scope(), "MeshSection", &[9]);
    let (scope_id, section) = scope("MeshSection", 7);
    let sketch_id = format!("f3d:{ENTRY}:design-parameter-scope#4096");
    let sketch = DesignParameterScope::empty(
        &sketch_id,
        DesignFeatureKind::try_from("Sketch".to_owned()).unwrap(),
        5,
    );
    for kind in [
        Some("Sketch"),
        Some("SpatialSketch"),
        Some("NativePlaceholder"),
        None,
    ] {
        let mut features = vec![feature(
            "synthetic:model:feature#section",
            "MeshSection",
            Some(scope_id.clone()),
        )];
        if let Some(kind) = kind {
            let mut receiver = feature(
                "synthetic:model:feature#sketch",
                if kind == "SpatialSketch" {
                    "Sketch"
                } else {
                    kind
                },
                Some(sketch_id.clone()),
            );
            if kind == "SpatialSketch" {
                receiver
                    .evaluation
                    .set_definition(FeatureDefinition::Operation(
                        FeatureOperation::SpatialSketch {
                            sketch: Some(
                                cadmpeg_ir::sketches::SpatialSketchId::mint(
                                    "synthetic:model:spatial-sketch#section",
                                )
                                .unwrap(),
                            ),
                        },
                    ));
            }
            features.push(receiver);
        }
        let codes = bind(&fixture, &mut features, &[section.clone(), sketch.clone()]);
        let FeatureDefinition::Operation(FeatureOperation::MeshSection {
            source: MeshSelection::Native { members, .. },
            plane,
            sketch: bound,
        }) = features[0].evaluation.definition()
        else {
            panic!("mesh section");
        };
        assert_eq!(members.as_ref().unwrap().as_slice(), ["9"]);
        assert!(plane.is_none());
        assert_eq!(
            bound.as_ref().map(FeatureId::as_str),
            matches!(kind, Some("Sketch" | "SpatialSketch"))
                .then_some("synthetic:model:feature#sketch")
        );
        assert_eq!(
            codes,
            [
                "mesh.scope-prefix-uninterpreted",
                "feature.mesh-section-curves-unresolved",
                "feature.mesh-body-selection-assumed",
                "feature.mesh-operation-source-unresolved",
                "feature.mesh-section-plane-unresolved"
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
    for (kind, has_native) in [
        ("MeshSmooth", false),
        ("MeshSmooth", true),
        ("MeshSection", true),
        ("MeshShell", true),
        ("MeshReverseNormals", true),
    ] {
        let mut bytes = record(7, 128);
        bytes[20] = 2;
        let fixture = with_source_records(bytes, kind, &[]);
        let (native, scope) = scope(kind, 7);
        let scopes = if has_native { vec![scope] } else { Vec::new() };
        let mut features = [feature(
            "synthetic:model:feature#operation",
            kind,
            has_native.then_some(native),
        )];
        let codes = bind(&fixture, &mut features, &scopes);
        let mut expected_codes = vec!["feature.mesh-operation-scope-unparsed"];
        if kind == "MeshShell" {
            expected_codes.push("feature.mesh-shell-controls-unresolved");
        }
        if kind == "MeshSection" {
            expected_codes.push("feature.mesh-section-plane-unresolved");
        }
        assert_eq!(codes, expected_codes);
        match features[0].evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::MeshModify {
                source,
                modification,
            }) => {
                assert_eq!(kind, "MeshSmooth");
                assert_eq!(
                    source,
                    &MeshSelection::Unresolved {
                        feature: features[0].id.clone()
                    }
                );
                assert_eq!(modification, &MeshModification::Smooth { smoothness: None });
            }
            FeatureDefinition::Operation(FeatureOperation::MeshSection {
                source,
                plane,
                sketch,
            }) => {
                assert_eq!(kind, "MeshSection");
                assert_eq!(
                    source,
                    &MeshSelection::Unresolved {
                        feature: features[0].id.clone()
                    }
                );
                assert!(plane.is_none());
                assert!(sketch.is_none());
            }
            FeatureDefinition::Operation(FeatureOperation::ReverseShape { source }) => {
                assert_eq!(kind, "MeshReverseNormals");
                assert_eq!(source, &BodySelection::Unresolved);
            }
            FeatureDefinition::Operation(FeatureOperation::Shell {
                bodies, thickness, ..
            }) => {
                assert_eq!(kind, "MeshShell");
                assert_eq!(bodies, &Some(BodySelection::Unresolved));
                assert!(thickness.is_none());
            }
            _ => panic!("mesh operation"),
        }
    }
}

#[test]
fn missing_or_malformed_metadata_keeps_a_typed_unresolved_operation() {
    let mut fixture = with_source_records(reverse_scope(), "MeshReverseNormals", &[9]);
    for metadata in [None, Some(vec![1, 2, 3])] {
        fixture.metadata = metadata;
        let (native, scope) = scope("MeshReverseNormals", 7);
        let mut features = [feature(
            "synthetic:model:feature#operation",
            "MeshReverseNormals",
            Some(native),
        )];
        let codes = bind(&fixture, &mut features, &[scope]);
        assert_eq!(codes, ["feature.mesh-operation-scope-unparsed"]);
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::ReverseShape {
                source: BodySelection::Unresolved
            })
        ));
    }
}

fn placement(entry: &str, scope_record_index: u32, legacy: bool) -> DesignSketchPlacement {
    use crate::records::sketch_placement::{DesignSketchFrame, DesignSketchFrameForm};

    let matrix = [
        [0.0, 0.0, 1.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 1.0, 0.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
    .try_into()
    .unwrap();
    DesignSketchPlacement {
        id: format!("f3d:{entry}:sketch-placement#0"),
        scope_record_index: Some(scope_record_index),
        entity_id: "0_35".to_owned().try_into().unwrap(),
        display_name: None,
        visibility: None,
        class_tag: "305".to_owned().try_into().unwrap(),
        record_index: 50,
        paired_class_tag: "306".to_owned().try_into().unwrap(),
        frame: DesignSketchFrame::new(
            0,
            if legacy {
                DesignSketchFrameForm::ScopeLegacy305(matrix)
            } else {
                DesignSketchFrameForm::ScopeExplicit(matrix)
            },
        )
        .unwrap(),
    }
}

#[test]
fn section_retains_the_unique_native_plane_without_resolved_sketch_geometry() {
    let fixture = with_source_records(section_scope(), "MeshSection", &[9]);
    let (native, scope) = scope("MeshSection", 7);
    for legacy in [false, true] {
        for linked_feature in [false, true] {
            let mut features = vec![feature(
                "synthetic:model:feature#section",
                "MeshSection",
                Some(native.clone()),
            )];
            let sketch_scope_id = format!("f3d:{ENTRY}:design-parameter-scope#5");
            let scopes = [
                scope.clone(),
                DesignParameterScope::empty(
                    &sketch_scope_id,
                    DesignFeatureKind::try_from("Sketch".to_owned()).unwrap(),
                    5,
                ),
            ];
            if linked_feature {
                features.push(feature(
                    "synthetic:model:feature#sketch",
                    "Sketch",
                    Some(sketch_scope_id),
                ));
            }
            let placements = [
                placement("Other/Design/BulkStream.dat", 5, legacy),
                placement(ENTRY, 7, legacy),
                placement(ENTRY, 5, legacy),
            ];
            bind_inputs(
                &fixture,
                &mut features,
                MeshOperationInputs {
                    scopes: &scopes,
                    owners: &[],
                    parameters: &[],
                    placements: &placements,
                    sketches: &[],
                },
            );
            let FeatureDefinition::Operation(FeatureOperation::MeshSection {
                plane: Some(plane),
                sketch,
                ..
            }) = features[0].evaluation.definition()
            else {
                panic!("section plane");
            };
            let scale = if legacy { 1.0 } else { 10.0 };
            assert_eq!(
                plane.origin().get(),
                Point3::new(2.0 * scale, 3.0 * scale, 4.0 * scale)
            );
            assert_eq!(plane.normal().get(), Vector3::new(1.0, 0.0, 0.0));
            assert_eq!(plane.u_axis().get(), Vector3::new(0.0, 1.0, 0.0));
            assert_eq!(
                sketch.as_ref().map(FeatureId::as_str),
                linked_feature.then_some("synthetic:model:feature#sketch")
            );
        }
    }
}

#[test]
fn section_rejects_ambiguous_native_planes_and_sketch_feature_links() {
    let (_, section) = scope("MeshSection", 7);
    let native = format!("f3d:{ENTRY}:design-parameter-scope#5");
    let sketch_scope = DesignParameterScope::empty(
        &native,
        DesignFeatureKind::try_from("Sketch".to_owned()).unwrap(),
        5,
    );
    let scopes = [section, sketch_scope.clone()];
    let placements = [placement(ENTRY, 5, false), placement(ENTRY, 5, true)];
    let features = [
        feature(
            "synthetic:model:feature#sketch1",
            "Sketch",
            Some(native.clone()),
        ),
        feature("synthetic:model:feature#sketch2", "Sketch", Some(native)),
    ];
    let inputs = MeshOperationInputs {
        scopes: &scopes,
        owners: &[],
        parameters: &[],
        placements: &placements,
        sketches: &[],
    };
    assert_eq!(
        section_binding(
            &inputs,
            &features,
            crate::ids::native_stream(&scopes[0].id),
            5
        ),
        (None, None)
    );
    let duplicate_scopes = [sketch_scope.clone(), sketch_scope];
    let inputs = MeshOperationInputs {
        scopes: &duplicate_scopes,
        ..inputs
    };
    assert_eq!(
        section_binding(
            &inputs,
            &features[..1],
            crate::ids::native_stream(&scopes[0].id),
            5
        ),
        (None, None)
    );
}

#[test]
fn section_prefers_the_linked_model_sketch_plane() {
    let native = format!("f3d:{ENTRY}:design-parameter-scope#5");
    let scopes = [DesignParameterScope::empty(
        &native,
        DesignFeatureKind::try_from("Sketch".to_owned()).unwrap(),
        5,
    )];
    let sketch_id = cadmpeg_ir::sketches::SketchId::mint("synthetic:model:sketch#5").unwrap();
    let mut features = [feature(
        "synthetic:model:feature#sketch",
        "Sketch",
        Some(native),
    )];
    features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id.clone())),
        }));
    let sketches = [Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(7.0, 8.0, 9.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: Default::default(),
        native_ref: None,
    }];
    let placements = [placement(ENTRY, 5, false), placement(ENTRY, 5, true)];
    let inputs = MeshOperationInputs {
        scopes: &scopes,
        owners: &[],
        parameters: &[],
        placements: &placements,
        sketches: &sketches,
    };
    let (plane, sketch) = section_binding(
        &inputs,
        &features,
        crate::ids::native_stream(&scopes[0].id),
        5,
    );
    assert_eq!(plane.unwrap().origin().get(), Point3::new(7.0, 8.0, 9.0));
    assert_eq!(sketch, Some(features[0].id.clone()));
}

fn parameter_inputs(
    value: f64,
    unit: Option<&str>,
    owner_scope: u32,
) -> ([DesignParameterOwner; 1], [DesignParameter; 1]) {
    use crate::records::parameters::{
        DesignParameterDraft, DesignParameterOwnerWire, DesignParameterSource,
    };

    let owners = [DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("f3d:{ENTRY}:parameter-owner#11"),
        byte_offset: 0,
        frame_length: 103,
        class_tag: "292".to_owned().try_into().unwrap(),
        record_index: 11,
        scope_record_index: owner_scope,
        local_ordinal: 0,
        evaluated_value: value,
        evaluated_value_offset: 40,
        parameter_record_index: 12,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: 13,
    })
    .unwrap()];
    let parameters = [DesignParameter::try_from(DesignParameterDraft {
        id: format!("f3d:{ENTRY}:parameter#12"),
        byte_offset: 0,
        class_tag: "292".to_owned().try_into().unwrap(),
        record_index: 12,
        source_ordinal: 0,
        source: DesignParameterSource::new("FeatureInput".into(), Some(11), None).unwrap(),
        expression: format!("{value} {}", unit.unwrap_or_default()),
        expression_offset: 20,
        source_kind_offset: 40,
        unit: unit.map(|unit| crate::records::identity::RecordedValue {
            value: unit.into(),
            offset: 80,
        }),
        name: "Thickness".into(),
        name_offset: 90,
        evaluated_value: value,
        evaluated_value_offset: 120,
    })
    .unwrap()];
    (owners, parameters)
}

#[test]
fn shell_keeps_ordered_local_bodies_and_unknown_controls() {
    let mut bytes = record(7, 128);
    bytes[21..32].copy_from_slice(&reference(11));
    bytes[32..43].copy_from_slice(&reference(30));
    bytes[43..54].copy_from_slice(&reference(20));
    bytes[58..69].copy_from_slice(&reference(8));
    let fixture = with_source_records(bytes, "MeshShell", &[9, 3]);
    let (native, scope) = scope("MeshShell", 7);
    for value in [0.2, 0.0] {
        let (owners, parameters) = parameter_inputs(value, Some("cm"), 7);
        let mut features = [feature(
            "synthetic:model:feature#shell",
            "MeshShell",
            Some(native.clone()),
        )];
        let codes = bind_inputs(
            &fixture,
            &mut features,
            MeshOperationInputs {
                scopes: std::slice::from_ref(&scope),
                owners: &owners,
                parameters: &parameters,
                placements: &[],
                sketches: &[],
            },
        );
        let FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies:
                Some(BodySelection::Local {
                    bodies,
                    native: selection,
                }),
            removed_faces,
            thickness,
            outward,
            mode,
            join,
            resolve_intersections,
            allow_self_intersections,
        }) = features[0].evaluation.definition()
        else {
            panic!("shell");
        };
        assert_eq!(bodies.as_slice(), ["9", "3"]);
        assert_eq!(selection.as_str(), native);
        assert_eq!(removed_faces, &FaceSelection::Faces(Vec::new()));
        assert_eq!(*thickness, PositiveLength::new(value * 10.0));
        assert!(
            outward.is_none()
                && mode.is_none()
                && join.is_none()
                && resolve_intersections.is_none()
                && allow_self_intersections.is_none()
        );
        let mut expected = vec![
            "feature.mesh-body-selection-assumed",
            "feature.mesh-operation-source-unresolved",
        ];
        if value == 0.0 {
            expected.push("feature.mesh-operation-parameter-unbound");
        }
        expected.push("feature.mesh-shell-controls-unresolved");
        assert_eq!(codes, expected);
    }
}

#[test]
fn smoothness_requires_a_dimensionless_parameter_owned_by_its_scope() {
    let mut bytes = record(7, 128);
    bytes[21..32].copy_from_slice(&reference(11));
    bytes[32..43].copy_from_slice(&reference(30));
    bytes[43..54].copy_from_slice(&reference(20));
    bytes[58..69].copy_from_slice(&reference(8));
    let fixture = with_source_records(bytes, "MeshSmooth", &[9]);
    let (native, scope) = scope("MeshSmooth", 7);
    for (unit, owner_scope, expected) in [
        (None, 7, Some(0.25)),
        (Some("cm"), 7, None),
        (Some("rad"), 7, None),
        (Some("unknown"), 7, None),
        (None, 99, None),
    ] {
        let (owners, parameters) = parameter_inputs(0.25, unit, owner_scope);
        let mut features = [feature(
            "synthetic:model:feature#smooth",
            "MeshSmooth",
            Some(native.clone()),
        )];
        let codes = bind_inputs(
            &fixture,
            &mut features,
            MeshOperationInputs {
                scopes: std::slice::from_ref(&scope),
                owners: &owners,
                parameters: &parameters,
                placements: &[],
                sketches: &[],
            },
        );
        let FeatureDefinition::Operation(FeatureOperation::MeshModify {
            source:
                MeshSelection::Native {
                    native: selection,
                    members: Some(members),
                },
            modification: MeshModification::Smooth { smoothness },
        }) = features[0].evaluation.definition()
        else {
            panic!("mesh smooth");
        };
        assert_eq!(selection.as_str(), native);
        assert_eq!(members.as_slice(), ["9"]);
        assert_eq!(*smoothness, expected.and_then(Fraction::new));
        let mut expected_codes = vec![
            "feature.mesh-body-selection-assumed",
            "feature.mesh-operation-source-unresolved",
        ];
        if expected.is_none() {
            expected_codes.push("feature.mesh-operation-parameter-unbound");
        }
        assert_eq!(codes, expected_codes);
        assert_eq!(features[0].source_properties["mesh-input-geometry"], "20");
    }
}

#[test]
fn null_section_sketch_reference_ignores_zero_identity_records() {
    let native = format!("f3d:{ENTRY}:design-parameter-scope#0");
    let scopes = [DesignParameterScope::empty(
        &native,
        DesignFeatureKind::try_from("Sketch".to_owned()).unwrap(),
        0,
    )];
    let features = [feature(
        "synthetic:model:feature#sketch",
        "Sketch",
        Some(native),
    )];
    let placements = [placement(ENTRY, 0, false)];
    let inputs = MeshOperationInputs {
        scopes: &scopes,
        owners: &[],
        parameters: &[],
        placements: &placements,
        sketches: &[],
    };
    assert_eq!(
        section_binding(
            &inputs,
            &features,
            crate::ids::native_stream(&scopes[0].id),
            0
        ),
        (None, None)
    );
}
