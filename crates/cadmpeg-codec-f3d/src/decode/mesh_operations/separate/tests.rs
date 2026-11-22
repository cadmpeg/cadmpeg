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

fn fixture(
    selector: u32,
    words: [u32; 2],
    version: u32,
    change: impl FnOnce(&mut Vec<u8>),
) -> (Vec<u8>, Vec<u8>) {
    let mut scope = record(88, *b"256", 21);
    scope.extend_from_slice(&2u32.to_le_bytes());
    scope.extend_from_slice(&reference(41));
    scope.extend_from_slice(&reference(42));
    scope.extend_from_slice(&selector.to_le_bytes());
    for word in words {
        scope.extend_from_slice(&word.to_le_bytes());
    }
    scope.extend_from_slice(&reference(57));
    scope.push(0);
    crate::test_support::lp_utf16(&mut scope, "12345678-1357-2468-9876-123456789abc");
    scope.extend_from_slice(&[0; 3]);
    scope.extend_from_slice(&2u32.to_le_bytes());
    scope.extend_from_slice(&reference(57));
    scope.extend_from_slice(&reference(96));
    change(&mut scope);
    let mut group = record(57, *b"257", 21);
    group.extend_from_slice(&1u32.to_le_bytes());
    group.extend_from_slice(&reference(96));
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&17u32.to_le_bytes());
    group.extend_from_slice(&reference(74));
    group.extend_from_slice(&0x0000_0004_0000_0000u64.to_le_bytes());
    let mut member = record(96, *b"258", 21);
    member.extend_from_slice(&reference(101));
    member.extend_from_slice(&reference(74));
    member.extend_from_slice(&19u32.to_le_bytes());
    crate::test_support::lp_utf16(&mut member, "12345678-1357-2468-9876-123456789abc");
    crate::test_support::lp_utf16(&mut member, "abcdefab-cdef-abcd-efab-cdefabcdefab");
    member.extend_from_slice(&23u32.to_le_bytes());
    member.extend_from_slice(&29u32.to_le_bytes());
    let mut stream = Vec::new();
    let mut index = Vec::new();
    for (id, bytes) in [
        (88, scope),
        (57, group),
        (96, member),
        (41, record(41, *b"259", 11)),
        (42, record(42, *b"259", 11)),
        (101, record(101, *b"259", 11)),
        (74, record(74, *b"260", 11)),
    ] {
        index.push((id, u64_from_index(stream.len())));
        stream.extend(bytes);
    }
    let meta = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (
                "6B6FDE2E-D9B9-4866-9D07-3123C919B0D2",
                "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF",
                version,
                "Fusion",
                &[88],
            ),
            (
                "22975A91-16AD-4CC9-A5E4-B4F8B4C27881",
                "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
                0,
                "Fusion",
                &[57],
            ),
            (
                "1AA9CC7B-5C34-45DA-AE14-0444E7F966F2",
                "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                0,
                "Fusion",
                &[96],
            ),
            (
                "EA90DA22-556C-4C61-89BB-20C2681B7A9D",
                "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
                7,
                "ParaMesh",
                &[41, 42, 101],
            ),
            (
                "abcdefab-cdef-abcd-efab-cdefabcdefab",
                "12345678-1357-2468-9876-123456789abc",
                0,
                "Fusion",
                &[74],
            ),
        ],
        &index,
    );
    (stream, meta)
}

fn unbound_feature(scope_id: &str) -> Feature {
    Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("test:model:feature#separate").unwrap(),
        ordinal: 7,
        name: Some("Saved separation".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: [(
            cadmpeg_core::nonblank_literal!("prior"),
            "retained".to_owned(),
        )]
        .into(),
        source_tag: Some("MeshSeparate".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "MeshSeparate".into(),
                parameters: [(
                    cadmpeg_core::nonblank_literal!("distance"),
                    "2 * extent".to_owned(),
                )]
                .into(),
            }),
            vec![cadmpeg_ir::ids::BodyId::mint("test:model:body#previous").unwrap()]
                .try_into()
                .unwrap(),
        ),
        native_ref: Some(scope_id.to_owned()),
    }
}

fn project(stream: &[u8], metadata: Option<&[u8]>, scope_count: usize) -> (Feature, Vec<String>) {
    project_with(stream, metadata, scope_count, |_| {})
}

