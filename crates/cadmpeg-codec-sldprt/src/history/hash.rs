// SPDX-License-Identifier: Apache-2.0
//! Stable hashes of projected and native history state.

use crate::records::{FeatureContent, FeatureHistory};
use cadmpeg_core::CodecError;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
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
pub(crate) fn hash_records<T: Serialize + ?Sized>(ctx: &DecodeContext<'_>, value: &T) -> Result<String, CodecError> {
    const OPERATION: &str = "hash SLDPRT canonical history bytes";
    ctx.charge_work(8192, OPERATION)?;
    cadmpeg_ir::hash::canonical_json_sha256_with_charge(value, |bytes| {
        ctx.charge_work(bytes.checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?, OPERATION)
    })
}

pub(crate) fn feature_hash(ctx: &DecodeContext<'_>, model: &cadmpeg_ir::document::Model) -> Result<String, CodecError> {
    admit_feature_parents(ctx, model)?;
    let mut features = collect_hash_views(ctx, model
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
        )?;
    sort_hash_views(ctx, &mut features, |feature| feature.id.as_str())?;
    hash_records(ctx, &features)
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
pub(crate) fn history_hash(ctx: &DecodeContext<'_>, histories: &[FeatureHistory]) -> Result<String, CodecError> {
    hash_records(ctx, histories)
}

/// Stable hash of neutral configurations.
pub(crate) fn configuration_hash(ctx: &DecodeContext<'_>, 
    configurations: &[DesignConfiguration],
) -> Result<String, CodecError> {
    let mut configurations = collect_hash_views(ctx, configurations.iter())?;
    sort_hash_views(ctx, &mut configurations, |configuration| configuration.id.as_str())?;
    hash_records(ctx, &configurations)
}

/// Stable hash of configuration-local evaluated parameter state.
pub(crate) fn configuration_parameter_value_hash(ctx: &DecodeContext<'_>, 
    configurations: &[DesignConfiguration],
) -> Result<String, CodecError> {
    ctx.charge_work(u64_from_index(configurations.len()), "scan SLDPRT configuration hash views")?;
    hash_keyed_records(ctx, 
        configurations
            .iter()
            .filter(|configuration| !configuration.parameter_values.is_empty())
            .map(|configuration| (&configuration.id, &configuration.parameter_values)),
    )
}

/// Stable hash of configuration-local evaluated feature state.
pub(crate) fn configuration_feature_state_hash(ctx: &DecodeContext<'_>, 
    configurations: &[DesignConfiguration],
) -> Result<String, CodecError> {
    ctx.charge_work(u64_from_index(configurations.len()), "scan SLDPRT configuration hash views")?;
    hash_keyed_records(ctx, 
        configurations
            .iter()
            .filter(|configuration| !configuration.feature_states.is_empty())
            .map(|configuration| (&configuration.id, &configuration.feature_states)),
    )
}

/// Stable hash of native configuration records.
pub(crate) fn native_configuration_hash(ctx: &DecodeContext<'_>, 
    histories: &[FeatureHistory],
) -> Result<String, CodecError> {
    ctx.charge_work(u64_from_index(histories.len()), "scan SLDPRT native configuration hash views")?;
    let mut configurations = collect_hash_views(ctx, histories
        .iter()
        .flat_map(|history| &history.configurations)
        )?;
    sort_hash_views(ctx, &mut configurations, |configuration| configuration.id.as_str())?;
    hash_records(ctx, &configurations)
}

/// Stable hash of neutral feature parameters.
pub(crate) fn parameter_hash(ctx: &DecodeContext<'_>, parameters: &[DesignParameter]) -> Result<String, CodecError> {
    let mut parameters = collect_hash_views(ctx, parameters.iter())?;
    sort_hash_views(ctx, &mut parameters, |parameter| parameter.id.as_str())?;
    hash_records(ctx, &parameters)
}

/// Stable hash of native feature parameters, properties, and ordering.
pub(crate) fn native_parameter_hash(ctx: &DecodeContext<'_>, histories: &[FeatureHistory]) -> Result<String, CodecError> {
    let mut parameters = Vec::new();
    for history in histories {
        ctx.charge_work(u64_from_index(history.features.len()), "scan SLDPRT hash features")?;
        for feature in &history.features {
            ctx.charge_work(u64_from_index(feature.content.len()), "scan SLDPRT hash dimensions")?;
            let dimensions = collect_hash_views(ctx, feature.content.iter().filter_map(|item| match item {
                FeatureContent::Dimension(name) => Some(name),
                _ => None,
            }))?;
            ctx.reserve_collection_vec(&mut parameters, 1, "retain SLDPRT native parameter hash view")?;
            parameters.push((&feature.id, &feature.parameters, &feature.dimension_properties, dimensions));
        }
    }
    sort_hash_views(ctx, &mut parameters, |parameter| parameter.0.as_str())?;
    hash_records(ctx, &parameters)
}

fn hash_keyed_records<'id, V: Serialize>(
    ctx: &DecodeContext<'_>,
    records: impl Iterator<Item = (&'id cadmpeg_ir::features::ConfigurationId, V)>,
) -> Result<String, CodecError> {
    let mut records = collect_hash_views(ctx, records)?;
    sort_hash_views(ctx, &mut records, |record| record.0.as_str())?;
    hash_records(ctx, &records)
}

fn collect_hash_views<T>(ctx: &DecodeContext<'_>, mut values: impl Iterator<Item = T>) -> Result<Vec<T>, CodecError> {
    let mut views = Vec::new();
    loop {
        ctx.charge_work(1, "scan SLDPRT canonical hash views")?;
        let Some(value) = values.next() else { break; };
        ctx.reserve_collection_vec(&mut views, 1, "retain SLDPRT canonical hash views")?;
        views.push(value);
    }
    Ok(views)
}

fn sort_hash_views<T>(ctx: &DecodeContext<'_>, values: &mut [T], key: impl Fn(&T) -> &str) -> Result<(), CodecError> {
    const OPERATION: &str = "sort SLDPRT canonical hash views";
    let count = u64_from_index(values.len());
    ctx.charge_work(count, OPERATION)?;
    let bytes = values.iter().try_fold(0u64, |bytes, value| bytes.checked_add(u64_from_index(key(value).len())))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let levels = u64::from(u64::BITS - count.leading_zeros()) + 1;
    let work = count.checked_mul(u64_from_index(std::mem::size_of::<T>())).and_then(|storage| bytes.checked_add(storage))
        .and_then(|bytes| bytes.checked_mul(levels)).and_then(|work| work.checked_mul(8))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, OPERATION)?;
    let scratch = count.checked_mul(u64_from_index(std::mem::size_of::<T>()))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let _scratch = ctx.reserve_scoped(scratch, OPERATION)?;
    values.sort_by(|left, right| key(left).cmp(key(right)));
    Ok(())
}

