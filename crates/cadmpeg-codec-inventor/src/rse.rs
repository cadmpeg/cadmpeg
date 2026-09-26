// SPDX-License-Identifier: Apache-2.0
//! `RSe` storage navigation and stable governing types.

use std::collections::BTreeMap;

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot, CompoundStreamId};
use cadmpeg_container::compression::inflate_zlib_exact;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::IdentityKey;

use crate::database::{
    parse_database, parse_registry, parse_revisions, DatabaseHeader, RevisionTable, RseDatabase,
    RseSchema, SegmentRegistry,
};
use crate::kernel::{select_active_carrier, ActiveCarrierState};
use crate::layout::bulk_envelope as envelope;
use crate::record_issue::{admit_formatted, admit_issue_detail};
use crate::records::{frame_bulk_records, parse_meta_tables, MetaTables, RseRecordTable};

fn rse_issue_detail(ctx: &DecodeContext<'_>, error: CodecError) -> Result<String, CodecError> {
    if matches!(error, CodecError::ResourceLimit(_)) {
        return Err(error);
    }
    admit_issue_detail(ctx, &error, "retain RSe issue detail")?;
    crate::issue_detail(error)
}

/// A validated `V<n>` storage-band number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StorageBand(u32);

impl StorageBand {
    fn parse(component: &str) -> Option<Self> {
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
pub(crate) struct SegmentToken(IdentityKey);

/// Which of the two `RSe` streams a segment name introduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentPrefix {
    Metadata,
    Bulk,
}

impl SegmentToken {
    fn parse(
        ctx: &DecodeContext<'_>,
        name: &str,
    ) -> Result<Option<(SegmentPrefix, Self)>, CodecError> {
        let Some((prefix, token)) = name.split_at_checked(1) else {
            return Ok(None);
        };
        let prefix = match prefix.chars().next() {
            Some('M') => SegmentPrefix::Metadata,
            Some('B') => SegmentPrefix::Bulk,
            _ => return Ok(None),
        };
        if token.is_empty() || token.contains('#') || token.chars().any(char::is_whitespace) {
            return Ok(None);
        }
        ctx.charge_retained(token.len() as u64, "retain RSe segment token")?;
        let Ok(token) = IdentityKey::try_new(token) else {
            return Ok(None);
        };
        Ok(Some((prefix, Self(token))))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// This token as an identity key.
    pub(crate) fn key(&self) -> &IdentityKey {
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
    Unresolved,
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegistryJoin {
    pub(crate) version_major: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DocumentKind {
    Part,
    Assembly,
    Drawing,
    Presentation,
    Mixed,
    Unknown,
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
            Self::Mixed => "mixed_part_assembly",
            Self::Unknown => "unknown",
        }
    }
}

impl SegmentKind {
    fn classify(
        ctx: &DecodeContext<'_>,
        display_name: &str,
        type_name: Option<&str>,
    ) -> Result<Self, CodecError> {
        let kind = match type_name {
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
            Some(type_name) => {
                ctx.charge_retained(type_name.len() as u64, "retain RSe unknown segment kind")?;
                Self::Unknown(type_name.into())
            }
            None => {
                ctx.charge_retained(display_name.len() as u64, "retain RSe unknown segment kind")?;
                Self::Unknown(display_name.into())
            }
        };
        Ok(kind)
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
            Self::Unresolved => "unresolved",
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
pub(crate) struct SegmentDescriptor<'a, B = SegmentBulk<'a>> {
    pub(crate) pair: SegmentPair,
    pub(crate) registry: Option<RegistryJoin>,
    pub(crate) kind: SegmentKind,
    pub(crate) identity_issues: Vec<String>,
    pub(crate) meta: SegmentMetaState<'a>,
    pub(crate) bulk: SegmentBulkState<B>,
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
    pub(crate) records: RecordFrameState<'a>,
}

#[derive(Debug)]
struct BulkEnvelope<'a> {
    prefix: [u8; 16],
    form: BulkForm,
    compressed: View<'a>,
    expanded: View<'a>,
}

#[derive(Debug)]
pub(crate) enum RecordFrameState<'a> {
    Framed(RseRecordTable<'a>),
    Unavailable(String),
}

#[derive(Debug)]
pub(crate) enum SegmentBulkState<B> {
    Framed(B),
    Malformed(String),
}

#[derive(Debug)]
pub(crate) enum ParsedState<T> {
    Absent,
    Parsed(T),
    Unavailable(String),
}

#[derive(Debug)]
pub(crate) enum DatabaseState {
    Parsed(RseDatabase),
    Unframed { schema: RseSchema, detail: String },
    Unreadable(String),
}

#[derive(Debug)]
pub(crate) struct DatabaseDescriptor {
    pub(crate) band: StorageBand,
    pub(crate) stream: CompoundStreamId,
    pub(crate) state: DatabaseState,
}

impl DatabaseDescriptor {
    pub(crate) fn declared_schema(&self) -> Option<RseSchema> {
        match &self.state {
            DatabaseState::Parsed(database) => Some(database.schema),
            DatabaseState::Unframed { schema, .. } => Some(*schema),
            DatabaseState::Unreadable(_) => None,
        }
    }

    pub(crate) fn issue_detail(&self) -> Option<String> {
        match &self.state {
            DatabaseState::Parsed(_) => None,
            DatabaseState::Unframed { schema, detail } => {
                Some(DatabaseHeader::unframed_detail(*schema, detail))
            }
            DatabaseState::Unreadable(detail) => Some(detail.clone()),
        }
    }
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
                ctx.charge_collection_items(1, "index RSe database stream")?;
                databases.push((band, stream.id()));
                continue;
            }
            let Some(name) = direct_rse_child(path) else {
                continue;
            };
            let Some((prefix, token)) = SegmentToken::parse(ctx, name)? else {
                continue;
            };
            match prefix {
                SegmentPrefix::Metadata => {
                    if !metadata.contains_key(&token) {
                        ctx.charge_collection_items(1, "index RSe metadata stream")?;
                    }
                    metadata.insert(token, stream.id());
                }
                SegmentPrefix::Bulk => {
                    if !bulk.contains_key(&token) {
                        ctx.charge_collection_items(1, "index RSe bulk stream")?;
                    }
                    bulk.insert(token, stream.id());
                }
            }
        }
        databases.sort_by_key(|(band, _)| *band);
        ctx.charge_collection_items(databases.len() as u64, "admit RSe database descriptors")?;
        let mut database_descriptors = Vec::with_capacity(databases.len());
        for (band, stream_id) in databases {
            let state = match snapshot.stream_by_id(stream_id) {
                Some(stream) => match snapshot
                    .open(ctx, stream)
                    .and_then(|view| parse_database(ctx, view.window()))
                {
                    Ok(DatabaseHeader::Supported(database)) => DatabaseState::Parsed(database),
                    Ok(DatabaseHeader::Unframed { schema, detail }) => {
                        DatabaseState::Unframed { schema, detail }
                    }
                    Err(error) => DatabaseState::Unreadable(rse_issue_detail(ctx, error)?),
                },
                None => {
                    ctx.charge_retained(
                        "RSe database stream handle is absent".len() as u64,
                        "retain RSe missing database detail",
                    )?;
                    DatabaseState::Unreadable("RSe database stream handle is absent".into())
                }
            };
            database_descriptors.push(DatabaseDescriptor {
                band,
                stream: stream_id,
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
                Err(error) => ParsedState::Unavailable(rse_issue_detail(ctx, error)?),
            },
        };
        let revisions = match snapshot.stream("RSeStorage/RSeDbRevisionInfo") {
            None => ParsedState::Absent,
            Some(stream) => match snapshot
                .open(ctx, stream)
                .and_then(|view| parse_revisions(ctx, view.window()))
            {
                Ok(value) => ParsedState::Parsed(value),
                Err(error) => ParsedState::Unavailable(rse_issue_detail(ctx, error)?),
            },
        };
        let mut pairs = Vec::new();
        for (token, metadata_id) in &metadata {
            let Some(bulk_id) = bulk.get(token) else {
                continue;
            };
            ctx.charge_collection_items(1, "pair RSe segment streams")?;
            ctx.charge_retained(token.as_str().len() as u64, "retain RSe paired token")?;
            pairs.push(SegmentPair {
                token: token.clone(),
                metadata: *metadata_id,
                bulk: *bulk_id,
            });
        }
        ctx.charge_collection_items(pairs.len() as u64, "admit RSe segment descriptors")?;
        let mut segments = pairs
            .into_iter()
            .map(
                |pair| -> Result<SegmentDescriptor<'a, BulkEnvelope<'a>>, CodecError> {
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
                            detail: rse_issue_detail(ctx, error)?,
                        },
                    };
                    let bulk = snapshot
                        .stream_by_id(pair.bulk)
                        .ok_or_else(|| {
                            CodecError::Malformed("RSe bulk stream handle is absent".into())
                        })
                        .and_then(|entry| snapshot.open(ctx, entry))
                        .and_then(|view| parse_bulk_stream(ctx, view));
                    let bulk = match bulk {
                        Ok(bulk) => SegmentBulkState::Framed(bulk),
                        Err(error) => SegmentBulkState::Malformed(rse_issue_detail(ctx, error)?),
                    };
                    Ok(SegmentDescriptor {
                        pair,
                        registry: None,
                        kind: SegmentKind::Unresolved,
                        identity_issues: Vec::new(),
                        meta,
                        bulk,
                    })
                },
            )
            .collect::<Result<Vec<_>, _>>()?;
        if let ParsedState::Parsed(registry) = &registry {
            join_registry(ctx, &mut segments, registry)?;
        } else {
            for segment in &mut segments {
                if let SegmentMetaState::Parsed(meta) = &segment.meta {
                    segment.kind = SegmentKind::classify(ctx, &meta.display_name, None)?;
                } else {
                    segment.kind = SegmentKind::Unresolved;
                }
                push_identity_issue(
                    ctx,
                    &mut segment.identity_issues,
                    format_args!("segment registry is unavailable"),
                )?;
            }
        }
        let segments = frame_segment_records(ctx, segments)?;
        let mut unpaired_metadata = Vec::new();
        for token in metadata.keys().filter(|token| !bulk.contains_key(*token)) {
            ctx.charge_collection_items(1, "collect RSe unpaired metadata")?;
            ctx.charge_retained(
                token.as_str().len() as u64,
                "retain RSe unpaired metadata token",
            )?;
            unpaired_metadata.push(token.clone());
        }
        let mut unpaired_bulk = Vec::new();
        for token in bulk.keys().filter(|token| !metadata.contains_key(*token)) {
            ctx.charge_collection_items(1, "collect RSe unpaired bulk")?;
            ctx.charge_retained(
                token.as_str().len() as u64,
                "retain RSe unpaired bulk token",
            )?;
            unpaired_bulk.push(token.clone());
        }
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
        (true, true) => DocumentKind::Mixed,
        (false, false) => DocumentKind::Unknown,
    }
}

