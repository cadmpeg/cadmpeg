use super::*;
use crate::records::feature::scope::DesignFeatureKind;
use cadmpeg_core::decode::View;
use std::io::{Cursor, Write};

const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

fn reference(target: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[0] = 1;
    bytes[1..5].copy_from_slice(&target.to_le_bytes());
    bytes
}

fn record(index: u32, tag: [u8; 3], len: usize) -> Vec<u8> {
    let mut bytes = vec![0; len];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(&tag);
    bytes[7..11].copy_from_slice(&index.to_le_bytes());
    bytes
}

fn scope() -> Vec<u8> {
    let mut bytes = record(7, *b"256", 21);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&16u32.to_le_bytes());
    for value in [
        0.0f64, -1.0, 0.0, 2.5, 1.0, 0.0, 0.0, -3.25, 0.0, 0.0, 2.0, 7.5, 0.0, 0.0, 0.0, 1.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&reference(8));
    bytes.push(0);
    for target in [91, 92, 93] {
        bytes.extend_from_slice(&reference(target));
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&reference(8));
    bytes.push(0);
    bytes
}

fn fixture(scope: Vec<u8>, second_member: u32) -> (Vec<u8>, Vec<u8>) {
    let mut group = record(8, *b"257", 25);
    group[21..25].copy_from_slice(&2u32.to_le_bytes());
    group.extend_from_slice(&reference(11));
    group.extend_from_slice(&reference(second_member));
    let mut first = record(11, *b"258", 21);
    first.extend_from_slice(&reference(31));
    let mut second = record(12, *b"258", 21);
    second.extend_from_slice(&reference(32));
    let mut stream = Vec::new();
    let mut index = Vec::new();
    for (id, bytes) in [
        (7, scope),
        (8, group),
        (11, first),
        (12, second),
        (31, record(31, *b"259", 11)),
        (32, record(32, *b"259", 11)),
    ] {
        index.push((id, u64::try_from(stream.len()).unwrap()));
        stream.extend_from_slice(&bytes);
    }
    let meta = crate::test_support::streams_fixture::design_metastream_with_records(
        &[
            (
                "E3EB3760-4362-48D6-ABF3-698B0C12C5A9",
                "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF",
                1,
                "Fusion",
                &[7],
            ),
            (
                MESH_GROUP_GUID,
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
                &[11, 12],
            ),
            (
                "EA90DA22-556C-4C61-89BB-20C2681B7A9D",
                "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
                7,
                "ParaMesh",
                &[31, 32],
            ),
        ],
        &index,
    );
    (stream, meta)
}

fn feature() -> Feature {
    let mut evaluation = cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "MeshPlaneCut".into(),
            parameters: [(cadmpeg_core::nonblank_literal!("distance"), "6 mm".into())].into(),
        }),
    );
    evaluation.set_outputs(
        vec![cadmpeg_ir::ids::BodyId::mint("test:model:body#result").unwrap()]
            .try_into()
            .unwrap(),
    );
    Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("test:model:feature#cut").unwrap(),
        ordinal: 3,
        name: Some("Synthetic cut".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: [(
            cadmpeg_core::nonblank_literal!("existing"),
            "retained".into(),
        )]
        .into(),
        source_tag: Some("MeshPlaneCut".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation,
        native_ref: Some(format!("f3d:{ENTRY}:design-parameter-scope#0")),
    }
}

