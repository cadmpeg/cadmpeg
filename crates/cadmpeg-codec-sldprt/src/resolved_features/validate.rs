//! Native lane validation findings.

use super::assembly::is_supplemental_config_lane;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::{
    check::{Check, Finding},
    Severity,
};
use std::collections::HashSet;

/// Validate `SolidWorks` native feature-input byte references.
pub(crate) fn validate_native(
    ctx: &DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("sldprt") else {
        return Ok(Vec::new());
    };
    let native = match crate::native::SldprtNative::load_charged(ctx, namespace) {
        Ok(native) => native,
        Err(error) => return invalid_namespace(ctx, &error),
    };
    let mut findings = Vec::new();
    for history in &native.feature_histories {
        if let Err(error) = crate::writer::validate_feature_graph(ctx, &history.features) {
            if matches!(error, CodecError::ResourceLimit(_)) {
                return Err(error);
            }
            push_finding(
                ctx,
                &mut findings,
                Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: crate::text_admission::format_retained(
                        ctx,
                        format_args!("{error}"),
                        "format SLDPRT native finding",
                    )?,
                    entity: Some(copy_finding_id(ctx, &history.id)?),
                },
            )?;
        }
        if !history.content.is_empty() {
            let configurations = collect_history_ids(
                ctx,
                history
                    .configurations
                    .iter()
                    .map(|configuration| configuration.id.as_str()),
            )?;
            let root_features = collect_history_ids(
                ctx,
                history
                    .features
                    .iter()
                    .filter(|feature| feature.tree_parent.is_none())
                    .map(|feature| feature.id.as_str()),
            )?;
            let all_features = collect_history_ids(
                ctx,
                history.features.iter().map(|feature| feature.id.as_str()),
            )?;
            let mut seen_configurations = HashSet::new();
            let mut seen_features = HashSet::new();
            for item in &history.content {
                let error = match item {
                    crate::records::HistoryContent::Configuration(id) => {
                        ctx.reserve_set(
                            &mut seen_configurations,
                            1,
                            "index SLDPRT native history content",
                        )?;
                        if !configurations.contains(id.as_str()) {
                            Some(crate::text_admission::format_retained(
                                ctx,
                                format_args!(
                                    "SolidWorks history root references missing configuration {id}"
                                ),
                                "format SLDPRT native finding",
                            )?)
                        } else if !seen_configurations.insert(id.as_str()) {
                            Some(crate::text_admission::format_retained(
                                ctx,
                                format_args!("SolidWorks history root repeats configuration {id}"),
                                "format SLDPRT native finding",
                            )?)
                        } else {
                            None
                        }
                    }
                    crate::records::HistoryContent::Feature(id) => {
                        ctx.reserve_set(
                            &mut seen_features,
                            1,
                            "index SLDPRT native history content",
                        )?;
                        if !all_features.contains(id.as_str()) {
                            Some(crate::text_admission::format_retained(
                                ctx,
                                format_args!(
                                    "SolidWorks history root references missing feature {id}"
                                ),
                                "format SLDPRT native finding",
                            )?)
                        } else if !root_features.contains(id.as_str()) {
                            Some(crate::text_admission::format_retained(
                                ctx,
                                format_args!(
                                    "SolidWorks history root references nested feature {id}"
                                ),
                                "format SLDPRT native finding",
                            )?)
                        } else if !seen_features.insert(id.as_str()) {
                            Some(crate::text_admission::format_retained(
                                ctx,
                                format_args!("SolidWorks history root repeats feature {id}"),
                                "format SLDPRT native finding",
                            )?)
                        } else {
                            None
                        }
                    }
                    crate::records::HistoryContent::Text(_) => None,
                };
                if let Some(message) = error {
                    push_finding(
                        ctx,
                        &mut findings,
                        Finding {
                            check: Check::NativeLinks,
                            severity: Severity::Error,
                            message,
                            entity: Some(copy_finding_id(ctx, &history.id)?),
                        },
                    )?;
                }
            }
            for missing in configurations.difference(&seen_configurations) {
                push_finding(
                    ctx,
                    &mut findings,
                    Finding {
                        check: Check::NativeLinks,
                        severity: Severity::Error,
                        message: crate::text_admission::format_retained(
                            ctx,
                            format_args!("SolidWorks history root omits configuration {missing}"),
                            "format SLDPRT native finding",
                        )?,
                        entity: Some(copy_finding_id(ctx, &history.id)?),
                    },
                )?;
            }
            for missing in root_features.difference(&seen_features) {
                push_finding(
                    ctx,
                    &mut findings,
                    Finding {
                        check: Check::NativeLinks,
                        severity: Severity::Error,
                        message: crate::text_admission::format_retained(
                            ctx,
                            format_args!("SolidWorks history root omits feature {missing}"),
                            "format SLDPRT native finding",
                        )?,
                        entity: Some(copy_finding_id(ctx, &history.id)?),
                    },
                )?;
            }
        }
    }
    let (mut expected_histories, _history_reservation) =
        crate::native::admission::collect_temporary_clones(
            ctx,
            native.feature_histories.iter(),
            "validate SLDPRT expected histories",
        )?;
    let (history_lanes, _lane_reservation) = crate::native::admission::collect_temporary_clones(
        ctx,
        native
            .feature_input_lanes
            .iter()
            .filter(|lane| !is_supplemental_config_lane(lane)),
        "validate SLDPRT history lanes",
    )?;
    crate::resolved_features::classes::bind_history_classes(
        ctx,
        &mut expected_histories,
        &history_lanes,
    )?;
    for (history, expected_history) in native.feature_histories.iter().zip(&expected_histories) {
        for (feature, expected_feature) in history.features.iter().zip(&expected_history.features) {
            if feature.input_class != expected_feature.input_class {
                push_finding(ctx, &mut findings, Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message:
                        "SolidWorks history feature class does not match its feature-input index"
                            .into(),
                    entity: Some(copy_finding_id(ctx, &feature.id)?),
                })?;
            }
        }
    }
    let expected_lanes = crate::native::lanes::expected_lanes_charged(ctx, &native)?;
    for (lane, expected_lane) in expected_lanes {
        for (entity, expected_entity) in lane
            .sketch_entities
            .iter()
            .zip(&expected_lane.sketch_entities)
        {
            if entity.feature_ref != expected_entity.feature_ref {
                push_finding(
                    ctx,
                    &mut findings,
                    Finding {
                        check: Check::NativeLinks,
                        severity: Severity::Error,
                        message:
                            "SolidWorks sketch-input marker has inconsistent feature ownership"
                                .into(),
                        entity: Some(copy_finding_id(ctx, entity.id())?),
                    },
                )?;
            }
            if entity.links != expected_entity.links {
                push_finding(
                    ctx,
                    &mut findings,
                    Finding {
                        check: Check::NativeLinks,
                        severity: Severity::Error,
                        message: "SolidWorks sketch-input marker has inconsistent local links"
                            .into(),
                        entity: Some(copy_finding_id(ctx, entity.id())?),
                    },
                )?;
            }
        }
    }

    Ok(findings)
}

