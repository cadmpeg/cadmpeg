// SPDX-License-Identifier: Apache-2.0
//! STEP Part 21 ZIP-container rules.

use cadmpeg_core::container::ContainerRole;

use std::collections::HashMap;
use std::path::Path;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) mod recovery;

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

    if !has_zip_magic(prefix.window()) && !ArchiveSnapshot::has_footer(ctx, prefix.window())? {
        return Ok(false);
    }
    match ArchiveSnapshot::contains_matching_name(ctx, prefix, |name| name == ROOT_NAME) {
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
    for entry in archive.entries() {
        validate_entry_name(&entry.name)?;
    }
    let root_entry = archive.entry(ROOT_NAME).ok_or_else(|| {
        CodecError::WrongFormat(format!("STEP ZIP has no required root {ROOT_NAME}"))
    })?;
    let data_start = root_entry
        .data_start
        .ok_or_else(|| CodecError::malformed("STEP ZIP root has no readable local frame"))?;
    let root_view = archive.open(ctx, &root_entry.name)?;
    Ok(OpenedRoot {
        archive,
        view: root_view,
        data_start,
    })
}

/// Resolves one archive URI against the directory of its referencing member.
fn resolve_uri<'a>(base_member: &'a str, uri: &'a str) -> Result<ReferenceTarget<'a>, CodecError> {
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

    if let Some((directory, _)) = base_member.rsplit_once('/') {
        for component in directory.split('/') {
            components.push(component);
        }
    }
    if path.is_empty() {
        return Ok(ReferenceTarget::Internal {
            member: base_member.to_owned(),
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
            component => components.push(component),
        }
    }
    if components.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "STEP ZIP URI resolves to no member: {uri:?}"
        )));
    }
    Ok(ReferenceTarget::Internal {
        member: components.join("/"),
        query,
        fragment,
    })
}

/// Resolves all root-file resource bindings and checks internal members.
pub(crate) fn root_reference_notes(
    archive: &ArchiveSnapshot<'_>,
    exchange: &crate::parse::Exchange,
) -> Result<Vec<String>, CodecError> {
    // CE-02: Annex A.4 makes subsidiary access a root reference operation;
    // this pass records the binding and does not import a subsidiary graph.
    let anchors = anchor_targets(exchange);
    let mut notes = Vec::new();
    for reference in exchange.references() {
        let name = reference.name;
        let uri = forwarded_reference_uri(&anchors, &reference.uri);

        match resolve_uri(ROOT_NAME, uri)? {
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
                notes.push(reference_note(
                    "internal resource ",
                    name,
                    &member,
                    query,
                    fragment,
                ));
            }
            ReferenceTarget::External => {
                notes.push(reference_note("external resource ", name, uri, None, None));
            }
        }
    }
    Ok(notes)
}

fn reference_note(
    prefix: &str,
    name: crate::parse::ReferenceName,
    target: &str,
    query: Option<&str>,
    fragment: Option<&str>,
) -> String {
    let (marker, id) = match name {
        crate::parse::ReferenceName::Entity(id) => ('#', id),
        crate::parse::ReferenceName::Value(id) => ('@', id),
    };
    let mut note = format!("{prefix}{marker}{id} -> {target}");
    if let Some(query) = query {
        note.push('?');
        note.push_str(query);
    }
    if let Some(fragment) = fragment {
        note.push('#');
        note.push_str(fragment);
    }
    note
}

/// Anchor names mapped to their values; anchor names are unique per exchange.
fn anchor_targets(exchange: &crate::parse::Exchange) -> HashMap<&str, &crate::parse::Value> {
    exchange
        .anchors()
        .iter()
        .map(|anchor| (anchor.name.as_str(), &anchor.value))
        .collect()
}

fn forwarded_reference_uri<'a>(
    anchors: &HashMap<&str, &'a crate::parse::Value>,
    uri: &'a str,
) -> &'a str {
    let Some(fragment) = uri.strip_prefix('#') else {
        return uri;
    };
    match anchors.get(fragment) {
        Some(crate::parse::Value::Resource(target)) => target.as_str(),
        _ => uri,
    }
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
