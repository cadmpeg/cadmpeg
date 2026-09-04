// SPDX-License-Identifier: Apache-2.0
//! `RSe` storage navigation and stable governing types.

use std::collections::BTreeMap;

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot, CompoundStreamId};
use cadmpeg_container::compression::inflate_zlib_exact;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::database::{
    parse_database, parse_registry, parse_revisions, DatabaseHeader, RevisionTable, RseDatabase,
    RseSchema, SegmentRegistry,
};
use crate::kernel::{select_active_carrier, ActiveCarrierState};
use crate::layout::bulk_envelope as envelope;
use crate::records::{frame_bulk_records, parse_meta_tables, MetaTables, RseRecordTable};

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

/// The marker and version an `RSe` metadata stream declares in its first two
/// fields, as read.
///
/// [`parse_meta_stream`] attempts the version-8 body grammar for every marker
/// and version pair. The declaration is kept whether the body parses or not,
/// because the dialect classifier reports what the file said.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct MetaStreamDeclaration {
    pub(crate) marker: String,
    pub(crate) version: u16,
}

impl MetaStreamDeclaration {
    /// The one marker this codec implements a segment metadata grammar for.
    pub(crate) const VERIFIED_MARKER: &'static str = "RSe Meta Stream Version 8";
    /// The one version word this codec implements a segment metadata grammar for.
    pub(crate) const VERIFIED_VERSION: u16 = 8;

