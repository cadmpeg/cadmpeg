// SPDX-License-Identifier: Apache-2.0
//! STEP Part 21 ZIP-container rules.

use cadmpeg_core::container::ContainerRole;
use std::collections::BTreeMap;

use cadmpeg_container::{ArchiveSnapshot, ZipCompression};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
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
pub(crate) struct OpenedRoot<'a, 'ctx> {
    pub(crate) archive: ArchiveSnapshot<'a>,
    pub(crate) view: View<'a>,
    pub(crate) data_start: u64,
    _storage: ScopedReservation<'ctx>,
}

/// Opens and validates the required root member of one STEP ZIP container.
pub(crate) fn open_root<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'a>,
    root: View<'a>,
) -> Result<OpenedRoot<'a, 'ctx>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "STEP ZIP directory storage")?;
    let archive = storage
        .with_storage(|| ArchiveSnapshot::new(ctx, root))
        .or_else(|error| match error {
            CodecError::Malformed(message) => Err(CodecError::Malformed(
                ctx.copy_retained_text(&message, "STEP ZIP directory error")?,
            )),
            CodecError::NotImplemented(message) => Err(CodecError::NotImplemented(
                ctx.copy_retained_text(&message, "STEP ZIP directory error")?,
            )),
            error => Err(error),
        })?;
    let mut entries = archive.entries().iter();
    while let Some(entry) = ctx.next_charged(&mut entries, "STEP open root borrowed traversal")? {
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
        _storage: storage,
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
        .position_by(uri.as_bytes(), |byte| Ok(*byte == b'#'), "STEP ZIP URI fragment split")?
        .map_or((uri, None), |separator| (&uri[..separator], Some(&uri[separator + 1..])));
    if fragment
        .map(|fragment| ctx.any_by(fragment.as_bytes(), |byte| Ok(*byte == b'#'), "STEP ZIP fragment separator containment"))
        .transpose()?
        .unwrap_or(false)
    {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("invalid STEP ZIP URI fragment {uri:?}"),
            "STEP ZIP error text",
        )?));
    }
    let (path, query) = ctx
        .position_by(uri.as_bytes(), |byte| Ok(*byte == b'?'), "STEP ZIP URI query split")?
        .map_or((uri, None), |separator| (&uri[..separator], Some(&uri[separator + 1..])));
    if path.starts_with('/') {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("STEP ZIP URI escapes the archive root: {uri:?}"),
            "STEP ZIP error text",
        )?));
    }
    let mut components = Vec::new();
    let mut component_bytes = ctx.reserve_scoped(0, "step_zip_uri_components_temp")?;
    if let Some(separator) =
        ctx.rposition_by(base_member.as_bytes(), |byte| Ok(*byte == b'/'), "STEP ZIP base member reverse split")?
    {
        let directory = &base_member[..separator];
        let mut start = 0;
        while start <= directory.len() {
            let end = ctx.position_by(
                &directory.as_bytes()[start..],
                |byte| Ok(*byte == b'/'),
                "STEP ZIP base directory traversal",
            )?.map_or(directory.len(), |relative| start + relative);
            let component = &directory[start..end];
            start = end + 1;
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
    let mut start = 0;
    while start <= path.len() {
        ctx.charge_work(1, "STEP ZIP URI component traversal")?;
        let end = ctx
            .position_by(
                &path.as_bytes()[start..],
                |byte| Ok(*byte == b'/'),
                "STEP ZIP URI delimiter search",
            )?
            .map_or(path.len(), |relative| start + relative);
        let component = &path[start..end];
        start = end + 1;
        match component {
            "" => {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("invalid empty path component in STEP ZIP URI {uri:?}"),
                    "STEP ZIP error text",
                )?));
            }
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(CodecError::Malformed(ctx.format_retained(
                        format_args!("STEP ZIP URI escapes the archive root: {uri:?}"),
                        "STEP ZIP error text",
                    )?));
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
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("STEP ZIP URI resolves to no member: {uri:?}"),
            "STEP ZIP error text",
        )?));
    }
    let component_len = ctx.fold(&components, 0_usize, |total, component| {
        total.checked_add(component.len()).ok_or_else(|| {
            ctx.refuse_codec_limit("step_zip_uri_member", 0, 1)
        })
    }, "STEP resolve uri traversal")?;
    let member_len = component_len.checked_add(components.len() - 1)
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_uri_member", 0, 1))?;
    let mut member = String::new();

    ctx.reserve_scoped_string(member_bytes, &mut member, member_len, "step_zip_uri_member")?;
    let mut visited_items = (components[..]).iter().enumerate();
    while let Some((index, component)) = ctx.next_charged(&mut visited_items, "STEP resolve uri traversal")? {
        if index != 0 {
            member_bytes.with_storage(|| {
                ctx.push_retained_char(&mut member, '/', "STEP ZIP member separator character")
            })?;
        }
        member_bytes
            .with_storage(|| ctx.append_retained(&mut member, component, "step_zip_uri_member"))?;
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
    let mut binding_storage = ctx.reserve_scoped(0, "STEP ZIP anchor index storage")?;
    let mut bindings = BTreeMap::new();
    let mut indexed = false;
    let mut references = exchange.references().iter();
    while let Some(reference) = ctx.next_charged(
        &mut references,
        "STEP root reference notes borrowed traversal",
    )? {
        let name = reference.name;
        if !indexed && reference.uri.starts_with('#') && reference.uri.len() > 1 {
            let mut visited_items = (exchange.anchors()).iter();
            while let Some(anchor) = ctx.next_charged(&mut visited_items, "STEP ZIP anchor index traversal")? {
                if let crate::parse::Value::Resource(target) = &anchor.value {
                    binding_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut bindings,
                            anchor.name.as_str(),
                            target.as_str(),
                            "STEP ZIP anchor index entries",
                        )
                    })?;
                }
            }
            indexed = true;
        }
        let uri = forwarded_reference_uri(&bindings, &reference.uri, ctx)?;
        let mut member_bytes = ctx.reserve_scoped(0, "step_zip_uri_member_temp")?;
        match resolve_uri(ctx, &mut member_bytes, ROOT_NAME, uri)? {
            ReferenceTarget::Internal {
                member,
                query,
                fragment,
            } => {
                if archive.entry(ctx, &member)?.is_none() {
                    return Err(CodecError::Malformed(ctx.format_retained(
                        format_args!(
                            "STEP ZIP resource {uri:?} for {name} has no archive member {member:?}"
                        ),
                        "STEP ZIP error text",
                    )?));
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
    let (marker, id) = match name {
        crate::parse::ReferenceName::Entity(id) => ('#', id),
        crate::parse::ReferenceName::Value(id) => ('@', id),
    };
    let mut note = ctx.format_retained(
        format_args!("{prefix}{marker}{id} -> {target}"),
        "step_zip_reference_note",
    )?;
    if let Some(query) = query {
        ctx.push_retained_char(&mut note, '?', "STEP ZIP reference query character")?;
        ctx.append_retained(&mut note, query, "step_zip_reference_note")?;
    }
    if let Some(fragment) = fragment {
        ctx.push_retained_char(&mut note, '#', "STEP ZIP reference fragment character")?;
        ctx.append_retained(&mut note, fragment, "step_zip_reference_note")?;
    }
    ctx.push_vec(notes, note, "step_zip_reference_notes")?;
    Ok(())
}

fn forwarded_reference_uri<'a>(
    bindings: &BTreeMap<&'a str, &'a str>,
    uri: &'a str,
    ctx: &DecodeContext<'_>,
) -> Result<&'a str, CodecError> {
    let Some(fragment) = uri.strip_prefix('#') else {
        return Ok(uri);
    };
    Ok(ctx
        .get_btree_map(bindings, fragment, "step_zip_anchor_lookup")?
        .copied()
        .unwrap_or(uri))
}

