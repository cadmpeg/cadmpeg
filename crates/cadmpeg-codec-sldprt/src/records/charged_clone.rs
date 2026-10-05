//! Charged copies of decoded feature-history records.

#[cfg(test)]
use super::FEATURE_HISTORY_CLONE_COUNT;
use super::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent, TreeParent};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

mod lane;

pub(crate) trait CloneCharged: Sized {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError>;
}

impl CloneCharged for String {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.len()), operation)?;
        ctx.copy_retained_text(self, operation)
    }
}

impl CloneCharged for u32 {
    fn clone_charged(&self, _: &DecodeContext<'_>, _: &'static str) -> Result<Self, CodecError> {
        Ok(*self)
    }
}

impl<T: CloneCharged> CloneCharged for Option<T> {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        self.as_ref()
            .map(|value| value.clone_charged(ctx, operation))
            .transpose()
    }
}

impl<T: CloneCharged> CloneCharged for Vec<T> {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut copy = Vec::new();
        ctx.reserve_vec(&mut copy, self.len(), operation)?;
        for value in self {
            ctx.charge_work(1, operation)?;
            copy.push(value.clone_charged(ctx, operation)?);
        }
        Ok(copy)
    }
}

impl CloneCharged for Feature {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        clone_history_feature(ctx, operation, self)
    }
}

impl CloneCharged for Configuration {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        clone_history_configuration(ctx, operation, self)
    }
}

fn copy_history_text(ctx: &DecodeContext<'_>, value: &str) -> Result<String, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(value.len()),
        "clone SLDPRT history text",
    )?;
    ctx.copy_retained_text(value, "clone SLDPRT history text")
}

fn clone_history_properties<K>(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    properties: &BTreeMap<K, String>,
    key: impl Fn(&DecodeContext<'_>, &K) -> Result<K, CodecError>,
) -> Result<BTreeMap<K, String>, CodecError>
where
    K: Ord + cadmpeg_core::decode::cost::DecodeCost,
{
    let mut copy = BTreeMap::new();
    for (name, value) in ctx.admit_iter(properties, "scan SLDPRT properties values")? {
        ctx.insert_btree_map(
            &mut copy,
            key(ctx, name)?,
            copy_history_text(ctx, value)?,
            operation,
        )?;
    }
    Ok(copy)
}

fn copy_history_key(
    ctx: &DecodeContext<'_>,
    key: &NonBlankString,
) -> Result<NonBlankString, CodecError> {
    NonBlankString::for_decode(
        ctx,
        copy_history_text(ctx, key.as_str())?,
        "validate nonblank text",
    )?
    .ok_or_else(|| CodecError::malformed("blank admitted SLDPRT history key"))
}

fn clone_feature_content(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    content: &[FeatureContent],
) -> Result<Vec<FeatureContent>, CodecError> {
    let mut copy = Vec::new();
    for item in ctx.admit_iter(content, operation)? {
        let text = match item {
            FeatureContent::Dimension(value) => {
                FeatureContent::Dimension(copy_history_text(ctx, value)?)
            }
            FeatureContent::Feature(value) => {
                FeatureContent::Feature(copy_history_text(ctx, value)?)
            }
            FeatureContent::Text(value) => FeatureContent::Text(copy_history_text(ctx, value)?),
        };
        ctx.reserve_vec(&mut copy, 1, operation)?;
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
    for item in ctx.admit_iter(content, operation)? {
        let text = match item {
            HistoryContent::Configuration(value) => {
                HistoryContent::Configuration(copy_history_text(ctx, value)?)
            }
            HistoryContent::Feature(value) => {
                HistoryContent::Feature(copy_history_text(ctx, value)?)
            }
            HistoryContent::Text(value) => HistoryContent::Text(copy_history_text(ctx, value)?),
        };
        ctx.reserve_vec(&mut copy, 1, operation)?;
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
        Some(TreeParent::Record {
            record_id,
            source_id,
        }) => Some(TreeParent::Record {
            record_id: copy_history_text(ctx, record_id)?,
            source_id: *source_id,
        }),
        Some(TreeParent::Source(source)) => Some(TreeParent::Source(*source)),
        None => None,
    };
    let mut dimension_properties = BTreeMap::new();
    for (name, properties) in ctx.admit_iter(
        &feature.dimension_properties,
        "scan SLDPRT clone_history_feature values",
    )? {
        ctx.insert_btree_map(
            &mut dimension_properties,
            copy_history_text(ctx, name)?,
            clone_history_properties(ctx, operation, properties, copy_history_key)?,
            operation,
        )?;
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
        input_class: feature
            .input_class
            .as_deref()
            .map(|value| copy_history_text(ctx, value))
            .transpose()?,
        suppressed: feature.suppressed,
        parameters: clone_history_properties(
            ctx,
            operation,
            &feature.parameters,
            copy_history_key,
        )?,
        dimension_properties,
        properties: clone_history_properties(
            ctx,
            operation,
            &feature.properties,
            copy_history_key,
        )?,
        text: feature
            .text
            .as_deref()
            .map(|value| copy_history_text(ctx, value))
            .transpose()?,
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
        material: configuration
            .material
            .as_deref()
            .map(|value| copy_history_text(ctx, value))
            .transpose()?,
        properties: clone_history_properties(
            ctx,
            operation,
            &configuration.properties,
            copy_history_key,
        )?,
    })
}

impl CloneCharged for FeatureHistory {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        #[cfg(test)]
        FEATURE_HISTORY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        let mut configurations = Vec::new();
        for configuration in
            ctx.admit_iter(&self.configurations, "scan SLDPRT clone_charged values")?
        {
            let item = clone_history_configuration(ctx, operation, configuration)?;
            ctx.reserve_vec(&mut configurations, 1, operation)?;
            configurations.push(item);
        }
        let mut features = Vec::new();
        for feature in ctx.admit_iter(&self.features, "scan SLDPRT clone_charged values")? {
            let item = clone_history_feature(ctx, operation, feature)?;
            ctx.reserve_vec(&mut features, 1, operation)?;
            features.push(item);
        }
        Ok(FeatureHistory {
            id: copy_history_text(ctx, &self.id)?,
            part_name: self
                .part_name
                .as_deref()
                .map(|value| copy_history_text(ctx, value))
                .transpose()?,
            properties: clone_history_properties(
                ctx,
                operation,
                &self.properties,
                copy_history_key,
            )?,
            content: clone_history_content(ctx, operation, &self.content)?,
            configurations,
            features,
        })
    }
}

pub(crate) fn clone_histories_charged(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
    operation: &'static str,
) -> Result<Vec<FeatureHistory>, CodecError> {
    let mut copy = Vec::new();
    for history in ctx.admit_iter(histories, operation)? {
        let item = history.clone_charged(ctx, operation)?;
        ctx.reserve_vec(&mut copy, 1, operation)?;
        copy.push(item);
    }
    Ok(copy)
}