    pub(crate) fn is_verified(&self) -> bool {
        self.marker == Self::VERIFIED_MARKER && self.version == Self::VERIFIED_VERSION
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DocumentKind {
    Part,
    Assembly,
    Drawing,
    Presentation,
    Unknown(String),
}

impl DocumentKind {
    pub(crate) fn parse_property(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("part") {
            Some(Self::Part)
        } else if value.eq_ignore_ascii_case("assembly") {
            Some(Self::Assembly)
        } else if value.eq_ignore_ascii_case("drawing") {
            Some(Self::Drawing)
        } else if value.eq_ignore_ascii_case("presentation") {
            Some(Self::Presentation)
        } else {
            None
        }
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Part => "part",
            Self::Assembly => "assembly",
            Self::Drawing => "drawing",
            Self::Presentation => "presentation",
            Self::Unknown(detail) => detail,
        }
    }
}

impl SegmentKind {
    fn classify(display_name: &str, type_name: Option<&str>) -> Self {
        match type_name {
            Some("PmBrepSegmentType") => Self::PmBRep,
            Some("PmDcSegmentType") => Self::PmDc,
            Some("PmGRxSegmentType") => Self::PmGraphics,
            Some("PmAppSegmentType") => Self::PmApp,
            Some("PmBRxSegmentType" | "PmBrowserSegment") => Self::PmBrowser,
            Some("PmResultSegmentType") => Self::PmResult,
            Some("FBAttributeSegment") => Self::FbAttribute,
            Some("AmDcSegmentType") => Self::AmDc,
            Some("AmBREPSegmentType") => Self::AmBRep,
            Some("AmGRxSegmentType") => Self::AmGraphics,
            Some("AmAppSegmentType") => Self::AmApp,
            Some("AmBRxSegmentType") => Self::AmBrowser,
            Some("AmRxSegmentType") => Self::AmRx,
            Some("NotebookSegmentType") => Self::Notebook,
            Some("FWxDesignViewType" | "FWxDesignViewManagerType") => Self::DesignView,
            Some(type_name) => Self::Unknown(type_name.into()),
            None => Self::Unknown(display_name.into()),
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
    /// The marker and version the stream declared, kept verbatim: the grammar
    /// applied to the body is the version-8 one whatever this says.
    pub(crate) declared: MetaStreamDeclaration,
    pub(crate) header_values: [u16; 8],
    pub(crate) display_name: String,
    pub(crate) segment_id: [u8; 16],
    pub(crate) state_words: [u32; 3],
    pub(crate) created: String,
    pub(crate) modified: String,
    pub(crate) body_form: u8,
    pub(crate) body: View<'a>,
    pub(crate) tables: MetaTables<'a>,
}

#[derive(Debug)]
pub(crate) enum SegmentMetaState<'a> {
    /// The body parsed under the version-8 grammar. The declaration it carries
    /// is not always the verified pair: a foreign marker or version whose body
    /// obeys the grammar is read, and its declaration is what makes the
    /// document dialect-unverified.
    Parsed(Box<SegmentMeta<'a>>),
    /// The stream did not parse. `declared` carries the marker and version when
    /// they were read before the failure, and is `None` when the stream ended
    /// or failed inside those two fields — in which case the stream declares no
    /// dialect evidence at all.
    Malformed {
        declared: Option<MetaStreamDeclaration>,
        detail: String,
    },
}

impl SegmentMetaState<'_> {
    /// The marker and version this stream declared, where it declared them.
    ///
    /// [`Self::Parsed`] reports the declaration it was read from, not the
    /// verified pair: the version-8 grammar is attempted on every stream, so a
    /// parsed stream is not evidence that it declared version 8, and reporting
    /// the verified pair here would erase the unverified admission the
    /// declaration earns.
    pub(crate) fn declaration(&self) -> Option<MetaStreamDeclaration> {
        match self {
            Self::Parsed(meta) => Some(meta.declared.clone()),
            Self::Malformed { declared, .. } => declared.clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct SegmentDescriptor<'a> {
    pub(crate) pair: SegmentPair,
    pub(crate) registry_index: Option<usize>,
    pub(crate) registry_version_major: Option<u8>,
    pub(crate) kind: SegmentKind,
    pub(crate) identity_issues: Vec<String>,
    pub(crate) meta: SegmentMetaState<'a>,
    pub(crate) bulk: SegmentBulkState<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BulkForm(u16);

impl BulkForm {
    pub(crate) const fn value(self) -> u16 {
        self.0
    }
}

#[derive(Debug)]
pub(crate) struct SegmentBulk<'a> {
    pub(crate) prefix: [u8; 16],
    pub(crate) form: BulkForm,
    pub(crate) compressed: View<'a>,
    pub(crate) expanded: View<'a>,
    pub(crate) records: Option<RecordFrameState<'a>>,
}

#[derive(Debug)]
pub(crate) enum RecordFrameState<'a> {
    Framed(RseRecordTable<'a>),
    Unavailable(String),
}

#[derive(Debug)]
pub(crate) enum SegmentBulkState<'a> {
    Framed(SegmentBulk<'a>),
    Malformed(String),
}

#[derive(Debug)]
pub(crate) enum ParsedState<T> {
    Absent,
    Parsed(T),
    Unavailable(String),
}

#[derive(Debug)]
pub(crate) struct DatabaseDescriptor {
    pub(crate) band: StorageBand,
    pub(crate) stream: CompoundStreamId,
    /// The schema this `RSeDb` stream declared, or `None` when the stream did
    /// not read that far. Present for every declared schema, including schema
    /// 31, when its body leaves `state` as [`ParsedState::Unavailable`].
    pub(crate) declared_schema: Option<RseSchema>,
    pub(crate) state: ParsedState<RseDatabase>,
}

/// `RSe` paths established from the compound directory.
#[derive(Debug)]
pub(crate) struct RseInventory<'a> {
    pub(crate) databases: Vec<DatabaseDescriptor>,
    pub(crate) registry: ParsedState<SegmentRegistry>,
    pub(crate) revisions: ParsedState<RevisionTable>,
    pub(crate) segments: Vec<SegmentDescriptor<'a>>,
    pub(crate) unpaired_metadata: Vec<SegmentToken>,
    pub(crate) unpaired_bulk: Vec<SegmentToken>,
    pub(crate) active_carrier: ActiveCarrierState<'a>,
}

impl<'a> RseInventory<'a> {
    pub(crate) fn build(
        ctx: &DecodeContext<'a>,
        snapshot: &CompoundSnapshot<'a>,
    ) -> Result<Self, CodecError> {
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
        let mut database_descriptors = Vec::with_capacity(databases.len());
        for (band, stream_id) in databases {
            let (declared_schema, state) = match snapshot.stream_by_id(stream_id) {
                Some(stream) => match snapshot
                    .open(ctx, stream)
                    .and_then(|view| parse_database(ctx, view.window()))
                {
                    Ok(DatabaseHeader::Supported(database)) => {
                        (Some(database.schema), ParsedState::Parsed(database))
                    }
                    Ok(DatabaseHeader::Unframed { schema, detail }) => (
                        Some(schema),
                        ParsedState::Unavailable(DatabaseHeader::unframed_detail(schema, &detail)),
                    ),
                    Err(error) => (None, ParsedState::Unavailable(crate::issue_detail(error)?)),
                },
                None => (
                    None,
                    ParsedState::Unavailable("RSe database stream handle is absent".into()),
                ),
            };
            database_descriptors.push(DatabaseDescriptor {
                band,
                stream: stream_id,
                declared_schema,
                state,
            });
        }
        // The registry takes the schema-31 grammar whatever the `RSeDb` streams
        // declared, including when they declared nothing or disagreed. What the
        // grammar cannot frame degrades here, which is a structural outcome; the
        // declarations decide the admission, not whether the attempt is made.
        let registry = match snapshot.stream("RSeStorage/RSeSegInfo") {
            None => ParsedState::Absent,
            Some(stream) => match snapshot
                .open(ctx, stream)
                .and_then(|view| parse_registry(ctx, view.window()))
            {
                Ok(value) => ParsedState::Parsed(value),
                Err(error) => ParsedState::Unavailable(crate::issue_detail(error)?),
            },
        };
        let revisions = match snapshot.stream("RSeStorage/RSeDbRevisionInfo") {
            None => ParsedState::Absent,
            Some(stream) => match snapshot
                .open(ctx, stream)
                .and_then(|view| parse_revisions(ctx, view.window()))
            {
                Ok(value) => ParsedState::Parsed(value),
                Err(error) => ParsedState::Unavailable(crate::issue_detail(error)?),
            },
        };
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
        let mut segments = pairs
            .into_iter()
            .map(|pair| -> Result<SegmentDescriptor<'a>, CodecError> {
                let meta = snapshot
                    .stream_by_id(pair.metadata)
                    .ok_or_else(|| {
                        CodecError::Malformed("RSe metadata stream handle is absent".into())
                    })
                    .and_then(|entry| snapshot.open(ctx, entry))
                    .and_then(|view| parse_meta_stream(ctx, view));
                let meta = match meta {
                    Ok(meta) => meta,
                    Err(error) => SegmentMetaState::Malformed {
                        declared: None,
                        detail: crate::issue_detail(error)?,
                    },
                };
                let bulk = snapshot
                    .stream_by_id(pair.bulk)
                    .ok_or_else(|| CodecError::Malformed("RSe bulk stream handle is absent".into()))
                    .and_then(|entry| snapshot.open(ctx, entry))
                    .and_then(|view| parse_bulk_stream(ctx, view));
                let bulk = match bulk {
                    Ok(bulk) => SegmentBulkState::Framed(bulk),
                    Err(error) => SegmentBulkState::Malformed(crate::issue_detail(error)?),
                };
                Ok(SegmentDescriptor {
                    pair,
                    registry_index: None,
                    registry_version_major: None,
                    kind: SegmentKind::Unknown("unresolved".into()),
                    identity_issues: Vec::new(),
                    meta,
                    bulk,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let ParsedState::Parsed(registry) = &registry {
            join_registry(&mut segments, registry);
        } else {
            for segment in &mut segments {
                if let SegmentMetaState::Parsed(meta) = &segment.meta {
                    segment.kind = SegmentKind::classify(&meta.display_name, None);
                }
                segment
                    .identity_issues
                    .push("segment registry is unavailable".into());
            }
        }
        frame_segment_records(ctx, &mut segments)?;
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
        let document_kind = document_kind_for_segments(&segments);
        let active_carrier = select_active_carrier(&segments, &document_kind);
        Ok(Self {
            databases: database_descriptors,
            registry,
            revisions,
            segments,
            unpaired_metadata,
            unpaired_bulk,
            active_carrier,
        })
    }

    pub(crate) fn document_kind(&self) -> DocumentKind {
        document_kind_for_segments(&self.segments)
    }
}

fn document_kind_for_segments(segments: &[SegmentDescriptor<'_>]) -> DocumentKind {
    let has_part = segments.iter().any(|segment| {
        matches!(
            segment.kind,
            SegmentKind::PmBRep
                | SegmentKind::PmDc
                | SegmentKind::PmGraphics
                | SegmentKind::PmApp
                | SegmentKind::PmBrowser
                | SegmentKind::PmResult
        )
    });
    let has_assembly = segments.iter().any(|segment| {
        matches!(
            segment.kind,
            SegmentKind::AmDc
                | SegmentKind::AmBRep
                | SegmentKind::AmGraphics
                | SegmentKind::AmApp
                | SegmentKind::AmBrowser
                | SegmentKind::AmRx
        )
    });
    match (has_part, has_assembly) {
        (true, false) => DocumentKind::Part,
        (false, true) => DocumentKind::Assembly,
        (true, true) => DocumentKind::Unknown("mixed_part_assembly".into()),
        (false, false) => DocumentKind::Unknown("unknown".into()),
    }
}

fn join_registry(segments: &mut [SegmentDescriptor<'_>], registry: &SegmentRegistry) {
    for segment in segments {
        let SegmentMetaState::Parsed(meta) = &segment.meta else {
            segment
                .identity_issues
                .push("segment metadata is unavailable".into());
            continue;
        };
        let matches = registry
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.segment_id == meta.segment_id)
            .collect::<Vec<_>>();
        let [(index, entry)] = matches.as_slice() else {
            segment.identity_issues.push(if matches.is_empty() {
                "metadata segment id is absent from the registry".into()
            } else {
                "metadata segment id is duplicated in the registry".into()
            });
            segment.kind = SegmentKind::classify(&meta.display_name, None);
            continue;
        };
        segment.registry_index = Some(*index);
        segment.registry_version_major = Some(entry.version.major);
        segment.kind = SegmentKind::classify(&entry.display_name, Some(&entry.type_name));
        if entry.display_name != meta.display_name {
            segment.identity_issues.push(format!(
                "registry display name {:?} differs from metadata display name {:?}",
                entry.display_name, meta.display_name
            ));
        }
    }
}

fn parse_bulk_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
) -> Result<SegmentBulk<'a>, CodecError> {
    let bytes = source.window();
    let header = bytes
        .get(..envelope::LEN)
        .ok_or_else(|| CodecError::Malformed("truncated RSe bulk envelope".into()))?;
    if bytes.len() == header.len() {
        return Err(CodecError::Malformed(
            "RSe bulk envelope has no compressed member".into(),
        ));
    }
    let mut prefix = [0; 16];
    prefix.copy_from_slice(&header[envelope::PREFIX..envelope::FORM]);
    let form = BulkForm(View::u16_le_at(header, envelope::FORM).expect("18-byte bulk header"));
    let compressed = source
        .child(source.start() + header.len(), source.end())
        .ok_or_else(|| CodecError::Malformed("RSe bulk member range is invalid".into()))?;
    let expanded = inflate_zlib_exact(ctx, compressed)?;
    Ok(SegmentBulk {
        prefix,
        form,
        compressed,
        expanded,
        records: None,
    })
}

fn frame_segment_records<'a>(
    ctx: &DecodeContext<'a>,
    segments: &mut [SegmentDescriptor<'a>],
) -> Result<(), CodecError> {
    for segment in segments {
        let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
            continue;
        };
        let expanded = bulk.expanded;
        let result = match (&segment.meta, segment.registry_version_major) {
            (SegmentMetaState::Parsed(meta), Some(version)) => {
                frame_bulk_records(ctx, expanded, &meta.tables, version)
            }
            (SegmentMetaState::Parsed(_), None) => Err(CodecError::Malformed(
                "RSe record framing requires the segment registry version".into(),
            )),
            _ => Err(CodecError::Malformed(
                "RSe record framing requires parsed segment metadata".into(),
            )),
        };
        bulk.records = Some(match result {
            Ok(records) => RecordFrameState::Framed(records),
            Err(error) => RecordFrameState::Unavailable(crate::issue_detail(error)?),
        });
    }
    Ok(())
}

/// Reads one `RSe` metadata stream, keeping its declaration through failure.
///
/// The marker and version are read first and then never lost: a body that
/// fails after them is [`SegmentMetaState::Malformed`] carrying the
/// declaration, and only a failure inside those two fields leaves the stream
/// with no declaration to report. Charging the dialect from the declaration a
/// failed parse actually read is what keeps the loss and the report from
/// disagreeing about the same bytes.
fn parse_meta_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
) -> Result<SegmentMetaState<'a>, CodecError> {
    let mut cursor = MetaCursor::new(source);
    let marker = cursor.length_prefixed_utf8("marker")?;
    let version = cursor.u16("version")?;
    let declared = MetaStreamDeclaration { marker, version };
    // The marker and version are a declaration, never a gate: the version-8
    // grammar is attempted on every stream, and a body that does not obey it is
    // `Malformed` with the declaration intact.
    match parse_meta_stream_v8(ctx, source, cursor, declared.clone()) {
        Ok(meta) => Ok(SegmentMetaState::Parsed(Box::new(meta))),
        Err(error) => Ok(SegmentMetaState::Malformed {
            declared: Some(declared),
            detail: crate::issue_detail(error)?,
        }),
    }
}

fn parse_meta_stream_v8<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    mut cursor: MetaCursor<'a>,
    declared: MetaStreamDeclaration,
) -> Result<SegmentMeta<'a>, CodecError> {
    let header_values = cursor.u16_array("header values")?;
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
        .child(cursor.position(), source.end())
        .ok_or_else(|| CodecError::Malformed("RSe metadata body range is invalid".into()))?;
    let body = inflate_zlib_exact(ctx, compressed)?;
    let tables = parse_meta_tables(ctx, body)?;
    Ok(SegmentMeta {
        declared,
        header_values,
        display_name,
        segment_id,
        state_words,
        created,
        modified,
        body_form,
        body,
        tables,
    })
}

