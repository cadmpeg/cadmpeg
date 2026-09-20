// SPDX-License-Identifier: Apache-2.0
//! Stable hashes of projected and native history state.

use crate::records::{FeatureContent, FeatureHistory};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignConfiguration, DesignParameter};
use serde::Serialize;

/// The digest of one record set's canonical JSON.
///
/// What is hashed is the record's own `Serialize` rendering, written through
/// [`cadmpeg_ir::hash::canonical_json_sha256`]: the serialized field names and
/// the serialized values, in canonical order. Rust type names, field order in
/// the declaration, and `Debug` formatting are not part of the digest, so
/// renaming a type or reordering its declaration leaves every digest where it
/// is. Renaming a serialized field, or changing a value, moves it.
///
/// # Errors
///
/// The IR carries raw `f64`, and canonical JSON refuses a non-finite one.
pub(crate) fn hash_records<T: Serialize + ?Sized>(value: &T) -> Result<String, CodecError> {
    Ok(cadmpeg_ir::hash::canonical_json_sha256(&value)?)
}

pub(crate) fn feature_hash(model: &cadmpeg_ir::document::Model) -> Result<String, CodecError> {
    let mut features = model
        .features
        .iter()
        .map(|feature| FeatureHashView {
            id: &feature.id,
            ordinal: feature.ordinal,
            name: feature.name.as_deref(),
            suppressed: feature.suppressed,
            parent: model.feature_parent(&feature.id),
            dependencies: &feature.dependencies,
            source_properties: &feature.source_properties,
            source_tag: feature.source_tag.as_deref(),
            source_text: feature.source_text.as_deref(),
            source_content: &feature.source_content,
            outputs: feature.evaluation.outputs(),
            definition: feature.evaluation.definition(),
            native_ref: feature.native_ref.as_deref(),
        })
        .collect::<Vec<_>>();
    features.sort_by(|left, right| left.id.cmp(right.id));
    hash_records(&features)
}

/// The feature state the digest covers: the neutral feature and the tree
/// parent, which the feature record does not carry itself.
#[derive(Serialize)]
struct FeatureHashView<'a> {
    id: &'a cadmpeg_ir::features::FeatureId,
    ordinal: u64,
    name: Option<&'a str>,
    suppressed: Option<bool>,
    parent: Option<&'a cadmpeg_ir::features::FeatureId>,
    dependencies: &'a cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::features::FeatureId>,
    source_properties: &'a std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    source_tag: Option<&'a str>,
    source_text: Option<&'a str>,
    source_content: &'a cadmpeg_ir::features::FeatureContent,
    outputs: &'a [cadmpeg_ir::ids::BodyId],
    definition: &'a cadmpeg_ir::features::FeatureDefinition,
    native_ref: Option<&'a str>,
}

/// Stable hash of the native feature histories.
pub(crate) fn history_hash(histories: &[FeatureHistory]) -> Result<String, CodecError> {
    hash_records(histories)
}

/// Stable hash of neutral configurations.
pub(crate) fn configuration_hash(
    configurations: &[DesignConfiguration],
) -> Result<String, CodecError> {
    let mut configurations = configurations.to_vec();
    configurations.sort_by(|left, right| left.id.cmp(&right.id));
    hash_records(&configurations)
}

/// Stable hash of configuration-local evaluated parameter state.
pub(crate) fn configuration_parameter_value_hash(
    configurations: &[DesignConfiguration],
) -> Result<String, CodecError> {
    hash_keyed_records(
        configurations
            .iter()
            .filter(|configuration| !configuration.parameter_values.is_empty())
            .map(|configuration| (&configuration.id, &configuration.parameter_values)),
    )
}

/// Stable hash of configuration-local evaluated feature state.
pub(crate) fn configuration_feature_state_hash(
    configurations: &[DesignConfiguration],
) -> Result<String, CodecError> {
    hash_keyed_records(
        configurations
            .iter()
            .filter(|configuration| !configuration.feature_states.is_empty())
            .map(|configuration| (&configuration.id, &configuration.feature_states)),
    )
}

/// Stable hash of native configuration records.
pub(crate) fn native_configuration_hash(
    histories: &[FeatureHistory],
) -> Result<String, CodecError> {
    let mut configurations = histories
        .iter()
        .flat_map(|history| history.configurations.clone())
        .collect::<Vec<_>>();
    configurations.sort_by(|left, right| left.id.cmp(&right.id));
    hash_records(&configurations)
}

/// Stable hash of neutral feature parameters.
pub(crate) fn parameter_hash(parameters: &[DesignParameter]) -> Result<String, CodecError> {
    let mut parameters = parameters.to_vec();
    parameters.sort_by(|left, right| left.id.cmp(&right.id));
    hash_records(&parameters)
}

/// Stable hash of native feature parameters, properties, and ordering.
pub(crate) fn native_parameter_hash(histories: &[FeatureHistory]) -> Result<String, CodecError> {
    let mut parameters = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| {
            (
                feature.id.clone(),
                feature.parameters.clone(),
                feature.dimension_properties.clone(),
                feature
                    .content
                    .iter()
                    .filter_map(|item| match item {
                        FeatureContent::Dimension(name) => Some(name.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    parameters.sort_by(|left, right| left.0.cmp(&right.0));
    hash_records(&parameters)
}

fn hash_keyed_records<K: Ord + Serialize, V: Serialize>(
    records: impl Iterator<Item = (K, V)>,
) -> Result<String, CodecError> {
    let mut records = records.collect::<Vec<_>>();
    records.sort_by(|left, right| left.0.cmp(&right.0));
    hash_records(&records)
}

#[cfg(test)]
mod tests {
    use super::hash_records;
    use crate::records::Configuration;
    use crate::records::FeatureHistory;
    use std::collections::BTreeMap;

    fn history(part_name: &str) -> FeatureHistory {
        FeatureHistory {
            id: "sldprt:history:feature-history#0".to_owned(),
            part_name: Some(part_name.to_owned()),
            properties: BTreeMap::from([(
                cadmpeg_core::nonblank_literal!("Version"),
                "2024".to_owned(),
            )]),
            content: Vec::new(),
            configurations: vec![Configuration {
                id: "sldprt:history:configuration#0:0".to_owned(),
                parent: "sldprt:history:feature-history#0".to_owned(),
                ordinal: 0,
                source_index: Some(0),
                name: "Default".into(),
                material: None,
                properties: BTreeMap::new(),
            }],
            features: Vec::new(),
        }
    }

    /// The digest is over the record's canonical bytes, so a record rebuilt
    /// from those bytes digests to the same value.
    #[test]
    fn a_record_digests_the_same_as_the_record_read_back_from_its_bytes() {
        let record = history("Bracket");
        let bytes = serde_json::to_vec(&record).expect("the record serializes");
        let read_back: FeatureHistory =
            serde_json::from_slice(&bytes).expect("the record reads back");

        assert_eq!(read_back, record);
        assert_eq!(
            hash_records(&read_back).expect("the record digests"),
            hash_records(&record).expect("the record digests"),
        );
    }

    /// One changed field moves the digest.
    #[test]
    fn a_changed_field_moves_the_digest() {
        assert_ne!(
            hash_records(&history("Bracket")).expect("the record digests"),
            hash_records(&history("Plate")).expect("the record digests"),
        );
    }
}
