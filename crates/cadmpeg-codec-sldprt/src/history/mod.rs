// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` Keywords XML feature history.

pub(crate) mod bind;
pub(crate) mod classify;
pub(crate) mod configuration;
mod encode;
pub(crate) mod extrude_direction;
pub(crate) mod hash;
pub(crate) mod literals;
pub(crate) mod parameters;
pub(crate) mod project;
pub(crate) mod selections;
pub(crate) mod write;

use self::classify::classless_builtin_node;

use crate::container::{ContainerScan, Section};
use crate::records::FeatureSource;
use crate::records::{
    Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent, HistoryElement,
    HistoryElementContent,
};
use cadmpeg_core::decode::{ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, Part};
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap};

/// Keys one source element's attributes, charging every key the reader cannot
/// key.
///
/// The element is still transferred. A key holding no non-whitespace character
/// cannot be asked for, and a key the element states twice is already taken, so
/// the charge names the element and, for a restated key, the key itself.
fn keyed_attributes<'name, 'value>(
    losses: &mut Vec<LossNote>,
    record: &str,
    entries: impl IntoIterator<Item = (&'name str, &'value str)>,
) -> BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let mut kept = BTreeMap::new();
    for (name, value) in entries {
        if name.chars().all(char::is_whitespace) {
            report_unkeyed_property(losses, record, None);
            continue;
        }
        if kept.contains_key(name) {
            report_unkeyed_property(losses, record, Some(name));
            continue;
        }
        if let Some(name) = cadmpeg_core::text::NonBlankString::new(name.to_owned()) {
            kept.insert(name, value.to_owned());
        }
    }
    kept
}

fn report_unkeyed_property(losses: &mut Vec<LossNote>, record: &str, duplicate: Option<&str>) {
    let message = match duplicate {
        Some(key) => format!(
            "{record} states the property {key} a second time; the property is not transferred"
        ),
        None => {
            format!("{record} states a property with a blank key; the property is not transferred")
        }
    };
    losses.push(crate::loss::SldprtLossCode::SourcePropertyKeyBlank.note(message));
}

/// Whether `stream` is the history summary the writer generates
/// (`Contents/Keywords` or `Contents/Keywords-<n>`), a stream no
/// `SolidWorks` part holds.
fn is_history_summary(stream: &str) -> bool {
    stream
        .strip_prefix("Contents/Keywords")
        .is_some_and(|rest| {
            rest.is_empty()
                || rest.strip_prefix('-').is_some_and(|index| {
                    !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit())
                })
        })
}

/// Whether `stream` is the native Keywords index of a block-envelope or
/// compound-file part.
fn is_native_keywords(stream: &str) -> bool {
    matches!(
        stream,
        "swXmlContents/KeyWords" | "swXmlContents/KeyWords__ZLB"
    )
}