pub(crate) fn fuzz_meta_stream(ctx: &DecodeContext<'_>, source: View<'_>) {
    let _ = parse_meta_stream(ctx, source);
}

pub(crate) fn fuzz_bulk_stream(ctx: &DecodeContext<'_>, source: View<'_>) {
    let _ = parse_bulk_stream(ctx, source);
}

struct MetaCursor<'a> {
    source: View<'a>,
}

impl<'a> MetaCursor<'a> {
    const fn new(source: View<'a>) -> Self {
        Self { source }
    }

    fn remaining(&self) -> usize {
        self.source.remaining()
    }

    fn position(&self) -> usize {
        self.source.position()
    }

    fn take(&mut self, len: usize, what: &'static str) -> Result<&'a [u8], CodecError> {
        Ok(self
            .source
            .req_take(len)
            .map_err(|error| error.during(what))?)
    }

    fn u8(&mut self, what: &'static str) -> Result<u8, CodecError> {
        Ok(self.source.req_u8().map_err(|error| error.during(what))?)
    }

    fn u16(&mut self, what: &'static str) -> Result<u16, CodecError> {
        Ok(self
            .source
            .req_u16_le()
            .map_err(|error| error.during(what))?)
    }

    fn u32(&mut self, what: &'static str) -> Result<u32, CodecError> {
        Ok(self
            .source
            .req_u32_le()
            .map_err(|error| error.during(what))?)
    }

    fn u32_array<const N: usize>(&mut self, what: &'static str) -> Result<[u32; N], CodecError> {
        let mut values = [0; N];
        for value in &mut values {
            *value = self.u32(what)?;
        }
        Ok(values)
    }

    fn u16_array<const N: usize>(&mut self, what: &'static str) -> Result<[u16; N], CodecError> {
        let mut values = [0; N];
        for value in &mut values {
            *value = self.u16(what)?;
        }
        Ok(values)
    }

    fn length(&mut self, what: &'static str, width: usize) -> Result<usize, CodecError> {
        let count = usize::try_from(self.u32(what)?)
            .map_err(|_| CodecError::malformed(format_args!("RSe metadata {what} is too large")))?;
        count.checked_mul(width).ok_or_else(|| {
            CodecError::malformed(format_args!("RSe metadata {what} length overflows"))
        })
    }

    fn length_prefixed_utf8(&mut self, what: &'static str) -> Result<String, CodecError> {
        let len = self.length(what, 1)?;
        if len > 256 {
            return Err(CodecError::malformed(format_args!(
                "RSe metadata {what} exceeds 256 bytes"
            )));
        }
        let bytes = self.take(len, what)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| CodecError::malformed(format_args!("RSe metadata {what} is not UTF-8")))
    }

