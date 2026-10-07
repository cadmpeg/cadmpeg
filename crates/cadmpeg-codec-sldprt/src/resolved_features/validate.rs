//! Native lane validation findings.

use super::assembly::is_supplemental_config_lane_charged;
use crate::records::charged_clone::CloneCharged;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::{
    check::{Check, Finding},
    Severity,
};
use std::collections::BTreeSet;

/// Validate `SolidWorks` native feature-input byte references.
pub(crate) fn validate_native(
    ctx: &DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("sldprt") else {
        return Ok(Vec::new());
    };
    let (native, _native_storage) = match ctx
        .with_scoped_storage("load SLDPRT validation records", || {
            crate::native::SldprtNative::load_charged(ctx, namespace)
        }) {
        Ok(native) => native,
        Err(error) => return invalid_namespace(ctx, &error),
    };
    let mut temporary = ctx.reserve_scoped(0, "SLDPRT validation workspace")?;
    let mut findings = Vec::new();
    for history in ctx.admit_iter(&native.feature_histories, "scan SLDPRT native histories")? {
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
                    message: ctx
                        .format_retained(format_args!("{error}"), "format SLDPRT native finding")?,
                    entity: Some(copy_finding_id(ctx, &history.id)?),
                },
            )?;
        }
        if !history.content.is_empty() {
            let configurations = temporary.with_storage(|| {
                ctx.collect_btree_set(
                    history
                        .configurations
                        .iter()
                        .map(|configuration| configuration.id.as_str()),
                    "index SLDPRT native history content",
                )
            })?;
            let root_features = temporary.with_storage(|| {
                ctx.collect_btree_set(
                    ctx.admit_iter(&history.features, "scan SLDPRT native features")?
                        .filter(|feature| feature.tree_parent.is_none())
                        .map(|feature| feature.id.as_str()),
                    "index SLDPRT native history content",
                )
            })?;
            let all_features = temporary.with_storage(|| {
                ctx.collect_btree_set(
                    history.features.iter().map(|feature| feature.id.as_str()),
                    "index SLDPRT native history content",
                )
            })?;
            let mut seen_configurations = BTreeSet::new();
            let mut seen_features = BTreeSet::new();
            for item in ctx.admit_iter(&history.content, "scan SLDPRT native history content")? {
                let error = match item {
                    crate::records::HistoryContent::Configuration(id) => {
                        if !ctx.contains_btree_set(
                            &configurations,
                            id.as_str(),
                            "find SLDPRT native history content",
                        )? {
                            Some(ctx.format_retained(
                                format_args!(
                                    "SolidWorks history root references missing configuration {id}"
                                ),
                                "format SLDPRT native finding",
                            )?)
                        } else if !temporary.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut seen_configurations,
                                id.as_str(),
                                "index SLDPRT native history content",
                            )
                        })? {
                            Some(ctx.format_retained(
                                format_args!("SolidWorks history root repeats configuration {id}"),
                                "format SLDPRT native finding",
                            )?)
                        } else {
                            None
                        }
                    }
                    crate::records::HistoryContent::Feature(id) => {
                        if !ctx.contains_btree_set(
                            &all_features,
                            id.as_str(),
                            "find SLDPRT native history content",
                        )? {
                            Some(ctx.format_retained(
                                format_args!(
                                    "SolidWorks history root references missing feature {id}"
                                ),
                                "format SLDPRT native finding",
                            )?)
                        } else if !ctx.contains_btree_set(
                            &root_features,
                            id.as_str(),
                            "find SLDPRT native history content",
                        )? {
                            Some(ctx.format_retained(
                                format_args!(
                                    "SolidWorks history root references nested feature {id}"
                                ),
                                "format SLDPRT native finding",
                            )?)
                        } else if !temporary.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut seen_features,
                                id.as_str(),
                                "index SLDPRT native history content",
                            )
                        })? {
                            Some(ctx.format_retained(
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
            for missing in ctx.admit_iter(&configurations, "scan SLDPRT omitted history content")? {
                if ctx.contains_btree_set(
                    &seen_configurations,
                    *missing,
                    "find SLDPRT native history content",
                )? {
                    continue;
                }
                push_finding(
                    ctx,
                    &mut findings,
                    Finding {
                        check: Check::NativeLinks,
                        severity: Severity::Error,
                        message: ctx.format_retained(
                            format_args!("SolidWorks history root omits configuration {missing}"),
                            "format SLDPRT native finding",
                        )?,
                        entity: Some(copy_finding_id(ctx, &history.id)?),
                    },
                )?;
            }
            for missing in ctx.admit_iter(&root_features, "scan SLDPRT omitted history content")? {
                if ctx.contains_btree_set(
                    &seen_features,
                    *missing,
                    "find SLDPRT native history content",
                )? {
                    continue;
                }
                push_finding(
                    ctx,
                    &mut findings,
                    Finding {
                        check: Check::NativeLinks,
                        severity: Severity::Error,
                        message: ctx.format_retained(
                            format_args!("SolidWorks history root omits feature {missing}"),
                            "format SLDPRT native finding",
                        )?,
                        entity: Some(copy_finding_id(ctx, &history.id)?),
                    },
                )?;
            }
        }
    }
    let (mut expected_histories, mut history_reservation) =
        ctx.with_scoped_storage("validate SLDPRT expected histories", || {
            ctx.try_collect_retained_with(
                native.feature_histories.iter(),
                "validate SLDPRT expected histories",
                |record| record.clone_charged(ctx, "validate SLDPRT expected histories"),
            )
        })?;
    let mut history_lanes = Vec::new();
    for lane in ctx.admit_iter(&native.feature_input_lanes, "scan SLDPRT validation lanes")? {
        if !is_supplemental_config_lane_charged(ctx, lane)? {
            temporary.with_storage(|| ctx.push_vec(&mut history_lanes, lane, "validate SLDPRT history lanes"))?;
        }
    }
    history_reservation.with_storage(|| {
        crate::resolved_features::classes::bind_history_classes(
            ctx,
            &mut expected_histories,
            &history_lanes,
        )
    })?;
    for (history, expected_history) in ctx
        .admit_iter(&native.feature_histories, "scan SLDPRT expected histories")?
        .zip(&expected_histories)
    {
        for (feature, expected_feature) in ctx
            .admit_iter(&history.features, "scan SLDPRT expected history features")?
            .zip(&expected_history.features)
        {
            if !ctx.equal(
                &feature.input_class,
                &expected_feature.input_class,
                "compare SLDPRT feature classes",
            )? {
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
    let crate::native::lanes::ExpectedLanes {
        pairs: expected_lanes,
        storage: _expected_lanes_reservation,
    } = crate::native::lanes::expected_lanes_charged(ctx, &native)?;
    for (lane, expected_lane) in ctx.admit_iter(&expected_lanes, "scan SLDPRT expected lanes")? {
        for (entity, expected_entity) in ctx
            .admit_iter(
                &lane.sketch_entities,
                "scan SLDPRT expected sketch entities",
            )?
            .zip(&expected_lane.sketch_entities)
        {
            if !ctx.equal(
                &entity.feature_ref,
                &expected_entity.feature_ref,
                "compare SLDPRT sketch ownership",
            )? {
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
            if !ctx.equal(
                &entity.links,
                &expected_entity.links,
                "compare SLDPRT sketch links",
            )? {
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
    let mut findings = Vec::new();
    let message = ctx.format_retained(
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
    ctx.copy_retained_text(id, "retain SLDPRT native finding identity")
}

fn push_finding(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    finding: Finding,
) -> Result<(), CodecError> {
    ctx.push_vec(findings, finding, "collect SLDPRT native findings")
}

#[cfg(test)]
mod tests;
