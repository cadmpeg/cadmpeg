// SPDX-License-Identifier: Apache-2.0
//! The texts and attribute names of the Keywords history as the reader's XML
//! parser reads them back (§3.4). A character XML 1.0 does not allow is
//! written as U+FFFD, a property whose name is not an attribute name of its
//! element is left out, and a dimension whose name becomes the name of another
//! dimension of its record is left out; each is named.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::history::write::features::generated_feature_record_id;
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::records::{FeatureContent, HistoryContent};

/// Make every text and attribute name of the histories of `native` one the
/// reader's XML parser reads back, and name each record changed. Records are
/// named by the identity of the `ir` feature or parameter they write.
pub(super) fn readable_keywords(ir: &CadIr, native: &mut SldprtNative) -> Vec<LossNote> {
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| {
            let record = feature
                .native_ref
                .clone()
                .unwrap_or_else(|| generated_feature_record_id(&feature.id));
            (record, feature.id.as_str())
        })
        .collect::<HashMap<_, _>>();
    let parameters = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (parameter.owner.as_ref()?.as_str(), parameter.name.as_str()),
                parameter.id.as_str(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut replaced = BTreeSet::new();
    let mut left_out = Vec::new();
    let mut dimensions_left_out = Vec::new();
    for history in &mut native.feature_histories {
        let label = format!("history {}", history.id);
        let mut changed = history.part_name.as_mut().is_some_and(replace);
        changed |= properties(&mut history.properties, &["Name"], &label, &mut left_out);
        for item in &mut history.content {
            if let HistoryContent::Text(text) = item {
                changed |= replace(text);
            }
        }
        if changed {
            replaced.insert(label);
        }
        for configuration in &mut history.configurations {
            let label = format!("configuration {}", configuration.id);
            let mut changed = replace(&mut configuration.name);
            changed |= configuration.material.as_mut().is_some_and(replace);
            changed |= properties(
                &mut configuration.properties,
                &["Name", "SourceIndex", "Material"],
                &label,
                &mut left_out,
            );
            if changed {
                replaced.insert(label);
            }
        }
        for feature in &mut history.features {
            let owner = features
                .get(&feature.id)
                .map_or_else(|| feature.id.clone(), |id| (*id).to_owned());
            let owner = owner.as_str();
            let mut changed = replace(&mut feature.name);
            changed |= replace(&mut feature.kind);
            changed |= feature.text.as_mut().is_some_and(replace);
            changed |= properties(
                &mut feature.properties,
                &["id", "Name", "Type", "Suppressed"],
                owner,
                &mut left_out,
            );
            for item in &mut feature.content {
                if let FeatureContent::Text(text) = item {
                    changed |= replace(text);
                }
            }
            if changed {
                replaced.insert(owner.to_owned());
            }
            for (name, expression) in &mut feature.parameters {
                if replace(expression) {
                    replaced.insert(dimension_label(&parameters, owner, name.as_str()));
                }
            }
            for (name, properties_of) in &mut feature.dimension_properties {
                let label = dimension_label(&parameters, owner, name);
                if properties(properties_of, &["Name"], &label, &mut left_out) {
                    replaced.insert(label);
                }
            }
            let (written, collided) = dimension_names(feature);
            for name in written {
                replaced.insert(dimension_label(&parameters, owner, &name));
            }
            for name in collided {
                let label = dimension_label(&parameters, owner, &name);
                replaced.remove(&label);
                dimensions_left_out.push(label);
            }
        }
    }
    let mut notes = Vec::new();
    if !dimensions_left_out.is_empty() {
        notes.push(SldprtLossCode::WriterFeaturesOmitted.note(format!(
            "the SLDPRT history writer writes no dimension for {} parameter(s): with U+FFFD in \
             place of each character XML 1.0 does not allow, the name of each is the name of \
             another dimension of its Keywords record, which holds one dimension per name: {}",
            dimensions_left_out.len(),
            dimensions_left_out.join(", ")
        )));
    }
    if !left_out.is_empty() {
        notes.push(SldprtLossCode::WriterFeatureFieldsOmitted.note(format!(
            "the SLDPRT history writer writes {} record(s) without the source properties whose \
             names are not attribute names of their Keywords element (an ASCII XML name without \
             a namespace prefix, other than xmlns and the element's own attributes): {}",
            left_out.len(),
            left_out.join("; ")
        )));
    }
    if !replaced.is_empty() {
        notes.push(
            SldprtLossCode::WriterKeywordsCharactersReplaced.note(format!(
            "the SLDPRT history writer writes U+FFFD in place of each character XML 1.0 does not \
             allow in the texts of {} record(s): {}",
            replaced.len(),
            replaced.into_iter().collect::<Vec<_>>().join(", ")
        )),
        );
    }
    notes
}