fn push_identity_issue(
    ctx: &DecodeContext<'_>,
    issues: &mut Vec<String>,
    detail: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "admit RSe segment identity issue")?;
    admit_formatted(ctx, detail, "retain RSe segment identity issue")?;
    issues.push(format!("{detail}"));
    Ok(())
}

fn join_registry<B>(
    ctx: &DecodeContext<'_>,
    segments: &mut [SegmentDescriptor<'_, B>],
    registry: &SegmentRegistry,
) -> Result<(), CodecError> {
    for segment in segments {
        let SegmentMetaState::Parsed(meta) = &segment.meta else {
            push_identity_issue(
                ctx,
                &mut segment.identity_issues,
                format_args!("segment metadata is unavailable"),
            )?;
            segment.kind = SegmentKind::Unresolved;
            continue;
        };
        let mut matches = registry
            .entries
            .iter()
            .filter(|entry| entry.segment_id == meta.segment_id);
        let first = matches.next();
        let second = matches.next();
        let Some(entry) = first.filter(|_| second.is_none()) else {
            let detail = if first.is_none() {
                "metadata segment id is absent from the registry"
            } else {
                "metadata segment id is duplicated in the registry"
            };
            push_identity_issue(ctx, &mut segment.identity_issues, format_args!("{detail}"))?;
            segment.kind = SegmentKind::classify(ctx, &meta.display_name, None)?;
            continue;
        };
        segment.registry = Some(RegistryJoin {
            version_major: entry.version.major,
        });
        segment.kind = SegmentKind::classify(ctx, &entry.display_name, Some(&entry.type_name))?;
        if entry.display_name != meta.display_name {
            push_identity_issue(
                ctx,
                &mut segment.identity_issues,
                format_args!(
                    "registry display name {:?} differs from metadata display name {:?}",
                    entry.display_name, meta.display_name
                ),
            )?;
        }
    }
    Ok(())
}

fn parse_bulk_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
) -> Result<BulkEnvelope<'a>, CodecError> {
    let mut header = source;
    let prefix = crate::reader::array::<{ envelope::FORM - envelope::PREFIX }>(
        &mut header,
        "bulk envelope prefix",
    )?;
    let form = BulkForm(crate::reader::u16(&mut header, "bulk envelope form")?);
    if header.remaining() == 0 {
        return Err(CodecError::Malformed(
            "RSe bulk envelope has no compressed member".into(),
        ));
    }
    let compressed = source
        .child(source.start() + envelope::LEN, source.end())
        .ok_or_else(|| CodecError::Malformed("RSe bulk member range is invalid".into()))?;
    let expanded = inflate_zlib_exact(ctx, compressed)?;
    Ok(BulkEnvelope {
        prefix,
        form,
        compressed,
        expanded,
    })
}

