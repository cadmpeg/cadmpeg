// SPDX-License-Identifier: Apache-2.0
//! Custom `FeatureScript` features keep a named loss at their stop.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::bundle::{changed, read, timeline};
use crate::test_support::*;
use serde_json::json;

#[test]
fn custom_features_name_the_missing_frozen_shape_even_with_rollback_details() {
    let source = fixture("plate-fillet");
    let (original, _) = read(&source).unwrap();
    for ordinal in [1, 2] {
        let bytes = changed(&source, |path, value| {
            if path.ends_with("features.json") {
                // A custom studio can use the same feature type as a built-in.
                value["features"][ordinal]["namespace"] = json!("synthetic");
            }
        });
        let (model, report) = read(&bytes).unwrap();
        let wire = timeline(&model);
        let feature = &wire["model"]["features"][ordinal];
        assert_eq!(feature["definition"]["value"]["kind"], "native");
        assert_eq!(feature["outputs"], json!([]));
        let name = feature["name"].as_str().unwrap();
        let suffix = format!("; subject: {name}");
        let unsupported: Vec<_> = report
            .losses()
            .iter()
            .filter(|l| l.code.as_deref() == Some("onshape/history.not-supported"))
            .collect();
        // The custom feature has exactly one loss. A later feature that consumed its result can
        // add its own; nothing earlier may.
        let later: Vec<&str> = wire["model"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .skip(ordinal + 1)
            .filter_map(|f| f["name"].as_str())
            .collect();
        let losses: Vec<_> = unsupported
            .iter()
            .filter(|l| l.detail.ends_with(&suffix))
            .collect();
        assert_eq!(losses.len(), 1, "{unsupported:?}");
        for other in unsupported.iter().filter(|l| !l.detail.ends_with(&suffix)) {
            assert!(
                later
                    .iter()
                    .any(|name| other.detail.ends_with(&format!("; subject: {name}"))),
                "{other:?}"
            );
        }
        let reason = format!(
            "custom FeatureScript feature {name} is not supported yet: bundle format 1 \
             lacks complete per-feature body geometry to prove a frozen step"
        );
        assert_eq!(
            losses[0].detail,
            format!("{reason}; kept the final STEP geometry{suffix}")
        );
        assert!(feature["definition"]["value"]["value"]["kind"]
            .as_str()
            .unwrap()
            .contains(&reason));
        assert_eq!(model.source_step, original.source_step);
        assert_eq!(model.ir.geometry, original.ir.geometry);
    }
}

#[test]
fn custom_feature_stop_uses_its_id_when_the_name_is_absent_or_blank() {
    for name in [None, Some(""), Some(" \t ")] {
        let bytes = changed(&fixture("unsupported"), |path, value| {
            if path.ends_with("features.json") {
                let feature = value["features"][2].as_object_mut().unwrap();
                match name {
                    Some(name) => feature.insert("name".into(), json!(name)),
                    None => feature.remove("name"),
                };
            }
        });
        let (_, report) = read(&bytes).unwrap();
        assert!(
            report.losses().iter().any(|l| l
                .detail
                .contains("custom FeatureScript feature Custom feature")
                && l.detail.ends_with("; subject: Custom feature")),
            "{name:?}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn suppressed_custom_features_do_not_report_a_frozen_shape_gap() {
    for key in ["suppressed", "parentSuppressed"] {
        let bytes = changed(&fixture("unsupported"), |path, value| {
            if path.ends_with("features.json") {
                value["features"][2][key] = json!(true);
            }
        });
        let (_, report) = read(&bytes).unwrap();
        assert!(!report.losses().iter().any(|l| l
            .code
            .as_deref()
            .is_some_and(|code| code.starts_with("onshape/"))));
    }
}