fn bind(
    stream: &[u8],
    metadata: Option<&[u8]>,
    scope_count: usize,
    feature: &mut Feature,
) -> Vec<LossNote> {
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_fixture::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(stream).unwrap();
    if let Some(meta) = metadata {
        zip.start_file(ENTRY.replace("BulkStream.dat", "MetaStream.dat"), stored)
            .unwrap();
        zip.write_all(meta).unwrap();
    }
    let archive = zip.finish().unwrap().into_inner();
    let scope = DesignParameterScope::empty(
        &format!("f3d:{ENTRY}:design-parameter-scope#0"),
        DesignFeatureKind::try_from("MeshPlaneCut".to_owned()).unwrap(),
        7,
    );
    let scopes = std::iter::repeat_n(scope, scope_count).collect::<Vec<_>>();
    let mut losses = Vec::new();
    crate::test_support::zip_fixture::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            bind_features(
                ctx,
                scan,
                std::slice::from_mut(feature),
                &scopes,
                &mut losses,
            )
        })
        .unwrap();
    });
    losses
}

fn parts(feature: &Feature) -> (&MeshSelection, &MeshPlaneCut) {
    let FeatureDefinition::Operation(FeatureOperation::MeshModify {
        source,
        modification: MeshModification::PlaneCut { cut },
    }) = feature.evaluation.definition()
    else {
        panic!("plane cut")
    };
    (source, cut)
}

#[test]
fn binding_preserves_feature_outputs_parameters_and_historical_native_members() {
    let (stream, meta) = fixture(scope(), 12);
    let mut feature = feature();
    let previous = feature.clone();
    let losses = bind(&stream, Some(&meta), 1, &mut feature);
    let (source, cut) = parts(&feature);
    let MeshSelection::Native {
        members: Some(members),
        ..
    } = source
    else {
        panic!("native membership")
    };
    assert_eq!(members.as_slice(), ["31", "32"]);
    assert_eq!(cut.origin.unwrap().get(), Point3::new(25.0, -32.5, 75.0));
    assert_eq!(cut.normal.unwrap().get(), Vector3::new(0.0, 0.0, 2.0));
    assert_eq!(cut.mode, Some(MeshPlaneCutMode::Trim));
    assert_eq!(cut.fill, Some(MeshPlaneCutFill::Minimal));
    assert_eq!(cut.retained_side, Some(MeshPlaneCutSide::Positive));
    assert_eq!(feature.id, previous.id);
    assert_eq!(feature.name, previous.name);
    assert_eq!(feature.ordinal, previous.ordinal);
    assert_eq!(feature.evaluation.outputs(), previous.evaluation.outputs());
    assert_eq!(
        feature.source_properties.get("parameter:distance").unwrap(),
        "6 mm"
    );
    assert_eq!(
        feature.source_properties.get("existing").unwrap(),
        "retained"
    );
    for code in [
        "membership-assumed",
        "source-unresolved",
        "matrix-orientation-assumed",
        "units-assumed",
        "model-space-assumed",
        "retained-side-assumed",
    ] {
        assert!(losses
            .iter()
            .any(|loss| loss.code.local_code() == format!("feature.mesh-plane-cut-{code}")));
    }
    assert_eq!(
        feature
            .source_properties
            .get("mesh-plane-cut:output-group")
            .unwrap(),
        "null"
    );
}

#[test]
fn missing_members_preserve_ordinals_and_counts_without_zero_identities() {
    let (stream, meta) = fixture(scope(), 77);
    let mut feature = feature();
    let losses = bind(&stream, Some(&meta), 1, &mut feature);
    assert!(matches!(
        parts(&feature).0,
        MeshSelection::Native { members: None, .. }
    ));
    for (key, value) in [
        ("source-member-count", "2"),
        ("source-member:0", "11"),
        ("source-body:0", "31"),
        ("source-member:1", "77"),
        ("source-body:1", "unread"),
    ] {
        assert_eq!(
            feature
                .source_properties
                .get(format!("mesh-plane-cut:{key}").as_str())
                .unwrap(),
            value
        );
    }
    assert!(losses
        .iter()
        .any(|loss| loss.code.local_code() == "feature.mesh-plane-cut-selection-unresolved"));
}