fn admit_feature_parents(ctx: &DecodeContext<'_>, model: &cadmpeg_ir::document::Model) -> Result<(), CodecError> {
    const OPERATION: &str = "match SLDPRT feature hash parents";
    let count = u64_from_index(model.features.len());
    ctx.charge_work(count, OPERATION)?;
    let mut key_bytes = 0u64;
    let mut children_count = 0u64;
    for feature in &model.features {
        key_bytes = key_bytes.checked_add(u64_from_index(feature.id.as_str().len()))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        if let cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::TreeNode { children, .. }) = feature.evaluation.definition() {
            ctx.charge_work(u64_from_index(children.len()), OPERATION)?;
            for child in children {
                key_bytes = key_bytes.checked_add(u64_from_index(child.as_str().len()))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                children_count = children_count.checked_add(1).ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            }
        }
    }
    let work = count.checked_add(children_count).and_then(|slots| slots.checked_mul(64))
        .and_then(|work| work.checked_add(key_bytes.checked_mul(8)?)).and_then(|work| work.checked_mul(count))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, OPERATION)
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
            hash_records(&cadmpeg_test_support::service_decode_context(), &read_back).expect("the record digests"),
            hash_records(&cadmpeg_test_support::service_decode_context(), &record).expect("the record digests"),
        );
    }

    /// One changed field moves the digest.
    #[test]
    fn a_changed_field_moves_the_digest() {
        assert_ne!(
            hash_records(&cadmpeg_test_support::service_decode_context(), &history("Bracket")).expect("the record digests"),
            hash_records(&cadmpeg_test_support::service_decode_context(), &history("Plate")).expect("the record digests"),
        );
    }
}