/// Whether a member name is a name of a Keywords history member:
/// `KeyWords`, a compound-file `KeyWords__ZLB` stream, or a written
/// `Keywords-<n>` member.
fn names_history(name: Option<&str>) -> bool {
    let Some(leaf) = name.and_then(|name| name.rsplit('/').next()) else {
        return false;
    };
    let leaf = leaf.to_ascii_lowercase();
    leaf == "keywords"
        || leaf == "keywords__zlb"
        || leaf.strip_prefix("keywords-").is_some_and(|index| {
            !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// The part a Keywords history member is when no record is read from it.
const HISTORY_MEMBER_PART: &str = "history-member";

/// Charges a Keywords history member that no record is read from, and gives
/// up its section as a part. The source record holds the section's bytes, so
/// the part keeps only their digest.
fn report_unread(
    ctx: &DecodeContext<'_>,
    section: Section<'_>,
    losses: &mut Vec<LossNote>,
    given_up: &mut Vec<GivenUpPart>,
    cause: &str,
) -> Result<(), CodecError> {
    let stream = section.source_stream();
    let note = crate::loss::SldprtLossCode::HistoryUnread.note(format!(
        "Keywords member {}: {cause}; no configuration or feature of the history is read",
        stream.as_str()
    ));
    let part = Part::at(
        "sldprt",
        HISTORY_MEMBER_PART,
        0,
        section.stored_payload(),
        [ArenaClass::Design],
    )
    .named(stream.as_str())
    .in_stream(stream.clone())
    .digest_only();
    let error = CodecError::malformed(format_args!("{}: {}", note.code, note.message));
    given_up.push(ctx.give_up_part(part, error)?);
    losses.push(note);
    Ok(())
}

/// Element levels of one `Application` element the native history keeps, the
/// element itself being level 1.
///
/// Each level is three containers of the stored record (the element, its
/// content array and the content item), and a CADIR document is read with the
/// 128 containers a `serde_json` text parse admits from the document root. A
/// stored element of 40 levels is the deepest that reads back, so 16 levels
/// keep the record under half of that; corpus elements nest 2 levels.
const MAX_APPLICATION_LEVELS: usize = 16;

/// The elements of one `Application` subtree below [`MAX_APPLICATION_LEVELS`].
#[derive(Default)]
struct ApplicationCut {
    /// Elements not kept, each counted once.
    elements: usize,
    /// Level of the deepest element not kept.
    deepest: usize,
    /// The first element not kept and the element it sits in.
    first: Option<String>,
}

impl ApplicationCut {
    /// Counts `child`, an element child of `parent` at `level`, with its
    /// subtree.
    fn take(
        &mut self,
        parent: roxmltree::Node<'_, '_>,
        child: roxmltree::Node<'_, '_>,
        level: usize,
    ) {
        self.first.get_or_insert_with(|| {
            let tag = child.tag_name().name();
            let named = match child.attribute("Name") {
                Some(name) => format!("{tag} {name:?}"),
                None => tag.to_owned(),
            };
            format!(
                "element {named} inside the level-{level} element {}",
                parent.tag_name().name()
            )
        });
        let mut pending = vec![(child, level + 1)];
        while let Some((node, at)) = pending.pop() {
            self.elements += 1;
            self.deepest = self.deepest.max(at);
            pending.extend(
                node.children()
                    .filter(roxmltree::Node::is_element)
                    .map(|grandchild| (grandchild, at + 1)),
            );
        }
    }
}

/// Charges an `Application` element of a Keywords root, which the native
/// history keeps and no neutral record holds, from its stored record.
fn report_application(losses: &mut Vec<LossNote>, member: &str, application: &HistoryElement) {
    let name = application_name(application);
    let children = application
        .content
        .iter()
        .filter_map(|item| match item {
            HistoryElementContent::Element(child) => Some(match child.attributes.get("Name") {
                Some(name) => format!("{} {name:?}", child.tag),
                None => child.tag.clone(),
            }),
            HistoryElementContent::Text(_) => None,
        })
        .collect::<Vec<_>>();
    let held = if children.is_empty() {
        "no child element".to_owned()
    } else {
        format!("the child elements {}", children.join(", "))
    };
    losses.push(crate::loss::SldprtLossCode::HistoryApplicationNotTransferred.note(format!(
        "Keywords member {member}: the Application element {name} with {held} is kept in the native history; no neutral record holds it, so a write to another format omits it"
    )));
}

/// Charges the elements of an `Application` subtree that the native history
/// does not keep.
fn report_application_cut(
    losses: &mut Vec<LossNote>,
    member: &str,
    application: &HistoryElement,
    cut: &ApplicationCut,
) {
    let Some(first) = &cut.first else {
        return;
    };
    let count = match cut.elements {
        1 => format!("1 element below level {MAX_APPLICATION_LEVELS} is"),
        elements => format!("{elements} elements below level {MAX_APPLICATION_LEVELS} are"),
    };
    losses.push(crate::loss::SldprtLossCode::HistoryApplicationNestingCut.note(format!(
        "Keywords member {member}: the Application element {} nests {} element levels; the native history keeps {MAX_APPLICATION_LEVELS}, so {count} not kept (the first: {first}); a SolidWorks write that regenerates the history omits them",
        application_name(application),
        cut.deepest,
    )));
}

/// Charges the forms of a Keywords text that the native history does not keep
/// and that change a value: comments, processing instructions, the edge
/// whitespace of a text value, and namespaces. Whitespace between elements,
/// attribute order, quoting, CDATA and character references are XML-equal
/// forms of what the history keeps. An empty comment between two text values
/// is the separator of two text items, which a regenerated text writes again,
/// so it is not charged.
fn report_normalised(losses: &mut Vec<LossNote>, member: &str, document: &roxmltree::Document<'_>) {
    let (mut comments, mut instructions, mut trimmed, mut namespaced) =
        (0usize, 0usize, 0usize, 0usize);
    for node in document.descendants() {
        match node.node_type() {
            roxmltree::NodeType::Comment => {
                comments += usize::from(!is_text_separator(node));
            }
            roxmltree::NodeType::PI => instructions += 1,
            roxmltree::NodeType::Text => {
                let text = node.text().unwrap_or_default();
                let kept = text.trim();
                let value = !kept.is_empty()
                    || (node.prev_sibling().is_none() && node.next_sibling().is_none());
                trimmed += usize::from(value && kept.len() != text.len());
            }
            roxmltree::NodeType::Element => {
                namespaced += usize::from(node.tag_name().namespace().is_some())
                    + node
                        .attributes()
                        .filter(|attribute| attribute.namespace().is_some())
                        .count();
            }
            roxmltree::NodeType::Root => {}
        }
    }
    let forms = [
        (comments, "", "comment(s)"),
        (instructions, "", "processing instruction(s)"),
        (trimmed, "the edge whitespace of ", "text value(s)"),
        (namespaced, "the namespace of ", "name(s)"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count != 0)
    .map(|(count, part, form)| format!("{part}{count} {form}"))
    .collect::<Vec<_>>();
    if forms.is_empty() {
        return;
    }
    losses.push(crate::loss::SldprtLossCode::HistoryXmlNormalised.note(format!(
        "Keywords member {member}: the native history drops {}; a SolidWorks write that regenerates the history writes the text without them",
        forms.join(", ")
    )));
}

/// Whether `comment` is empty and sits between two text nodes that the
/// history keeps as text items.
fn is_text_separator(comment: roxmltree::Node<'_, '_>) -> bool {
    let kept = |node: Option<roxmltree::Node<'_, '_>>| {
        node.is_some_and(|node| {
            node.is_text() && !node.text().unwrap_or_default().trim().is_empty()
        })
    };
    comment.text().unwrap_or_default().is_empty()
        && kept(comment.prev_sibling())
        && kept(comment.next_sibling())
}

/// The `Name` of a stored `Application` element, as a note states it.
fn application_name(application: &HistoryElement) -> String {
    application
        .attributes
        .get("Name")
        .map_or_else(|| "without a Name".to_owned(), |name| format!("{name:?}"))
}

/// The native record of an XML element at `level` and its subtree to
/// [`MAX_APPLICATION_LEVELS`]; `cut` counts the elements below it.
fn history_element(
    losses: &mut Vec<LossNote>,
    record: &str,
    node: roxmltree::Node<'_, '_>,
    level: usize,
    cut: &mut ApplicationCut,
) -> HistoryElement {
    let attributes = keyed_attributes(
        losses,
        record,
        node.attributes()
            .map(|attribute| (attribute.name(), attribute.value())),
    );
    let mut content = Vec::new();
    for child in node.children() {
        if child.is_text() {
            let value = child.text().unwrap_or_default().trim();
            if !value.is_empty() {
                content.push(HistoryElementContent::Text(value.to_owned()));
            }
        } else if child.is_element() {
            if level < MAX_APPLICATION_LEVELS {
                content.push(HistoryElementContent::Element(history_element(
                    losses,
                    record,
                    child,
                    level + 1,
                    cut,
                )));
            } else {
                cut.take(node, child, level);
            }
        }
    }
    HistoryElement {
        tag: node.tag_name().name().to_owned(),
        attributes,
        content,
    }
}

pub(crate) fn histories(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<Vec<FeatureHistory>, CodecError> {
    // A generated part holds its history in the writer's summary; its native
    // Keywords index lists the skeleton tree the summary already accounts for.
    let summary = scan
        .sections()
        .any(|section| is_history_summary(section.source_stream().as_str()));
    scan.sections()
        .try_fold(Vec::new(), |mut histories, section| {
            let source = section.ordinal();
            if summary && is_native_keywords(section.source_stream().as_str()) {
                return Ok(histories);
            }
            let stream = section.source_stream();
            let named = names_history(section.name());
            let Some(payload) = section.xml_payload() else {
                if named {
                    report_unread(
                        ctx,
                        section,
                        losses,
                        given_up,
                        "no XML payload is decoded from it",
                    )?;
                }
                return Ok(histories);
            };
            let Some(text) = crate::container::xml_text(payload) else {
                if named {
                    report_unread(
                        ctx,
                        section,
                        losses,
                        given_up,
                        "its bytes are not UTF-8 or UTF-16LE text",
                    )?;
                }
                return Ok(histories);
            };
            let doc = match ctx.run_part(|| ctx.parse_xml(text.as_str(), "decode XML tree"))? {
                Ok(tree) => tree,
                Err(error) => {
                    if named {
                        let cause = match error {
                            CodecError::Malformed(message) => {
                                format!("its XML does not parse: {message}")
                            }
                            refused => format!("its XML is refused: {refused}"),
                        };
                        report_unread(ctx, section, losses, given_up, &cause)?;
                    }
                    return Ok(histories);
                }
            };
            let root = doc.root_element();
            if !root.tag_name().name().contains("Keywords") {
                if named {
                    let cause = format!(
                        "its root element {:?} is not a Keywords root",
                        root.tag_name().name()
                    );
                    report_unread(ctx, section, losses, given_up, &cause)?;
                }
                return Ok(histories);
            }
            report_normalised(losses, stream.as_str(), &doc);
            let parent = format!("sldprt:history:feature-history#{source}");
            let applications = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Application")
                .map(|node| {
                    let mut cut = ApplicationCut::default();
                    let element = history_element(losses, parent.as_str(), node, 1, &mut cut);
                    report_application(losses, stream.as_str(), &element);
                    report_application_cut(losses, stream.as_str(), &element, &cut);
                    (node, element)
                })
                .collect::<Vec<_>>();
            let configurations = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .try_fold(Vec::new(), |mut configurations, (ordinal, node)| {
                    let id = format!("sldprt:history:configuration#{source}:{ordinal}");
                    crate::annotations::note(
                        annotations,
                        id.as_str(),
                        stream,
                        cadmpeg_core::decode::u64_from_index(node.range().start),
                        "Configuration",
                        Exactness::ByteExact,
                    )?;
                    let properties = keyed_attributes(
                        losses,
                        &id,
                        node.attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "Name" | "Material" | "SourceIndex")
                            })
                            .map(|attribute| (attribute.name(), attribute.value())),
                    );
                    configurations.push(Configuration {
                        id,
                        parent: parent.clone(),
                        ordinal: u32::try_from(ordinal).map_err(|_| {
                            CodecError::malformed(
                                "SLDPRT history configuration ordinal exceeds u32",
                            )
                        })?,
                        source_index: node
                            .attribute("SourceIndex")
                            .and_then(|value| value.parse().ok()),
                        name: node.attribute("Name").unwrap_or("").to_owned(),
                        material: node
                            .attribute("Material")
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned),
                        properties,
                    });
                    Ok::<_, CodecError>(configurations)
                })?;
            let feature_nodes = || {
                root.descendants().filter(|node| {
                    node.is_element()
                        && !matches!(
                            node.tag_name().name(),
                            "Keywords" | "Configuration" | "Dimension"
                        )
                        && !node.ancestors().any(|ancestor| {
                            applications
                                .iter()
                                .any(|(application, _)| *application == ancestor)
                        })
                })
            };
            let feature_ids = feature_nodes()
                .enumerate()
                .map(|(ordinal, node)| {
                    (
                        node.range().start,
                        format!("sldprt:history:feature#{source}:{ordinal}"),
                    )
                })
                .collect::<HashMap<_, _>>();
            let features = feature_nodes().enumerate().try_fold(
                Vec::new(),
                |mut features, (ordinal, node)| {
                    let source_id = feature_ids.get(&node.range().start).ok_or_else(|| {
                        CodecError::malformed("missing SLDPRT history feature identity")
                    })?;
                    let id = source_id.clone();
                    crate::annotations::note(
                        annotations,
                        id.as_str(),
                        stream,
                        cadmpeg_core::decode::u64_from_index(node.range().start),
                        node.tag_name().name(),
                        Exactness::ByteExact,
                    )?;
                    let properties = keyed_attributes(
                        losses,
                        &id,
                        node.attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "id" | "Name" | "Type" | "Suppressed")
                            })
                            .map(|attribute| (attribute.name(), attribute.value())),
                    );
                    let content = node
                        .children()
                        .filter_map(|child| {
                            if child.is_text() {
                                let value = child.text().unwrap_or_default().trim();
                                (!value.is_empty()).then(|| FeatureContent::Text(value.to_owned()))
                            } else if !child.is_element() {
                                None
                            } else if child.tag_name().name() == "Dimension" {
                                child
                                    .attribute("Name")
                                    .map(|name| FeatureContent::Dimension(name.to_owned()))
                            } else {
                                feature_ids
                                    .get(&child.range().start)
                                    .map(|id| FeatureContent::Feature(id.clone()))
                            }
                        })
                        .collect::<Vec<_>>();
                    let mut dimension_properties = BTreeMap::new();
                    for dimension in node.children().filter(|child| {
                        child.is_element() && child.tag_name().name() == "Dimension"
                    }) {
                        let Some(name) = dimension.attribute("Name") else {
                            continue;
                        };
                        let properties = keyed_attributes(
                            losses,
                            name,
                            dimension
                                .attributes()
                                .filter(|attribute| attribute.name() != "Name")
                                .map(|attribute| (attribute.name(), attribute.value())),
                        );
                        if properties.is_empty() {
                            continue;
                        }
                        if let Some(previous) = dimension_properties.get_mut(name) {
                            *previous = properties;
                        } else {
                            dimension_properties.insert(name.to_owned(), properties);
                        }
                    }
                    let mut parameters = BTreeMap::new();
                    for dimension in node.children().filter(|child| {
                        child.is_element() && child.tag_name().name() == "Dimension"
                    }) {
                        let Some(name) = dimension.attribute("Name") else {
                            continue;
                        };
                        if name.chars().all(char::is_whitespace) {
                            continue;
                        }
                        let value = dimension.text().unwrap_or_default().trim();
                        if let Some(previous) = parameters.get_mut(name) {
                            value.clone_into(previous);
                        } else if let Some(name) =
                            cadmpeg_core::text::NonBlankString::new(name.to_owned())
                        {
                            parameters.insert(name, value.to_owned());
                        }
                    }
                    let text = if node.children().any(|child| child.is_element()) {
                        None
                    } else {
                        node.text()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned)
                    };
                    features.push(Feature {
                        id,
                        parent: parent.clone(),
                        xml_tag: node.tag_name().name().to_owned(),
                        tree_parent: node.ancestors().skip(1).find_map(|ancestor| {
                            let record_id = feature_ids.get(&ancestor.range().start)?;
                            Some(crate::records::TreeParent::Record {
                                record_id: record_id.clone(),
                                source_id: ancestor
                                    .attribute("id")
                                    .and_then(|value| FeatureSource::try_from(value).ok()),
                            })
                        }),
                        source_id: node
                            .attribute("id")
                            .and_then(|value| FeatureSource::try_from(value).ok()),
                        ordinal: u32::try_from(ordinal).map_err(|_| {
                            CodecError::malformed("SLDPRT history feature ordinal exceeds u32")
                        })?,
                        name: node.attribute("Name").unwrap_or("").to_owned(),
                        kind: node
                            .attribute("Type")
                            .unwrap_or_else(|| node.tag_name().name())
                            .to_owned(),
                        input_class: None,
                        suppressed: node
                            .attribute("Suppressed")
                            .is_some_and(|value| matches!(value, "1" | "true" | "True")),
                        parameters,
                        dimension_properties,
                        properties,
                        text,
                        content,
                    });
                    Ok::<_, CodecError>(features)
                },
            )?;
            let mut configuration_ordinal = 0usize;
            let content = root
                .children()
                .filter_map(|child| {
                    if child.is_text() {
                        let value = child.text().unwrap_or_default().trim();
                        (!value.is_empty()).then(|| HistoryContent::Text(value.to_owned()))
                    } else if !child.is_element() {
                        None
                    } else if child.tag_name().name() == "Configuration" {
                        let id = format!(
                            "sldprt:history:configuration#{source}:{configuration_ordinal}"
                        );
                        configuration_ordinal += 1;
                        Some(HistoryContent::Configuration(id))
                    } else if let Some((_, element)) = applications
                        .iter()
                        .find(|(application, _)| *application == child)
                    {
                        Some(HistoryContent::Element(element.clone()))
                    } else {
                        feature_ids
                            .get(&child.range().start)
                            .map(|id| HistoryContent::Feature(id.clone()))
                    }
                })
                .collect::<Vec<_>>();
            let id = parent;
            crate::annotations::note(
                annotations,
                id.as_str(),
                stream,
                0,
                "Keywords",
                Exactness::ByteExact,
            )?;
            let mut properties = keyed_attributes(
                losses,
                &id,
                root.attributes()
                    .filter(|attribute| attribute.name() != "Name")
                    .map(|attribute| (attribute.name(), attribute.value())),
            );
            let mut spans: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
            for node in feature_nodes() {
                let Some(raw) = node.attribute("id") else {
                    continue;
                };
                if crate::records::FeatureSource::try_from(raw).is_err() {
                    continue;
                }
                let range = node.range();
                let Some(len) = range.end.checked_sub(range.start) else {
                    return Err(CodecError::malformed(
                        "SLDPRT history feature range ends before it starts",
                    ));
                };
                spans
                    .entry(raw.to_owned())
                    .or_default()
                    .push((range.start, len));
            }
            let mut duplicates: Vec<(String, Vec<(usize, usize)>)> = spans
                .into_iter()
                .filter(|(_, occurrences)| occurrences.len() > 1)
                .collect();
            duplicates.sort();
            duplicates.truncate(8);
            if !duplicates.is_empty() {
                let ids = duplicates
                    .iter()
                    .map(|(source, _)| source.clone())
                    .collect::<Vec<_>>()
                    .join(",");
                let span_text = duplicates
                    .iter()
                    .map(|(source, occurrences)| {
                        let list = occurrences
                            .iter()
                            .map(|(offset, len)| format!("{offset}+{len}"))
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("source {source} at {list}")
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                for (key, value) in [
                    ("duplicate_source_ids", ids),
                    ("duplicate_source_spans", span_text),
                ] {
                    if let Some(key) = cadmpeg_core::text::NonBlankString::new(key.to_owned()) {
                        properties.insert(key, value);
                    }
                }
            }
            histories.push(FeatureHistory {
                id,
                part_name: root
                    .attribute("Name")
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                properties,
                content,
                configurations,
                features,
            });
            Ok(histories)
        })
}

pub(crate) fn enrich_scene_classes(
    histories: &mut [FeatureHistory],
    scene_classes: &HashMap<u32, String>,
) {
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let Some(source) = feature.source_value() else {
            continue;
        };
        if feature.input_class.is_none() && classless_builtin_node(feature) {
            feature.input_class = scene_classes.get(&source).cloned();
        }
    }
}

#[cfg(test)]
pub(in crate::history) mod tests;