#[test]
fn a_missing_first_member_keeps_the_later_body_at_its_original_ordinal() {
    let (mut stream, meta) = fixture(scope(), 12);
    let first = scope().len() + 25;
    stream[first..first + 11].copy_from_slice(&reference(77));
    let mut feature = feature();
    bind(&stream, Some(&meta), 1, &mut feature);
    assert!(matches!(
        parts(&feature).0,
        MeshSelection::Native { members: None, .. }
    ));
    assert_eq!(
        feature
            .source_properties
            .get("mesh-plane-cut:source-body:0")
            .unwrap(),
        "unread"
    );
    assert_eq!(
        feature
            .source_properties
            .get("mesh-plane-cut:source-body:1")
            .unwrap(),
        "32"
    );
}

#[test]
fn absent_or_ambiguous_scopes_and_missing_definitions_keep_actual_feature_anchors() {
    let (stream, meta) = fixture(scope(), 12);
    for (metadata, scope_count) in [
        (None, 1),
        (Some(&[][..]), 1),
        (Some(meta.as_slice()), 0),
        (Some(meta.as_slice()), 2),
    ] {
        let mut feature = feature();
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: cadmpeg_ir::features::UnresolvedFamily::Brep,
            }));
        bind(&stream, metadata, scope_count, &mut feature);
        assert_eq!(parts(&feature).1, &MeshPlaneCut::default());
        assert_eq!(
            parts(&feature).0,
            &MeshSelection::Unresolved {
                feature: feature.id.clone()
            }
        );
    }
}

#[test]
fn plane_and_controls_survive_partial_and_semantically_invalid_components() {
    let bytes = scope();
    for (mode, fill, side) in [(0, 0, 0), (1, 1, 1), (2, 2, 0), (99, 98, 97)] {
        let mut bytes = bytes.clone();
        bytes[21..25].copy_from_slice(&u32::to_le_bytes(mode));
        bytes[157..161].copy_from_slice(&u32::to_le_bytes(fill));
        bytes[161] = side;
        let parsed = crate::design::decode::mesh_operations::plane_cut::read_scope(
            View::over_retained(&bytes),
        );
        let mut notes = Notes {
            source: "synthetic",
            properties: Vec::new(),
            losses: Vec::new(),
        };
        let cut = controls(&parsed, &mut notes);
        assert_eq!(
            cut.mode,
            match mode {
                0 => Some(MeshPlaneCutMode::Trim),
                1 => Some(MeshPlaneCutMode::SplitBodies),
                2 => Some(MeshPlaneCutMode::SplitFaces),
                _ => None,
            }
        );
        assert_eq!(
            cut.fill,
            match fill {
                0 => Some(MeshPlaneCutFill::NoFill),
                1 => Some(MeshPlaneCutFill::Minimal),
                2 => Some(MeshPlaneCutFill::Uniform),
                _ => None,
            }
        );
        assert_eq!(
            cut.retained_side,
            match side {
                0 => Some(MeshPlaneCutSide::Positive),
                1 => Some(MeshPlaneCutSide::Negative),
                _ => None,
            }
        );
        for code in if mode == 2 {
            vec!["split-faces-assumed", "uniform-fill-assumed"]
        } else if mode == 99 {
            vec!["selector-unruled"]
        } else {
            Vec::new()
        } {
            assert!(notes
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == format!("feature.mesh-plane-cut-{code}")));
        }
    }
    for (end, origin, normal) in [(117, false, true), (125, true, true)] {
        let parsed = crate::design::decode::mesh_operations::plane_cut::read_scope(
            View::over_retained(&bytes[..end]),
        );
        let mut notes = Notes {
            source: "synthetic",
            properties: Vec::new(),
            losses: Vec::new(),
        };
        let cut = controls(&parsed, &mut notes);
        assert_eq!(cut.origin.is_some(), origin);
        assert_eq!(cut.normal.is_some(), normal);
        assert_eq!(cut.mode, Some(MeshPlaneCutMode::Trim));
    }
    let mut bytes = bytes;
    bytes[109..117].copy_from_slice(&f64::NAN.to_le_bytes());
    let (stream, meta) = fixture(bytes, 12);
    let mut feature = feature();
    bind(&stream, Some(&meta), 1, &mut feature);
    assert!(parts(&feature).1.normal.is_none());
    assert!(parts(&feature).1.origin.is_some());
    assert_eq!(parts(&feature).1.fill, Some(MeshPlaneCutFill::Minimal));
}

