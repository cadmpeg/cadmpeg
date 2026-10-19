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
    let native = match crate::native::SldprtNative::load_with_context(ctx, namespace) {
        Ok(native) => native,
        Err(error) => {
            let findings = invalid_namespace(&error);
            // A session refusal inside the namespace load fails validation;
            // only a malformed namespace becomes a finding.
            if let CodecError::ResourceLimit(limit) = CodecError::from(error) {
                return Err(CodecError::ResourceLimit(limit));
            }
            return Ok(findings);
        }
    };
    let mut findings = Vec::new();
    for history in &native.feature_histories {
        if let Err(error) = crate::writer::validate_feature_graph(&history.features) {
            findings.push(Finding {
                related: Vec::new(),
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: error.to_string(),
                entity: Some(history.id.clone()),
            });
        }
        if !history.content.is_empty() {
            let configurations = history
                .configurations
                .iter()
                .map(|configuration| configuration.id.as_str())
                .collect::<HashSet<_>>();
            let root_features = history
                .features
                .iter()
                .filter(|feature| feature.tree_parent.is_none())
                .map(|feature| feature.id.as_str())
                .collect::<HashSet<_>>();
            let all_features = history
                .features
                .iter()
                .map(|feature| feature.id.as_str())
                .collect::<HashSet<_>>();
            let mut seen_configurations = HashSet::new();
            let mut seen_features = HashSet::new();
            for item in &history.content {
                let error = match item {
                    crate::records::HistoryContent::Configuration(id) => {
                        if !configurations.contains(id.as_str()) {
                            Some((
                                format!(
                                    "SolidWorks history root references missing configuration {id}"
                                ),
                                None,
                            ))
                        } else if !seen_configurations.insert(id.as_str()) {
                            Some((
                                format!("SolidWorks history root repeats configuration {id}"),
                                Some(id),
                            ))
                        } else {
                            None
                        }
                    }
                    crate::records::HistoryContent::Feature(id) => {
                        if !all_features.contains(id.as_str()) {
                            Some((
                                format!("SolidWorks history root references missing feature {id}"),
                                None,
                            ))
                        } else if !root_features.contains(id.as_str()) {
                            Some((
                                format!("SolidWorks history root references nested feature {id}"),
                                Some(id),
                            ))
                        } else if !seen_features.insert(id.as_str()) {
                            Some((
                                format!("SolidWorks history root repeats feature {id}"),
                                Some(id),
                            ))
                        } else {
                            None
                        }
                    }
                    crate::records::HistoryContent::Text(_) => None,
                };
                if let Some((message, related)) = error {
                    findings.push(Finding {
                        related: related
                            .filter(|id| *id != &history.id)
                            .cloned()
                            .into_iter()
                            .collect(),
                        check: Check::NativeLinks,
                        severity: Severity::Error,
                        message,
                        entity: Some(history.id.clone()),
                    });
                }
            }
            for missing in history
                .configurations
                .iter()
                .map(|configuration| configuration.id.as_str())
                .filter(|id| seen_configurations.insert(*id))
            {
                findings.push(Finding {
                    related: (missing != history.id.as_str())
                        .then(|| missing.to_owned())
                        .into_iter()
                        .collect(),
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("SolidWorks history root omits configuration {missing}"),
                    entity: Some(history.id.clone()),
                });
            }
            for missing in history
                .features
                .iter()
                .filter(|feature| feature.tree_parent.is_none())
                .map(|feature| feature.id.as_str())
                .filter(|id| seen_features.insert(*id))
            {
                findings.push(Finding {
                    related: (missing != history.id.as_str())
                        .then(|| missing.to_owned())
                        .into_iter()
                        .collect(),
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("SolidWorks history root omits feature {missing}"),
                    entity: Some(history.id.clone()),
                });
            }
        }
    }
    let history_lanes = native
        .feature_input_lanes
        .iter()
        .filter(|lane| !is_supplemental_config_lane(lane))
        .collect::<Vec<_>>();
    let expected_classes = crate::resolved_features::classes::history_classes(
        &native.feature_histories,
        &history_lanes,
    );
    for (feature, expected_class) in native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .zip(expected_classes)
    {
        if feature.input_class.as_deref() != expected_class {
            findings.push(Finding {
                related: Vec::new(),
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "SolidWorks history feature class does not match its feature-input index"
                    .into(),
                entity: Some(feature.id.clone()),
            });
        }
    }
    for (lane, expected_lane) in crate::native::lanes::expected_lanes(ctx, &native)? {
        for (entity, expected_entity) in lane
            .sketch_entities
            .iter()
            .zip(&expected_lane.sketch_entities)
        {
            if entity.feature_ref != expected_entity.feature_ref {
                findings.push(Finding {
                    related: Vec::new(),
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "SolidWorks sketch-input marker has inconsistent feature ownership"
                        .into(),
                    entity: Some(entity.id().to_owned()),
                });
            }
            if entity.links != expected_entity.links {
                findings.push(Finding {
                    related: Vec::new(),
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "SolidWorks sketch-input marker has inconsistent local links".into(),
                    entity: Some(entity.id().to_owned()),
                });
            }
        }
    }

    Ok(findings)
}

fn invalid_namespace(error: &cadmpeg_ir::NativeConvertError) -> Vec<Finding> {
    vec![Finding {
        related: Vec::new(),
        check: Check::NativeLinks,
        severity: Severity::Error,
        message: format!("invalid SolidWorks native namespace: {error}"),
        entity: None,
    }]
}

#[cfg(test)]
mod tests;
