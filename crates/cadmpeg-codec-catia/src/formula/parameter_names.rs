// SPDX-License-Identifier: Apache-2.0
//! Unique names for document-scoped typed parameters.

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::DesignParameter;
use std::collections::HashSet;

/// Preserve source names and reserve them before suffixing collisions.
pub(super) fn normalize(parameters: &mut [DesignParameter]) -> Result<(), CodecError> {
    let mut reserved = HashSet::new();
    for parameter in parameters
        .iter()
        .filter(|parameter| !parameter.name.is_empty())
    {
        let name = parameter.name.clone();
        reserved.insert(name);
    }
    let mut used = HashSet::new();
    for parameter in parameters {
        let source_name = parameter.name.clone();
        if !source_name.is_empty() && used.insert(source_name.clone()) {
            continue;
        }

        let base = if source_name.is_empty() {
            "Parameter"
        } else {
            source_name.as_str()
        };
        let mut suffix = 1u32;
        let neutral_name = loop {
            let candidate = format!("{base}#{suffix}");
            suffix = suffix.checked_add(1).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{} exceeds its local bound",
                    "catia_parameter_name_collision"
                ))
            })?;
            if !reserved.contains(&candidate) && used.insert(candidate.clone()) {
                break candidate;
            }
        };
        parameter.name = neutral_name;
        parameter
            .properties
            .insert(cadmpeg_core::nonblank_literal!("source_name"), source_name);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
