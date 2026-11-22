use super::*;
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::parameters::{
    DesignParameterDraft, DesignParameterOwnerWire, DesignParameterSource,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
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

fn fixture(operand_version: u32, null_second_operand: bool) -> (Vec<u8>, Vec<u8>) {
    let mut scope = record(7, *b"256", 21);
    for owner in [11, 31, 32, 33] {
        scope.extend_from_slice(&reference(owner));
    }
    if null_second_operand {
        scope.extend_from_slice(&2u32.to_le_bytes());
        scope.extend_from_slice(&reference(20));
        scope.push(0);
    } else {
        scope.extend_from_slice(&1u32.to_le_bytes());
        scope.extend_from_slice(&reference(20));
    }
    scope.extend_from_slice(&[1, 1, 0, 0, 0, 0]);
    scope.extend_from_slice(&36u32.to_le_bytes());
    for word in "00000000-0000-0000-0000-000000000000".encode_utf16() {
        scope.extend_from_slice(&word.to_le_bytes());
    }
    let mut operand = record(20, *b"257", 21);
    operand.extend_from_slice(&reference(21));
    operand.extend_from_slice(&reference(22));
    operand.extend_from_slice(&[1, 1, 0, 0, 0, 0]);
    let mut stream = Vec::new();
    let mut index = Vec::new();
    for (id, bytes) in [(7u64, scope), (20, operand)] {
        index.push((id, u64::try_from(stream.len()).unwrap()));
        stream.extend_from_slice(&bytes);
    }
    let meta = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (
                "86133A9B-B604-4283-B9BA-C5D365F72763",
                "95644CD5-1338-40F2-8016-1FB68DB06296",
                3,
                "Fusion",
                &[7],
            ),
            (
                "7102E543-AD48-4709-B559-286F5F3A7189",
                "D0D7E795-ECF7-48A8-A7A3-F983C5A29F8B",
                operand_version,
                "Fusion",
                &[20],
            ),
        ],
        &index,
    );
    (stream, meta)
}

fn parameter_inputs() -> ([DesignParameterOwner; 1], [DesignParameter; 1]) {
    let owners = [DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("f3d:{ENTRY}:parameter-owner#11"),
        byte_offset: 0,
        frame_length: 103,
        class_tag: "292".to_owned().try_into().unwrap(),
        record_index: 11,
        scope_record_index: 7,
        local_ordinal: 0,
        evaluated_value: 1.0,
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
        expression: "10 mm".into(),
        expression_offset: 20,
        source_kind_offset: 40,
        unit: Some(crate::records::identity::RecordedValue {
            value: "mm".into(),
            offset: 80,
        }),
        name: "d4".into(),
        name_offset: 90,
        evaluated_value: 1.0,
        evaluated_value_offset: 120,
    })
    .unwrap()];
    (owners, parameters)
}

fn native() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Native {
        kind: "MeshTessellate".into(),
        parameters: [(cadmpeg_core::nonblank_literal!("d4"), "10 mm".to_owned())].into(),
    })
}

fn project(stream: &[u8], metadata: Option<&[u8]>) -> (Feature, Vec<String>) {
    let scope_id = format!("f3d:{ENTRY}:design-parameter-scope#0");
    let scope = DesignParameterScope::empty(
        &scope_id,
        DesignFeatureKind::try_from("MeshTessellate".to_owned()).unwrap(),
        7,
    );
    let mut feature = Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("test:model:feature#tessellate").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("MeshTessellate".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(native()),
        native_ref: Some(scope_id),
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
    let (owners, parameters) = parameter_inputs();
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            bind_features(
                ctx,
                scan,
                std::slice::from_mut(&mut feature),
                &TessellateInputs {
                    scopes: std::slice::from_ref(&scope),
                    owners: &owners,
                    parameters: &parameters,
                },
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

#[test]
fn tessellation_records_owner_roles_operand_group_and_produced_body() {
    let (stream, meta) = fixture(1, false);
    let (feature, codes) = project(&stream, Some(&meta));
    assert_eq!(feature.evaluation.definition(), &native());
    for (key, expected) in [
        ("mesh-tessellate-owner:maximum-edge-length", "11"),
        ("mesh-tessellate-parameter:maximum-edge-length", "d4"),
        ("mesh-tessellate-owner:normal-deviation", "31"),
        ("mesh-tessellate-owner:aspect-ratio", "32"),
        ("mesh-tessellate-owner:surface-deviation", "33"),
        ("mesh-tessellate-operand-count", "1"),
        ("mesh-tessellate-refinement", "1"),
        ("mesh-tessellate-operand:0", "20"),
        ("mesh-tessellate-group:0", "21"),
        ("mesh-tessellate-body:0", "22"),
    ] {
        assert_eq!(feature.source_properties[key], expected, "{key}");
    }
    assert!(!feature
        .source_properties
        .contains_key("mesh-tessellate-parameter:normal-deviation"));
    assert_eq!(
        codes,
        [
            "feature.mesh-operation-parameter-unbound",
            "feature.mesh-operation-parameter-unbound",
            "feature.mesh-operation-parameter-unbound",
            "feature.mesh-tessellate-source-unresolved",
        ]
    );
}

#[test]
fn tessellation_with_an_unreadable_operand_keeps_the_scope_fields() {
    let (stream, meta) = fixture(2, false);
    let (feature, codes) = project(&stream, Some(&meta));
    assert_eq!(feature.evaluation.definition(), &native());
    assert_eq!(feature.source_properties["mesh-tessellate-operand:0"], "20");
    assert!(!feature
        .source_properties
        .contains_key("mesh-tessellate-body:0"));
    assert!(codes.contains(&"feature.mesh-tessellate-source-unresolved".to_owned()));
}

#[test]
fn tessellation_without_metadata_keeps_its_native_parameters() {
    let (stream, _) = fixture(1, false);
    let (feature, codes) = project(&stream, None);
    assert_eq!(feature.evaluation.definition(), &native());
    assert!(feature.source_properties.is_empty());
    assert_eq!(codes, ["feature.mesh-tessellate-scope-unparsed"]);
}

#[test]
fn tessellation_retains_a_changed_tail_byte() {
    let (mut stream, meta) = fixture(1, false);
    let tail_byte_2 = crate::layout::mesh_tessellate_scope_prefix::OPERAND_COUNT + 4 + 11 + 2;
    stream[tail_byte_2] = 7;
    let (feature, codes) = project(&stream, Some(&meta));
    assert_eq!(
        feature.source_properties["mesh-field:tessellate-tail-byte:2"],
        "7"
    );
    assert!(codes.contains(&"feature.mesh-tessellate-fields-uninterpreted".to_owned()));
}

#[test]
fn tessellation_binds_no_operand_after_an_unread_reference() {
    let (stream, meta) = fixture(1, true);
    let (feature, codes) = project(&stream, Some(&meta));
    assert_eq!(
        feature.source_properties["mesh-tessellate-operand-count"],
        "2"
    );
    assert_eq!(feature.source_properties["mesh-tessellate-operand:0"], "20");
    assert!(!feature
        .source_properties
        .contains_key("mesh-tessellate-operand:1"));
    assert_eq!(
        codes
            .iter()
            .filter(|code| code.as_str() == "feature.mesh-tessellate-source-unresolved")
            .count(),
        1
    );
    assert!(codes.contains(&"feature.mesh-tessellate-scope-unparsed".to_owned()));
}
