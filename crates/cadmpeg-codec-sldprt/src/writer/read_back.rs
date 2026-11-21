// SPDX-License-Identifier: Apache-2.0
//! The features of another format against what their written Keywords
//! records read back as (§3.4): each value a record does not hold, and each
//! selection a record holds by a token of the source document, is named.

use std::collections::HashMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId};
use cadmpeg_ir::report::loss::LossNote;
use serde_json::Value;

use crate::history::project::neutral_feature_id;
use crate::history::write::features::generated_feature_record_id;
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

/// Relative difference within which a number of a definition reads back as
/// the same number through its decimal literal.
const EPS_READ_BACK_NUMBER_E9: f64 = 1.0e-9;

/// The selection kinds whose native token the history encoder writes as the
/// selection: in a document of another format, a token of that document.
const SOURCE_TOKEN_SELECTIONS: [&str; 6] = [
    "native",
    "generated",
    "resolved",
    "local",
    "historical",
    "historical_partial",
];

/// The definitions of the features of another format, as the writer received
/// them.
pub(super) struct SourceDefinitions(HashMap<FeatureId, FeatureDefinition>);

impl SourceDefinitions {
    pub(super) fn of(ir: &CadIr) -> Self {
        Self(
            ir.model
                .features
                .iter()
                .map(|feature| (feature.id.clone(), feature.evaluation.definition().clone()))
                .collect(),
        )
    }

    /// The notes for the features `ir` keeps: that `native` holds them as
    /// Keywords records alone, the values their records read back without,
    /// and the selections their records hold by a source token.
    pub(super) fn notes(&self, ir: &CadIr, native: Option<&SldprtNative>) -> Vec<LossNote> {
        let kept = ir
            .model
            .features
            .iter()
            .filter_map(|feature| Some((feature, self.0.get(&feature.id)?)))
            .collect::<Vec<_>>();
        let Some(native) = native.filter(|_| !kept.is_empty()) else {
            return Vec::new();
        };
        let ids = kept
            .iter()
            .map(|(feature, _)| feature.id.as_str())
            .collect::<Vec<_>>();
        let mut notes = vec![SldprtLossCode::WriterHistoryKeywordsOnly.note(format!(
            "the SLDPRT history writer writes {} feature(s) as Keywords history records \
             alone: SolidWorks keeps a feature's definition in the objects of Config-N and \
             Config-N-ResolvedFeatures, which the written part does not hold for them, so \
             cadmpeg reads these features back and SolidWorks does not rebuild them: {}",
            ids.len(),
            ids.join(", ")
        ))];
        let projected = match crate::history::write::project_features_with_native_inputs(native) {
            Ok(projected) => projected,
            Err(error) => {
                notes.push(SldprtLossCode::WriterFeatureFieldsOmitted.note(format!(
                    "the SLDPRT history writer cannot read its written history records back \
                         ({error}); it does not compare the values of these {} feature(s) with \
                         their records: {}",
                    ids.len(),
                    ids.join(", ")
                )));
                return notes;
            }
        };
        let projected = projected
            .iter()
            .map(|feature| (&feature.id, feature.evaluation.definition()))
            .collect::<HashMap<_, _>>();
        let mut tokens = Vec::new();
        let mut differences = Vec::new();
        for (feature, source) in kept {
            let record = feature
                .native_ref
                .clone()
                .unwrap_or_else(|| generated_feature_record_id(&feature.id));
            let read = neutral_feature_id(&record)
                .ok()
                .and_then(|id| projected.get(&id).copied());
            let Some(read) = read else {
                differences.push(format!(
                    "{} (the record reads back as no feature)",
                    feature.id.as_str()
                ));
                continue;
            };
            let mut comparison = Comparison::default();
            comparison.compare(&value(source), &value(read), "");
            if comparison.token {
                tokens.push(feature.id.as_str());
            }
            if !comparison.differences.is_empty() {
                differences.push(format!(
                    "{} ({})",
                    feature.id.as_str(),
                    comparison.differences.join("; ")
                ));
            }
        }
        if !tokens.is_empty() {
            notes.push(
                SldprtLossCode::WriterSelectionIdentitiesOmitted.note(format!(
                "the SLDPRT history writer writes the selections of {} feature(s) by a token of \
                 the source document, the identity of a source record; the token names no record \
                 of the written part, so the selection does not resolve in it: {}",
                tokens.len(),
                tokens.join(", ")
            )),
            );
        }
        if !differences.is_empty() {
            notes.push(SldprtLossCode::WriterFeatureFieldsOmitted.note(format!(
                "the SLDPRT history writer writes {} feature(s) whose records read back without \
                 these values of their source definitions: {}",
                differences.len(),
                differences.join(", ")
            )));
        }
        notes
    }
}

