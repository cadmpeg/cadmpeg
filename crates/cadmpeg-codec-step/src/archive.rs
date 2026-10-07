// SPDX-License-Identifier: Apache-2.0
//! STEP Part 21 ZIP-container rules.

use cadmpeg_core::container::ContainerRole;

use std::fmt::Write;
use std::path::Path;

use cadmpeg_container::{ArchiveSnapshot, ZipCompression};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

/// The required root member name from Part 21 Annex A.4.
pub(crate) const ROOT_NAME: &str = "ISO-10303.p21";

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReferenceTarget<'a> {
    Internal {
        member: String,
        query: Option<&'a str>,
        fragment: Option<&'a str>,
    },
    External,
}

/// Returns whether the input begins with a ZIP local-file header.
pub(crate) fn has_zip_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// Returns whether a detection prefix names the required STEP root member.
pub(crate) fn has_root_marker(
    ctx: &DecodeContext<'_>,
    prefix: View<'_>,
) -> Result<bool, CodecError> {
    // The root name is evidence only in the structured central directory.
    const MAX_PROBE_BYTES: usize = 1024 * 1024;
    ctx.charge_work(
        u64_from_index(prefix.window().len().min(4)),
        "STEP ZIP detection magic",
    )?;
    if !has_zip_magic(prefix.window()) || prefix.window().len() > MAX_PROBE_BYTES {
        return Ok(false);
    }
    match ArchiveSnapshot::contains_name(ctx, prefix, ROOT_NAME) {
        Ok(found) => Ok(found),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(false),
    }
}

/// One STEP ZIP container whose required root member is proven present.
pub(crate) struct OpenedRoot<'a> {
    pub(crate) archive: ArchiveSnapshot<'a>,
    pub(crate) view: View<'a>,
    pub(crate) data_start: u64,
}

/// Opens and validates the required root member of one STEP ZIP container.
pub(crate) fn open_root<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<OpenedRoot<'a>, CodecError> {
    let archive = ArchiveSnapshot::new(ctx, root)?;
    for entry in ctx.admit_iter(archive.entries(), "STEP open root borrowed traversal")? {
        validate_entry_name(ctx, &entry.name)?;
        if entry.uses_utf8_name_encoding() {
            return Err(CodecError::Malformed(
                "STEP ZIP uses prohibited Unicode filename support".into(),
            ));
        }
        if entry.compression == ZipCompression::Zstd {
            return Err(CodecError::NotImplemented(
                "STEP ZIP requires PKZIP 2.04g stored or Deflate entries".into(),
            ));
        }
    }
    let root_entry = archive.entry(ctx, ROOT_NAME)?.ok_or_else(|| {
        CodecError::WrongFormat(format!("STEP ZIP has no required root {ROOT_NAME}"))
    })?;
    let data_start = root_entry.data_start;
    let root_view = archive.open(ctx, &root_entry.name)?;
    Ok(OpenedRoot {
        archive,
        view: root_view,
        data_start,
    })
}

