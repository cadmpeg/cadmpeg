// SPDX-License-Identifier: Apache-2.0
//! `RSe` storage navigation and stable governing types.

use std::collections::BTreeMap;

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot, CompoundStreamId};
use cadmpeg_container::compression::inflate_zlib_exact;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// A validated `V<n>` storage-band number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StorageBand(u32);

impl StorageBand {
    pub(crate) fn parse(component: &str) -> Option<Self> {
        let (prefix, digits) = component.split_at_checked(1)?;
        if !prefix.eq_ignore_ascii_case("V") {
            return None;
        }
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok().map(Self)
    }

    pub(crate) const fn value(self) -> u32 {
        self.0
    }
}

/// Exact suffix shared by one `RSe` metadata and bulk stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SegmentToken(String);

impl SegmentToken {
    fn parse(name: &str) -> Option<(char, Self)> {
        let (prefix, token) = name.split_at_checked(1)?;
        let prefix = prefix.chars().next()?;
        if !matches!(prefix, 'M' | 'B') || token.is_empty() {
            return None;
        }
        Some((prefix, Self(token.into())))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// One exact M/B stream pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentPair {
    pub(crate) token: SegmentToken,
    pub(crate) metadata: CompoundStreamId,
    pub(crate) bulk: CompoundStreamId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MetaStreamVersion(u16);

impl MetaStreamVersion {
    pub(crate) const fn value(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SegmentKind {
    PmBRep,
    PmDc,
    PmGraphics,
    PmApp,
    PmBrowser,
    PmResult,
    FbAttribute,
    AmDc,
    AmBRep,
    AmGraphics,
    AmApp,
    AmBrowser,
    AmRx,
    Notebook,
    DesignView,
    Unknown(String),
}

impl SegmentKind {
    fn classify(display_name: &str) -> Self {
        match display_name {
            "PmBRepSegment" => Self::PmBRep,
            "PmDCSegment" => Self::PmDc,
            "PmGraphicsSegment" => Self::PmGraphics,
            "PmAppSegment" => Self::PmApp,
            "PmBrowserSegment" => Self::PmBrowser,
            "PmResultSegment" => Self::PmResult,
            "FBAttributeSegment" => Self::FbAttribute,
            "AmDCSegment" => Self::AmDc,
            "AmBRepSegment" => Self::AmBRep,
            "AmGraphicsSegment" => Self::AmGraphics,
            "AmAppSegment" => Self::AmApp,
            "AmBrowserSegment" => Self::AmBrowser,
            "AmRxSegment" => Self::AmRx,
            "NBNotebookSegment" => Self::Notebook,
            "DesignViewSegment" => Self::DesignView,
            name => Self::Unknown(name.into()),
        }
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            Self::PmBRep => "pm_brep",
            Self::PmDc => "pm_dc",
            Self::PmGraphics => "pm_graphics",
            Self::PmApp => "pm_app",
            Self::PmBrowser => "pm_browser",
            Self::PmResult => "pm_result",
            Self::FbAttribute => "fb_attribute",
            Self::AmDc => "am_dc",
            Self::AmBRep => "am_brep",
            Self::AmGraphics => "am_graphics",
            Self::AmApp => "am_app",
            Self::AmBrowser => "am_browser",
            Self::AmRx => "am_rx",
            Self::Notebook => "notebook",
            Self::DesignView => "design_view",
            Self::Unknown(name) => name,
        }
    }
}

#[derive(Debug)]
pub(crate) struct SegmentMeta<'a> {
    pub(crate) version: MetaStreamVersion,
    pub(crate) header_words: [u32; 4],
    pub(crate) display_name: String,
    pub(crate) kind: SegmentKind,
    pub(crate) segment_id: [u8; 16],
    pub(crate) state_words: [u32; 3],
    pub(crate) created: String,
    pub(crate) modified: String,
    pub(crate) body_form: u8,
    pub(crate) body: View<'a>,
}

#[derive(Debug)]
pub(crate) enum SegmentMetaState<'a> {
    Parsed(SegmentMeta<'a>),
    Unsupported { marker: String, version: u16 },
    Malformed(String),
}

#[derive(Debug)]
pub(crate) struct SegmentDescriptor<'a> {
    pub(crate) pair: SegmentPair,
    pub(crate) meta: SegmentMetaState<'a>,
}

/// `RSe` paths established from the compound directory.
#[derive(Debug)]
pub(crate) struct RseInventory<'a> {
    pub(crate) storage_bands: Vec<(StorageBand, CompoundStreamId)>,
    pub(crate) segments: Vec<SegmentDescriptor<'a>>,
    pub(crate) unpaired_metadata: Vec<SegmentToken>,
    pub(crate) unpaired_bulk: Vec<SegmentToken>,
}