#[test]
fn source_group_resource_refusal_remains_a_resource_error() {
    let (stream, meta) = fixture(scope(), 12);
    let meta = crate::metastream::parse(&meta, "synthetic");
    let records = MeshOperationRecords::index(
        View::over_retained(&stream),
        std::sync::Arc::new(meta.unwrap()),
    );
    let parsed = records.plane_cut_scope(7).unwrap();
    let mut notes = Notes {
        source: "synthetic",
        properties: Vec::new(),
        losses: Vec::new(),
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = 0;
    let result = crate::test_support::with_decode_policy(&policy, |ctx| {
        captured(ctx, &records, &parsed, &mut notes)
    });
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.operation == "F3D mesh plane cut member facts")
    );
}

#[test]
fn source_facts_survive_a_later_reference_failure_and_output_members_stay_provenance() {
    for output in [false, true] {
        let mut bytes = scope();
        if output {
            bytes.splice(173..174, reference(8));
        } else {
            bytes.truncate(178);
        }
        let (stream, meta) = fixture(bytes, 12);
        let mut feature = feature();
        let losses = bind(&stream, Some(&meta), 1, &mut feature);
        let MeshSelection::Native {
            members: Some(members),
            ..
        } = parts(&feature).0
        else {
            panic!("source membership")
        };
        assert_eq!(members.as_slice(), ["31", "32"]);
        assert_eq!(parts(&feature).1.fill, Some(MeshPlaneCutFill::Minimal));
        if output {
            assert_eq!(
                feature
                    .source_properties
                    .get("mesh-plane-cut:output-member-count")
                    .unwrap(),
                "2"
            );
            assert_eq!(
                feature
                    .source_properties
                    .get("mesh-plane-cut:output-body:1")
                    .unwrap(),
                "32"
            );
        } else {
            assert!(losses
                .iter()
                .any(|loss| loss.code.local_code() == "feature.mesh-plane-cut-tail-partial"));
        }
    }
}

#[test]
fn missing_group_and_zero_normal_preserve_other_checked_facts() {
    let mut bytes = scope();
    bytes[162..173].copy_from_slice(&reference(77));
    bytes[109..117].copy_from_slice(&0.0f64.to_le_bytes());
    let (stream, meta) = fixture(bytes, 12);
    let mut feature = feature();
    bind(&stream, Some(&meta), 1, &mut feature);
    assert!(matches!(
        parts(&feature).0,
        MeshSelection::Native { members: None, .. }
    ));
    assert!(parts(&feature).1.normal.is_none());
    assert!(parts(&feature).1.origin.is_some());
    assert_eq!(parts(&feature).1.mode, Some(MeshPlaneCutMode::Trim));
    assert_eq!(
        feature
            .source_properties
            .get("mesh-plane-cut:source-group")
            .unwrap(),
        "77"
    );
}

#[test]
fn admitted_scope_supplies_controls_without_a_native_definition_and_other_tags_stay_unchanged() {
    let (stream, meta) = fixture(scope(), 12);
    let mut feature = feature();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: cadmpeg_ir::features::UnresolvedFamily::Brep,
        }));
    bind(&stream, Some(&meta), 1, &mut feature);
    assert_eq!(parts(&feature).1.mode, Some(MeshPlaneCutMode::Trim));
    feature.source_tag = Some("Other".into());
    let previous = feature.clone();
    assert!(bind(&stream, Some(&meta), 1, &mut feature).is_empty());
    assert_eq!(feature, previous);
}