fn value(definition: &FeatureDefinition) -> Value {
    serde_json::to_value(definition).unwrap_or(Value::Null)
}

#[derive(Default)]
struct Comparison {
    /// A selection reads back as a token of the source document.
    token: bool,
    /// Each source value the record reads back without, by field path.
    differences: Vec<String>,
}

impl Comparison {
    /// Compare the source value at `path` with the value read back there.
    /// Field paths leave out the `value` of each tagged variant. A reference,
    /// a record with an identity field, reads back under the identity of a
    /// written record; the writer's dependency check compares references.
    fn compare(&mut self, source: &Value, read: &Value, path: &str) {
        match (source, read) {
            (Value::Object(source), Value::Object(read)) => {
                let source_kind = source.get("kind").and_then(Value::as_str);
                let read_kind = read.get("kind").and_then(Value::as_str);
                if read_kind == Some("native")
                    && read.get("value").is_some_and(Value::is_string)
                    && source_kind.is_some_and(|kind| SOURCE_TOKEN_SELECTIONS.contains(&kind))
                {
                    self.token = true;
                    return;
                }
                if source.values().any(identity) {
                    return;
                }
                if source_kind != read_kind {
                    self.differences.push(format!(
                        "{}: {} read as {}",
                        label(path),
                        source_kind.unwrap_or("a record"),
                        read_kind.unwrap_or("a record")
                    ));
                    return;
                }
                for (key, value) in source {
                    if value.is_null() || key == "kind" {
                        continue;
                    }
                    let child = match (path.is_empty(), key.as_str()) {
                        (_, "value") => path.to_owned(),
                        (true, _) => key.clone(),
                        (false, _) => format!("{path}.{key}"),
                    };
                    match read.get(key) {
                        Some(read) => self.compare(value, read, &child),
                        None if identity(value)
                            || value
                                .as_object()
                                .is_some_and(|record| record.values().any(identity)) => {}
                        None => self.differences.push(format!(
                            "{}: {} lost",
                            label(&child),
                            summary(value)
                        )),
                    }
                }
            }
            (Value::Array(source), Value::Array(read)) if source.len() == read.len() => {
                for (index, (source, read)) in source.iter().zip(read).enumerate() {
                    self.compare(source, read, &format!("{path}[{index}]"));
                }
            }
            (Value::Number(source_number), Value::Number(read_number)) => {
                let (Some(left), Some(right)) = (source_number.as_f64(), read_number.as_f64())
                else {
                    return;
                };
                if (left - right).abs()
                    > EPS_READ_BACK_NUMBER_E9 * (1.0 + left.abs().max(right.abs()))
                {
                    self.read_as(path, source, read);
                }
            }
            (Value::String(left), Value::String(right))
                if left == right || (left.contains(':') && right.contains(':')) => {}
            _ if source == read => {}
            _ => self.read_as(path, source, read),
        }
    }

    fn read_as(&mut self, path: &str, source: &Value, read: &Value) {
        self.differences.push(format!(
            "{}: {} read as {}",
            label(path),
            summary(source),
            summary(read)
        ));
    }
}

/// Whether `value` is an identity or a list of identities.
fn identity(value: &Value) -> bool {
    let text = |value: &Value| value.as_str().is_some_and(|text| text.contains(':'));
    text(value)
        || value
            .as_array()
            .is_some_and(|items| !items.is_empty() && items.iter().all(text))
}

fn label(path: &str) -> &str {
    if path.is_empty() {
        "operation"
    } else {
        path
    }
}

fn summary(value: &Value) -> String {
    match value {
        Value::Object(map) => map
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("a record")
            .to_owned(),
        Value::Array(items) => format!("{} item(s)", items.len()),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
