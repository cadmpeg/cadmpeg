// SPDX-License-Identifier: Apache-2.0
//! The names, tags and texts of the Keywords history as the reader's XML
//! parser reads them back (§3.4). This is the one place that makes a native
//! history writable; the Keywords emitter only escapes. A character XML 1.0
//! does not allow is written as U+FFFD. A property whose name is not an
//! attribute name of its element is left out. A feature whose tag XML cannot
//! hold, or that the reader reads as another record, is written under
//! `Feature` with its kind in `Type`. An element of the history whose tag XML
//! cannot hold is left out with its subtree. A dimension whose name becomes
//! the name of another dimension of its record is left out. A text item or
//! dimension value is written without its edge whitespace, and a text item
//! that holds only whitespace is left out. Each is named.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::history::write::features::generated_feature_record_id;
use crate::history::write::xml::{valid_xml_name, xml_char};
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::records::{FeatureContent, HistoryContent, HistoryElement, HistoryElementContent};

/// The attributes the emitter writes itself on each element it generates. A
/// property with one of these names is left out: it would repeat the
/// attribute, or the reader would read it as the field.
const ROOT_ATTRIBUTES: &[&str] = &["Name"];
const CONFIGURATION_ATTRIBUTES: &[&str] = &["Name", "SourceIndex", "Material"];
const FEATURE_ATTRIBUTES: &[&str] = &["id", "Name", "Type", "Suppressed"];
const DIMENSION_ATTRIBUTES: &[&str] = &["Name"];

/// What [`readable_keywords`] changed, by record.
#[derive(Default)]
struct Changes {
    /// Records with a character replaced.
    replaced: BTreeSet<String>,
    /// Records with a text item or dimension value trimmed or left out.
    trimmed: BTreeSet<String>,
    /// Records with properties left out, each with the names.
    left_out: Vec<String>,
    /// Dimensions left out.
    dimensions_left_out: Vec<String>,
    /// Features written under `Feature`, each with its tag.
    tags: Vec<String>,
    /// `SolidWorks` features whose source tag is not written, each with the
    /// tag and the tag written.
    source_tags: Vec<String>,
    /// History elements left out with their subtrees.
    elements: Vec<String>,
}

