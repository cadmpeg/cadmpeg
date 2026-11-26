// SPDX-License-Identifier: Apache-2.0
//! Bounded ZIP admission and member views.
use crate::error::Error;
use crate::loss::OnshapeLossCode;
use cadmpeg_container::{ArchiveSnapshot, ZipCompression};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) const SCHEMA: &str = "dutchman.onshape-bundle/1";
const MEMBERS: usize = 4096;
const JSON_BYTES: u64 = 32 << 20;
const EXPANDED_BYTES: u64 = 256 << 20;
const DIRECTORY_BYTES: u64 = 16 << 20;

pub(super) struct Bundle<'a> {
    pub(super) snapshot: ArchiveSnapshot<'a>,
    members: BTreeMap<String, View<'a>>,
    json: BTreeMap<String, Value>,
    pub(super) losses: Vec<LossNote>,
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
        let mut prefixes: Vec<String> = manifest
            .get("elements")
            .and_then(Value::as_array)
            .filter(|elements| elements.len() <= super::reader::ROWS)
            .into_iter()
            .flatten()
            .filter_map(|element| element.get("id").and_then(Value::as_str))
            .map(|id| format!("partstudios/{id}/"))
            .collect();
        if let Some(root) = studio_prefix("", &manifest) {
            if !prefixes.contains(&root) {
                prefixes.push(root);
            }
        }
        let mut members = BTreeMap::new();
        let mut json = BTreeMap::from([("manifest.json".to_owned(), manifest)]);
        let mut remaining = EXPANDED_BYTES;
        let mut losses = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        // Captured manifests come first so that each one names the responses it owns.
        let mut entries: Vec<_> = snapshot.entries().iter().collect();
        entries.sort_by_key(|entry| !reference_manifest(&entry.name));
        for entry in entries {
            let name = &entry.name;
            if !seen.insert(name)
                || name.contains('\\')
                || name
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".." || p.contains(':'))
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
                losses.push(OnshapeLossCode::MemberUnreadable.note(format!("{name}: invalid name, repeated name, encryption or unsupported method; omitted the member")));
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
                losses.push(OnshapeLossCode::MemberTooLarge.note(format!(
                    "{name}: expanded member exceeds {member_limit} bytes; omitted the member"
                )));
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
                        losses.push(
                            OnshapeLossCode::MemberTooLarge
                                .note(format!("{name}: {limit:?}; omitted the member")),
                        );
                        continue;
                    }
                    Err(error @ CodecError::ResourceLimit(_)) => return Err(error.into()),
                    Err(error)
                        if prefixes
                            .iter()
                            .any(|prefix| name.strip_prefix(prefix) == Some("export.step")) =>
                    {
                        losses.push(
                            OnshapeLossCode::StepUnreadable
                                .note(format!("{name}: {error}; final geometry is missing")),
                        );
                        continue;
                    }
                    Err(error) => {
                        losses.push(
                            OnshapeLossCode::MemberUnreadable
                                .note(format!("{name}: {error}; omitted the member")),
                        );
                        continue;
                    }
                }
            };
            let length = cadmpeg_core::decode::u64_from_index(data.window().len());
            if has_extension(name, "json") && length > JSON_BYTES {
                losses.push(OnshapeLossCode::MemberTooLarge.note(format!(
                    "{name}: expanded member exceeds {JSON_BYTES} bytes; omitted the member"
                )));
                continue;
            }
            let captured = reference_manifest(name);
            if captured
                || prefixes.iter().any(|prefix| schema_response(name, prefix))
                || (name.starts_with("assemblies/")
                    && (name.ends_with("/definition.json") || name.ends_with("/features.json")))
            {
                let parsed = match std::str::from_utf8(data.window()) {
                    Ok(text) => {
                        match ctx.run_part(|| ctx.parse_json_value(text, "Onshape JSON member"))? {
                            Ok(value) => Ok(value),
                            Err(CodecError::ResourceLimit(limit))
                                if limit.scope == cadmpeg_core::decode::LimitScope::Session =>
                            {
                                return Err(CodecError::ResourceLimit(limit).into())
                            }
                            Err(error) => Err(error.to_string()),
                        }
                    }
                    Err(error) => Err(error.to_string()),
                };
                match parsed {
                    Ok(value) => {
                        if captured {
                            let folder = name.strip_suffix("manifest.json").unwrap_or_default();
                            prefixes.extend(studio_prefix(folder, &value));
                        }
                        json.insert(name.clone(), value);
                    }
                    Err(error) => {
                        let retained = name.ends_with("/sketches.json");
                        let action = if retained {
                            "retained the member bytes"
                        } else {
                            "omitted the member"
                        };
                        losses.push(
                            OnshapeLossCode::MemberUnreadable
                                .note(format!("{name}: {error}; {action}")),
                        );
                        if !retained {
                            continue;
                        }
                    }
                }
            }
            members.insert(name.clone(), data);
        }
        Ok(Self {
            snapshot,
            members,
            json,
            losses,
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
    pub(super) fn admit_microversions(&mut self, expected: &str) {
        // A captured studio has its own pin. The nearest enclosing manifest owns each
        // member; a referenced response is never checked against the host pin.
        let pins: BTreeMap<String, String> = self
            .json
            .iter()
            .filter_map(|(path, value)| {
                let folder = path.strip_suffix("manifest.json")?;
                let mid = value.get("document")?.get("microversion")?.as_str()?;
                Some((folder.to_owned(), mid.to_owned()))
            })
            .collect();
        // The owning folder is the one whose studio prefix `{folder}partstudios/{id}/`
        // is the longest prefix of the member path. An element id can itself end in
        // `partstudios`, so the path is never split at the last `partstudios/`.
        let studios: Vec<(String, &str)> = self
            .json
            .iter()
            .filter_map(|(path, value)| {
                let folder = path.strip_suffix("manifest.json")?;
                Some((studio_prefix(folder, value)?, folder))
            })
            .collect();
        let mut omitted = Vec::new();
        for (path, value) in &self.json {
            if path.strip_suffix("manifest.json").is_some() {
                continue;
            }
            let owner = studios
                .iter()
                .filter(|(prefix, _)| path.starts_with(prefix.as_str()))
                .max_by_key(|(prefix, _)| prefix.len())
                .map(|(_, folder)| *folder);
            let expected = match owner {
                Some("") | None => expected,
                Some(folder) => match pins.get(folder) {
                    Some(mid) => mid.as_str(),
                    None => {
                        self.losses.push(OnshapeLossCode::MemberUnreadable.note(format!(
                            "{path}: the referenced studio microversion is missing; omitted the member"
                        )));
                        omitted.push(path.clone());
                        continue;
                    }
                },
            };
            for key in ["sourceMicroversion", "microversionId"] {
                if let Err(reason) = super::reader::microversion(value, key, expected) {
                    let action = if path.ends_with("/sketches.json") {
                        "retained the member bytes"
                    } else {
                        "omitted the member"
                    };
                    self.losses.push(
                        OnshapeLossCode::MemberUnreadable
                            .note(format!("{path}: {reason}; {action}")),
                    );
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
            self.losses
                .push(OnshapeLossCode::TopologyTooLarge.note(format!(
                    "{path}: topology exceeds the row limit; omitted the topology table"
                )));
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

/// The member folder of the responses a manifest selects.
fn studio_prefix(folder: &str, manifest: &Value) -> Option<String> {
    let element = manifest
        .get("root_element")
        .or_else(|| manifest.pointer("/elements/0/id"))?
        .as_str()?;
    (!element.is_empty()
        && element
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
    .then(|| format!("{folder}partstudios/{element}/"))
}

/// A captured referenced studio manifest at `references/<document>/<version>/<element>/`,
/// nested at most seven levels deep.
fn reference_manifest(name: &str) -> bool {
    let mut remaining = name;
    for _ in 0..crate::reader::MAX_REFERENCE_DEPTH {
        let Some(path) = remaining.strip_prefix("references/") else {
            return false;
        };
        let mut parts = path.splitn(4, '/');
        for _ in 0..3 {
            if !parts
                .next()
                .is_some_and(|id| id.len() == 24 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return false;
            }
        }
        let Some(tail) = parts.next() else {
            return false;
        };
        if tail == "manifest.json" {
            return true;
        }
        remaining = tail;
    }
    false
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

#[cfg(test)]
mod tests;