fn frame_segment_records<'a>(
    ctx: &DecodeContext<'a>,
    segments: Vec<SegmentDescriptor<'a, BulkEnvelope<'a>>>,
) -> Result<Vec<SegmentDescriptor<'a>>, CodecError> {
    ctx.charge_collection_items(segments.len() as u64, "admit RSe framed segments")?;
    segments
        .into_iter()
        .map(|segment| {
            let bulk = match segment.bulk {
                SegmentBulkState::Malformed(detail) => SegmentBulkState::Malformed(detail),
                SegmentBulkState::Framed(bulk) => {
                    let result = match (&segment.meta, segment.registry) {
                        (SegmentMetaState::Parsed(meta), Some(registry)) => frame_bulk_records(
                            ctx,
                            bulk.expanded,
                            &meta.tables,
                            registry.version_major,
                        ),
                        (SegmentMetaState::Parsed(_), None) => Err(CodecError::Malformed(
                            "RSe record framing requires the segment registry version".into(),
                        )),
                        _ => Err(CodecError::Malformed(
                            "RSe record framing requires parsed segment metadata".into(),
                        )),
                    };
                    let records = match result {
                        Ok(records) => RecordFrameState::Framed(records),
                        Err(error) => RecordFrameState::Unavailable(rse_issue_detail(ctx, error)?),
                    };
                    SegmentBulkState::Framed(SegmentBulk {
                        prefix: bulk.prefix,
                        form: bulk.form,
                        compressed: bulk.compressed,
                        expanded: bulk.expanded,
                        records,
                    })
                }
            };
            Ok(SegmentDescriptor {
                pair: segment.pair,
                registry: segment.registry,
                kind: segment.kind,
                identity_issues: segment.identity_issues,
                meta: segment.meta,
                bulk,
            })
        })
        .collect()
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
    let marker = cursor.length_prefixed_utf8(ctx, "marker")?;
    let version = cursor.u16("version")?;
    let declared = MetaStreamDeclaration { marker, version };
    // The marker and version are a declaration, never a gate: the version-8
    // grammar is attempted on every stream, and a body that does not obey it is
    // `Malformed` with the declaration intact.
    ctx.charge_retained(
        declared.marker.len() as u64,
        "retain RSe metadata declaration marker",
    )?;
    match parse_meta_stream_v8(ctx, source, cursor, declared.clone()) {
        Ok(meta) => {
            ctx.charge_collection_items(1, "admit RSe parsed metadata segment")?;
            Ok(SegmentMetaState::Parsed(Box::new(meta)))
        }
        Err(error) => Ok(SegmentMetaState::Malformed {
            declared: Some(declared),
            detail: rse_issue_detail(ctx, error)?,
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
    let display_name = cursor.length_prefixed_utf16(ctx, "display name")?;
    let mut segment_id = [0; 16];
    segment_id.copy_from_slice(cursor.take(16, "segment id")?);
    let state_words = cursor.u32_array("state words")?;
    let created = cursor.length_prefixed_utf8(ctx, "creation timestamp")?;
    let modified = cursor.length_prefixed_utf8(ctx, "modification timestamp")?;
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
    let _probe = parse_meta_stream(ctx, source);
}

pub(crate) fn fuzz_bulk_stream(ctx: &DecodeContext<'_>, source: View<'_>) {
    let _probe = parse_bulk_stream(ctx, source);
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
        crate::reader::take(&mut self.source, len, what)
    }

    fn u8(&mut self, what: &'static str) -> Result<u8, CodecError> {
        crate::reader::u8(&mut self.source, what)
    }

    fn u16(&mut self, what: &'static str) -> Result<u16, CodecError> {
        crate::reader::u16(&mut self.source, what)
    }

    fn u32(&mut self, what: &'static str) -> Result<u32, CodecError> {
        crate::reader::u32(&mut self.source, what)
    }

    fn u32_array<const N: usize>(&mut self, what: &'static str) -> Result<[u32; N], CodecError> {
        crate::reader::u32_array(&mut self.source, what)
    }

    fn u16_array<const N: usize>(&mut self, what: &'static str) -> Result<[u16; N], CodecError> {
        crate::reader::u16_array(&mut self.source, what)
    }

    fn length(&mut self, what: &'static str, width: usize) -> Result<usize, CodecError> {
        let count = usize::try_from(self.u32(what)?)
            .map_err(|_| CodecError::malformed(format_args!("RSe metadata {what} is too large")))?;
        count.checked_mul(width).ok_or_else(|| {
            CodecError::malformed(format_args!("RSe metadata {what} length overflows"))
        })
    }

    fn length_prefixed_utf8(
        &mut self,
        ctx: &DecodeContext<'_>,
        what: &'static str,
    ) -> Result<String, CodecError> {
        let len = self.length(what, 1)?;
        if len > 256 {
            return Err(CodecError::malformed(format_args!(
                "RSe metadata {what} exceeds 256 bytes"
            )));
        }
        let bytes = self.take(len, what)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| CodecError::malformed(format_args!("RSe metadata {what} is not UTF-8")))?;
        ctx.charge_retained(text.len() as u64, "retain RSe metadata UTF-8 field")?;
        Ok(text.to_owned())
    }

    fn length_prefixed_utf16(
        &mut self,
        ctx: &DecodeContext<'_>,
        what: &'static str,
    ) -> Result<String, CodecError> {
        let len = self.length(what, 2)?;
        if len > 8_192 {
            return Err(CodecError::malformed(format_args!(
                "RSe metadata {what} exceeds 4096 UTF-16 units"
            )));
        }
        let malformed = || CodecError::malformed(format_args!("RSe metadata {what} is not UTF-16"));
        let utf8_bytes =
            crate::reader::utf16_utf8_len(self.source, len / 2).ok_or_else(malformed)?;
        let _units = ctx.reserve_scoped(len as u64, "decode RSe metadata UTF-16 units")?;
        ctx.charge_retained(utf8_bytes as u64, "retain RSe metadata UTF-16 field")?;
        self.source.utf16_le(len / 2).ok_or_else(malformed)
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
    use std::collections::BTreeSet;
    use std::io::Write as _;

    use cadmpeg_container::compound::CompoundSnapshot;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use super::{
        parse_bulk_stream, parse_meta_stream, push_identity_issue, rse_issue_detail, MetaCursor,
        MetaStreamDeclaration, RseInventory, SegmentKind, SegmentMetaState, SegmentToken,
    };
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    fn inventory_refusal_operations(
        dimension: ResourceDimension,
        maximum_limit: u64,
    ) -> BTreeSet<&'static str> {
        let bytes = crate::test_support::test_fixtures::primary_envelope_fixture();
        inventory_refusal_operations_for(&bytes, dimension, maximum_limit)
    }

    fn inventory_refusal_operations_for(
        bytes: &[u8],
        dimension: ResourceDimension,
        maximum_limit: u64,
    ) -> BTreeSet<&'static str> {
        let arena = DecodeArena::new();
        let (setup_ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
                .expect("primary envelope fits service policy");
        let snapshot = CompoundSnapshot::new(&setup_ctx, root)
            .expect("primary envelope compound directory parses");
        let mut operations = BTreeSet::new();
        for limit in 0..=maximum_limit {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
                _ => panic!("unsupported inventory limit dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
                .expect("primary envelope fits input cap");
            if let Err(CodecError::ResourceLimit(refusal)) = RseInventory::build(&ctx, &snapshot) {
                if refusal.dimension == dimension {
                    operations.insert(refusal.operation);
                }
            }
        }
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("primary envelope fits service policy");
        RseInventory::build(&ctx, &snapshot).expect("primary envelope inventory builds");
        operations
    }

    #[test]
    fn rse_inventory_stream_collections_refuse_before_insertion() {
        let operations = inventory_refusal_operations(ResourceDimension::CollectionItems, 128);
        for operation in [
            "index RSe database stream",
            "index RSe bulk stream",
            "index RSe metadata stream",
            "admit RSe database descriptors",
            "pair RSe segment streams",
            "admit RSe segment descriptors",
            "admit RSe framed segments",
        ] {
            assert!(
                operations.contains(operation),
                "missing refusal at {operation}"
            );
        }
    }

    #[test]
    fn rse_inventory_paired_token_refuses_retained_limit_before_copy() {
        let operations = inventory_refusal_operations(ResourceDimension::RetainedBytes, 1024);
        assert!(
            operations.contains("retain RSe paired token"),
            "paired token copy must be admitted"
        );
    }

    #[test]
    fn rse_unpaired_streams_refuse_collection_and_retained_limits_before_copy() {
        const DIRECTORY_OFFSET: usize = 512;
        const DIRECTORY_ENTRY_LEN: usize = 128;
        const BULK_ENTRY: usize = 6;
        const META_ENTRY: usize = 7;

        let mut metadata_only = crate::test_support::test_fixtures::primary_envelope_fixture();
        metadata_only[DIRECTORY_OFFSET + BULK_ENTRY * DIRECTORY_ENTRY_LEN] = b'C';
        let metadata_collections = inventory_refusal_operations_for(
            &metadata_only,
            ResourceDimension::CollectionItems,
            128,
        );
        assert!(metadata_collections.contains("collect RSe unpaired metadata"));
        let metadata_retained = inventory_refusal_operations_for(
            &metadata_only,
            ResourceDimension::RetainedBytes,
            1024,
        );
        assert!(metadata_retained.contains("retain RSe unpaired metadata token"));

        let mut bulk_only = crate::test_support::test_fixtures::primary_envelope_fixture();
        bulk_only[DIRECTORY_OFFSET + META_ENTRY * DIRECTORY_ENTRY_LEN] = b'C';
        let bulk_collections =
            inventory_refusal_operations_for(&bulk_only, ResourceDimension::CollectionItems, 128);
        assert!(bulk_collections.contains("collect RSe unpaired bulk"));
        let bulk_retained =
            inventory_refusal_operations_for(&bulk_only, ResourceDimension::RetainedBytes, 1024);
        assert!(bulk_retained.contains("retain RSe unpaired bulk token"));
    }

    #[test]
    fn rse_unknown_segment_kind_refuses_before_name_copy() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits input cap");
        assert!(matches!(
            SegmentKind::classify(&ctx, "abc", None),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain RSe unknown segment kind"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
            .expect("empty root fits service policy");
        assert_eq!(
            SegmentKind::classify(&ctx, "abc", None)
                .expect("service policy admits kind")
                .label(),
            "abc"
        );
    }

    #[test]
    fn rse_identity_issue_refuses_collection_and_retained_limits_before_push() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits input cap");
        let mut issues = Vec::new();
        assert!(matches!(
            push_identity_issue(&ctx, &mut issues, format_args!("issue")),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit RSe segment identity issue"
        ));
        assert!(issues.is_empty());

        policy.limits.max_collection_items = DecodePolicy::service().limits.max_collection_items;
        policy.limits.max_retained_bytes = 4;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits input cap");
        assert!(matches!(
            push_identity_issue(&ctx, &mut issues, format_args!("issue")),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain RSe segment identity issue"
        ));
        assert!(issues.is_empty());

        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
            .expect("empty root fits service policy");
        push_identity_issue(&ctx, &mut issues, format_args!("issue"))
            .expect("service policy admits issue");
        assert_eq!(issues, ["issue"]);
    }

    #[test]
    fn rse_issue_detail_refuses_retained_limit_before_formatting() {
        let error = CodecError::Malformed("bad registry".into());
        let detail = error.to_string();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = (detail.len() - 1) as u64;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits input cap");
        assert!(matches!(
            rse_issue_detail(&ctx, error),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain RSe issue detail"
        ));

        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
            .expect("empty root fits service policy");
        assert_eq!(
            rse_issue_detail(&ctx, CodecError::Malformed("bad registry".into()))
                .expect("service policy admits detail"),
            detail
        );
    }

    #[test]
    fn segment_token_refuses_retained_limit_before_key_creation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits input cap");
        assert!(matches!(
            SegmentToken::parse(&ctx, "Mseg"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain RSe segment token"
        ));

        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
            .expect("empty root fits service policy");
        let (_, token) = SegmentToken::parse(&ctx, "Mseg")
            .expect("service policy admits token")
            .expect("valid metadata name");
        assert_eq!(token.as_str(), "seg");
    }

    #[test]
    fn invalid_segment_token_skips_without_retained_charge() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits input cap");
        assert!(SegmentToken::parse(&ctx, "M#")
            .expect("invalid name is skipped")
            .is_none());
    }

    #[test]
    fn metadata_utf8_field_refuses_retained_limit_before_copy() {
        let mut bytes = Vec::new();
        push_bytes(&mut bytes, "é".as_bytes());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("metadata field fits input cap");
        let mut cursor = MetaCursor::new(root);
        assert!(matches!(
            cursor.length_prefixed_utf8(&ctx, "marker"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain RSe metadata UTF-8 field"
        ));

        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("metadata field fits service policy");
        assert_eq!(
            MetaCursor::new(root)
                .length_prefixed_utf8(&ctx, "marker")
                .expect("valid UTF-8 field"),
            "é"
        );
    }

    #[test]
    fn metadata_utf16_field_refuses_retained_limit_before_decode() {
        let mut bytes = Vec::new();
        push_utf16(&mut bytes, "€");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 2;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("metadata field fits input cap");
        let mut cursor = MetaCursor::new(root);
        assert!(matches!(
            cursor.length_prefixed_utf16(&ctx, "display name"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain RSe metadata UTF-16 field"
        ));

        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("metadata field fits service policy");
        assert_eq!(
            MetaCursor::new(root)
                .length_prefixed_utf16(&ctx, "display name")
                .expect("valid UTF-16 field"),
            "€"
        );
    }

    #[test]
    fn metadata_utf16_units_refuse_materialized_limit_before_decode() {
        let mut bytes = Vec::new();
        push_utf16(&mut bytes, "A");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("metadata field fits input cap");
        let mut cursor = MetaCursor::new(root);
        assert!(matches!(
            cursor.length_prefixed_utf16(&ctx, "display name"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "decode RSe metadata UTF-16 units"
        ));
    }

    #[test]
    fn metadata_declaration_refuses_retained_limit_before_marker_clone() {
        let bytes = meta_fixture(false);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            (MetaStreamDeclaration::VERIFIED_MARKER.len() * 2 - 1) as u64;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("metadata stream fits input cap");
        assert!(matches!(
            parse_meta_stream(&ctx, root),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain RSe metadata declaration marker"
        ));
    }

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
    fn a_truncated_bulk_envelope_is_located_and_names_its_field() {
        for (len, expected) in [
            (10, "Truncated bulk envelope prefix at offset 0"),
            (17, "Truncated bulk envelope form at offset 16"),
        ] {
            let bytes = vec![0x3c; len];
            let arena = DecodeArena::new();
            let text =
                match DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()) {
                    Ok((ctx, root)) => match parse_bulk_stream(&ctx, root) {
                        Ok(envelope) => format!("the read succeeded with {envelope:?}"),
                        Err(CodecError::Truncated {
                            location,
                            operation,
                        }) => format!("Truncated {operation} at offset {}", location.offset),
                        Err(error) => error.to_string(),
                    },
                    Err(error) => error.to_string(),
                };
            assert_eq!(text, expected);
        }
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
