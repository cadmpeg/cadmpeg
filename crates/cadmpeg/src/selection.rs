// SPDX-License-Identifier: Apache-2.0
//! Entity selection by ID: an exact ID, else one unique ID suffix.
//!
//! Every command that selects entities by ID resolves them with one rule and
//! reports a miss or an ambiguity with one wording.

/// Failure to select one record by ID.
pub(crate) enum ResolveError {
    Missing,
    Ambiguous(Vec<String>),
}

/// Resolves the first exact ID or one unique suffix match.
pub(crate) fn resolve_one<'a, T: Copy>(
    request: &str,
    indexed: impl Iterator<Item = (Option<&'a str>, T)> + Clone,
) -> Result<T, ResolveError> {
    for (id, value) in indexed.clone() {
        if id == Some(request) {
            return Ok(value);
        }
    }
    let mut suffix = Vec::new();
    for (id, value) in indexed {
        if let Some(id) = id {
            if id.ends_with(request) {
                suffix.push((id, value));
            }
        }
    }
    match suffix.as_slice() {
        [] => Err(ResolveError::Missing),
        [(_, value)] => Ok(*value),
        _ => Err(ResolveError::Ambiguous(
            suffix.into_iter().map(|(id, _)| id.to_owned()).collect(),
        )),
    }
}

/// The miss message: the arena, its entry count and close IDs.
pub(crate) fn miss_id_message(
    arena: &str,
    request: &str,
    entry_count: u64,
    all_ids: &[String],
) -> String {
    const SHOWN: usize = 10;
    let lower = request.to_lowercase();
    let mut label = "close ids";
    let mut names: Vec<&str> = all_ids
        .iter()
        .filter(|id| id.to_lowercase().contains(&lower))
        .take(SHOWN)
        .map(String::as_str)
        .collect();
    if names.is_empty() {
        label = "ids include";
        names = all_ids.iter().take(SHOWN).map(String::as_str).collect();
    }
    if names.is_empty() {
        format!(
            "no record in {arena} ({entry_count} entries) has string id {request:?}; \
             none of the entries carry a JSON-string `id` field"
        )
    } else {
        format!(
            "no record in {arena} ({entry_count} entries) has id {request:?}; {label}: {}; \
             pass a longer suffix or the full id",
            names.join(", ")
        )
    }
}

/// The ambiguity message: the suffix and the IDs it matches.
pub(crate) fn ambiguous_message(request: &str, arena: &str, matches: &[String]) -> String {
    const SHOWN: usize = 10;
    let shown: Vec<&str> = matches.iter().take(SHOWN).map(String::as_str).collect();
    format!(
        "ambiguous id suffix {request:?} in {arena}; matches: {}; pass a longer \
         suffix or the full id",
        shown.join(", ")
    )
}
