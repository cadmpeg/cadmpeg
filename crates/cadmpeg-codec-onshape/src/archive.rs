// SPDX-License-Identifier: Apache-2.0
//! Bounded ZIP admission and member views.
use crate::error::Error;
use crate::loss::OnshapeLossCode;
use cadmpeg_container::{ArchiveSnapshot, EntryRecord, ZipCompression};
use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{record_given_up, DecodeParts, GivenUpPart, GivenUpRecord, Part};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) const SCHEMA: &str = "dutchman.onshape-bundle/1";
const MEMBERS: usize = 4096;
const JSON_BYTES: u64 = 32 << 20;
const EXPANDED_BYTES: u64 = 256 << 20;
const DIRECTORY_BYTES: u64 = 16 << 20;

/// The part an archive entry is when the bundle omits it.
const ARCHIVE_ENTRY_PART: &str = "archive-entry";

pub(super) struct Bundle<'a> {
    pub(super) snapshot: ArchiveSnapshot<'a>,
    root: View<'a>,
    members: BTreeMap<String, View<'a>>,
    json: BTreeMap<String, Value>,
    pub(super) losses: Vec<LossNote>,
    /// The entries the bundle omits, in the order it omits them.
    pub(super) given_up: Vec<GivenUpPart>,
}
fn failure(detail: impl std::fmt::Display) -> Error {
    Error::Decode(format!("Onshape bundle: {detail}"))
}
impl<'a> Bundle<'a> {
    pub(super) fn open(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, Error> {
        directory(root).ok_or_else(|| {
            failure("the ZIP directory exceeds the bundle limits or cannot be read")
        })?;
        let snapshot = ArchiveSnapshot::with_repeated_names(ctx, root)?;
        if snapshot.recovery_reason().is_some() || snapshot.entries().len() > MEMBERS {
            return Err(failure("the ZIP directory is invalid"));
        }
        let manifest_entry = snapshot
            .entry("manifest.json")
            .ok_or_else(|| failure("manifest.json is missing"))?;
        if manifest_entry.uncompressed_size > JSON_BYTES {
            return Err(failure("the expanded data exceeds the bundle limit"));
        }
        if !matches!(
            manifest_entry.compression,
            ZipCompression::Stored | ZipCompression::Deflate
        ) {
            return Err(failure("the manifest compression method is unsupported"));
        }
        let manifest_view = snapshot.open(ctx, "manifest.json")?;
        let manifest_text = std::str::from_utf8(manifest_view.window())
            .map_err(|e| failure(format!("manifest.json: {e}")))?;
        let manifest = ctx.parse_json_value(manifest_text, "Onshape manifest")?;
        let prefixes: Vec<String> = manifest
            .get("elements")
            .and_then(Value::as_array)
            .filter(|elements| elements.len() <= super::reader::ROWS)
            .into_iter()
            .flatten()
            .filter_map(|element| element.get("id").and_then(Value::as_str))
            .map(|id| format!("partstudios/{id}/"))
            .collect();
        let mut members = BTreeMap::new();
        let mut json = BTreeMap::from([("manifest.json".to_owned(), manifest)]);
        let mut remaining = EXPANDED_BYTES;
        let mut losses = Vec::new();
        let mut given_up = Vec::new();
        let mut first_stored = BTreeMap::<&str, Option<(ZipCompression, &[u8])>>::new();
        for entry in snapshot.entries() {
            let name = &entry.name;
            let stored = entry_view(root, entry).map(|view| (entry.compression, view.window()));
            // `Some(true)` marks a later copy whose stored bytes agree with the first copy.
            let repeated = match first_stored.get(name.as_str()) {
                Some(first) => Some(first.is_some() && *first == stored),
                None => {
                    first_stored.insert(name.as_str(), stored);
                    None
                }
            };
            let invalid_name = name.contains('\\')
                || name
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".." || p.contains(':'));
            if repeated.is_some()
                || invalid_name
                || entry.encrypted
                || !matches!(
                    entry.compression,
                    ZipCompression::Stored | ZipCompression::Deflate
                )
            {
                if name == "manifest.json" {
                    return Err(failure(
                        "manifest name, repetition or compression is invalid",
                    ));
                }
                let note = OnshapeLossCode::MemberUnreadable.note(format!("{name}: invalid name, repeated name, encryption or unsupported method; omitted the member"));
                if repeated == Some(true) {
                    losses.push(note);
                } else {
                    give_up_entry(ctx, root, entry, note, false, &mut losses, &mut given_up)?;
                }
                continue;
            }
            let member_limit = if has_extension(name, "json") {
                JSON_BYTES
            } else {
                u64::MAX
            };
            let member_limit = if entry.compression == ZipCompression::Deflate {
                member_limit.min(ctx.per_expand_allowance())
            } else {
                member_limit
            };
            let member_limit = member_limit.min(remaining);
            if entry.uncompressed_size > member_limit {
                let note = OnshapeLossCode::MemberTooLarge.note(format!(
                    "{name}: expanded member exceeds {member_limit} bytes; omitted the member"
                ));
                give_up_entry(ctx, root, entry, note, true, &mut losses, &mut given_up)?;
                continue;
            }
            remaining -= entry.uncompressed_size;
            let data = if name == "manifest.json" {
                manifest_view
            } else {
                match ctx.run_part(|| snapshot.open(ctx, name))? {
                    Ok(data) => data,
                    Err(CodecError::ResourceLimit(limit))
                        if limit.scope == cadmpeg_core::decode::LimitScope::Part =>
                    {
                        let note = OnshapeLossCode::MemberTooLarge
                            .note(format!("{name}: {limit:?}; omitted the member"));
                        give_up_entry(ctx, root, entry, note, true, &mut losses, &mut given_up)?;
                        continue;
                    }
                    Err(error @ CodecError::ResourceLimit(_)) => return Err(error.into()),
                    Err(error)
                        if prefixes
                            .iter()
                            .any(|prefix| name.strip_prefix(prefix) == Some("export.step")) =>
                    {
                        let note = OnshapeLossCode::StepUnreadable
                            .note(format!("{name}: {error}; final geometry is missing"));
                        give_up_entry(ctx, root, entry, note, false, &mut losses, &mut given_up)?;
                        continue;
                    }
                    Err(error) => {
                        let note = OnshapeLossCode::MemberUnreadable
                            .note(format!("{name}: {error}; omitted the member"));
                        give_up_entry(ctx, root, entry, note, false, &mut losses, &mut given_up)?;
                        continue;
                    }
                }
            };
            let length = cadmpeg_core::decode::u64_from_index(data.window().len());
            if has_extension(name, "json") && length > JSON_BYTES {
                let note = OnshapeLossCode::MemberTooLarge.note(format!(
                    "{name}: expanded member exceeds {JSON_BYTES} bytes; omitted the member"
                ));
                give_up_entry(ctx, root, entry, note, true, &mut losses, &mut given_up)?;
                continue;
            }
            if prefixes.iter().any(|prefix| schema_response(name, prefix))
                || (name.starts_with("assemblies/")
                    && (name.ends_with("/definition.json") || name.ends_with("/features.json")))
            {
                // An error carries whether a part resource limit stopped the parse.
                let parsed = match std::str::from_utf8(data.window()) {
                    Ok(text) => {
                        match ctx.run_part(|| ctx.parse_json_value(text, "Onshape JSON member"))? {
                            Ok(value) => Ok(value),
                            Err(CodecError::ResourceLimit(limit))
                                if limit.scope == cadmpeg_core::decode::LimitScope::Session =>
                            {
                                return Err(CodecError::ResourceLimit(limit).into())
                            }
                            Err(error @ CodecError::ResourceLimit(_)) => {
                                Err((error.to_string(), true))
                            }
                            Err(error) => Err((error.to_string(), false)),
                        }
                    }
                    Err(error) => Err((error.to_string(), false)),
                };
                match parsed {
                    Ok(value) => {
                        json.insert(name.clone(), value);
                    }
                    Err((error, bound)) => {
                        let retained = name.ends_with("/sketches.json");
                        let action = if retained {
                            "retained the member bytes"
                        } else {
                            "omitted the member"
                        };
                        let note = OnshapeLossCode::MemberUnreadable
                            .note(format!("{name}: {error}; {action}"));
                        if retained {
                            losses.push(note);
                        } else {
                            give_up_entry(
                                ctx,
                                root,
                                entry,
                                note,
                                bound,
                                &mut losses,
                                &mut given_up,
                            )?;
                            continue;
                        }
                    }
                }
            }
            members.insert(name.clone(), data);
        }
        Ok(Self {
            snapshot,
            root,
            members,
            json,
            losses,
            given_up,
        })
    }
    pub(super) fn view(&self, path: &str) -> Result<View<'a>, Error> {
        self.members
            .get(path)
            .copied()
            .ok_or_else(|| failure(format!("{path} is missing")))
    }
    pub(super) fn contains(&self, path: &str) -> bool {
        self.members.contains_key(path)
    }
    pub(super) fn json(&self, path: &str) -> Result<&Value, Error> {
        self.json
            .get(path)
            .ok_or_else(|| failure(format!("{path} is missing")))
    }
    pub(super) fn admit_microversions(
        &mut self,
        ctx: &DecodeContext<'_>,
        expected: &str,
    ) -> Result<(), Error> {
        let mut omitted = Vec::new();
        for (path, value) in &self.json {
            if path == "manifest.json" {
                continue;
            }
            for key in ["sourceMicroversion", "microversionId"] {
                if let Err(reason) = super::reader::microversion(value, key, expected) {
                    let retained = path.ends_with("/sketches.json");
                    let action = if retained {
                        "retained the member bytes"
                    } else {
                        "omitted the member"
                    };
                    let note = OnshapeLossCode::MemberUnreadable
                        .note(format!("{path}: {reason}; {action}"));
                    if retained {
                        self.losses.push(note);
                    } else if let Some(entry) = self.snapshot.entry(path) {
                        give_up_entry(
                            ctx,
                            self.root,
                            entry,
                            note,
                            false,
                            &mut self.losses,
                            &mut self.given_up,
                        )?;
                    } else {
                        self.losses.extend(record_given_up(path, note));
                    }
                    omitted.push(path.clone());
                    break;
                }
            }
        }
        for path in omitted {
            self.json.remove(&path);
            if !path.ends_with("/sketches.json") {
                self.members.remove(&path);
            }
        }
        Ok(())
    }
    /// The root offset and the length of the stored bytes of member `path`.
    pub(super) fn stored_span(&self, path: &str) -> Option<(u64, u64)> {
        let range = self.snapshot.entry(path)?.stored_range().ok()?;
        Some((range.start, range.end.checked_sub(range.start)?))
    }
    /// Commits the omitted entries to `ir` and `fidelity`, and returns
    /// their `record.unreadable` losses.
    pub(super) fn commit_given_up(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut cadmpeg_ir::CadIr,
        fidelity: &mut cadmpeg_ir::SourceFidelity,
    ) -> Result<Vec<LossNote>, CodecError> {
        std::mem::take(&mut self.given_up)
            .into_iter()
            .map(|part| part.commit(ctx, ir, fidelity))
            .collect()
    }
    pub(super) fn admit_topology(&mut self, ctx: &DecodeContext<'_>) -> Result<(), Error> {
        let mut count = 0usize;
        let mut omitted = Vec::new();
        for (path, response) in &self.json {
            if !path.contains("/bodydetails/") {
                continue;
            }
            let Some(bodies) = response.get("bodies").and_then(Value::as_array) else {
                continue;
            };
            let rows = (|| {
                let mut count = 0usize;
                let mut add = |length: usize| -> Option<()> {
                    if length > super::reader::ROWS {
                        return None;
                    }
                    count = count.checked_add(length)?;
                    Some(())
                };
                add(bodies.len())?;
                for body in bodies {
                    for key in ["faces", "edges", "vertices"] {
                        if let Some(rows) = body.get(key).and_then(Value::as_array) {
                            add(rows.len())?;
                        }
                    }
                    if let Some(faces) = body.get("faces").and_then(Value::as_array) {
                        for face in faces {
                            if let Some(loops) = face.get("loops").and_then(Value::as_array) {
                                add(loops.len())?;
                                for boundary in loops {
                                    if let Some(coedges) =
                                        boundary.get("coedges").and_then(Value::as_array)
                                    {
                                        add(coedges.len())?;
                                    }
                                }
                            }
                        }
                    }
                }
                Some(count)
            })();
            if let Some(rows) = rows {
                count = count
                    .checked_add(rows)
                    .ok_or_else(|| failure("topology count overflow"))?;
            } else {
                omitted.push(path.clone());
            }
        }
        for path in omitted {
            self.json.remove(&path);
            let note = OnshapeLossCode::TopologyTooLarge.note(format!(
                "{path}: topology exceeds the row limit; omitted the topology table"
            ));
            let Some((offset, length)) = self.stored_span(&path) else {
                self.losses.extend(record_given_up(&path, note));
                continue;
            };
            let note = note.with_provenance(cadmpeg_ir::SourceProvenance::root("onshape", offset));
            let unreadable = GivenUpRecord::caused_by(&path, &note)
                .spanning(length)
                .loss();
            self.losses.extend([note, unreadable]);
        }
        Ok(ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(count),
            "Onshape topology admission",
        )?)
    }
    pub(super) fn retain_assets(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut cadmpeg_ir::CadIr,
    ) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
        use cadmpeg_ir::assets::{Asset, AssetContent, AssetData, AssetId};
        let mut losses = Vec::new();
        for (path, data) in &self.members {
            if data.is_empty() {
                losses.push(
                    crate::loss::OnshapeLossCode::EmptyMember
                        .note(format!("empty bundle member {path:?}")),
                );
                continue;
            }
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(data.window().len()),
                "retain Onshape member",
            )?;
            ctx.charge_entities(1, "Onshape asset")?;
            let key = cadmpeg_ir::ids::IdentityKey::encode_key_text(path);
            ir.model.assets.push(Asset {
                id: AssetId::mint(format!("onshape:document:asset#{key}"))
                    .map_err(|e| CodecError::malformed(e.to_string()))?,
                name: cadmpeg_core::text::NonBlankString::new(path.clone()),
                media_type: cadmpeg_core::text::NonBlankString::new(
                    if has_extension(path, "json") {
                        "application/json"
                    } else if has_extension(path, "step") {
                        "application/step"
                    } else {
                        "application/octet-stream"
                    },
                ),
                content: AssetContent::Embedded {
                    data: AssetData::new(data.window().to_vec())
                        .ok_or_else(|| CodecError::malformed("empty Onshape asset"))?,
                },
                native_ref: None,
            });
        }
        Ok(losses)
    }
}
/// Gives up `entry`, which the bundle omits for the codec loss `note`, as a
/// part over its stored bytes, or over its directory record where its stored
/// bytes have no bounded span. `digest_only` keeps only the digest of the
/// bytes, for an entry omitted because it exceeds a bound. The codec loss
/// goes to `losses` and the part to `given_up`.
fn give_up_entry(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    entry: &EntryRecord,
    note: LossNote,
    digest_only: bool,
    losses: &mut Vec<LossNote>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<(), CodecError> {
    let Some(view) = entry_view(root, entry) else {
        losses.extend(record_given_up(&entry.name, note));
        return Ok(());
    };
    let mut part = Part::new(
        "onshape",
        ARCHIVE_ENTRY_PART,
        view,
        [ArenaClass::Model, ArenaClass::Design],
    )
    .named(entry.name.as_str());
    if digest_only {
        part = part.digest_only();
    }
    let error = CodecError::malformed(format_args!("{}: {}", note.code, note.message));
    let part = ctx.give_up_part(part, error)?;
    losses.push(note);
    // Two directory records that name one stored span are one part.
    if given_up.iter().any(|held| held.id() == part.id()) {
        losses.push(part.loss());
    } else {
        given_up.push(part);
    }
    Ok(())
}

/// The stored bytes of `entry` in `root`, or its directory record where the
/// stored bytes have no bounded span.
fn entry_view<'a>(root: View<'a>, entry: &EntryRecord) -> Option<View<'a>> {
    let range = entry
        .stored_range()
        .unwrap_or_else(|_| entry.declaration_range());
    root.child(
        usize::try_from(range.start).ok()?,
        usize::try_from(range.end).ok()?,
    )
}