impl<'a> RseInventory<'a> {
    pub(crate) fn build(ctx: &DecodeContext<'a>, snapshot: &CompoundSnapshot<'a>) -> Self {
        let mut databases = Vec::new();
        let mut metadata = BTreeMap::new();
        let mut bulk = BTreeMap::new();
        for entry in snapshot.entries() {
            let CompoundEntry::Stream(stream) = entry else {
                continue;
            };
            let path = stream.path();
            if let Some(band) = database_band(path) {
                databases.push((band, stream.id()));
                continue;
            }
            let Some(name) = direct_rse_child(path) else {
                continue;
            };
            let Some((prefix, token)) = SegmentToken::parse(name) else {
                continue;
            };
            match prefix {
                'M' => {
                    metadata.insert(token, stream.id());
                }
                'B' => {
                    bulk.insert(token, stream.id());
                }
                _ => unreachable!("validated segment prefix"),
            }
        }
        databases.sort_by_key(|(band, _)| *band);
        let pairs = metadata
            .iter()
            .filter_map(|(token, metadata)| {
                bulk.get(token).map(|bulk| SegmentPair {
                    token: token.clone(),
                    metadata: *metadata,
                    bulk: *bulk,
                })
            })
            .collect::<Vec<_>>();
        let segments = pairs
            .into_iter()
            .map(|pair| {
                let meta = snapshot
                    .stream_by_id(pair.metadata)
                    .ok_or_else(|| {
                        CodecError::Malformed("RSe metadata stream handle is absent".into())
                    })
                    .and_then(|entry| snapshot.open(ctx, entry))
                    .and_then(|view| parse_meta_stream_v8(ctx, view));
                let meta = match meta {
                    Ok(meta) => meta,
                    Err(error) => SegmentMetaState::Malformed(error.to_string()),
                };
                SegmentDescriptor { pair, meta }
            })
            .collect();
        let unpaired_metadata = metadata
            .keys()
            .filter(|token| !bulk.contains_key(*token))
            .cloned()
            .collect();
        let unpaired_bulk = bulk
            .keys()
            .filter(|token| !metadata.contains_key(*token))
            .cloned()
            .collect();
        Self {
            storage_bands: databases,
            segments,
            unpaired_metadata,
            unpaired_bulk,
        }
    }
}

fn parse_meta_stream_v8<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
) -> Result<SegmentMetaState<'a>, CodecError> {
    let mut cursor = MetaCursor::new(source.window());
    let marker = cursor.length_prefixed_utf8("marker")?;
    let version = cursor.u16("version")?;
    if marker != "RSe Meta Stream Version 8" || version != 8 {
        return Ok(SegmentMetaState::Unsupported { marker, version });
    }
    let header_words = cursor.u32_array("header words")?;
    let display_name = cursor.length_prefixed_utf16("display name")?;
    let mut segment_id = [0; 16];
    segment_id.copy_from_slice(cursor.take(16, "segment id")?);
    let state_words = cursor.u32_array("state words")?;
    let created = cursor.length_prefixed_utf8("creation timestamp")?;
    let modified = cursor.length_prefixed_utf8("modification timestamp")?;
    let body_form = cursor.u8("body form")?;
    if cursor.remaining() == 0 {
        return Err(CodecError::Malformed(
            "RSe metadata stream has no compressed body".into(),
        ));
    }
    let compressed = source
        .child(source.start() + cursor.position, source.end())
        .ok_or_else(|| CodecError::Malformed("RSe metadata body range is invalid".into()))?;
    let body = inflate_zlib_exact(ctx, compressed)?;
    let kind = SegmentKind::classify(&display_name);
    Ok(SegmentMetaState::Parsed(SegmentMeta {
        version: MetaStreamVersion(version),
        header_words,
        display_name,
        kind,
        segment_id,
        state_words,
        created,
        modified,
        body_form,
        body,
    }))
}