fn project_with(
    stream: &[u8],
    metadata: Option<&[u8]>,
    scope_count: usize,
    configure: impl FnOnce(&mut Feature),
) -> (Feature, Vec<String>) {
    let scope_id = format!("f3d:{ENTRY}:design-parameter-scope#0");
    let scope = DesignParameterScope::empty(
        &scope_id,
        DesignFeatureKind::try_from("MeshSeparate".to_owned()).unwrap(),
        88,
    );
    let mut feature = unbound_feature(&scope_id);
    configure(&mut feature);
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(stream).unwrap();
    if let Some(meta) = metadata {
        zip.start_file(ENTRY.replace("BulkStream.dat", "MetaStream.dat"), stored)
            .unwrap();
        zip.write_all(meta).unwrap();
    }
    let archive = zip.finish().unwrap().into_inner();
    let mut losses = Vec::new();
    let scopes = std::iter::repeat_n(scope, scope_count).collect::<Vec<_>>();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            bind_features(
                ctx,
                scan,
                std::slice::from_mut(&mut feature),
                &scopes,
                &mut losses,
            )
            .unwrap();
            let before = feature.clone();
            let count = losses.len();
            bind_features(
                ctx,
                scan,
                std::slice::from_mut(&mut feature),
                &scopes,
                &mut losses,
            )
            .unwrap();
            assert_eq!(feature, before);
            assert_eq!(losses.len(), count);
        });
    });
    (
        feature,
        losses
            .into_iter()
            .map(|note| note.code.local_code().to_owned())
            .collect(),
    )
}

fn separation(feature: &Feature) -> (&MeshSelection, &MeshSeparation) {
    let FeatureDefinition::Operation(FeatureOperation::MeshModify {
        source,
        modification: MeshModification::Separate { separation },
    }) = feature.evaluation.definition()
    else {
        panic!("neutral separation");
    };
    (source, separation)
}

#[test]
fn separate_projects_ordered_sources_outputs_and_auxiliary_facts() {
    let (stream, meta) = fixture(0, [0, 1], 2, |_| {});
    let (feature, codes) = project(&stream, Some(&meta), 1);
    let (source, settings) = separation(&feature);
    let MeshSelection::Native {
        native,
        members: Some(members),
    } = source
    else {
        panic!("native members");
    };
    assert_eq!(Some(native.as_str()), feature.native_ref.as_deref());
    assert_eq!(members.as_slice(), ["101"]);
    assert_eq!(settings.method, Some(MeshSeparationMethod::Shells));
    assert_eq!(settings.multiple_bodies, Some(false));
    assert_eq!(settings.keep_body, Some(true));
    assert_eq!(settings.groups, None);
    for (key, expected) in [
        ("mesh-separate-output:0:target", "41"),
        ("mesh-separate-output:1:target", "42"),
        ("mesh-separate-output:0:registration", "3"),
        ("mesh-separate-group:word:0", "17"),
        ("mesh-separate-group:role", "17179869184"),
        ("mesh-separate-group:reference:2:target", "74"),
        ("mesh-separate-member:0:reference:1:target", "74"),
        ("mesh-separate-member:0:word:0", "19"),
        ("mesh-separate-member:0:word:1", "23"),
        ("mesh-separate-member:0:word:2", "29"),
        (
            "mesh-separate-member:0:identifier:1",
            "abcdefab-cdef-abcd-efab-cdefabcdefab",
        ),
        ("prior", "retained"),
        ("parameter:distance", "2 * extent"),
    ] {
        assert_eq!(feature.source_properties[key], expected, "{key}");
    }
    assert_eq!(feature.ordinal, 7);
    assert_eq!(feature.name.as_deref(), Some("Saved separation"));
    assert_eq!(
        feature
            .evaluation
            .outputs()
            .iter()
            .map(cadmpeg_ir::ids::BodyId::as_str)
            .collect::<Vec<_>>(),
        ["test:model:body#previous"]
    );
    for suffix in [
        "shells-method-assumed",
        "output-role-assumed",
        "output-unresolved",
        "source-membership-assumed",
        "source-unresolved",
        "reference-roles-assumed",
        "source-fields-uninterpreted",
        "multiple-bodies-carrier-assumed",
        "multiple-bodies-polarity-assumed",
        "keep-body-carrier-assumed",
        "keep-body-polarity-assumed",
    ] {
        assert!(
            codes.contains(&format!("feature.mesh-separate-{suffix}")),
            "{suffix}"
        );
    }
}

