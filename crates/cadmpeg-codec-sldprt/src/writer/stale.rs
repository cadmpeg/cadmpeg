// SPDX-License-Identifier: Apache-2.0
//! The stale mark of the states the writer writes without regenerating them
//! (§3.4, States): `SolidWorks` stores it on a configuration row of the
//! `swSolidWorks` document as `swConfigurationNeedsUpdate`.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::tree::XmlDocument;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use cadmpeg_ir::codec::write::evaluation_stale;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::history::write::brep_agreement::ShapingEdit;

/// The states of one write that take the stale mark.
#[derive(Default)]
pub(super) struct StaleStates {
    /// The configurations, by name, whose row takes the mark.
    pub(super) configurations: BTreeSet<String>,
    /// The records whose written edit the stored state of a configuration
    /// does not show, by the name of that configuration; `None` for those of
    /// no named configuration.
    edited: BTreeMap<Option<String>, BTreeSet<String>>,
    /// The evaluations the document states `Stale` or `Failed`, by the name
    /// of their configuration; `None` for those of no named configuration.
    evaluations: BTreeMap<Option<String>, Vec<String>>,
}

impl StaleStates {
    /// The states of `ir` that take the stale mark: the configuration of
    /// each edit in `shaped`, which the write carries without regenerating
    /// the solid it shapes, and the configuration of each evaluation the
    /// document states `Stale` or `Failed`.
    pub(super) fn of(ir: &CadIr, shaped: &[ShapingEdit]) -> Self {
        let mut states = Self::default();
        for (configuration, record) in shaped {
            let name = match configuration {
                Some(id) => ir
                    .model
                    .configurations
                    .iter()
                    .find(|candidate| &candidate.id == id)
                    .and_then(|configuration| configuration.name.clone()),
                None => Some(super::active_configuration_name(ir)),
            };
            states.configurations.extend(name.clone());
            states
                .edited
                .entry(name)
                .or_default()
                .insert(record.clone());
        }
        let names = ir
            .design
            .configurations
            .iter()
            .map(|configuration| (&configuration.id, configuration.name.as_str()))
            .collect::<BTreeMap<_, _>>();
        for evaluation in &ir.design.evaluations {
            if evaluation.status == EvaluationStatus::Current {
                continue;
            }
            let name = evaluation
                .configuration
                .as_ref()
                .and_then(|id| names.get(id))
                .map(|name| (*name).to_owned());
            states.configurations.extend(name.clone());
            states
                .evaluations
                .entry(name)
                .or_default()
                .push(evaluation.id.as_str().to_owned());
        }
        states
    }

    /// The `export:evaluation.stale` losses of the write, given the
    /// configurations whose row the output marks: one naming the edits of
    /// each configuration, and one naming each `Stale` or `Failed`
    /// evaluation whose configuration has no row the output marks.
    pub(super) fn losses(&self, marked: &BTreeSet<String>) -> Vec<LossNote> {
        let mut losses = Vec::new();
        for (name, edits) in &self.edited {
            let reason = match name {
                Some(name) if marked.contains(name) => format!(
                    "writes these edits without regenerating the state they shape, and marks \
                     configuration {name} swConfigurationNeedsUpdate"
                ),
                Some(name) => format!(
                    "writes these edits without regenerating the state they shape, and holds no \
                     row of configuration {name} to mark"
                ),
                None => "writes these edits without regenerating the state they shape, and holds \
                         no named configuration row to mark"
                    .to_owned(),
            };
            losses.extend(evaluation_stale("SLDPRT", &reason, edits.iter().cloned()));
        }
        let unmarked = self
            .evaluations
            .iter()
            .filter(|(name, _)| name.as_ref().is_none_or(|name| !marked.contains(name)))
            .flat_map(|(_, evaluations)| evaluations.iter().cloned())
            .collect::<Vec<_>>();
        losses.extend(evaluation_stale(
            "SLDPRT",
            "writes these Stale or Failed states with no configuration row that takes their \
             stale mark",
            unmarked,
        ));
        losses
    }
}

/// A payload with the stale mark, and the names of the rows that carry it.
type Marked = (Vec<u8>, Vec<String>);

/// `payload` with `swConfigurationNeedsUpdate="YES"` on each `swConfiguration`
/// row whose `swName` is in `names`, and the names of the rows that carry the
/// mark, parsed in the session `ctx`. `None` where `payload` is no
/// `swSolidWorks` document or holds no such row.
pub(super) fn mark(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    names: &BTreeSet<String>,
) -> Result<Option<Marked>, CodecError> {
    let Some(text) = crate::container::xml_text(payload) else {
        return Ok(None);
    };
    match ctx.parse_xml(&text, "SLDPRT stale configuration mark") {
        Ok(document) => Ok(marked(&document, &text, payload, names)),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(None),
    }
}

/// `text`, parsed as `document`, with the stale mark on each row of `names`
/// ([`mark`]).
fn marked(
    document: &XmlDocument<'_>,
    text: &str,
    payload: &[u8],
    names: &BTreeSet<String>,
) -> Option<Marked> {
    const MARK: &str = "swConfigurationNeedsUpdate=\"YES\"";
    if document.root_element().tag_name().name() != "swSolidWorks" {
        return None;
    }
    let mut edits = Vec::new();
    let mut marked = Vec::new();
    for row in document
        .descendants()
        .filter(|node| node.has_tag_name("swConfiguration"))
    {
        let Some(name) = row.attribute("swName").filter(|name| names.contains(*name)) else {
            continue;
        };
        let mut attributes = row.attributes();
        match attributes.find(|attribute| attribute.name() == "swConfigurationNeedsUpdate") {
            Some(mark) if mark.value().eq_ignore_ascii_case("yes") => {}
            Some(mark) => edits.push((document.source_range(mark.range()), MARK.to_owned())),
            None => {
                let stored = row
                    .attributes()
                    .find(|attribute| attribute.name() == "swName")?;
                let end = document.source_range(stored.range()).end;
                edits.push((end..end, format!(" {MARK}")));
            }
        }
        marked.push(name.to_owned());
    }
    if marked.is_empty() {
        return None;
    }
    if edits.is_empty() {
        return Some((payload.to_vec(), marked));
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut output = text.to_owned();
    for (range, replacement) in edits {
        output.replace_range(range, &replacement);
    }
    Some((crate::container::xml_bytes(payload, &output), marked))
}

/// Sets the stale mark of each configuration in `names` on the
/// `swSolidWorks` documents of `sections`, and returns the names of the rows
/// that carry it.
pub(super) fn mark_sections(
    ctx: &DecodeContext<'_>,
    sections: &mut [(String, Vec<u8>)],
    names: &BTreeSet<String>,
) -> Result<Vec<String>, CodecError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let mut marked = Vec::new();
    for (_, payload) in sections {
        if let Some((patched, rows)) = mark(ctx, payload, names)? {
            *payload = patched;
            marked.extend(rows);
        }
    }
    Ok(marked)
}

#[cfg(test)]
mod tests;
