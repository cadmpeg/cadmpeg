// SPDX-License-Identifier: Apache-2.0
//! Source-link fixture mutation for refusal controls.

pub(super) fn append_link_to_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &str,
    links: &mut Vec<String>,
    link: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    if ctx.equal(link, id, "Rhino source link equality")? {
        return Ok(false);
    }
    let Err(first) = ctx.binary_search_by(
        links,
        |existing| ctx.compare(existing.as_str(), link, "Rhino source link comparison"),
        "Rhino source link search",
    )?
    else {
        return Ok(true);
    };
    let copy = ctx.copy_retained_text(link, "Rhino unknown record link copy")?;
    ctx.insert_vec(links, first, copy, "Rhino unknown record links")?;
    Ok(true)
}