#[test]
fn separate_methods_scope_and_each_boolean_project_independently() {
    for selector in [0, 1, 2, 0x0001_0000, 0x0002_0000, 93] {
        for words in [[0, 0], [1, 1], [0, 1], [1, 0], [7, 1], [1, 9]] {
            let (stream, meta) = fixture(selector, words, 2, |_| {});
            let (feature, codes) = project(&stream, Some(&meta), 1);
            let (_, settings) = separation(&feature);
            assert_eq!(
                settings.method,
                match selector {
                    0 => Some(MeshSeparationMethod::Shells),
                    0x0001_0000 | 0x0002_0000 => Some(MeshSeparationMethod::FaceGroups),
                    _ => None,
                }
            );
            assert_eq!(
                settings.groups,
                match selector {
                    0x0001_0000 => Some(MeshSeparationGroups::All {}),
                    0x0002_0000 => Some(MeshSeparationGroups::Selected { members: None }),
                    _ => None,
                }
            );
            assert_eq!(
                settings.multiple_bodies,
                (words[0] <= 1).then_some(words[0] == 1)
            );
            assert_eq!(settings.keep_body, (words[1] <= 1).then_some(words[1] == 1));
            for (ordinal, word) in words.iter().enumerate() {
                assert_eq!(
                    feature.source_properties
                        [format!("mesh-separate-lane-word:{ordinal}").as_str()],
                    word.to_string()
                );
            }
            if selector == 0x0002_0000 {
                assert!(codes.contains(&"feature.mesh-separate-selected-groups-assumed".to_owned()));
            }
            if selector == 0x0001_0000 {
                assert!(codes.contains(&"feature.mesh-separate-all-groups-assumed".to_owned()));
            }
            if matches!(selector, 1 | 2 | 93) || words.iter().any(|word| *word > 1) {
                assert!(codes.contains(&"feature.mesh-separate-selector-unruled".to_owned()));
            }
        }
    }
}

#[test]
fn separate_partial_controls_scope_failures_and_repetition_mismatches_survive() {
    let (stream, meta) = fixture(0x0001_0000, [1, 0], 2, |scope| scope.truncate(56));
    let (feature, codes) = project(&stream, Some(&meta), 1);
    assert_eq!(separation(&feature).1.multiple_bodies, Some(true));
    assert_eq!(separation(&feature).1.keep_body, None);
    assert_eq!(
        feature.source_properties["mesh-separate-lane"],
        "0100000000"
    );
    assert!(codes.contains(&"feature.mesh-separate-tail-partial".to_owned()));
    for (version, scope_count, metadata) in
        [(1, 1, true), (2, 0, true), (2, 2, true), (2, 1, false)]
    {
        let (stream, meta) = fixture(0, [0, 0], version, |_| {});
        let (feature, codes) = project(&stream, metadata.then_some(meta.as_slice()), scope_count);
        assert_eq!(separation(&feature).1, &MeshSeparation::default());
        if scope_count != 1 {
            assert_eq!(
                separation(&feature).0,
                &MeshSelection::Unresolved {
                    feature: feature.id.clone()
                }
            );
        }
        assert_eq!(feature.source_properties["prior"], "retained");
        assert_eq!(
            feature.source_properties["parameter:distance"],
            "2 * extent"
        );
        assert!(codes.contains(&"feature.mesh-separate-scope-unparsed".to_owned()));
    }
    let (stream, meta) = fixture(0, [0, 0], 2, |scope| {
        scope[155..159].copy_from_slice(&74u32.to_le_bytes());
    });
    let (feature, codes) = project(&stream, Some(&meta), 1);
    assert_eq!(
        feature.source_properties["mesh-separate-source:target"],
        "57"
    );
    assert_eq!(
        feature.source_properties["mesh-separate-common:0:target"],
        "74"
    );
    assert!(codes.contains(&"feature.mesh-separate-reference-mismatch".to_owned()));
}

#[test]
fn separate_null_invalid_and_duplicate_outputs_remain_native_positions() {
    for (change, state) in [(0, "null"), (1, "present"), (2, "present"), (3, "invalid")] {
        let (stream, meta) = fixture(0, [0, 0], 2, |scope| match change {
            0 => {
                scope.splice(25..36, [0]);
            }
            1 => scope[26..30].copy_from_slice(&888u32.to_le_bytes()),
            2 => scope[37..41].copy_from_slice(&41u32.to_le_bytes()),
            _ => {
                scope[25] = 9;
            }
        });
        let (feature, codes) = project(&stream, Some(&meta), 1);
        assert_eq!(
            feature.source_properties["mesh-separate-output:0:state"],
            state
        );
        assert_eq!(feature.evaluation.outputs().len(), 1);
        if change == 1 || change == 3 {
            assert!(codes.contains(&"feature.mesh-separate-reference-unresolved".to_owned()));
        }
        if change == 2 {
            assert_eq!(
                feature.source_properties["mesh-separate-output:1:target"],
                "41"
            );
        }
    }
}

