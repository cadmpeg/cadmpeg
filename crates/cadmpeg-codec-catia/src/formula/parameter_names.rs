// SPDX-License-Identifier: Apache-2.0
//! Unique names for document-scoped typed parameters.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::features::DesignParameter;
use std::collections::HashSet;

/// Preserve source names and reserve them before suffixing collisions.
pub(super) fn normalize(
    ctx: &DecodeContext<'_>,
    parameters: &mut [DesignParameter],
) -> Result<(), CodecError> {
    let mut reserved = HashSet::new();
    for parameter in parameters
        .iter()
        .filter(|parameter| !parameter.name.is_empty())
    {
        let name = ctx.copy_retained_text(&parameter.name, "catia_parameter_reserved_name")?;
        ctx.insert_hash_set(&mut reserved, name, "catia_parameter_reserved_names")?;
    }
    let mut used = HashSet::new();
    for parameter in parameters {
        let source_name = ctx.copy_retained_text(&parameter.name, "catia_parameter_source_name")?;
        if !source_name.is_empty()
            && ctx.insert_hash_set(
                &mut used,
                ctx.copy_retained_text(&source_name, "catia_parameter_used_name")?,
                "catia_parameter_used_names",
            )?
        {
            continue;
        }

        let base = if source_name.is_empty() {
            "Parameter"
        } else {
            source_name.as_str()
        };
        let mut suffix = 1u32;
        let neutral_name = loop {
            ctx.charge_work(1, "catia_parameter_name_collision")?;
            let candidate = ctx.format_retained(
                format_args!("{base}#{suffix}"),
                "catia_parameter_neutral_name",
            )?;
            suffix = suffix.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_parameter_name_collision", u64::MAX, u64::MAX)
            })?;
            if !reserved.contains(&candidate)
                && ctx.insert_hash_set(
                    &mut used,
                    ctx.copy_retained_text(&candidate, "catia_parameter_used_name")?,
                    "catia_parameter_used_names",
                )?
            {
                break candidate;
            }
        };
        parameter.name = neutral_name;
        ctx.insert_btree_map(
            &mut parameter.properties,
            cadmpeg_core::nonblank_literal!("source_name"),
            source_name,
            "catia_parameter_source_property",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
