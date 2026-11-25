// SPDX-License-Identifier: Apache-2.0
//! The features of another format against what their written Keywords
//! records read back as (§3.4): each value a record does not hold, each
//! selection a record holds by a token of the source document, and each
//! feature whose record states fewer bodies than it produces, is named.

use std::collections::HashMap;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId};
use cadmpeg_ir::report::loss::LossNote;
use serde_json::Value;

use crate::history::project::neutral_feature_id;
use crate::history::write::features::generated_feature_record_id;
use crate::loss::{named, SldprtLossCode};
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

/// The definitions of the features of another format, with the number of
/// bodies each produces, as the writer received them.
pub(super) struct SourceDefinitions(HashMap<FeatureId, (FeatureDefinition, usize)>);

impl SourceDefinitions {
    pub(super) fn of(ir: &CadIr) -> Self {
        Self(
            ir.model
                .features
                .iter()
                .map(|feature| {
                    (
                        feature.id.clone(),
                        (
                            feature.evaluation.definition().clone(),
                            feature.evaluation.outputs().len(),
                        ),
                    )
                })
                .collect(),
        )
    }

    /// The notes for the features `ir` keeps: that `native` holds them as
    /// Keywords records alone, the values and the bodies their records read
    /// back without, and the selections their records hold by a source token.
    /// The records are read back under `limits`.
    pub(super) fn notes(
        &self,
        ir: &CadIr,
        native: Option<&SldprtNative>,
        limits: ResourceLimits,
    ) -> Vec<LossNote> {
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
        let projected =
            match crate::history::write::project_features_with_native_inputs(native, limits) {
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
            .map(|feature| (&feature.id, feature))
            .collect::<HashMap<_, _>>();
        let profiles = ir
            .model
            .sketches
            .iter()
            .map(|sketch| (sketch.id.as_str(), sketch.profiles.len()))
            .collect::<HashMap<_, _>>();
        let mut tokens = Vec::new();
        let mut differences = Vec::new();
        let mut bodiless = Vec::new();
        let mut bodies = 0;
        for (feature, (source, outputs)) in kept {
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
            if let Some(missing) = outputs
                .checked_sub(read.evaluation.outputs().len())
                .filter(|missing| *missing > 0)
            {
                bodiless.push(feature.id.as_str());
                bodies += missing;
            }
            let mut comparison = Comparison::new(&profiles);
            comparison.compare(&value(source), &value(read.evaluation.definition()), "");
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
        if !bodiless.is_empty() {
            notes.push(SldprtLossCode::WriterFeatureFieldsOmitted.note(format!(
                "the SLDPRT history writer writes {} feature(s) whose records read back without \
                 {bodies} of the bodies their source features produce: {}",
                bodiless.len(),
                named(&bodiless)
            )));
        }
        notes
    }
}

fn value(definition: &FeatureDefinition) -> Value {
    cadmpeg_core::serde_erase::to_json_value(definition).unwrap_or(Value::Null)
}

struct Comparison<'a> {
    /// The number of profiles of each sketch, by sketch identity.
    profiles: &'a HashMap<&'a str, usize>,
    /// A selection reads back as a token of the source document.
    token: bool,
    /// Each source value the record reads back without, by field path.
    differences: Vec<String>,
}

impl<'a> Comparison<'a> {
    fn new(profiles: &'a HashMap<&'a str, usize>) -> Self {
        Self {
            profiles,
            token: false,
            differences: Vec::new(),
        }
    }

    /// Compare the source value at `path` with the value read back there.
    /// Field paths leave out the `value` of each tagged variant. An identity
    /// reads back as the identity of a written record, so identities and
    /// tagged references are not compared; the writer's dependency check
    /// compares references.
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
                if reference(source) {
                    return;
                }
                if source_kind != read_kind {
                    let whole_record = read.values().any(identity);
                    if whole_record && self.every_profile(source) {
                        return;
                    }
                    let source_kind = source_kind.unwrap_or("a record");
                    self.differences.push(if whole_record {
                        format!(
                            "{}: {source_kind} read as a reference to a whole record",
                            label(path)
                        )
                    } else {
                        format!(
                            "{}: {source_kind} read as {}",
                            label(path),
                            read_kind.unwrap_or("a record")
                        )
                    });
                    return;
                }
                for (key, value) in source {
                    if value.is_null() || key == "kind" || identity(value) {
                        continue;
                    }
                    let child = match (path.is_empty(), key.as_str()) {
                        (_, "value") => path.to_owned(),
                        (true, _) => key.clone(),
                        (false, _) => format!("{path}.{key}"),
                    };
                    match read.get(key) {
                        Some(read) => self.compare(value, read, &child),
                        None if value.as_object().is_some_and(reference) => {}
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

    /// Whether `source` selects every profile of its sketch, which the
    /// whole sketch selects too.
    fn every_profile(&self, source: &serde_json::Map<String, Value>) -> bool {
        if source.get("kind").and_then(Value::as_str) != Some("sketch_profiles") {
            return false;
        }
        let selection = source.get("value");
        let Some(count) = selection
            .and_then(|selection| selection.get("sketch"))
            .and_then(Value::as_str)
            .and_then(|sketch| self.profiles.get(sketch))
        else {
            return false;
        };
        let Some(indices) = selection
            .and_then(|selection| selection.get("profiles"))
            .and_then(Value::as_array)
            .map(|indices| {
                indices
                    .iter()
                    .map(|index| index.as_u64().and_then(|index| usize::try_from(index).ok()))
                    .collect::<Option<std::collections::BTreeSet<_>>>()
            })
        else {
            return false;
        };
        *count > 0 && indices.is_some_and(|indices| indices.into_iter().eq(0..*count))
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

/// Whether `record` is a tagged variant whose value is an identity.
fn reference(record: &serde_json::Map<String, Value>) -> bool {
    record.contains_key("kind") && record.get("value").is_some_and(identity)
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::Comparison;

    #[test]
    fn a_value_beside_an_identity_is_compared() {
        let profiles = HashMap::new();
        let mut comparison = Comparison::new(&profiles);
        comparison.compare(
            &json!({"kind": "sketch_profiles", "value": {"sketch": "other:sketch#a", "profiles": [0, 1]}}),
            &json!({"kind": "sketch_profiles", "value": {"sketch": "sldprt:sketch#1", "profiles": [0]}}),
            "profile",
        );
        assert_eq!(
            comparison.differences,
            ["profile.profiles: 2 item(s) read as 1 item(s)"]
        );
    }

    #[test]
    fn a_selection_of_every_profile_reads_back_as_its_whole_sketch() {
        let profiles = HashMap::from([("other:sketch#a", 2)]);
        let read = json!({"kind": "sketch", "value": "sldprt:sketch#1"});
        let selection = |indices: &[u32]| json!({"kind": "sketch_profiles", "value": {"sketch": "other:sketch#a", "profiles": indices}});
        let mut every = Comparison::new(&profiles);
        every.compare(&selection(&[1, 0]), &read, "profile");
        assert!(every.differences.is_empty(), "{:?}", every.differences);
        let mut subset = Comparison::new(&profiles);
        subset.compare(&selection(&[1]), &read, "profile");
        assert_eq!(
            subset.differences,
            ["profile: sketch_profiles read as a reference to a whole record"]
        );
    }
}
