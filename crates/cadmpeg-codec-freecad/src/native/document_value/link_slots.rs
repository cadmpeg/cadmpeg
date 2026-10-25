// SPDX-License-Identifier: Apache-2.0
//! Reference carriers associate with typed slots independently of literal names.

use crate::native::ValueSlots;
use crate::persistence::{link_grammar, LinkGrammar};
use cadmpeg_core::CodecError;

/// Returns `(value order, typed link slot)` for unambiguously associated carriers.
pub(crate) fn link_carrier_slots(
    kind: &str,
    values: &ValueSlots,
    links_len: usize,
) -> Result<Vec<(usize, usize)>, CodecError> {
    let expression =
        crate::persistence::document_properties::value_parts::links::expression_container(kind);
    let grammar = link_grammar(kind);
    let (root_tag, child_tag) = match grammar {
        Some(LinkGrammar::LinkList) => ("LinkList", Some("Link")),
        Some(LinkGrammar::LinkSubList) => ("LinkSubList", Some("Link")),
        Some(LinkGrammar::XLinkSubList) => ("XLinkSubList", Some("XLink")),
        Some(grammar) => (grammar.root_tag(), None),
        None if expression => ("XLinks", Some("XLink")),
        None => return Ok(Vec::new()),
    };
    let roots: Vec<_> = values
        .iter()
        .filter(|value| value.tag == root_tag && (expression || value.parent.is_none()))
        .collect();
    let [root] = roots.as_slice() else {
        return Ok(Vec::new());
    };
    let Some(child_tag) = child_tag else {
        return Ok(if links_len == 1 {
            vec![(root.order, 0)]
        } else {
            Vec::new()
        });
    };
    let carriers: Vec<_> = values
        .iter()
        .filter(|value| value.parent == Some(root.order) && value.tag == child_tag)
        .collect();
    // Source rows carry explicit indices. Authored trees may omit them only
    // when the complete dense tree identifies every reference position.
    let dense = !values.has_missing()
        && carriers.len() == links_len
        && root
            .attributes
            .get("count")
            .and_then(|value| value.parse::<usize>().ok())
            == Some(links_len);
    let mut seen = std::collections::BTreeSet::new();
    let mut slots = Vec::new();
    for (ordinal, value) in carriers.into_iter().enumerate() {
        let Some(slot) = value.link_index.or(dense.then_some(ordinal)) else {
            continue;
        };
        if slot >= links_len || !seen.insert(slot) {
            return Err(CodecError::malformed(
                "reference carrier does not identify a unique typed link slot",
            ));
        }
        slots.push((value.order, slot));
    }
    Ok(slots)
}
