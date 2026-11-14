use super::*;

const SCOPE: &str = "f3d:synthetic/BulkStream.dat:design-parameter-scope#17";

fn local(names: &[&str]) -> BodySelection {
    BodySelection::Local {
        bodies: NativeSelections::try_from(
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        native: SelectionReference::try_from(SCOPE.to_owned()).unwrap(),
    }
}

fn notes() -> Notes<'static> {
    Notes {
        source: SCOPE,
        properties: Vec::new(),
        losses: Vec::new(),
    }
}

#[test]
fn combine_boolean_controls_preserve_operands_and_name_both_default_assumptions() {
    for (control, expected) in [
        (0, BooleanKind::Join),
        (1, BooleanKind::Cut),
        (2, BooleanKind::Intersect),
    ] {
        let mut notes = notes();
        let operation = project(
            Some(control),
            local(&["7"]),
            local(&["9", "8"]),
            Some(SCOPE),
            &mut notes,
        )
        .unwrap();
        let FeatureOperation::Combine {
            operands,
            op,
            keep_tools,
            destination,
        } = operation
        else {
            panic!("expected Boolean combination")
        };
        assert_eq!(op, expected);
        assert_eq!(operands.target(), &local(&["7"]));
        assert_eq!(operands.tools(), &local(&["9", "8"]));
        assert!(!keep_tools);
        assert_eq!(destination, CombineDestination::TargetComponent);
        assert!(notes
            .losses
            .iter()
            .any(|note| note.code.local_code() == "feature.mesh-combine-keep-tools-assumed-false"));
        assert!(notes.losses.iter().any(|note| note.code.local_code()
            == "feature.mesh-combine-destination-assumed-target-component"));
    }
}

#[test]
fn combine_merge_keeps_target_before_ordered_tools_and_names_the_interpretation() {
    let mut notes = notes();
    let operation = project(
        Some(3),
        local(&["7"]),
        local(&["9", "8"]),
        Some(SCOPE),
        &mut notes,
    )
    .unwrap();
    assert_eq!(
        operation,
        FeatureOperation::Compound {
            members: local(&["7", "9", "8"])
        }
    );
    assert!(notes
        .losses
        .iter()
        .any(|note| note.code.local_code() == "feature.mesh-combine-merge-assumed"));
    assert!(notes
        .losses
        .iter()
        .any(|note| note.code.local_code() == "feature.mesh-combine-keep-tools-unresolved"));
    assert!(notes
        .losses
        .iter()
        .any(|note| note.code.local_code() == "feature.mesh-combine-destination-unresolved"));
}

#[test]
fn combine_rejected_arity_and_overlap_keep_the_known_boolean_kind() {
    for (target, tools) in [
        (local(&["7", "6"]), local(&["9"])),
        (local(&["7"]), local(&["7", "9"])),
    ] {
        let mut notes = notes();
        let operation = project(Some(1), target, tools, Some(SCOPE), &mut notes).unwrap();
        let FeatureOperation::Combine { operands, op, .. } = operation else {
            panic!("expected cut")
        };
        assert_eq!(op, BooleanKind::Cut);
        assert_eq!(operands.target(), &BodySelection::Unresolved);
        assert_eq!(operands.tools(), &BodySelection::Unresolved);
        assert!(notes
            .losses
            .iter()
            .any(|note| note.code.local_code() == "feature.mesh-combine-selection-unresolved"));
    }
}

#[test]
fn combine_unknown_controls_keep_a_neutral_family_without_default_assumptions() {
    for control in [None, Some(99)] {
        let mut notes = notes();
        let operation = project(
            control,
            BodySelection::Unresolved,
            BodySelection::Unresolved,
            None,
            &mut notes,
        )
        .unwrap();
        assert_eq!(
            operation,
            FeatureOperation::Unresolved {
                family: UnresolvedFamily::Combine
            }
        );
        assert_eq!(notes.losses.len(), 1);
        assert_eq!(
            notes.losses[0].code.local_code(),
            "feature.mesh-combine-selector-unruled"
        );
    }
}

#[test]
fn combine_unavailable_merge_sources_do_not_invent_a_scope_identity() {
    let mut notes = notes();
    let operation = project(
        Some(3),
        BodySelection::Unresolved,
        BodySelection::Unresolved,
        None,
        &mut notes,
    )
    .unwrap();
    assert_eq!(
        operation,
        FeatureOperation::Compound {
            members: BodySelection::Unresolved
        }
    );
    assert!(notes.properties.is_empty());
}

#[test]
fn combine_missing_scope_still_binds_a_neutral_feature_and_keeps_parameters() {
    let mut feature = Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("synthetic:model:feature#combine").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("MeshCombine".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "MeshCombine".into(),
                parameters: [(cadmpeg_core::nonblank_literal!("d1"), "1 mm".into())].into(),
            }),
        ),
        native_ref: None,
    };
    let original_id = feature.id.clone();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(
        &mut zip,
        crate::zip_write::file_options(zip::CompressionMethod::Stored),
    );
    let bytes = zip.finish().unwrap().into_inner();
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&bytes, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            bind_features(
                ctx,
                scan,
                std::slice::from_mut(&mut feature),
                &[],
                &mut losses,
            )
        })
        .unwrap();
    });
    assert_eq!(feature.id, original_id);
    assert_eq!(feature.native_ref, None);
    assert_eq!(
        feature
            .source_properties
            .get("parameter:d1")
            .map(String::as_str),
        Some("1 mm")
    );
    assert_eq!(
        feature.evaluation.definition(),
        &FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Combine
        },)
    );
    assert!(losses
        .iter()
        .any(|note| note.code.local_code() == "feature.mesh-combine-scope-unparsed"));
}