    fn length_prefixed_utf16(&mut self, what: &'static str) -> Result<String, CodecError> {
        let len = self.length(what, 2)?;
        if len > 8_192 {
            return Err(CodecError::malformed(format_args!(
                "RSe metadata {what} exceeds 4096 UTF-16 units"
            )));
        }
        self.source
            .utf16_le(len / 2)
            .ok_or_else(|| CodecError::malformed(format_args!("RSe metadata {what} is not UTF-16")))
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
            parse_meta_stream(&ctx, root).expect("synthetic metadata stream parses")
        else {
            panic!("version-eight metadata state")
        };
        assert_eq!(meta.declared.version, 8);
        assert_eq!(meta.header_values, [1, 0, 2, 0, 3, 0, 4, 0]);
        assert_eq!(meta.display_name, "PmBRepSegment");
        assert_eq!(meta.segment_id, [0x5a; 16]);
        assert_eq!(meta.state_words, [5, 6, 7]);
        assert_eq!(meta.tables.blocks.len(), 2);
        assert_eq!(meta.tables.types.len(), 1);
    }

    #[test]
    fn meta_stream_v8_rejects_a_zlib_suffix() {
        let bytes = meta_fixture(true);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic metadata stream fits policy");
        let state = parse_meta_stream(&ctx, root).expect("the declaration reads before the body");
        let SegmentMetaState::Malformed { declared, detail } = state else {
            panic!("a zlib suffix fails after the declaration")
        };
        assert_eq!(
            declared,
            Some(MetaStreamDeclaration {
                marker: MetaStreamDeclaration::VERIFIED_MARKER.into(),
                version: MetaStreamDeclaration::VERIFIED_VERSION,
            }),
            "a body failure keeps the declaration the stream did read"
        );
        assert!(!detail.is_empty());
    }

