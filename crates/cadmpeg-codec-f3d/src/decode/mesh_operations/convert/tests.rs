use super::*;
use crate::records::feature::scope::DesignFeatureKind;
use std::io::{Cursor, Write};

const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

fn reference(target: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[0] = 1;
    bytes[1..5].copy_from_slice(&target.to_le_bytes());
    bytes
}

fn record(index: u32, tag: [u8; 3], length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(&tag);
    bytes[7..11].copy_from_slice(&index.to_le_bytes());
    bytes
}

fn fixture(scope_version: u32, input_version: u32) -> (Vec<u8>, Vec<u8>) {
    let mut scope = record(7, *b"256", 25);
    scope[20] = 1;
    scope.extend_from_slice(&3u32.to_le_bytes());
    scope.extend_from_slice(&2u32.to_le_bytes());
    scope.extend_from_slice(&reference(12));
    scope.extend_from_slice(&reference(11));
    scope.extend_from_slice(&[255, 255, 255, 255, 0, 0]);
    scope.extend_from_slice(&0u32.to_le_bytes());
    scope.extend_from_slice(&2u32.to_le_bytes());
    scope.extend_from_slice(&reference(14));
    scope.extend_from_slice(&reference(13));
    scope.extend_from_slice(&[0; 8]);
    scope.extend_from_slice(&36u32.to_le_bytes());
    for word in "00000000-0000-0000-0000-000000000000".encode_utf16() {
        scope.extend_from_slice(&word.to_le_bytes());
    }
    let mut frames = vec![(7u64, scope)];
    for (wrapper, group) in [(11, 8), (12, 9)] {
        let mut bytes = record(wrapper, *b"257", 38);
        bytes[22..33].copy_from_slice(&reference(group));
        frames.push((u64::from(wrapper), bytes));
    }
    for (group, member) in [(8, 21), (9, 22)] {
        let mut bytes = record(group, *b"258", 36);
        bytes[21..25].copy_from_slice(&1u32.to_le_bytes());
        bytes[25..36].copy_from_slice(&reference(member));
        frames.push((u64::from(group), bytes));
    }
    for (member, body) in [(21, 31), (22, 32)] {
        let mut bytes = record(member, *b"259", 32);
        bytes[21..32].copy_from_slice(&reference(body));
        frames.push((u64::from(member), bytes));
    }
    for body in [31, 32] {
        frames.push((u64::from(body), record(body, *b"260", 11)));
    }
    for (wrapper, body) in [(13, 51), (14, 52)] {
        let mut bytes = record(wrapper, *b"261", 25);
        bytes[21..25].copy_from_slice(&36u32.to_le_bytes());
        for word in "11111111-1111-1111-1111-111111111111".encode_utf16() {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[7, 1]);
        bytes.extend_from_slice(&reference(body));
        frames.push((u64::from(wrapper), bytes));
    }
    let mut stream = Vec::new();
    let mut index = Vec::new();
    for (id, bytes) in frames {
        index.push((id, u64::try_from(stream.len()).unwrap()));
        stream.extend_from_slice(&bytes);
    }
    let meta = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (
                "62B85DC3-27B5-47BA-9F13-B2527B72EE49",
                "2EDC2536-D95B-4469-8ACA-DC07124D9C43",
                scope_version,
                "Fusion",
                &[7],
            ),
            (
                "D6FB6F05-98CD-4BFF-BF90-594F77725DC1",
                "D0D7E795-ECF7-48A8-A7A3-F983C5A29F8B",
                input_version,
                "Fusion",
                &[11, 12],
            ),
            (
                "22975A91-16AD-4CC9-A5E4-B4F8B4C27881",
                "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
                0,
                "Fusion",
                &[8, 9],
            ),
            (
                "1AA9CC7B-5C34-45DA-AE14-0444E7F966F2",
                "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                0,
                "Fusion",
                &[21, 22],
            ),
            (
                "EA90DA22-556C-4C61-89BB-20C2681B7A9D",
                "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
                7,
                "ParaMesh",
                &[31, 32],
            ),
            (
                "D26351F0-5940-4D23-AA20-2C35475A6D9E",
                "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
                2,
                "Fusion",
                &[13, 14],
            ),
        ],
        &index,
    );
    (stream, meta)
}