fn dimension_label(parameters: &HashMap<(&str, &str), &str>, owner: &str, name: &str) -> String {
    parameters.get(&(owner, name)).map_or_else(
        || format!("{owner} dimension {name}"),
        |id| (*id).to_owned(),
    )
}

/// Write each dimension name of `feature` that holds a character XML 1.0
/// does not allow with U+FFFD in its place, in its parameters, its dimension
/// properties and its content. A dimension whose written name is already a
/// name of `feature` is left out. Returns the source names written and the
/// source names left out.
fn dimension_names(feature: &mut crate::records::Feature) -> (Vec<String>, Vec<String>) {
    let renames = feature
        .parameters
        .keys()
        .filter_map(|name| {
            let mut text = name.as_str().to_owned();
            replace(&mut text).then(|| (name.as_str().to_owned(), text))
        })
        .collect::<Vec<_>>();
    let mut written_names = Vec::new();
    let mut collided = Vec::new();
    for (source, written) in renames {
        let expression = feature.parameters.remove(source.as_str());
        let properties = feature.dimension_properties.remove(&source);
        if feature.parameters.contains_key(written.as_str()) {
            feature
                .content
                .retain(|item| !matches!(item, FeatureContent::Dimension(name) if *name == source));
            collided.push(source);
            continue;
        }
        if let (Some(expression), Some(name)) = (expression, NonBlankString::new(written.clone())) {
            feature.parameters.insert(name, expression);
        }
        if let Some(properties) = properties {
            feature
                .dimension_properties
                .insert(written.clone(), properties);
        }
        for item in &mut feature.content {
            if matches!(item, FeatureContent::Dimension(name) if *name == source) {
                *item = FeatureContent::Dimension(written.clone());
            }
        }
        written_names.push(source);
    }
    (written_names, collided)
}

/// Leave out of `properties` each entry whose name is not an attribute name
/// of an element whose own attributes are `reserved`, recording `label` with
/// the names, and replace the characters XML 1.0 does not allow in the
/// values. Returns whether a value changed.
fn properties(
    properties: &mut BTreeMap<NonBlankString, String>,
    reserved: &[&str],
    label: &str,
    left_out: &mut Vec<String>,
) -> bool {
    let names = properties
        .keys()
        .map(NonBlankString::as_str)
        .filter(|name| {
            !crate::history::write::xml::valid_xml_name(name)
                || name.contains(':')
                || *name == "xmlns"
                || reserved.contains(name)
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for name in &names {
        properties.remove(name.as_str());
    }
    if !names.is_empty() {
        left_out.push(format!("{label} ({})", names.join(", ")));
    }
    let mut changed = false;
    for value in properties.values_mut() {
        changed |= replace(value);
    }
    changed
}

/// Write U+FFFD in place of each character of `text` XML 1.0 does not allow,
/// and return whether one was replaced.
fn replace(text: &mut String) -> bool {
    let allowed = |character: char| {
        matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}')
            || character >= '\u{10000}'
    };
    if text.chars().all(allowed) {
        return false;
    }
    *text = text
        .chars()
        .map(|character| {
            if allowed(character) {
                character
            } else {
                char::REPLACEMENT_CHARACTER
            }
        })
        .collect();
    true
}
