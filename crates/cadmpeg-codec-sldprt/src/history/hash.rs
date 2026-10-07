// SPDX-License-Identifier: Apache-2.0
//! Stable hashes of projected and native history state.

use crate::records::{FeatureContent, FeatureHistory};
use cadmpeg_core::decode::DecodeContext;
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
pub(crate) fn hash_records<T: Serialize + ?Sized>(
    ctx: &DecodeContext<'_>,
    value: &T,
) -> Result<String, CodecError> {
    const OPERATION: &str = "hash SLDPRT canonical history bytes";
    cadmpeg_ir::hash::canonical_json_sha256(ctx, value, OPERATION).map_err(Into::into)
}

pub(crate) fn feature_hash(
    ctx: &DecodeContext<'_>,
    model: &cadmpeg_ir::document::Model,
) -> Result<String, CodecError> {
    const OPERATION: &str = "match SLDPRT feature hash parents";
    let views = ctx.with_scoped_storage("SLDPRT canonical hash views", || {
        let mut parent_storage = ctx.reserve_scoped(0, OPERATION)?;
        let tree_parents = crate::history::bind::tree_parents(ctx, &mut parent_storage, &model.features)?;
        let mut features = Vec::new();
        for feature in ctx.admit_iter(&model.features, "scan SLDPRT canonical hash views")? {
            // The structural owner, or the regeneration predecessor when no
            // tree node owns the feature.
            let parent = match ctx.get_hash_map(&tree_parents, &feature.id, OPERATION)? {
                Some((first, _)) => Some(*first),
                None => model.feature_regeneration_parent(&feature.id),
            };
            ctx.push_vec(
                &mut features,
                FeatureHashView {
                    id: &feature.id,
                    ordinal: feature.ordinal,
                    name: feature.name.as_deref(),
                    suppressed: feature.suppressed,
                    parent,
                    dependencies: &feature.dependencies,
                    source_properties: &feature.source_properties,
                    source_tag: feature.source_tag.as_deref(),
                    source_text: feature.source_text.as_deref(),
                    source_content: &feature.source_content,
                    outputs: feature.evaluation.outputs(),
                    definition: feature.evaluation.definition(),
                    native_ref: feature.native_ref.as_deref(),
                },
                "retain SLDPRT canonical hash views",
            )?;
        }
        ctx.stable_sort_by(
            &mut features,
            |value| &value.id,
            Ord::cmp,
            "sort SLDPRT canonical hash views",
        )?;
        Ok::<_, CodecError>(features)
    })?;
    hash_records(ctx, &views.0)
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
pub(crate) fn history_hash(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<String, CodecError> {
    hash_records(ctx, histories)
}

/// Stable hash of neutral configurations.
pub(crate) fn configuration_hash(
    ctx: &DecodeContext<'_>,
    configurations: &[DesignConfiguration],
) -> Result<String, CodecError> {
    let views = ctx.with_scoped_storage("SLDPRT canonical hash views", || {
        let mut configurations = collect_hash_views(
            ctx,
            ctx.admit_iter(configurations, "scan SLDPRT canonical hash views")?,
        )?;
        ctx.stable_sort_by(
            &mut configurations,
            |value| &value.id,
            Ord::cmp,
            "sort SLDPRT canonical hash views",
        )?;
        Ok::<_, CodecError>(configurations)
    })?;
    hash_records(ctx, &views.0)
}

/// Stable hash of configuration-local evaluated parameter state.
pub(crate) fn configuration_parameter_value_hash(
    ctx: &DecodeContext<'_>,
    configurations: &[DesignConfiguration],
) -> Result<String, CodecError> {
    hash_keyed_records(
        ctx,
        ctx.admit_iter(configurations, "scan SLDPRT configuration hash views")?
            .filter(|configuration| !configuration.parameter_values.is_empty())
            .map(|configuration| (&configuration.id, &configuration.parameter_values)),
    )
}

/// Stable hash of configuration-local evaluated feature state.
pub(crate) fn configuration_feature_state_hash(
    ctx: &DecodeContext<'_>,
    configurations: &[DesignConfiguration],
) -> Result<String, CodecError> {
    hash_keyed_records(
        ctx,
        ctx.admit_iter(configurations, "scan SLDPRT configuration hash views")?
            .filter(|configuration| !configuration.feature_states.is_empty())
            .map(|configuration| (&configuration.id, &configuration.feature_states)),
    )
}

/// Stable hash of native configuration records.
pub(crate) fn native_configuration_hash(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<String, CodecError> {
    let views = ctx.with_scoped_storage("SLDPRT canonical hash views", || {
        let mut configurations = Vec::new();
        for history in ctx.admit_iter(histories, "scan SLDPRT native configuration hash views")? {
            for configuration in
                ctx.admit_iter(&history.configurations, "scan SLDPRT canonical hash views")?
            {
                ctx.push_vec(
                    &mut configurations,
                    configuration,
                    "retain SLDPRT canonical hash views",
                )?;
            }
        }
        ctx.stable_sort_by(
            &mut configurations,
            |value| &value.id,
            Ord::cmp,
            "sort SLDPRT canonical hash views",
        )?;
        Ok::<_, CodecError>(configurations)
    })?;
    hash_records(ctx, &views.0)
}

/// Stable hash of neutral feature parameters.
pub(crate) fn parameter_hash(
    ctx: &DecodeContext<'_>,
    parameters: &[DesignParameter],
) -> Result<String, CodecError> {
    let views = ctx.with_scoped_storage("SLDPRT canonical hash views", || {
        let mut parameters = collect_hash_views(
            ctx,
            ctx.admit_iter(parameters, "scan SLDPRT canonical hash views")?,
        )?;
        ctx.stable_sort_by(
            &mut parameters,
            |value| &value.id,
            Ord::cmp,
            "sort SLDPRT canonical hash views",
        )?;
        Ok::<_, CodecError>(parameters)
    })?;
    hash_records(ctx, &views.0)
}

/// Stable hash of native feature parameters, properties, and ordering.
pub(crate) fn native_parameter_hash(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<String, CodecError> {
    let views = ctx.with_scoped_storage("SLDPRT canonical hash views", || {
        let mut parameters = Vec::new();
        for history in ctx.admit_iter(histories, "scan SLDPRT native hash histories")? {
            for feature in ctx.admit_iter(&history.features, "scan SLDPRT hash features")? {
                let dimensions = collect_hash_views(
                    ctx,
                    ctx.admit_iter(&feature.content, "scan SLDPRT hash dimensions")?
                        .filter_map(|item| match item {
                            FeatureContent::Dimension(name) => Some(name),
                            _ => None,
                        }),
                )?;
                ctx.reserve_vec(
                    &mut parameters,
                    1,
                    "retain SLDPRT native parameter hash view",
                )?;
                parameters.push((
                    &feature.id,
                    &feature.parameters,
                    &feature.dimension_properties,
                    dimensions,
                ));
            }
        }
        ctx.stable_sort_by(
            &mut parameters,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT canonical hash views",
        )?;
        Ok::<_, CodecError>(parameters)
    })?;
    hash_records(ctx, &views.0)
}

fn hash_keyed_records<'id, V: Serialize>(
    ctx: &DecodeContext<'_>,
    records: impl Iterator<Item = (&'id cadmpeg_ir::features::ConfigurationId, V)>,
) -> Result<String, CodecError> {
    let views = ctx.with_scoped_storage("SLDPRT canonical hash views", || {
        let mut records = collect_hash_views(ctx, records)?;
        ctx.stable_sort_by(
            &mut records,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT canonical hash views",
        )?;
        Ok::<_, CodecError>(records)
    })?;
    hash_records(ctx, &views.0)
}

/// Collect the views an admitted traversal yields.
fn collect_hash_views<T>(
    ctx: &DecodeContext<'_>,
    values: impl Iterator<Item = T>,
) -> Result<Vec<T>, CodecError> {
    let mut views = Vec::new();
    for value in values {
        ctx.push_vec(&mut views, value, "retain SLDPRT canonical hash views")?;
    }
    Ok(views)
}

#[cfg(test)]
mod tests {
    use super::{hash_records, native_parameter_hash};
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

    #[test]
    fn native_parameter_hash_refuses_history_traversal_work() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let histories = [history("Bracket")];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = native_parameter_hash(&ctx, &histories).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "scan SLDPRT native hash histories"));
        let ctx = cadmpeg_test_support::service_decode_context();
        assert_eq!(
            native_parameter_hash(&ctx, &histories).unwrap(),
            native_parameter_hash(&ctx, &[]).unwrap(),
        );
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
            hash_records(&cadmpeg_test_support::service_decode_context(), &read_back)
                .expect("the record digests"),
            hash_records(&cadmpeg_test_support::service_decode_context(), &record)
                .expect("the record digests"),
        );
    }

    /// One changed field moves the digest.
    #[test]
    fn a_changed_field_moves_the_digest() {
        assert_ne!(
            hash_records(
                &cadmpeg_test_support::service_decode_context(),
                &history("Bracket")
            )
            .expect("the record digests"),
            hash_records(
                &cadmpeg_test_support::service_decode_context(),
                &history("Plate")
            )
            .expect("the record digests"),
        );
    }
}