fn project(stream: &[u8], metadata: Option<&[u8]>, scoped: bool) -> (Feature, Vec<String>) {
    let scope_id = format!("f3d:{ENTRY}:design-parameter-scope#0");
    let scope = DesignParameterScope::empty(
        &scope_id,
        DesignFeatureKind::try_from("Convert Mesh".to_owned()).unwrap(),
        7,
    );
    let mut feature = Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("test:model:feature#convert").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Convert Mesh".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Convert Mesh".into(),
                parameters: [(
                    cadmpeg_core::nonblank_literal!("tolerance"),
                    "0.1 mm".to_owned(),
                )]
                .into(),
            }),
        ),
        native_ref: scoped.then_some(scope_id),
    };
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(stream).unwrap();
    if let Some(metadata) = metadata {
        zip.start_file(ENTRY.replace("BulkStream.dat", "MetaStream.dat"), stored)
            .unwrap();
        zip.write_all(metadata).unwrap();
    }
    let archive = zip.finish().unwrap().into_inner();
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            bind_features(
                ctx,
                scan,
                std::slice::from_mut(&mut feature),
                &[scope],
                &mut losses,
            )
        })
        .unwrap();
    });
    (
        feature,
        losses
            .into_iter()
            .map(|note| note.code.local_code().to_owned())
            .collect(),
    )
}

fn sources(feature: &Feature) -> &[MeshSelection] {
    let FeatureDefinition::Operation(FeatureOperation::MeshConvert(conversion)) =
        feature.evaluation.definition()
    else {
        panic!("mesh conversion");
    };
    &conversion.sources
}