#[test]
fn separate_incomplete_membership_retains_raw_members_controls_and_auxiliary_words() {
    for (record, relative) in [(96, 22), (57, 26)] {
        let (mut stream, meta) = fixture(0x0002_0000, [1, 0], 2, |_| {});
        let parsed = crate::metastream::parse(&meta, "synthetic metadata").unwrap();
        let at = usize::try_from(
            parsed
                .records
                .iter()
                .find(|entry| entry.entity_id == record)
                .unwrap()
                .bulk_offset,
        )
        .unwrap()
            + relative;
        stream[at..at + 4].copy_from_slice(&888u32.to_le_bytes());
        let (feature, codes) = project(&stream, Some(&meta), 1);
        assert!(matches!(
            separation(&feature).0,
            MeshSelection::Native { members: None, .. }
        ));
        assert_eq!(
            separation(&feature).1.groups,
            Some(MeshSeparationGroups::Selected { members: None })
        );
        if record == 96 {
            assert_eq!(
                feature.source_properties["mesh-separate-member:0:reference:0:target"],
                "888"
            );
            assert_eq!(
                feature.source_properties["mesh-separate-member:0:word:2"],
                "29"
            );
        } else {
            assert_eq!(
                feature.source_properties["mesh-separate-group:members:0:target"],
                "888"
            );
        }
        assert!(codes.contains(&"feature.mesh-separate-source-unresolved".to_owned()));
        assert!(codes.contains(&"feature.mesh-separate-membership-unresolved".to_owned()));
        assert!(codes.contains(&"feature.mesh-separate-reference-unresolved".to_owned()));
    }
    let (stream, meta) = fixture(0, [0, 0], 2, |scope| {
        scope[20] = 1;
        scope.splice(21..21, 77u32.to_le_bytes());
    });
    let (feature, codes) = project(&stream, Some(&meta), 1);
    assert_eq!(feature.source_properties["mesh-separate-prefix-word"], "77");
    assert_eq!(separation(&feature).1.keep_body, Some(false));
    assert!(codes.contains(&"feature.mesh-separate-prefix-assumed".to_owned()));
}

#[test]
fn separate_preserves_collisions_and_post_process_controls() {
    let (stream, meta) = fixture(0, [0, 1], 2, |_| {});
    let (feature, _) = project_with(&stream, Some(&meta), 1, |feature| {
        for (key, value) in [
            ("parameter:distance", "original"),
            ("parameter:distance:mesh-separate:0", "occupied"),
            ("mesh-separate-selector", "original-selector"),
        ] {
            feature
                .source_properties
                .insert(NonBlankString::new(key).unwrap(), value.into());
        }
        let operation = feature.evaluation.definition().operation().clone();
        feature
            .evaluation
            .set_definition(FeatureDefinition::PostProcess {
                operation,
                refine: true,
                fuzzy_tolerance:
                    cadmpeg_ir::features::operation_settings::FuzzyTolerance::Automatic,
            });
    });
    assert_eq!(feature.source_properties["parameter:distance"], "original");
    assert_eq!(
        feature.source_properties["parameter:distance:mesh-separate:0"],
        "occupied"
    );
    assert_eq!(
        feature.source_properties["parameter:distance:mesh-separate:1"],
        "2 * extent"
    );
    assert_eq!(
        feature.source_properties["mesh-separate-selector"],
        "original-selector"
    );
    assert_eq!(
        feature.source_properties["mesh-separate-selector:mesh-separate:0"],
        "0"
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::PostProcess {
            operation: FeatureOperation::MeshModify {
                modification: MeshModification::Separate { .. },
                ..
            },
            refine: true,
            fuzzy_tolerance: cadmpeg_ir::features::operation_settings::FuzzyTolerance::Automatic,
        }
    ));
}

#[test]
fn separate_identifier_text_obeys_retained_budget() {
    let bytes = [0, 0x4e].repeat(32);
    let source = SourceDetails {
        frame: View::over_retained(&[]),
        run: Run::default(),
        references: Vec::new(),
        words: Vec::new(),
        identifiers: vec![bytes.as_slice()],
        role: None,
        framing: None,
    };
    for (budget, refused) in [(200, true), (320, false)] {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = budget;
        let result = crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut notes = Notes {
                ctx,
                source: "synthetic",
                existing: &BTreeMap::new(),
                properties: Vec::new(),
                keys: HashSet::new(),
                losses: Vec::new(),
            };
            details(&mut notes, "member", &source)?;
            assert!(notes
                .properties
                .iter()
                .any(|(key, value)| key.as_str() == "member:identifier:0"
                    && value == &"一".repeat(32)));
            Ok(())
        });
        match result {
            Err(CodecError::ResourceLimit(limit)) if refused => {
                assert_eq!(limit.operation, "F3D Separate property value");
            }
            Ok(()) if !refused => (),
            other => panic!("unexpected result: {other:?}"),
        }
    }
}

