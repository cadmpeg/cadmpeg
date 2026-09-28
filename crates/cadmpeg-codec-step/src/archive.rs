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
pub(crate) fn has_root_marker(prefix: &[u8]) -> bool {
    // CE-07: the root name is evidence only when it is an entry in a
    // structurally parsed ZIP central directory. Payloads, comments, and
    // unrelated entry names are not root evidence.
    const MAX_PROBE_BYTES: usize = 1024 * 1024;
    if !has_zip_magic(prefix) || prefix.len() > MAX_PROBE_BYTES {
        return false;
    }
    zip::ZipArchive::new(std::io::Cursor::new(prefix))
        .ok()
        .is_some_and(|archive| archive.file_names().any(|name| name == ROOT_NAME))
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
    for entry in archive.entries() {
        validate_entry_name(&entry.name)?;
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
    let root_entry = archive.entry(ROOT_NAME).ok_or_else(|| {
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
    if has_uri_scheme(uri) || uri.starts_with("//") {
        return Ok(ReferenceTarget::External);
    }
    let (uri, fragment) = uri
        .split_once('#')
        .map_or((uri, None), |(uri, fragment)| (uri, Some(fragment)));
    if fragment.is_some_and(|fragment| fragment.contains('#')) {
        return Err(CodecError::malformed(format_args!(
            "invalid STEP ZIP URI fragment {uri:?}"
        )));
    }
    let (path, query) = uri
        .split_once('?')
        .map_or((uri, None), |(path, query)| (path, Some(query)));
    if path.starts_with('/') {
        return Err(CodecError::malformed(format_args!(
            "STEP ZIP URI escapes the archive root: {uri:?}"
        )));
    }
    let mut components = Vec::new();
    let mut component_bytes = ctx.reserve_scoped(0, "step_zip_uri_components_temp")?;
    if let Some((directory, _)) = base_member.rsplit_once('/') {
        for component in directory.split('/') {
            push_component(ctx, &mut component_bytes, &mut components, component)?;
        }
    }
    if path.is_empty() {
        member_bytes.grow(u64_from_index(base_member.len()))?;
        let mut member = String::new();
        member
            .try_reserve_exact(base_member.len())
            .map_err(|_| ctx.refuse_codec_limit("step_zip_uri_member", 0, 1))?;
        member.push_str(base_member);
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
            component => push_component(ctx, &mut component_bytes, &mut components, component)?,
        }
    }
    if components.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "STEP ZIP URI resolves to no member: {uri:?}"
        )));
    }
    let member_len = components
        .iter()
        .try_fold(0_usize, |total, component| {
            total.checked_add(component.len())
        })
        .and_then(|total| total.checked_add(components.len() - 1))
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_uri_member", 0, 1))?;
    member_bytes.grow(u64_from_index(member_len))?;
    let mut member = String::new();
    member
        .try_reserve_exact(member_len)
        .map_err(|_| ctx.refuse_codec_limit("step_zip_uri_member", 0, 1))?;
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            member.push('/');
        }
        member.push_str(component);
    }
    Ok(ReferenceTarget::Internal {
        member,
        query,
        fragment,
    })
}

fn push_component<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    components: &mut Vec<&'a str>,
    component: &'a str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "step_zip_uri_components")?;
    bytes.grow(u64_from_index(std::mem::size_of::<&str>()))?;
    components
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("step_zip_uri_components", 0, 1))?;
    components.push(component);
    Ok(())
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
    for reference in exchange.references() {
        let name = reference.name;
        let uri = forwarded_reference_uri(exchange, &reference.uri);
        let mut member_bytes = ctx.reserve_scoped(0, "step_zip_uri_member_temp")?;
        match resolve_uri(ctx, &mut member_bytes, ROOT_NAME, uri)? {
            ReferenceTarget::Internal {
                member,
                query,
                fragment,
            } => {
                if archive.entry(&member).is_none() {
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
    ctx.charge_collection_items(1, "step_zip_reference_notes")?;
    ctx.charge_retained(u64_from_index(len), "step_zip_reference_note")?;
    notes
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("step_zip_reference_notes", 0, 1))?;
    let mut note = String::new();
    note.try_reserve_exact(len)
        .map_err(|_| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    note.push_str(prefix);
    note.push(marker);
    let id = match name {
        crate::parse::ReferenceName::Entity(id) | crate::parse::ReferenceName::Value(id) => id,
    };
    write!(&mut note, "{id}")
        .map_err(|_| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    note.push_str(" -> ");
    note.push_str(target);
    if let Some(query) = query {
        note.push('?');
        note.push_str(query);
    }
    if let Some(fragment) = fragment {
        note.push('#');
        note.push_str(fragment);
    }
    notes.push(note);
    Ok(())
}

fn forwarded_reference_uri<'a>(exchange: &'a crate::parse::Exchange, uri: &'a str) -> &'a str {
    let Some(fragment) = uri.strip_prefix('#') else {
        return uri;
    };
    exchange
        .anchors()
        .iter()
        .find(|anchor| anchor.name == fragment)
        .and_then(|anchor| match &anchor.value {
            crate::parse::Value::Resource(uri) => Some(uri.as_str()),
            _ => None,
        })
        .unwrap_or(uri)
}

fn has_uri_scheme(uri: &str) -> bool {
    let Some(colon) = uri.find(':') else {
        return false;
    };
    let scheme = &uri[..colon];
    !scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// Classifies a physical ZIP member for the STEP container report.
pub(crate) fn classify_entry(name: &str) -> ContainerRole {
    match name {
        ROOT_NAME => ContainerRole::RootExchange,
        _ if name.ends_with('/') => ContainerRole::Directory,
        _ if extension_is(name, "p21")
            || extension_is(name, "step")
            || extension_is(name, "stp") =>
        {
            ContainerRole::SubsidiaryExchange
        }
        _ if extension_is(name, "zip") => ContainerRole::NestedArchive,
        _ => ContainerRole::Ancillary,
    }
}

fn extension_is(name: &str, expected: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn validate_entry_name(name: &str) -> Result<(), CodecError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
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