/// Resolves one archive URI against the directory of its referencing member.
fn resolve_uri<'a>(
    ctx: &DecodeContext<'_>,
    member_bytes: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    base_member: &'a str,
    uri: &'a str,
) -> Result<ReferenceTarget<'a>, CodecError> {
    if has_uri_scheme(ctx, uri)? || uri.starts_with("//") {
        return Ok(ReferenceTarget::External);
    }
    let (uri, fragment) = ctx
        .split_once(uri, "#", "STEP ZIP URI fragment split")?
        .map_or((uri, None), |(uri, fragment)| (uri, Some(fragment)));
    if fragment
        .map(|fragment| ctx.contains_text(fragment, "#", "STEP ZIP fragment separator containment"))
        .transpose()?
        .unwrap_or(false)
    {
        return Err(CodecError::malformed(format_args!(
            "invalid STEP ZIP URI fragment {uri:?}"
        )));
    }
    let (path, query) = ctx
        .split_once(uri, "?", "STEP ZIP URI query split")?
        .map_or((uri, None), |(path, query)| (path, Some(query)));
    if path.starts_with('/') {
        return Err(CodecError::malformed(format_args!(
            "STEP ZIP URI escapes the archive root: {uri:?}"
        )));
    }
    let mut components = Vec::new();
    let mut component_bytes = ctx.reserve_scoped(0, "step_zip_uri_components_temp")?;
    if let Some((directory, _)) =
        ctx.rsplit_once(base_member, "/", "STEP ZIP base member reverse split")?
    {
        for component in directory.split('/') {
            ctx.push_scoped_vec(
                &mut component_bytes,
                &mut components,
                component,
                "step_zip_uri_components",
            )?;
        }
    }
    if path.is_empty() {
        let member = ctx.copy_scoped_text(base_member, member_bytes, "step_zip_uri_member")?;
        return Ok(ReferenceTarget::Internal {
            member,
            query,
            fragment,
        });
    }
    for component in path.split('/') {
        match component {
            "" => {
                return Err(CodecError::malformed(format_args!(
                    "invalid empty path component in STEP ZIP URI {uri:?}"
                )));
            }
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(CodecError::malformed(format_args!(
                        "STEP ZIP URI escapes the archive root: {uri:?}"
                    )));
                }
            }
            component => ctx.push_scoped_vec(
                &mut component_bytes,
                &mut components,
                component,
                "step_zip_uri_components",
            )?,
        }
    }
    if components.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "STEP ZIP URI resolves to no member: {uri:?}"
        )));
    }
    let member_len = ctx
        .admit_iter(&components[..], "STEP resolve uri traversal")?
        .try_fold(0_usize, |total, component| {
            total.checked_add(component.len())
        })
        .and_then(|total| total.checked_add(components.len() - 1))
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_uri_member", 0, 1))?;
    let mut member = String::new();

    ctx.reserve_scoped_string(member_bytes, &mut member, member_len, "step_zip_uri_member")?;
    for (index, component) in ctx
        .admit_iter(&components[..], "STEP resolve uri traversal")?
        .enumerate()
    {
        if index != 0 {
            member_bytes.with_storage(|| {
                ctx.push_retained_char(&mut member, '/', "STEP ZIP member separator character")
            })?;
        }
        member.push_str(component);
    }
    Ok(ReferenceTarget::Internal {
        member,
        query,
        fragment,
    })
}

/// Resolves all root-file resource bindings and checks internal members.
pub(crate) fn root_reference_notes(
    ctx: &DecodeContext<'_>,
    archive: &ArchiveSnapshot<'_>,
    exchange: &crate::parse::Exchange,
) -> Result<Vec<String>, CodecError> {
    // CE-02: Annex A.4 makes subsidiary access a root reference operation;
    // this pass records the binding and does not import a subsidiary graph.
    let mut notes = Vec::new();
    for reference in ctx.admit_iter(
        exchange.references(),
        "STEP root reference notes borrowed traversal",
    )? {
        let name = reference.name;
        let uri = forwarded_reference_uri(exchange, &reference.uri, ctx)?;
        let mut member_bytes = ctx.reserve_scoped(0, "step_zip_uri_member_temp")?;
        match resolve_uri(ctx, &mut member_bytes, ROOT_NAME, uri)? {
            ReferenceTarget::Internal {
                member,
                query,
                fragment,
            } => {
                if archive.entry(ctx, &member)?.is_none() {
                    return Err(CodecError::malformed(format_args!(
                        "STEP ZIP resource {uri:?} for {name} has no archive member {member:?}"
                    )));
                }
                push_reference_note(
                    ctx,
                    &mut notes,
                    "internal resource ",
                    name,
                    &member,
                    query,
                    fragment,
                )?;
            }
            ReferenceTarget::External => {
                push_reference_note(ctx, &mut notes, "external resource ", name, uri, None, None)?;
            }
        }
    }
    Ok(notes)
}

