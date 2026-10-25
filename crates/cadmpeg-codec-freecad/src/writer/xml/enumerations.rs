// SPDX-License-Identifier: Apache-2.0
//! Recount choice selectors on the output clone without changing source ordinals.

#[cfg(test)]
mod tests;

use crate::native::ValueRecord;

/// A lost selected choice is never replaced by a neighbouring surviving choice.
pub(super) fn normalize(kind: &str, values: &mut [ValueRecord], complete: bool) -> bool {
    if kind != "App::PropertyEnumeration" {
        return true;
    }
    let lists = values
        .iter()
        .filter(|value| value.parent.is_none() && value.tag == "CustomEnumList")
        .collect::<Vec<_>>();
    if lists.is_empty() {
        return !values
            .iter()
            .any(|value| value.tag == "Integer" && value.attributes.contains_key("CustomEnum"));
    }
    if lists.len() != 1 {
        return false;
    }
    let list = lists[0];
    let Some(count) = list
        .attributes
        .get("count")
        .and_then(|count| count.parse::<usize>().ok())
    else {
        return false;
    };
    let choices = values
        .iter()
        .filter(|value| value.parent == Some(list.order) && value.tag == "Enum")
        .collect::<Vec<_>>();
    let dense = complete
        && choices.len() == count
        && choices.iter().all(|choice| choice.item_index.is_none());
    let selectors = values
        .iter()
        .enumerate()
        .filter(|(_, value)| value.parent.is_none() && value.tag == "Integer")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if selectors.len() != 1 {
        return false;
    }
    let selector = selectors[0];
    let Some(selected) = values[selector]
        .attributes
        .get("value")
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return false;
    };
    if selected >= count {
        // An out-of-range persisted index is its numeric native family.
        return true;
    }
    let mut found = None;
    for (current, choice) in choices.iter().enumerate() {
        let original = choice.item_index.or_else(|| dense.then_some(current));
        if original == Some(selected) {
            if found.replace(current).is_some() {
                return false;
            }
        }
    }
    let Some(current) = found else {
        return false;
    };
    if current != selected {
        values[selector]
            .attributes
            .insert("value".into(), current.to_string());
    }
    true
}

pub(super) fn omitted(kind: &str, values: &crate::native::ValueSlots) -> bool {
    if kind != "App::PropertyEnumeration" {
        return false;
    }
    let mut output = values.to_vec();
    !normalize(kind, &mut output, !values.has_missing())
}