fn invalid_namespace(
    ctx: &DecodeContext<'_>,
    error: &cadmpeg_ir::NativeConvertError,
) -> Result<Vec<Finding>, CodecError> {
    ctx.charge_work(0, "validate SLDPRT native namespace")?;
    let mut findings = Vec::new();
    let message = crate::text_admission::format_retained(
        ctx,
        format_args!("invalid SolidWorks native namespace: {error}"),
        "format SLDPRT native finding",
    )?;
    push_finding(
        ctx,
        &mut findings,
        Finding {
            check: Check::NativeLinks,
            severity: Severity::Error,
            message,
            entity: None,
        },
    )?;
    Ok(findings)
}

fn copy_finding_id(ctx: &DecodeContext<'_>, id: &str) -> Result<String, CodecError> {
    let copy_work = cadmpeg_core::decode::u64_from_index(id.len())
        .checked_mul(4)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "retain SLDPRT native finding identity",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    ctx.charge_work(copy_work, "retain SLDPRT native finding identity")?;
    crate::text_admission::format_retained(
        ctx,
        format_args!("{id}"),
        "retain SLDPRT native finding identity",
    )
}

fn push_finding(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    finding: Finding,
) -> Result<(), CodecError> {
    ctx.reserve_collection_vec(findings, 1, "collect SLDPRT native findings")?;
    findings.push(finding);
    Ok(())
}

fn collect_history_ids<'a>(
    ctx: &DecodeContext<'_>,
    items: impl Iterator<Item = &'a str>,
) -> Result<HashSet<&'a str>, CodecError> {
    let mut ids = HashSet::new();
    for id in items {
        ctx.reserve_set(&mut ids, 1, "index SLDPRT native history content")?;
        ids.insert(id);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests;
