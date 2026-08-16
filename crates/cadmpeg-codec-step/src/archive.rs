// SPDX-License-Identifier: Apache-2.0
//! STEP Part 21 ZIP-container rules.

use std::path::Path;

use cadmpeg_container::{ArchiveSnapshot, EntryCompression};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// The required root member name from Part 21 Annex A.4.
pub(crate) const ROOT_NAME: &str = "ISO-10303.p21";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReferenceTarget {
    Internal {
        member: String,
        query: Option<String>,
        fragment: Option<String>,
    },
    External,
}

/// Returns whether the input begins with a ZIP local-file header.
pub(crate) fn has_zip_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// Returns whether a detection prefix names the required STEP root member.
pub(crate) fn has_root_marker(prefix: &[u8]) -> bool {
    has_zip_magic(prefix)
        && prefix
            .windows(ROOT_NAME.len())
            .any(|window| window == ROOT_NAME.as_bytes())
}

/// Opens and validates the required root member of one STEP ZIP container.
pub(crate) fn open_root<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<(ArchiveSnapshot<'a>, View<'a>), CodecError> {
    let archive = ArchiveSnapshot::new(root)?;
    for entry in archive.entries() {
        validate_entry_name(&entry.name)?;
        if entry.compression == EntryCompression::Zstd {
            return Err(CodecError::NotImplemented(
                "STEP ZIP requires PKZIP 2.04g stored or Deflate entries".into(),
            ));
        }
    }
    let root_entry = archive.entry(ROOT_NAME).ok_or_else(|| {
        CodecError::WrongFormat(format!("STEP ZIP has no required root {ROOT_NAME}"))
    })?;
    let root_view = archive.open(ctx, root_entry)?;
    Ok((archive, root_view))
}

/// Resolves one archive URI against the directory of its referencing member.
pub(crate) fn resolve_uri(base_member: &str, uri: &str) -> Result<ReferenceTarget, CodecError> {
    if has_uri_scheme(uri) || uri.starts_with("//") {
        return Ok(ReferenceTarget::External);
    }
    let (uri, fragment) = uri.split_once('#').map_or((uri, None), |(uri, fragment)| {
        (uri, Some(fragment.to_owned()))
    });
    if fragment
        .as_deref()
        .is_some_and(|fragment| fragment.contains('#'))
    {
        return Err(CodecError::Malformed(format!(
            "invalid STEP ZIP URI fragment {uri:?}"
        )));
    }
    let (path, query) = uri
        .split_once('?')
        .map_or((uri, None), |(path, query)| (path, Some(query.to_owned())));
    if path.starts_with('/') {
        return Err(CodecError::Malformed(format!(
            "STEP ZIP URI escapes the archive root: {uri:?}"
        )));
    }
    let mut components = base_member
        .rsplit_once('/')
        .map_or_else(Vec::new, |(directory, _)| {
            directory.split('/').map(str::to_owned).collect()
        });
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
                return Err(CodecError::Malformed(format!(
                    "invalid empty path component in STEP ZIP URI {uri:?}"
                )))
            }
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(CodecError::Malformed(format!(
                        "STEP ZIP URI escapes the archive root: {uri:?}"
                    )));
                }
            }
            component => components.push(component.to_owned()),
        }
    }
    if components.is_empty() {
        return Err(CodecError::Malformed(format!(
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
    root_bytes: &[u8],
) -> Result<Vec<String>, CodecError> {
    let (exchange, _) = crate::parse::parse(root_bytes)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    let uris = exchange
        .references
        .iter()
        .map(|reference| (reference.name.as_str(), reference.uri.as_str()))
        .collect::<Vec<_>>();
    let mut notes = Vec::new();
    for (name, uri) in uris {
        match resolve_uri(ROOT_NAME, uri)? {
            ReferenceTarget::Internal {
                member,
                query,
                fragment,
            } => {
                if archive.entry(&member).is_none() {
                    return Err(CodecError::Malformed(format!(
                        "STEP ZIP resource {uri:?} for {name} has no archive member {member:?}"
                    )));
                }
                let query = query.map_or_else(String::new, |query| format!("?{query}"));
                let fragment = fragment.map_or_else(String::new, |fragment| format!("#{fragment}"));
                notes.push(format!(
                    "internal resource {name} -> {member}{query}{fragment}"
                ));
            }
            ReferenceTarget::External => {
                notes.push(format!("external resource {name} -> {uri}"));
            }
        }
    }
    Ok(notes)
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
pub(crate) fn classify_entry(name: &str) -> &'static str {
    match name {
        ROOT_NAME => "root-exchange",
        _ if name.ends_with('/') => "directory",
        _ if extension_is(name, "p21")
            || extension_is(name, "step")
            || extension_is(name, "stp") =>
        {
            "subsidiary-exchange"
        }
        _ if extension_is(name, "zip") => "nested-archive",
        _ => "ancillary",
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
        return Err(CodecError::Malformed(format!(
            "unsafe STEP ZIP entry path {name:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