    #[test]
    fn bulk_stream_frames_prefix_form_and_exact_zlib_member() {
        let bytes = bulk_fixture(false);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic bulk stream fits policy");
        assert_eq!(&bytes[..16], &[0x3c; 16]);
        assert_eq!(
            u16::from_le_bytes(bytes[16..18].try_into().expect("planted form")),
            0x0104
        );
        assert!(bytes.len() > 18);
        let bulk = parse_bulk_stream(&ctx, root).expect("synthetic bulk stream parses");
        assert_eq!(bulk.prefix.len(), 16);
        assert_eq!(bulk.prefix, [0x3c; 16]);
        assert_eq!(bulk.form.value(), 0x0104);
        assert_eq!(bulk.expanded.window(), b"framed bulk records");
    }

    #[test]
    fn bulk_stream_rejects_a_truncated_envelope() {
        let bytes = vec![0x3c; 17];
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("truncated envelope fits policy");
        assert!(parse_bulk_stream(&ctx, root).is_err());
    }

    #[test]
    fn bulk_stream_rejects_a_suffix_after_the_exact_zlib_member() {
        let bytes = bulk_fixture(true);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic bulk stream fits policy");
        assert!(parse_bulk_stream(&ctx, root).is_err());
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
            .write_all(&crate::records::synthetic_meta_table_body())
            .expect("write synthetic zlib body");
        bytes.extend_from_slice(&encoder.finish().expect("finish synthetic zlib body"));
        if suffix {
            bytes.push(0);
        }
        bytes
    }

    fn bulk_fixture(suffix: bool) -> Vec<u8> {
        let mut bytes = vec![0x3c; 16];
        bytes.extend_from_slice(&0x0104_u16.to_le_bytes());
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(b"framed bulk records")
            .expect("write synthetic bulk member");
        bytes.extend_from_slice(&encoder.finish().expect("finish synthetic bulk member"));
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