/// Make every name, tag and text of the histories of `native` one the
/// reader's XML parser reads back, and name each record changed. Records are
/// named by the identity of the `ir` feature or parameter they write.
pub(crate) fn readable_keywords(ir: &CadIr, native: &mut SldprtNative) -> Vec<LossNote> {
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| {
            let record = feature
                .native_ref
                .clone()
                .unwrap_or_else(|| generated_feature_record_id(&feature.id));
            (record, feature)
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
    let mut changes = Changes::default();
    for history in &mut native.feature_histories {
        let label = format!("history {}", history.id);
        let mut changed = history.part_name.as_mut().is_some_and(replace);
        changed |= properties(
            &mut history.properties,
            ROOT_ATTRIBUTES,
            &label,
            &mut changes.left_out,
        );
        history.content.retain_mut(|item| match item {
            HistoryContent::Text(text) => {
                changed |= replace(text);
                readable_text(text, &label, &mut changes.trimmed)
            }
            HistoryContent::Element(element) => readable_element(element, &label, &mut changes),
            HistoryContent::Configuration(_) | HistoryContent::Feature(_) => true,
        });
        if changed {
            changes.replaced.insert(label);
        }
        for configuration in &mut history.configurations {
            let label = format!("configuration {}", configuration.id);
            let mut changed = replace(&mut configuration.name);
            changed |= configuration.material.as_mut().is_some_and(replace);
            changed |= properties(
                &mut configuration.properties,
                CONFIGURATION_ATTRIBUTES,
                &label,
                &mut changes.left_out,
            );
            if changed {
                changes.replaced.insert(label);
            }
        }
        for feature in &mut history.features {
            let source = features.get(&feature.id).copied();
            let owner = source.map_or_else(
                || feature.id.clone(),
                |source| source.id.as_str().to_owned(),
            );
            let owner = owner.as_str();
            let tag = feature.xml_tag.as_str();
            let other_record = matches!(tag, "Keywords" | "Configuration" | "Dimension")
                || (feature.tree_parent.is_none() && tag == "Application");
            if !valid_xml_name(tag) || other_record {
                changes.tags.push(format!("{owner} <{tag}>"));
                feature.xml_tag = "Feature".into();
            }
            if let Some((source, tag)) = source.and_then(|source| {
                source
                    .source_tag
                    .as_ref()
                    .filter(|tag| source.id.as_str().starts_with("sldprt:") && !valid_xml_name(tag))
                    .map(|tag| (source, tag))
            }) {
                changes.source_tags.push(format!(
                    "{} {tag:?} as <{}>",
                    source.id.as_str(),
                    feature.xml_tag
                ));
            }
            let mut changed = replace(&mut feature.name);
            changed |= replace(&mut feature.kind);
            changed |= feature.text.as_mut().is_some_and(replace);
            if let Some(text) = &mut feature.text {
                if !readable_text(text, owner, &mut changes.trimmed) {
                    feature.text = None;
                }
            }
            changed |= properties(
                &mut feature.properties,
                FEATURE_ATTRIBUTES,
                owner,
                &mut changes.left_out,
            );
            feature.content.retain_mut(|item| match item {
                FeatureContent::Text(text) => {
                    changed |= replace(text);
                    readable_text(text, owner, &mut changes.trimmed)
                }
                FeatureContent::Dimension(_) | FeatureContent::Feature(_) => true,
            });
            if changed {
                changes.replaced.insert(owner.to_owned());
            }
            for (name, expression) in &mut feature.parameters {
                let label = || dimension_label(&parameters, owner, name.as_str());
                if replace(expression) {
                    changes.replaced.insert(label());
                }
                let kept = expression.trim();
                if kept.len() != expression.len() {
                    *expression = kept.to_owned();
                    changes.trimmed.insert(label());
                }
            }
            for (name, properties_of) in &mut feature.dimension_properties {
                let label = dimension_label(&parameters, owner, name);
                if properties(
                    properties_of,
                    DIMENSION_ATTRIBUTES,
                    &label,
                    &mut changes.left_out,
                ) {
                    changes.replaced.insert(label);
                }
            }
            let (written, collided) = dimension_names(feature);
            for name in written {
                changes
                    .replaced
                    .insert(dimension_label(&parameters, owner, &name));
            }
            for name in collided {
                let label = dimension_label(&parameters, owner, &name);
                changes.replaced.remove(&label);
                changes.dimensions_left_out.push(label);
            }
        }
    }
    changes.notes()
}

impl Changes {
    fn notes(self) -> Vec<LossNote> {
        let mut notes = Vec::new();
        if !self.dimensions_left_out.is_empty() {
            notes.push(SldprtLossCode::WriterFeaturesOmitted.note(format!(
                "the SLDPRT history writer writes no dimension for {} parameter(s): with U+FFFD \
                 in place of each character XML 1.0 does not allow, the name of each is the name \
                 of another dimension of its Keywords record, which holds one dimension per name: \
                 {}",
                self.dimensions_left_out.len(),
                self.dimensions_left_out.join(", ")
            )));
        }
        if !self.left_out.is_empty() {
            notes.push(SldprtLossCode::WriterFeatureFieldsOmitted.note(format!(
                "the SLDPRT history writer writes {} record(s) without the source properties \
                 whose names are not attribute names of their Keywords element (an XML name \
                 without a colon, other than xmlns and the attributes the writer writes on that \
                 element): {}",
                self.left_out.len(),
                self.left_out.join("; ")
            )));
        }
        if !self.tags.is_empty() {
            notes.push(SldprtLossCode::WriterHistoryNameNotWritten.note(format!(
                "the SLDPRT history writer writes {} feature(s) under the Feature tag, with \
                 their kind in Type, because XML cannot hold their tag or the reader reads an \
                 element of that tag as another record: {}",
                self.tags.len(),
                self.tags.join(", ")
            )));
        }
        if !self.source_tags.is_empty() {
            notes.push(SldprtLossCode::WriterHistoryNameNotWritten.note(format!(
                "the SLDPRT history writer writes {} SolidWorks feature(s) under another tag \
                 than the source tag they state, because XML cannot hold that tag: {}",
                self.source_tags.len(),
                self.source_tags.join(", ")
            )));
        }
        if !self.elements.is_empty() {
            notes.push(SldprtLossCode::WriterHistoryNameNotWritten.note(format!(
                "the SLDPRT history writer leaves out {} Keywords element(s) whose tag XML \
                 cannot hold, with the elements and texts inside them: {}",
                self.elements.len(),
                self.elements.join(", ")
            )));
        }
        if !self.trimmed.is_empty() {
            notes.push(SldprtLossCode::WriterKeywordsTextTrimmed.note(format!(
                "the SLDPRT history writer writes the text items and dimension values of {} \
                 record(s) without their edge whitespace, and leaves out their text items that \
                 hold only whitespace, which the reader does not keep: {}",
                self.trimmed.len(),
                self.trimmed.into_iter().collect::<Vec<_>>().join(", ")
            )));
        }
        if !self.replaced.is_empty() {
            notes.push(
                SldprtLossCode::WriterKeywordsCharactersReplaced.note(format!(
                "the SLDPRT history writer writes U+FFFD in place of each character XML 1.0 does \
                 not allow in the texts of {} record(s): {}",
                self.replaced.len(),
                self.replaced.into_iter().collect::<Vec<_>>().join(", ")
            )),
            );
        }
        notes
    }
}

/// Make `element`, an element of the history `parent` names, readable with
/// its subtree. Returns false when XML cannot hold its tag, so the caller
/// leaves it out.
fn readable_element(element: &mut HistoryElement, parent: &str, changes: &mut Changes) -> bool {
    let label = match element.attributes.get("Name") {
        Some(name) => format!("{parent} > {} {name:?}", element.tag),
        None => format!("{parent} > {}", element.tag),
    };
    if !valid_xml_name(&element.tag) {
        changes.elements.push(label);
        return false;
    }
    let mut changed = properties(&mut element.attributes, &[], &label, &mut changes.left_out);
    element.content.retain_mut(|item| match item {
        HistoryElementContent::Text(text) => {
            changed |= replace(text);
            readable_text(text, &label, &mut changes.trimmed)
        }
        HistoryElementContent::Element(child) => readable_element(child, &label, changes),
    });
    if changed {
        changes.replaced.insert(label);
    }
    true
}

/// Write `text`, a text item of the record `label` names, without its edge
/// whitespace. Returns false when nothing is left, so the caller leaves the
/// item out. Records `label` in `trimmed` when the item changes or is left
/// out.
fn readable_text(text: &mut String, label: &str, trimmed: &mut BTreeSet<String>) -> bool {
    let kept = text.trim();
    if kept.is_empty() || kept.len() != text.len() {
        *text = kept.to_owned();
        trimmed.insert(label.to_owned());
    }
    !text.is_empty()
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
        .filter(|name| !valid_xml_name(name) || *name == "xmlns" || reserved.contains(name))
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
    if text.chars().all(xml_char) {
        return false;
    }
    *text = text
        .chars()
        .map(|character| {
            if xml_char(character) {
                character
            } else {
                char::REPLACEMENT_CHARACTER
            }
        })
        .collect();
    true
}