fn push_reference_note(
    ctx: &DecodeContext<'_>,
    notes: &mut Vec<String>,
    prefix: &str,
    name: crate::parse::ReferenceName,
    target: &str,
    query: Option<&str>,
    fragment: Option<&str>,
) -> Result<(), CodecError> {
    let (marker, mut id) = match name {
        crate::parse::ReferenceName::Entity(id) => ('#', id),
        crate::parse::ReferenceName::Value(id) => ('@', id),
    };
    let mut digits = 1_usize;
    while id >= 10 {
        id /= 10;
        digits += 1;
    }
    let query_len = query
        .map_or(Some(0), |text| text.len().checked_add(1))
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    let fragment_len = fragment
        .map_or(Some(0), |text| text.len().checked_add(1))
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    let len = prefix
        .len()
        .checked_add(1 + digits + " -> ".len())
        .and_then(|len| len.checked_add(target.len()))
        .and_then(|len| len.checked_add(query_len))
        .and_then(|len| len.checked_add(fragment_len))
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    ctx.reserve_vec(notes, 1, "step_zip_reference_notes")?;

    let mut note = ctx.retained_string(len, "step_zip_reference_note")?;

    note.push_str(prefix);
    ctx.push_retained_char(&mut note, marker, "STEP ZIP reference marker character")?;
    let id = match name {
        crate::parse::ReferenceName::Entity(id) | crate::parse::ReferenceName::Value(id) => id,
    };
    write!(&mut note, "{id}")
        .map_err(|_| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    note.push_str(" -> ");
    note.push_str(target);
    if let Some(query) = query {
        ctx.push_retained_char(&mut note, '?', "STEP ZIP reference query character")?;
        note.push_str(query);
    }
    if let Some(fragment) = fragment {
        ctx.push_retained_char(&mut note, '#', "STEP ZIP reference fragment character")?;
        note.push_str(fragment);
    }
    notes.push(note);
    Ok(())
}

fn forwarded_reference_uri<'a>(
    exchange: &'a crate::parse::Exchange,
    uri: &'a str,
    ctx: &DecodeContext<'_>,
) -> Result<&'a str, CodecError> {
    let Some(fragment) = uri.strip_prefix('#') else {
        return Ok(uri);
    };
    for anchor in exchange.anchors() {
        ctx.charge_work(1, "step_zip_anchor_lookup")?;
        if anchor.name.len() != fragment.len() {
            continue;
        }
        ctx.charge_work(u64_from_index(fragment.len()), "step_zip_anchor_lookup")?;
        if anchor.name == fragment {
            return Ok(match &anchor.value {
                crate::parse::Value::Resource(target) => target.as_str(),
                _ => uri,
            });
        }
    }
    Ok(uri)
}

fn has_uri_scheme(ctx: &DecodeContext<'_>, uri: &str) -> Result<bool, CodecError> {
    let Some(colon) = uri.find(':') else {
        return Ok(false);
    };
    let scheme = &uri[..colon];
    Ok(!scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && ctx
            .admit_iter(scheme.as_bytes(), "STEP URI scheme traversal")?
            .copied()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')))
}

/// Classifies a physical ZIP member for the STEP container report.
pub(crate) fn classify_entry(
    ctx: &DecodeContext<'_>,
    name: &str,
) -> Result<ContainerRole, CodecError> {
    Ok(match name {
        ROOT_NAME => ContainerRole::RootExchange,
        _ if name.ends_with('/') => ContainerRole::Directory,
        _ if extension_is(ctx, name, "p21")?
            || extension_is(ctx, name, "step")?
            || extension_is(ctx, name, "stp")? =>
        {
            ContainerRole::SubsidiaryExchange
        }
        _ if extension_is(ctx, name, "zip")? => ContainerRole::NestedArchive,
        _ => ContainerRole::Ancillary,
    })
}

fn extension_is(ctx: &DecodeContext<'_>, name: &str, expected: &str) -> Result<bool, CodecError> {
    Ok(Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            ctx.eq_ignore_ascii_case(
                extension,
                expected,
                "STEP ZIP entry extension case equality",
            )
        })
        .transpose()?
        .unwrap_or(false))
}

fn validate_entry_name(ctx: &DecodeContext<'_>, name: &str) -> Result<(), CodecError> {
    if name.is_empty()
        || name.starts_with('/')
        || ctx.contains_text(name, "\\", "STEP ZIP entry backslash containment")?
        || ctx.contains_text(name, "\0", "STEP ZIP entry null containment")?
        || name.split('/').enumerate().any(|(index, component)| {
            component.is_empty() && !(index == name.split('/').count() - 1 && name.ends_with('/'))
                || component == "."
                || component == ".."
        })
    {
        return Err(CodecError::malformed(format_args!(
            "unsafe STEP ZIP entry path {name:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