#[test]
fn separate_staging_admits_exact_bytes_and_preserves_collisions() {
    let mut existing = BTreeMap::new();
    existing.insert(NonBlankString::new("key").unwrap(), "original".to_owned());
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1_000;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut notes = Notes {
            ctx,
            source: "synthetic",
            existing: &existing,
            properties: Vec::new(),
            keys: HashSet::new(),
            losses: Vec::new(),
        };
        notes
            .property(format_args!("key"), format_args!("first"))
            .unwrap();
        notes
            .property(format_args!("key"), format_args!("second"))
            .unwrap();
        assert_eq!(notes.properties[0].0.as_str(), "key:mesh-separate:0");
        assert_eq!(notes.properties[0].1, "first");
        assert_eq!(notes.properties[1].0.as_str(), "key:mesh-separate:1");
        assert_eq!(notes.properties[1].1, "second");
    });
    for (budget, succeeds) in [(10, false), (11, true)] {
        policy.limits.max_retained_bytes = budget;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let existing = BTreeMap::new();
            let mut notes = Notes {
                ctx,
                source: "synthetic",
                existing: &existing,
                properties: Vec::new(),
                keys: HashSet::new(),
                losses: Vec::new(),
            };
            let result = raw(&mut notes, "raw", &[0xab, 0xcd, 0xef, 0x01]);
            assert_eq!(result.is_ok(), succeeds);
            if succeeds {
                assert_eq!(notes.properties[0].1, "abcdef01");
            } else {
                assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
                assert!(notes.properties.is_empty());
            }
        });
    }
}

#[test]
fn separate_budget_refusal_does_not_mutate_feature_or_losses() {
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    let archive = zip.finish().unwrap().into_inner();
    for (budget, operation) in [
        (1, "F3D Separate property key"),
        (28, "F3D Separate loss message"),
    ] {
        let mut feature = unbound_feature("synthetic-scope");
        let original = feature.clone();
        let mut losses = Vec::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = budget;
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let result = crate::test_support::with_decode_policy(&policy, |ctx| {
                bind_features(
                    ctx,
                    scan,
                    std::slice::from_mut(&mut feature),
                    &[],
                    &mut losses,
                )
            });
            assert!(
                matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.operation == operation)
            );
        });
        assert_eq!(feature, original);
        assert!(losses.is_empty());
    }
}

#[test]
fn separate_parameter_text_is_admitted_before_copy() {
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    let archive = zip.finish().unwrap().into_inner();
    for name_is_long in [false, true] {
        let mut feature = unbound_feature("synthetic-scope");
        let name = if name_is_long {
            "n".repeat(256)
        } else {
            "n".to_owned()
        };
        let value = if name_is_long {
            "v".to_owned()
        } else {
            "v".repeat(256)
        };
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "MeshSeparate".into(),
                parameters: [(NonBlankString::new(name).unwrap(), value)].into(),
            }));
        let original = feature.clone();
        let mut losses = Vec::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 64;
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let result = crate::test_support::with_decode_policy(&policy, |ctx| {
                bind_features(
                    ctx,
                    scan,
                    std::slice::from_mut(&mut feature),
                    &[],
                    &mut losses,
                )
            });
            assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
        });
        assert_eq!(feature, original);
        assert!(losses.is_empty());
    }
}

#[test]
fn separate_raw_scope_refusal_leaves_feature_unchanged() {
    let (stream, meta) = fixture(0, [0, 1], 2, |_| {});
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(&stream).unwrap();
    zip.start_file(ENTRY.replace("BulkStream.dat", "MetaStream.dat"), stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let scope_id = format!("f3d:{ENTRY}:design-parameter-scope#0");
    let scope = DesignParameterScope::empty(
        &scope_id,
        DesignFeatureKind::try_from("MeshSeparate".to_owned()).unwrap(),
        88,
    );
    let mut feature = unbound_feature(&scope_id);
    let original = feature.clone();
    let mut losses = Vec::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 100;
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let result = crate::test_support::with_decode_policy(&policy, |ctx| {
            bind_features(
                ctx,
                scan,
                std::slice::from_mut(&mut feature),
                &[scope],
                &mut losses,
            )
        });
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.operation == "F3D Separate property value")
        );
    });
    assert_eq!(feature, original);
    assert!(losses.is_empty());
}