struct MetaCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> MetaCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn take(&mut self, len: usize, what: &str) -> Result<&'a [u8], CodecError> {
        let end = self.position.checked_add(len).ok_or_else(|| {
            CodecError::Malformed(format!("RSe metadata {what} length overflows"))
        })?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| CodecError::Malformed(format!("truncated RSe metadata {what}")))?;
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self, what: &str) -> Result<u8, CodecError> {
        Ok(self.take(1, what)?[0])
    }

    fn u16(&mut self, what: &str) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(
            self.take(2, what)?
                .try_into()
                .expect("two-byte cursor read"),
        ))
    }

    fn u32(&mut self, what: &str) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(
            self.take(4, what)?
                .try_into()
                .expect("four-byte cursor read"),
        ))
    }

    fn u32_array<const N: usize>(&mut self, what: &str) -> Result<[u32; N], CodecError> {
        let mut values = [0; N];
        for value in &mut values {
            *value = self.u32(what)?;
        }
        Ok(values)
    }

    fn length(&mut self, what: &str, width: usize) -> Result<usize, CodecError> {
        let count = usize::try_from(self.u32(what)?)
            .map_err(|_| CodecError::Malformed(format!("RSe metadata {what} is too large")))?;
        count
            .checked_mul(width)
            .ok_or_else(|| CodecError::Malformed(format!("RSe metadata {what} length overflows")))
    }

    fn length_prefixed_utf8(&mut self, what: &str) -> Result<String, CodecError> {
        let len = self.length(what, 1)?;
        if len > 256 {
            return Err(CodecError::Malformed(format!(
                "RSe metadata {what} exceeds 256 bytes"
            )));
        }
        let bytes = self.take(len, what)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| CodecError::Malformed(format!("RSe metadata {what} is not UTF-8")))
    }

    fn length_prefixed_utf16(&mut self, what: &str) -> Result<String, CodecError> {
        let len = self.length(what, 2)?;
        if len > 8_192 {
            return Err(CodecError::Malformed(format!(
                "RSe metadata {what} exceeds 4096 UTF-16 units"
            )));
        }
        let units = self
            .take(len, what)?
            .chunks_exact(2)
            .map(|word| u16::from_le_bytes([word[0], word[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units)
            .map_err(|_| CodecError::Malformed(format!("RSe metadata {what} is not UTF-16")))
    }
}

pub(crate) fn direct_rse_child(path: &str) -> Option<&str> {
    let mut components = path.split('/');
    let storage = components.next()?;
    let child = components.next()?;
    (storage.eq_ignore_ascii_case("RSeStorage") && components.next().is_none()).then_some(child)
}

pub(crate) fn database_band(path: &str) -> Option<StorageBand> {
    let mut components = path.split('/');
    let storage = components.next()?;
    let band = components.next()?;
    let name = components.next()?;
    (storage.eq_ignore_ascii_case("RSeStorage")
        && name.eq_ignore_ascii_case("RSeDb")
        && components.next().is_none())
    .then(|| StorageBand::parse(band))
    .flatten()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use super::*;

    #[test]
    fn meta_stream_v8_frames_header_and_exact_zlib_body() {
        let bytes = meta_fixture(false);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic metadata stream fits policy");
        let SegmentMetaState::Parsed(meta) =
            parse_meta_stream_v8(&ctx, root).expect("synthetic metadata stream parses")
        else {
            panic!("version-eight metadata state")
        };
        assert_eq!(meta.version.value(), 8);
        assert_eq!(meta.header_words, [1, 2, 3, 4]);
        assert_eq!(meta.display_name, "PmBRepSegment");
        assert_eq!(meta.kind, SegmentKind::PmBRep);
        assert_eq!(meta.segment_id, [0x5a; 16]);
        assert_eq!(meta.state_words, [5, 6, 7]);
        assert_eq!(meta.body.window(), b"typed metadata body");
    }

    #[test]
    fn meta_stream_v8_rejects_a_zlib_suffix() {
        let bytes = meta_fixture(true);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic metadata stream fits policy");
        assert!(parse_meta_stream_v8(&ctx, root).is_err());
    }

    fn meta_fixture(suffix: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_bytes(&mut bytes, b"RSe Meta Stream Version 8");
        bytes.extend_from_slice(&8_u16.to_le_bytes());
        for value in 1_u32..=4 {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        push_utf16(&mut bytes, "PmBRepSegment");
        bytes.extend_from_slice(&[0x5a; 16]);
        for value in 5_u32..=7 {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        push_bytes(&mut bytes, b"created");
        push_bytes(&mut bytes, b"modified");
        bytes.push(1);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"typed metadata body")
            .expect("write synthetic zlib body");
        bytes.extend_from_slice(&encoder.finish().expect("finish synthetic zlib body"));
        if suffix {
            bytes.push(0);
        }
        bytes
    }

    fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(&(value.len() as u32).to_le_bytes());
        output.extend_from_slice(value);
    }

    fn push_utf16(output: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        output.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            output.extend_from_slice(&unit.to_le_bytes());
        }
    }
}