fn has_uri_scheme(ctx: &DecodeContext<'_>, uri: &str) -> Result<bool, CodecError> {
    let Some(colon) = ctx.position_by(
        uri.as_bytes(),
        |byte| Ok(*byte == b':'),
        "STEP URI scheme colon search",
    )?
    else {
        return Ok(false);
    };
    let scheme = &uri[..colon];
    Ok(!scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && ctx.all_by(
            scheme.as_bytes().iter().copied(),
            |byte| Ok(byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')),
            "STEP URI scheme traversal",
        )?)
}

/// Classifies a physical ZIP member for the STEP container report.
pub(crate) fn classify_entry(
    ctx: &DecodeContext<'_>,
    name: &str,
) -> Result<ContainerRole, CodecError> {
    if name == ROOT_NAME {
        return Ok(ContainerRole::RootExchange);
    }
    if name.ends_with('/') {
        return Ok(ContainerRole::Directory);
    }
    let Some(delimiter) = ctx.rposition_by(
        name.as_bytes(),
        |byte| Ok(matches!(byte, b'.' | b'/')),
        "STEP ZIP extension component search",
    )?
    else {
        return Ok(ContainerRole::Ancillary);
    };
    if name.as_bytes()[delimiter] == b'/'
        || delimiter == 0
        || name.as_bytes()[delimiter - 1] == b'/'
    {
        return Ok(ContainerRole::Ancillary);
    }
    let extension = &name[delimiter + 1..];
    Ok(
        if extension.eq_ignore_ascii_case("p21")
            || extension.eq_ignore_ascii_case("step")
            || extension.eq_ignore_ascii_case("stp")
        {
            ContainerRole::SubsidiaryExchange
        } else if extension.eq_ignore_ascii_case("zip") {
            ContainerRole::NestedArchive
        } else {
            ContainerRole::Ancillary
        },
    )
}

fn validate_entry_name(ctx: &DecodeContext<'_>, name: &str) -> Result<(), CodecError> {
    let mut start = 0;
    let unsafe_name = name.is_empty()
        || name.starts_with('/')
        || ctx.any_by(
            name.as_bytes().iter().enumerate(),
            |(end, byte)| {
                if matches!(byte, b'\\' | 0) {
                    return Ok(true);
                }
                if *byte != b'/' {
                    return Ok(false);
                }
                let component = &name[start..end];
                start = end + 1;
                Ok(matches!(component, "" | "." | ".."))
            },
            "STEP ZIP entry path validation",
        )?
        || matches!(&name[start..], "." | "..");
    if unsafe_name {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("unsafe STEP ZIP entry path {name:?}"),
            "STEP ZIP error text",
        )?));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
