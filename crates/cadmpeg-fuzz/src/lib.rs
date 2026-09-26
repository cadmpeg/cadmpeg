// SPDX-License-Identifier: Apache-2.0
//! Shared inputs for the libFuzzer harnesses and the seed-generator binaries.

pub mod seed_paths;
pub mod seeds;

/// Compare a reparsed canonical document with the admitted source after arena sorting.
pub fn check_ir_canonical_roundtrip(
    mut original: cadmpeg_ir::CadIr,
    canonical: &str,
) -> Result<(), String> {
    original.finalize();
    let reparsed = cadmpeg_ir::CadIr::from_json(canonical)
        .map_err(|error| format!("canonical JSON cannot be reparsed: {error}"))?;
    if reparsed != original {
        return Err(format!(
            "canonical JSON changed the admitted structure: expected {original:?}, got {reparsed:?}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::features::{
        Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };

    use super::check_ir_canonical_roundtrip;

    #[test]
    fn canonical_roundtrip_check_rejects_reparse_failure_and_structure_change() {
        let original = cadmpeg_ir::CadIr::empty();
        let canonical = original.to_canonical_json().unwrap();
        assert!(check_ir_canonical_roundtrip(original.clone(), &canonical).is_ok());
        assert!(check_ir_canonical_roundtrip(original.clone(), "{")
            .unwrap_err()
            .contains("cannot be reparsed"));

        let mut changed = cadmpeg_ir::CadIr::empty();
        changed.model.features.push(Feature {
            id: FeatureId::mint("test:model:feature#changed").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            )),
            native_ref: None,
        });
        let mut earlier = changed.model.features[0].clone();
        earlier.id = FeatureId::mint("test:model:feature#before").unwrap();
        changed.model.features.push(earlier);
        let changed_canonical = changed.to_canonical_json().unwrap();
        assert!(check_ir_canonical_roundtrip(changed, &changed_canonical).is_ok());
        assert!(check_ir_canonical_roundtrip(original, &changed_canonical)
            .unwrap_err()
            .contains("changed the admitted structure"));
    }
}
