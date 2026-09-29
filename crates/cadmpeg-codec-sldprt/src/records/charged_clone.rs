//! Charged copies of decoded feature-history records.

use super::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent, TreeParent};
#[cfg(test)]
use super::FEATURE_HISTORY_CLONE_COUNT;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn copy_history_text(ctx: &DecodeContext<'_>, value: &str) -> Result<String, CodecError> {
    let mut copy = String::new();
    ctx.reserve_retained_string(&mut copy, value.len(), "clone SLDPRT history text")?;
    copy.push_str(value);
    Ok(copy)
}

fn clone_history_properties<K>(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    properties: &BTreeMap<K, String>,
    key: impl Fn(&DecodeContext<'_>, &K) -> Result<K, CodecError>,
) -> Result<BTreeMap<K, String>, CodecError>
where
    K: Ord,
{
    let mut copy = BTreeMap::new();
    for (name, value) in properties {
        ctx.charge_collection_items(1, operation)?;
        copy.insert(key(ctx, name)?, copy_history_text(ctx, value)?);
    }
    Ok(copy)
}

fn copy_history_key(ctx: &DecodeContext<'_>, key: &NonBlankString) -> Result<NonBlankString, CodecError> {
    NonBlankString::new(copy_history_text(ctx, key.as_str())?)
        .ok_or_else(|| CodecError::malformed("blank admitted SLDPRT history key"))
}

fn clone_feature_content(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    content: &[FeatureContent],
) -> Result<Vec<FeatureContent>, CodecError> {
    let mut copy = Vec::new();
    for item in content {
        let text = match item {
            FeatureContent::Dimension(value) => FeatureContent::Dimension(copy_history_text(ctx, value)?),
            FeatureContent::Feature(value) => FeatureContent::Feature(copy_history_text(ctx, value)?),
            FeatureContent::Text(value) => FeatureContent::Text(copy_history_text(ctx, value)?),
        };
        ctx.reserve_collection_vec(&mut copy, 1, operation)?;
        copy.push(text);
    }
    Ok(copy)
}

fn clone_history_content(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    content: &[HistoryContent],
) -> Result<Vec<HistoryContent>, CodecError> {
    let mut copy = Vec::new();
    for item in content {
        let text = match item {
            HistoryContent::Configuration(value) => HistoryContent::Configuration(copy_history_text(ctx, value)?),
            HistoryContent::Feature(value) => HistoryContent::Feature(copy_history_text(ctx, value)?),
            HistoryContent::Text(value) => HistoryContent::Text(copy_history_text(ctx, value)?),
        };
        ctx.reserve_collection_vec(&mut copy, 1, operation)?;
        copy.push(text);
    }
    Ok(copy)
}

fn clone_history_feature(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    feature: &Feature,
) -> Result<Feature, CodecError> {
    let tree_parent = match &feature.tree_parent {
        Some(TreeParent::Record { record_id, source_id }) => Some(TreeParent::Record {
            record_id: copy_history_text(ctx, record_id)?,
            source_id: *source_id,
        }),
        Some(TreeParent::Source(source)) => Some(TreeParent::Source(*source)),
        None => None,
    };
    let mut dimension_properties = BTreeMap::new();
    for (name, properties) in &feature.dimension_properties {
        ctx.charge_collection_items(1, operation)?;
        dimension_properties.insert(
            copy_history_text(ctx, name)?,
            clone_history_properties(ctx, operation, properties, copy_history_key)?,
        );
    }
    Ok(Feature {
        id: copy_history_text(ctx, &feature.id)?,
        parent: copy_history_text(ctx, &feature.parent)?,
        xml_tag: copy_history_text(ctx, &feature.xml_tag)?,
        tree_parent,
        source_id: feature.source_id,
        ordinal: feature.ordinal,
        name: copy_history_text(ctx, &feature.name)?,
        kind: copy_history_text(ctx, &feature.kind)?,
        input_class: feature.input_class.as_deref().map(|value| copy_history_text(ctx, value)).transpose()?,
        suppressed: feature.suppressed,
        parameters: clone_history_properties(ctx, operation, &feature.parameters, copy_history_key)?,
        dimension_properties,
        properties: clone_history_properties(ctx, operation, &feature.properties, copy_history_key)?,
        text: feature.text.as_deref().map(|value| copy_history_text(ctx, value)).transpose()?,
        content: clone_feature_content(ctx, operation, &feature.content)?,
    })
}

fn clone_history_configuration(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    configuration: &Configuration,
) -> Result<Configuration, CodecError> {
    Ok(Configuration {
        id: copy_history_text(ctx, &configuration.id)?,
        parent: copy_history_text(ctx, &configuration.parent)?,
        ordinal: configuration.ordinal,
        source_index: configuration.source_index,
        name: copy_history_text(ctx, &configuration.name)?,
        material: configuration.material.as_deref().map(|value| copy_history_text(ctx, value)).transpose()?,
        properties: clone_history_properties(ctx, operation, &configuration.properties, copy_history_key)?,
    })
}

pub(crate) fn clone_histories_charged(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
    operation: &'static str,
) -> Result<Vec<FeatureHistory>, CodecError> {
    let mut copy = Vec::new();
    for history in histories {
        #[cfg(test)]
        FEATURE_HISTORY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        let mut configurations = Vec::new();
        for configuration in &history.configurations {
            let item = clone_history_configuration(ctx, operation, configuration)?;
            ctx.reserve_collection_vec(&mut configurations, 1, operation)?;
            configurations.push(item);
        }
        let mut features = Vec::new();
        for feature in &history.features {
            let item = clone_history_feature(ctx, operation, feature)?;
            ctx.reserve_collection_vec(&mut features, 1, operation)?;
            features.push(item);
        }
        let item = FeatureHistory {
            id: copy_history_text(ctx, &history.id)?,
            part_name: history.part_name.as_deref().map(|value| copy_history_text(ctx, value)).transpose()?,
            properties: clone_history_properties(ctx, operation, &history.properties, copy_history_key)?,
            content: clone_history_content(ctx, operation, &history.content)?,
            configurations,
            features,
        };
        ctx.reserve_collection_vec(&mut copy, 1, operation)?;
        copy.push(item);
    }
    Ok(copy)
}

