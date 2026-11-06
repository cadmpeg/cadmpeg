// SPDX-License-Identifier: Apache-2.0
//! Cross-format product, presentation, and design projection.

use std::fs;

use super::workspace;

#[cfg(feature = "iges")]
fn iges(name: &str) -> cadmpeg_test_support::EditableDecodeResult {
    let path = workspace()
        .join("crates/cadmpeg-codec-iges/tests/golden/fixtures")
        .join(name);
    super::decode(&cadmpeg_codec_iges::IgesCodec, &fs::read(path).unwrap())
}

#[cfg(all(feature = "iges", feature = "step"))]
#[test]
fn iges_definition_levels_survive_as_step_layers() {
    use std::collections::BTreeSet;
    let source = iges("definition_levels.igs");
    let expected = BTreeSet::from(["2", "7", "11"]);
    assert_eq!(
        source
            .ir()
            .model
            .presentation_layers
            .iter()
            .map(|layer| layer.name.as_str())
            .collect::<BTreeSet<_>>(),
        expected
    );
    let step = cadmpeg_codec_step::StepCodec::default();
    let (written, _) = super::write_decode(&step, &step, source.ir(), None, None);
    let names = written
        .ir()
        .model
        .presentation_layers
        .iter()
        .map(|layer| layer.name.as_str())
        .collect::<BTreeSet<_>>();
    assert!(expected.is_subset(&names));
    for layer in &written.ir().model.presentation_layers {
        if expected.contains(layer.name.as_str()) {
            assert!(!layer.items.is_empty());
        }
    }
}

#[cfg(feature = "iges")]
#[test]
fn iges_external_references_and_primitive_solids_project() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::products::PrototypeReference;
    let references = iges("external_reference_forms.igs");
    assert_eq!(references.ir().model.occurrences.len(), 5);
    assert!(references
        .ir()
        .model
        .occurrences
        .iter()
        .all(|occurrence| matches!(occurrence.prototype, PrototypeReference::External { .. })));
    let solids = iges("primitive_solids.igs");
    assert_eq!(solids.ir().model.features.len(), 8);
    assert!(solids.ir().model.features.iter().all(|feature| matches!(
        feature.definition(),
        FeatureDefinition::Operation(FeatureOperation::Primitive { .. })
    )));
}

#[cfg(feature = "catia")]
#[test]
fn catia_external_reference_projects_to_external_occurrence() {
    use cadmpeg_ir::products::PrototypeReference;
    let path = workspace()
        .join("crates/cadmpeg-codec-catia/tests/golden/fixtures/external_preview.catpart");
    let source = super::decode(&cadmpeg_codec_catia::CatiaCodec, &fs::read(path).unwrap());
    assert_eq!(source.ir().model.occurrences.len(), 1);
    assert!(matches!(
        source.ir().model.occurrences[0].prototype,
        PrototypeReference::External { .. }
    ));
}

#[cfg(feature = "inventor")]
#[test]
fn inventor_model_states_project_to_configurations() {
    use assert_cmd::Command;
    let directory = tempfile::tempdir().unwrap();
    let input =
        workspace().join("crates/cadmpeg-codec-inventor/tests/golden/fixtures/model_states.ipt");
    let output = directory.path().join("model.cadir.json");
    Command::cargo_bin("cadmpeg")
        .unwrap()
        .args([
            "dump",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();
    let document = cadmpeg_ir::CadIr::from_json(&fs::read_to_string(output).unwrap()).unwrap();
    assert_eq!(document.model.configurations.len(), 1);
    assert_eq!(
        document.model.configurations[0].name.as_deref(),
        Some("Master")
    );
}