#[test]
fn conversion_projects_ordered_input_contexts_and_readable_output_identities() {
    let (stream, meta) = fixture(4, 1);
    let (feature, codes) = project(&stream, Some(&meta), true);
    assert_eq!(sources(&feature).len(), 2);
    for (index, expected) in ["32", "31"].into_iter().enumerate() {
        let MeshSelection::Native {
            native,
            members: Some(members),
        } = &sources(&feature)[index]
        else {
            panic!("native input");
        };
        assert_eq!(members.as_slice(), [expected]);
        assert_eq!(Some(native.as_str()), feature.native_ref.as_deref());
    }
    for (key, expected) in [
        ("mesh-conversion-control", "3"),
        ("mesh-conversion-input-count", "2"),
        ("mesh-conversion-input:0", "12"),
        ("mesh-conversion-group:0", "9"),
        ("mesh-conversion-body:0:0", "32"),
        ("mesh-conversion-output:0", "14"),
        ("mesh-conversion-output-body:0", "52"),
        ("mesh-conversion-output-control:0", "7"),
        ("parameter:tolerance", "0.1 mm"),
    ] {
        assert_eq!(feature.source_properties[key], expected);
    }
    assert_eq!(
        feature.source_properties["mesh-conversion-output-guid:1"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert!(codes.contains(&"feature.mesh-conversion-method-assumed".to_owned()));
    assert!(codes.contains(&"feature.mesh-conversion-envelope-assumed".to_owned()));
    assert_eq!(
        codes
            .iter()
            .filter(|code| code.as_str() == "feature.mesh-conversion-output-unresolved")
            .count(),
        2
    );
}

#[test]
fn conversion_methods_read_the_named_control_values_and_retain_unknown_values() {
    for (value, expected) in [
        (1u32, Some(MeshConversionMethod::Prismatic)),
        (3u32, Some(MeshConversionMethod::Faceted)),
        (7u32, None),
    ] {
        let (mut stream, meta) = fixture(4, 1);
        stream[25..29].copy_from_slice(&value.to_le_bytes());
        let (feature, codes) = project(&stream, Some(&meta), true);
        let FeatureDefinition::Operation(FeatureOperation::MeshConvert(conversion)) =
            feature.evaluation.definition()
        else {
            panic!("mesh conversion");
        };
        assert_eq!(conversion.method, expected);
        assert_eq!(
            feature.source_properties["mesh-conversion-control"],
            value.to_string()
        );
        let code = if expected.is_some() {
            "feature.mesh-conversion-method-assumed"
        } else {
            "feature.mesh-conversion-method-unresolved"
        };
        assert!(codes.contains(&code.to_owned()));
    }
}

#[test]
fn conversion_missing_definition_still_projects_a_typed_operation() {
    let (stream, meta) = fixture(4, 1);
    for (metadata, scoped) in [(None, true), (Some(meta.as_slice()), false)] {
        let (feature, codes) = project(&stream, metadata, scoped);
        assert_eq!(
            sources(&feature),
            [MeshSelection::Unresolved {
                feature: feature.id.clone()
            }]
        );
        assert!(codes.contains(&"feature.mesh-conversion-scope-unparsed".to_owned()));
        assert_eq!(feature.source_properties["parameter:tolerance"], "0.1 mm");
    }
}

#[test]
fn conversion_wrong_wrapper_version_keeps_both_input_contexts() {
    let (stream, meta) = fixture(4, 2);
    let (feature, codes) = project(&stream, Some(&meta), true);
    assert_eq!(sources(&feature).len(), 2);
    assert!(sources(&feature)
        .iter()
        .all(|source| matches!(source, MeshSelection::Native { members: None, .. })));
    assert!(codes.contains(&"feature.mesh-conversion-source-unresolved".to_owned()));
    let (stream, meta) = fixture(3, 1);
    let (feature, codes) = project(&stream, Some(&meta), true);
    assert!(matches!(
        sources(&feature),
        [MeshSelection::Unresolved { .. }]
    ));
    assert!(codes.contains(&"feature.mesh-conversion-scope-unparsed".to_owned()));
}

#[test]
fn conversion_missing_first_input_keeps_the_later_input() {
    let (mut stream, meta) = fixture(4, 1);
    stream[34..38].copy_from_slice(&99u32.to_le_bytes());
    let (feature, codes) = project(&stream, Some(&meta), true);
    assert!(matches!(
        sources(&feature)[0],
        MeshSelection::Native { members: None, .. }
    ));
    let MeshSelection::Native {
        members: Some(members),
        ..
    } = &sources(&feature)[1]
    else {
        panic!("second input");
    };
    assert_eq!(members.as_slice(), ["31"]);
    assert_eq!(feature.source_properties["mesh-conversion-input:0"], "99");
    assert!(codes.contains(&"feature.mesh-conversion-source-unresolved".to_owned()));
}

#[test]
fn conversion_keeps_a_readable_body_reference_with_an_unusable_definition() {
    let (mut stream, meta) = fixture(4, 1);
    stream[405..409].copy_from_slice(&99u32.to_le_bytes());
    let (feature, codes) = project(&stream, Some(&meta), true);
    assert_eq!(feature.source_properties["mesh-conversion-body:0:0"], "32");
    assert_eq!(
        feature.source_properties["mesh-conversion-member-count:0"],
        "1"
    );
    assert!(matches!(
        sources(&feature)[0],
        MeshSelection::Native { members: None, .. }
    ));
    let MeshSelection::Native {
        members: Some(members),
        ..
    } = &sources(&feature)[1]
    else {
        panic!("later input");
    };
    assert_eq!(members.as_slice(), ["31"]);
    assert!(codes.contains(&"feature.mesh-conversion-source-unresolved".to_owned()));
}

#[test]
fn conversion_keeps_a_member_reference_when_its_definition_is_missing() {
    let (mut stream, meta) = fixture(4, 1);
    stream[362..366].copy_from_slice(&99u32.to_le_bytes());
    let (feature, codes) = project(&stream, Some(&meta), true);
    assert!(matches!(
        sources(&feature)[0],
        MeshSelection::Native { members: None, .. }
    ));
    let MeshSelection::Native {
        members: Some(members),
        ..
    } = &sources(&feature)[1]
    else {
        panic!("later input");
    };
    assert_eq!(members.as_slice(), ["31"]);
    assert_eq!(
        feature
            .source_properties
            .get("mesh-conversion-member:0:0")
            .map(String::as_str),
        Some("22")
    );
    assert!(codes.contains(&"feature.mesh-conversion-source-unresolved".to_owned()));
}

#[test]
fn conversion_keeps_an_output_prefix_and_later_output_when_its_body_reference_is_missing() {
    let (mut stream, meta) = fixture(4, 1);
    stream.truncate(stream.len() - 11);
    let (feature, codes) = project(&stream, Some(&meta), true);
    assert_eq!(
        feature.source_properties["mesh-conversion-output-guid:0"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(
        feature.source_properties["mesh-conversion-output-control:0"],
        "7"
    );
    assert!(!feature
        .source_properties
        .contains_key("mesh-conversion-output-body:0"));
    assert_eq!(
        feature.source_properties["mesh-conversion-output-body:1"],
        "51"
    );
    assert!(codes.contains(&"feature.mesh-conversion-output-unresolved".to_owned()));
}