#[test]
fn combine_capture_preserves_group_boundaries_and_ordered_body_facts() {
    use crate::metastream::{MetaStream, RecordIndexEntry};
    use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    let mut bytes = Vec::new();
    let mut indexes = Vec::new();
    let mut append = |index: u32, tag: &[u8], target: Option<u32>, count: Option<u32>| {
        indexes.push(RecordIndexEntry {
            entity_id: u64::from(index),
            bulk_offset: u64::try_from(bytes.len()).unwrap(),
        });
        let mut record = vec![
            0;
            if count.is_some() {
                36
            } else if target.is_some() {
                32
            } else {
                11
            }
        ];
        record[..4].copy_from_slice(&3u32.to_le_bytes());
        record[4..7].copy_from_slice(tag);
        record[7..11].copy_from_slice(&index.to_le_bytes());
        if let Some(count) = count {
            record[21..25].copy_from_slice(&count.to_le_bytes());
        }
        if let Some(target) = target {
            let at = if count.is_some() { 25 } else { 21 };
            record[at] = 1;
            record[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
        }
        bytes.extend_from_slice(&record);
    };
    append(43, b"256", Some(53), Some(1));
    append(41, b"256", Some(51), Some(1));
    append(42, b"256", Some(52), Some(1));
    append(53, b"257", Some(9), None);
    append(51, b"257", Some(8), None);
    append(52, b"257", Some(7), None);
    append(9, b"258", None, None);
    append(8, b"258", None, None);
    append(7, b"258", None, None);
    let row = |guid: &str, base: &str, module: &str, entities: Vec<u64>| SegmentTypeData {
        byte_offset: 0,
        type_guid: guid.to_owned().try_into().unwrap(),
        type_guid_offset: 0,
        base_type_guid: BaseTypeGuid::Guid {
            value: base.to_owned().try_into().unwrap(),
            offset: 0,
        },
        version: 0,
        version_offset: 0,
        module: module.into(),
        entities: ReferenceRun::unlocated(entities),
    };
    let meta = std::sync::Arc::new(MetaStream {
        types: vec![
            row(
                "22975A91-16AD-4CC9-A5E4-B4F8B4C27881",
                "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
                "Fusion",
                vec![43, 41, 42],
            ),
            row(
                "1AA9CC7B-5C34-45DA-AE14-0444E7F966F2",
                "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                "Fusion",
                vec![53, 51, 52],
            ),
            row(
                "EA90DA22-556C-4C61-89BB-20C2681B7A9D",
                "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
                "ParaMesh",
                vec![9, 8, 7],
            ),
        ],
        records: indexes,
        secondary_records: Vec::new(),
    });
    let records =
        MeshOperationRecords::index(cadmpeg_core::decode::View::over_retained(&bytes), meta);
    let parsed = MeshCombineScope {
        version: 1,
        prefix: &[0; 9],
        flag: Some(0),
        prefix_word: None,
        control: Some(1),
        extra_word: None,
        raw_byte: Some(0),
        tool_count: Some(2),
        tools: vec![
            CombineReference::TypedLocal {
                target: 43,
                inline_type: "22975A91-16AD-4CC9-A5E4-B4F8B4C27881",
            },
            CombineReference::Local(41),
        ],
        output_geometry: Some(CombineReference::Local(61)),
        input_geometry: Some(CombineReference::Local(62)),
        tail_word: Some(0),
        target: Some(CombineReference::Local(42)),
        framing: None,
    };
    let mut notes = notes();
    let operation = crate::test_support::with_decode_context(|ctx| {
        captured(ctx, &records, &parsed, SCOPE, &mut notes)
    })
    .unwrap();
    let FeatureOperation::Combine { operands, op, .. } = operation else {
        panic!("expected cut")
    };
    assert_eq!(op, BooleanKind::Cut);
    assert_eq!(operands.target(), &local(&["7"]));
    assert_eq!(operands.tools(), &local(&["9", "8"]));
    assert!(notes
        .losses
        .iter()
        .any(|note| note.code.local_code() == "feature.mesh-combine-legacy-boolean-assumed"));
    for (key, value) in [
        ("mesh-combine:target:0:body:0", "7"),
        ("mesh-combine:tool:0:group", "43"),
        (
            "mesh-combine:tool:0:group:inline-type",
            "22975A91-16AD-4CC9-A5E4-B4F8B4C27881",
        ),
        ("mesh-combine:tool:1:group", "41"),
        ("mesh-combine:tool:0:member:0", "53"),
        ("mesh-combine:tool:0:body:0", "9"),
        ("mesh-combine:tool:1:body:0", "8"),
        ("mesh-combine:tool-count", "2"),
    ] {
        assert!(
            notes
                .properties
                .contains(&(key.to_owned(), value.to_owned())),
            "{key}"
        );
    }
}
