// SPDX-License-Identifier: Apache-2.0
//! Bounded ZIP admission and member views.
use crate::error::Error;
use cadmpeg_container::{ArchiveSnapshot, ZipCompression};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
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
}
fn failure(detail: impl std::fmt::Display) -> Error {
    Error::Decode(format!("Onshape bundle: {detail}"))
}
impl<'a> Bundle<'a> {
    pub(super) fn open(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, Error> {
        directory(root).ok_or_else(|| {
            failure("the ZIP directory exceeds the bundle limits or cannot be read")
        })?;
        let snapshot = ArchiveSnapshot::new(ctx, root)?;
        if snapshot.recovery_reason().is_some() || snapshot.entries().len() > MEMBERS {
            return Err(failure("the ZIP directory is invalid"));
        }
        let mut members = BTreeMap::new();
        let mut remaining = EXPANDED_BYTES;
        for entry in snapshot.entries() {
            let name = &entry.name;
            if name.contains('\\')
                || name
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".." || p.contains(':'))
                || entry.encrypted
                || !matches!(
                    entry.compression,
                    ZipCompression::Stored | ZipCompression::Deflate
                )
            {
                return Err(failure(
                    "a member name or compression method is unsupported",
                ));
            }
            let limit = if has_extension(name, "json") {
                JSON_BYTES.min(remaining)
            } else {
                remaining
            };
            if entry.uncompressed_size > limit {
                return Err(failure("the expanded data exceeds the bundle limit"));
            }
            let data = snapshot.open(ctx, name)?;
            let length = cadmpeg_core::decode::u64_from_index(data.window().len());
            remaining = remaining
                .checked_sub(length)
                .ok_or_else(|| failure("expanded data exceeds the bundle limit"))?;
            if length > limit || members.insert(name.clone(), data).is_some() {
                return Err(failure("a member repeats or exceeds its expanded limit"));
            }
        }
        Ok(Self { snapshot, members })
    }
    pub(super) fn bytes(&self, path: &str) -> Result<&[u8], Error> {
        Ok(self.view(path)?.window())
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
    pub(super) fn json(&self, path: &str) -> Result<Value, Error> {
        serde_json::from_slice(self.bytes(path)?).map_err(|e| failure(format!("{path}: {e}")))
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
    let bytes = root.window();
    if !bytes.starts_with(b"PK\x03\x04") {
        return None;
    }
    let start = bytes.len() - bytes.len().min(65_633);
    let tail = bytes.get(start..)?;
    let offset = tail.windows(4).rposition(|w| w == b"PK\x05\x06")?;
    let end = root.child(start.checked_add(offset)?, bytes.len())?;
    let word = |at| View::u16_le_at(end.window(), at);
    let dword = |at| View::u32_le_at(end.window(), at);
    if end.window().len() != 22usize.checked_add(usize::from(word(20)?))?
        || word(4)? != 0
        || word(6)? != 0
    {
        return None;
    }
    let locator = offset
        .checked_sub(20)
        .and_then(|at| root.child(start + at, start + offset));
    let (members, size, position) = if let Some(locator) =
        locator.filter(|l| l.window().starts_with(b"PK\x06\x07"))
    {
        let disk = View::u32_le_at(locator.window(), 4)?;
        let at = View::u64_le_at(locator.window(), 8)?;
        let disks = View::u32_le_at(locator.window(), 16)?;
        if disk != 0 || disks != 1 {
            return None;
        }
        let at = usize::try_from(at).ok()?;
        let record = root.child(at, at.checked_add(56)?)?;
        if !record.window().starts_with(b"PK\x06\x06") || record.window().get(16..24)? != [0; 8] {
            return None;
        }
        let value = |at| View::u64_le_at(record.window(), at);
        let size = value(4)?;
        if !(44..=DIRECTORY_BYTES).contains(&size)
            || at
                .checked_add(12)?
                .checked_add(usize::try_from(size).ok()?)?
                > bytes.len()
            || value(24)? != value(32)?
        {
            return None;
        }
        (value(32)?, value(40)?, value(48)?)
    } else {
        if word(8)? != word(10)? {
            return None;
        }
        (
            u64::from(word(10)?),
            u64::from(dword(12)?),
            u64::from(dword(16)?),
        )
    };
    if members > cadmpeg_core::decode::u64_from_index(MEMBERS)
        || size > DIRECTORY_BYTES
        || position.checked_add(size)? > cadmpeg_core::decode::u64_from_index(bytes.len())
    {
        return None;
    }
    Some(())
}

pub(super) fn manifest(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Value, Error> {
    directory(root)
        .ok_or_else(|| failure("the ZIP directory exceeds the bundle limits or cannot be read"))?;
    let snapshot = ArchiveSnapshot::new(ctx, root)?;
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
    serde_json::from_slice(view.window()).map_err(|e| failure(format!("manifest.json: {e}")))
}

fn has_extension(name: &str, suffix: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension == suffix)
}