pub(super) fn directory(root: View<'_>) -> Option<()> {
    ArchiveSnapshot::admit_directory(
        root,
        cadmpeg_core::decode::u64_from_index(MEMBERS),
        DIRECTORY_BYTES,
    )
}

pub(super) fn manifest(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Value, Error> {
    directory(root)
        .ok_or_else(|| failure("the ZIP directory exceeds the bundle limits or cannot be read"))?;
    let snapshot = ArchiveSnapshot::with_repeated_names(ctx, root)?;
    let entry = snapshot
        .entry("manifest.json")
        .ok_or_else(|| failure("manifest.json is missing"))?;
    if entry.uncompressed_size > JSON_BYTES
        || !matches!(
            entry.compression,
            ZipCompression::Stored | ZipCompression::Deflate
        )
    {
        return Err(failure("manifest.json exceeds its admission limit"));
    }
    let view = snapshot.open(ctx, "manifest.json")?;
    let text =
        std::str::from_utf8(view.window()).map_err(|e| failure(format!("manifest.json: {e}")))?;
    Ok(ctx.parse_json_value(text, "Onshape manifest")?)
}

fn has_extension(name: &str, suffix: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension == suffix)
}

fn schema_response(name: &str, prefix: &str) -> bool {
    let Some(member) = name.strip_prefix(prefix) else {
        return false;
    };
    if matches!(
        member,
        "features.json"
            | "sketches.json"
            | "configuration.json"
            | "variables.json"
            | "bodydetails/index.json"
    ) {
        return true;
    }
    member
        .strip_prefix("bodydetails/")
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(ordinal, extension)| {
            extension == "json"
                && !ordinal.is_empty()
                && ordinal.bytes().all(|b| b.is_ascii_digit())
        })
}
