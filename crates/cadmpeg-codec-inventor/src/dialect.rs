// SPDX-License-Identifier: Apache-2.0
//! Inventor dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, registry-generated
//! [`DialectId`] constants are the boundary, [`DialectRecovery::classify`] is
//! the one construction path, and the vocabulary is closed.
//!
//! # Declarations select identity; framing selects admission
//!
//! The `RSeDb` schema and `RSe` Meta Stream marker and version select the row.
//! Framing does not participate in identity. Admission separately requires
//! every declared stream to frame under the selected grammars. A stream that
//! declares schema 31 and metadata version 8 therefore keeps
//! `inventor:cfb3-rse31-meta8` when its body is malformed, with
//! [`Admission::Unverified`](cadmpeg_core::dialect::Admission::Unverified).
//! [`dialect_loss`] returns `None` exactly when admission is
//! [`Admission::Admitted`](cadmpeg_core::dialect::Admission::Admitted).
//!
//! # The row absorbs what the codec does not gate
//!
//! Neither gate refuses. A schema other than 31 leaves the `RSeDb` stream and
//! the segment registry unavailable, and a Meta Stream other than version 8
//! leaves that segment's metadata unread; decode continues in both cases and
//! degrades. That is
//! [`Admission::Unverified`](cadmpeg_core::dialect::Admission::Unverified)
//! exactly, and `using` names `inventor:cfb3-rse31-meta8` because the schema-31
//! registry grammar and the version-8 metadata grammar are the only ones this
//! codec implements — they are the strategy it applied, in the parts it could
//! apply.
//!
//! The pinned id says `cfb3`, and the codec never tests the CFB major version:
//! the shared compound parser accepts major 3 and major 4, and neither row
//! carries a `cfb_major_version` discriminant. A CFB v4 Inventor document
//! therefore classifies as `inventor:cfb3-rse31-meta8` when its `RSe`
//! declarations are the verified ones. Ids are pinned forever, so the name
//! stays and the fact is written down here and in the registry rather than
//! silently corrected. The observed major version is reported under
//! [`DialectMatch::declared`], which is where a declaration the codec does not
//! branch on belongs.
//!
//! # Absence of a declaration is not verification
//!
//! A document with no `RSeDb` stream declares no schema, and a document with no
//! readable Meta Stream declaration declares no metadata version. Neither
//! satisfies a discriminant of `inventor:cfb3-rse31-meta8`, so both land on the
//! totality row.
//!
//! # The declaration decides the label, never whether the grammar is applied
//!
//! `database::parse_database`, `database::parse_registry`,
//! `database::parse_revisions`, and `rse::parse_meta_stream` apply the schema-31
//! and version-8 grammars to every stream, whatever it declared. A stream those
//! grammars cannot frame degrades to `DatabaseState::Unframed` /
//! `DatabaseState::Unreadable`, `ParsedState::Unavailable`, or
//! `SegmentMetaState::Malformed` with its own issue record, which is a
//! structural outcome. So the loss message here states a grammar that was
//! actually applied, and a foreign declaration that parses is still unverified:
//! [`SegmentMetaState::declaration`] reports what the stream said, not what the
//! parse used.
//!
//! [`SegmentMetaState::declaration`]: crate::rse::SegmentMetaState::declaration

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::dialect::{DialectId, DialectMatch, Grammar};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::container::InventorContainer;
use crate::database::RseSchema;
use crate::kernel::{ActiveCarrierState, KernelFamily};
use crate::loss::InventorLossCode;
use crate::record_issue::admit_formatted;
use crate::rse::{DatabaseDescriptor, DatabaseState, MetaStreamDeclaration};

include!("dialect/registry_ids.rs");

/// Key of the CFB major version in [`DialectMatch::declared`].
///
/// Evidence the codec reports and never branches on: the shared compound
/// parser admits major 3 and major 4 alike.
const DECLARED_CFB_MAJOR_VERSION: &str = "cfb_major_version";
/// Key of the `RSeDb` schema declarations in [`DialectMatch::declared`].
///
/// A document may carry several `V<n>/RSeDb` streams. The value is every
/// distinct schema they declared, ascending, separated by `,`. The key is
/// absent when no `RSeDb` stream read as far as its schema word.
const DECLARED_RSE_DB_SCHEMA: &str = "rse_db_schema";
/// Key of the `RSe` Meta Stream marker declarations in [`DialectMatch::declared`].
///
/// Every distinct marker the segment metadata streams declared, in ascending
/// order, separated by `,`. Absent when no metadata stream read as far as its
/// marker.
const DECLARED_META_STREAM_MARKER: &str = "meta_stream_marker";
/// Key of the `RSe` Meta Stream version declarations in [`DialectMatch::declared`].
///
/// Every distinct version word the segment metadata streams declared,
/// ascending, separated by `,`. Absent under the same condition as
/// [`DECLARED_META_STREAM_MARKER`].
const DECLARED_META_STREAM_VERSION: &str = "meta_stream_version";

fn retained_format(
    ctx: &DecodeContext<'_>,
    value: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    admit_formatted(ctx, value, operation)?;
    Ok(value.to_string())
}

fn u64_len(ctx: &DecodeContext<'_>, len: usize) -> Result<u64, CodecError> {
    u64::try_from(len)
        .map_err(|_| ctx.refuse_codec_limit("Inventor dialect length", u64::MAX - 1, u64::MAX))
}

fn join(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = Result<String, CodecError>>,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut parts = Vec::new();
    let mut bytes = 0_u64;
    for value in values {
        let value = value?;
        ctx.charge_collection_items(1, "collect Inventor dialect join parts")?;
        bytes = bytes
            .checked_add(u64_len(ctx, value.len())?)
            .ok_or_else(|| {
                ctx.refuse_codec_limit("Inventor dialect joined bytes", u64::MAX - 1, u64::MAX)
            })?;
        parts.push(value);
    }
    if !parts.is_empty() {
        bytes = bytes
            .checked_add(u64_len(ctx, parts.len() - 1)?)
            .ok_or_else(|| {
                ctx.refuse_codec_limit("Inventor dialect joined bytes", u64::MAX - 1, u64::MAX)
            })?;
    }
    ctx.charge_retained(bytes, operation)?;
    Ok(parts.join(","))
}

/// One row of `docs/dialects.toml` under the `inventor` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InventorDialect {
    /// `RSeDb` schema 31 and `RSe` Meta Stream version 8, both declared.
    Cfb3Rse31Meta8,
    /// The mandatory totality row: any other declaration, and
    /// the absence of one. Admitted and degraded, never refused.
    Unknown,
}

impl InventorDialect {
    /// Every dialect identity this enum can name.
    #[cfg(test)]
    const ALL: [Self; 2] = [Self::Cfb3Rse31Meta8, Self::Unknown];

    /// The registry-generated id for this variant.
    const fn id(self) -> DialectId {
        match self {
            Self::Cfb3Rse31Meta8 => INVENTOR_CFB3_RSE31_META8,
            Self::Unknown => INVENTOR_UNKNOWN,
        }
    }
}

/// The version declarations one document carries, read where the decoder reads
/// them.
///
/// Built once from the parsed container and consulted by both the admission and
/// the loss. Nothing here re-reads bytes: the `RSeDb` schema survives its own
/// rejection on [`crate::database::DatabaseHeader`], and the Meta Stream marker
/// and version survive theirs on [`crate::rse::SegmentMetaState`].
pub(crate) struct DialectRecovery {
    /// CFB major version, as the compound header declared it.
    cfb_major_version: u16,
    /// Distinct `RSeDb` schema declarations, ascending.
    schemas: Vec<RseSchema>,
    /// Declared schemas whose bodies did not frame under the schema-31 grammar.
    unframed_schemas: Vec<RseSchema>,
    /// Distinct `RSe` Meta Stream declarations, ascending.
    meta_streams: Vec<MetaStreamDeclaration>,
    /// Declared Meta Streams whose bodies did not frame under the version-8
    /// grammar.
    unframed_meta_streams: Vec<MetaStreamDeclaration>,
}

impl DialectRecovery {
    /// Collects every version declaration the decode read from `container`.
    pub(crate) fn of(
        ctx: &DecodeContext<'_>,
        container: &InventorContainer<'_>,
    ) -> Result<Self, CodecError> {
        let mut schemas = Vec::new();
        for descriptor in &container.rse.databases {
            if let Some(schema) = DatabaseDescriptor::declared_schema(descriptor) {
                ctx.charge_collection_items(1, "collect Inventor dialect schemas")?;
                schemas.push(schema);
            }
        }
        schemas.sort_unstable_by_key(|schema| schema.value());
        schemas.dedup();
        let mut unframed_schemas = Vec::new();
        for descriptor in &container.rse.databases {
            if let DatabaseState::Unframed { schema, .. } = &descriptor.state {
                ctx.charge_collection_items(1, "collect Inventor unframed dialect schemas")?;
                unframed_schemas.push(*schema);
            }
        }
        unframed_schemas.sort_unstable_by_key(|schema| schema.value());
        unframed_schemas.dedup();
        let mut meta_streams = Vec::new();
        for segment in &container.rse.segments {
            if let Some(declaration) = segment.meta.declaration(ctx)? {
                ctx.charge_collection_items(1, "collect Inventor dialect metadata declarations")?;
                meta_streams.push(declaration);
            }
        }
        meta_streams.sort();
        meta_streams.dedup();
        let mut unframed_meta_streams = Vec::new();
        for segment in &container.rse.segments {
            if let crate::rse::SegmentMetaState::Malformed {
                declared: Some(declared),
                ..
            } = &segment.meta
            {
                ctx.charge_collection_items(1, "collect Inventor unframed dialect metadata")?;
                ctx.charge_retained(
                    u64_len(ctx, declared.marker.len())?,
                    "retain Inventor unframed dialect marker",
                )?;
                unframed_meta_streams.push(declared.clone());
            }
        }
        unframed_meta_streams.sort();
        unframed_meta_streams.dedup();
        Ok(Self {
            cfb_major_version: container.snapshot.major_version(),
            schemas,
            unframed_schemas,
            meta_streams,
            unframed_meta_streams,
        })
    }

    /// Evaluate identity and admission once from the parsed facts.
    pub(crate) fn classify(&self, ctx: &DecodeContext<'_>) -> Result<DialectMatch, CodecError> {
        let declaration_count = self
            .schemas
            .len()
            .checked_add(self.meta_streams.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit("Inventor dialect declaration count", u64::MAX - 1, u64::MAX)
            })?;
        ctx.charge_work(
            u64_len(ctx, declaration_count)?,
            "classify Inventor dialect declarations",
        )?;
        let identity_verified = !self.schemas.is_empty()
            && self
                .schemas
                .iter()
                .all(|schema| *schema == RseSchema::SCHEMA_31)
            && !self.meta_streams.is_empty()
            && self
                .meta_streams
                .iter()
                .all(MetaStreamDeclaration::is_verified);
        let framing_verified =
            self.unframed_schemas.is_empty() && self.unframed_meta_streams.is_empty();
        let dialect = if identity_verified {
            InventorDialect::Cfb3Rse31Meta8
        } else {
            InventorDialect::Unknown
        };
        let admitted = identity_verified && framing_verified;
        let mut declared = BTreeMap::new();
        ctx.charge_collection_items(1, "record Inventor dialect declaration")?;
        declared.insert(
            cadmpeg_core::nonblank_const!(DECLARED_CFB_MAJOR_VERSION),
            retained_format(
                ctx,
                format_args!("{}", self.cfb_major_version),
                "retain Inventor CFB version declaration",
            )?,
        );
        if !self.schemas.is_empty() {
            ctx.charge_collection_items(1, "record Inventor dialect declaration")?;
            declared.insert(
                cadmpeg_core::nonblank_const!(DECLARED_RSE_DB_SCHEMA),
                join(
                    ctx,
                    self.schemas.iter().map(|schema| {
                        retained_format(
                            ctx,
                            format_args!("{}", schema.value()),
                            "retain Inventor RSe schema declaration part",
                        )
                    }),
                    "retain Inventor RSe schema declaration",
                )?,
            );
        }
        if !self.meta_streams.is_empty() {
            ctx.charge_collection_items(1, "record Inventor dialect declaration")?;
            declared.insert(
                cadmpeg_core::nonblank_const!(DECLARED_META_STREAM_MARKER),
                join(
                    ctx,
                    self.meta_streams.iter().map(|declared| {
                        ctx.charge_retained(
                            u64_len(ctx, declared.marker.len())?,
                            "retain Inventor metadata marker declaration part",
                        )?;
                        Ok(declared.marker.clone())
                    }),
                    "retain Inventor metadata marker declaration",
                )?,
            );
            ctx.charge_collection_items(1, "record Inventor dialect declaration")?;
            declared.insert(
                cadmpeg_core::nonblank_const!(DECLARED_META_STREAM_VERSION),
                join(
                    ctx,
                    self.meta_streams.iter().map(|declared| {
                        retained_format(
                            ctx,
                            format_args!("{}", declared.version),
                            "retain Inventor metadata version declaration part",
                        )
                    }),
                    "retain Inventor metadata version declaration",
                )?,
            );
        }
        Ok(if admitted {
            DialectMatch::admitted(dialect.id())
        } else {
            DialectMatch::unverified(
                dialect.id(),
                Grammar::of(&InventorDialect::Cfb3Rse31Meta8.id()),
            )
        }
        .with_declared(declared))
    }

    /// The loss charged when the document's declarations do not select the
    /// grammar this codec read it with.
    fn unverified_loss(&self, ctx: &DecodeContext<'_>) -> Result<LossNote, CodecError> {
        let mut reasons = Vec::new();
        if !self.unframed_schemas.is_empty() {
            ctx.charge_collection_items(1, "collect Inventor dialect reasons")?;
            let schemas = join(
                ctx,
                self.unframed_schemas.iter().map(|schema| {
                    retained_format(
                        ctx,
                        format_args!("{}", schema.value()),
                        "retain Inventor unframed schema reason part",
                    )
                }),
                "retain Inventor unframed schema reason list",
            )?;
            reasons.push(retained_format(ctx, format_args!(
                "RSe database schema {schemas} is declared but its body does not frame under the schema-31 grammar"
            ), "retain Inventor unframed schema reason")?);
        }
        if self.schemas.is_empty() {
            ctx.charge_collection_items(1, "collect Inventor dialect reasons")?;
            ctx.charge_retained(
                u64_len(ctx, "no RSe database stream declares a schema".len())?,
                "retain Inventor absent schema reason",
            )?;
            reasons.push("no RSe database stream declares a schema".to_owned());
        } else {
            ctx.charge_work(
                u64_len(ctx, self.schemas.len())?,
                "scan Inventor foreign schemas",
            )?;
            if self
                .schemas
                .iter()
                .any(|schema| *schema != RseSchema::SCHEMA_31)
            {
                ctx.charge_collection_items(1, "collect Inventor dialect reasons")?;
                let foreign = join(
                    ctx,
                    self.schemas
                        .iter()
                        .filter(|schema| **schema != RseSchema::SCHEMA_31)
                        .map(|schema| {
                            retained_format(
                                ctx,
                                format_args!("{}", schema.value()),
                                "retain Inventor foreign schema reason part",
                            )
                        }),
                    "retain Inventor foreign schema reason list",
                )?;
                reasons.push(retained_format(
                    ctx,
                    format_args!("RSe database schema {foreign} is declared"),
                    "retain Inventor foreign schema reason",
                )?);
            }
        }
        if !self.unframed_meta_streams.is_empty() {
            ctx.charge_collection_items(1, "collect Inventor dialect reasons")?;
            let markers = join(
                ctx,
                self.unframed_meta_streams.iter().map(|declared| {
                    retained_format(
                        ctx,
                        format_args!("{:?}", declared.marker),
                        "retain Inventor unframed marker reason part",
                    )
                }),
                "retain Inventor unframed marker reason list",
            )?;
            let versions = join(
                ctx,
                self.unframed_meta_streams.iter().map(|declared| {
                    retained_format(
                        ctx,
                        format_args!("{}", declared.version),
                        "retain Inventor unframed version reason part",
                    )
                }),
                "retain Inventor unframed version reason list",
            )?;
            reasons.push(retained_format(ctx, format_args!(
                "RSe segment metadata marker {markers} version {versions} is declared but its body does not frame under the version-8 grammar"
            ), "retain Inventor unframed metadata reason")?);
        }
        if self.meta_streams.is_empty() {
            ctx.charge_collection_items(1, "collect Inventor dialect reasons")?;
            ctx.charge_retained(
                u64_len(
                    ctx,
                    "no RSe segment metadata stream declares a marker and version".len(),
                )?,
                "retain Inventor absent metadata reason",
            )?;
            reasons.push("no RSe segment metadata stream declares a marker and version".to_owned());
        } else {
            ctx.charge_work(
                u64_len(ctx, self.meta_streams.len())?,
                "scan Inventor foreign metadata",
            )?;
            if self
                .meta_streams
                .iter()
                .any(|declared| !declared.is_verified())
            {
                ctx.charge_collection_items(1, "collect Inventor dialect reasons")?;
                let markers = join(
                    ctx,
                    self.meta_streams
                        .iter()
                        .filter(|declared| !declared.is_verified())
                        .map(|declared| {
                            retained_format(
                                ctx,
                                format_args!("{:?}", declared.marker),
                                "retain Inventor foreign marker reason part",
                            )
                        }),
                    "retain Inventor foreign marker reason list",
                )?;
                let versions = join(
                    ctx,
                    self.meta_streams
                        .iter()
                        .filter(|declared| !declared.is_verified())
                        .map(|declared| {
                            retained_format(
                                ctx,
                                format_args!("{}", declared.version),
                                "retain Inventor foreign version reason part",
                            )
                        }),
                    "retain Inventor foreign version reason list",
                )?;
                reasons.push(retained_format(
                    ctx,
                    format_args!(
                        "RSe segment metadata marker {markers} version {versions} is declared"
                    ),
                    "retain Inventor foreign metadata reason",
                )?);
            }
        }
        let separator_count = if reasons.is_empty() {
            0
        } else {
            reasons.len() - 1
        };
        let mut reason_bytes = separator_count.checked_mul(2).ok_or_else(|| {
            ctx.refuse_codec_limit("Inventor dialect reason bytes", u64::MAX - 1, u64::MAX)
        })?;
        for reason in &reasons {
            reason_bytes = reason_bytes.checked_add(reason.len()).ok_or_else(|| {
                ctx.refuse_codec_limit("Inventor dialect reason bytes", u64::MAX - 1, u64::MAX)
            })?;
        }
        ctx.charge_retained(
            u64_len(ctx, reason_bytes)?,
            "retain Inventor joined dialect reasons",
        )?;
        let joined_reasons = reasons.join("; ");
        ctx.charge_collection_items(1, "collect Inventor dialect loss")?;
        ctx.charge_retained(8, "retain Inventor dialect loss namespace")?;
        ctx.charge_retained(
            u64_len(ctx, InventorLossCode::SourceDialectUnverified.code().len())?,
            "retain Inventor dialect loss code",
        )?;
        Ok(
            InventorLossCode::SourceDialectUnverified.note(retained_format(
                ctx,
                format_args!(
            "{}; this decode applied the only Inventor grammars this codec implements — RSe \
             database schema {} and RSe segment metadata marker {:?} version {} — to those \
             streams, and what they did not frame is reported as an unavailable stream with its \
             own issue record",
            joined_reasons,
            RseSchema::SCHEMA_31.value(),
            MetaStreamDeclaration::VERIFIED_MARKER,
            MetaStreamDeclaration::VERIFIED_VERSION
        ),
                "retain Inventor dialect loss message",
            )?),
        )
    }
}

/// The dialect-unverified loss required by a classified admission.
///
/// Presence is derived from `matched`; recovery evidence supplies only the
/// message detail and cannot independently select whether a loss exists.
pub(crate) fn dialect_loss(
    ctx: &DecodeContext<'_>,
    matched: &DialectMatch,
    recovery: &DialectRecovery,
) -> Result<Option<LossNote>, CodecError> {
    if matches!(
        matched.admission(),
        cadmpeg_core::dialect::Admission::Admitted
    ) {
        Ok(None)
    } else {
        Ok(Some(recovery.unverified_loss(ctx)?))
    }
}

/// The `acis:` kernel-layer match for one parsed active carrier.
fn kernel_layer(
    ctx: &DecodeContext<'_>,
    family: KernelFamily,
    header: &cadmpeg_asm::kernel_header::BinaryHeader,
) -> Result<DialectMatch, CodecError> {
    ctx.charge_work(1, "classify Inventor kernel dialect")?;
    if let Some(major) = header.metadata.save_format_major() {
        ctx.charge_collection_items(1, "collect Inventor kernel declaration")?;
        admit_formatted(
            ctx,
            format_args!("{major}"),
            "retain Inventor kernel save major",
        )?;
    }
    if let Some(minor) = header.metadata.save_format_minor() {
        ctx.charge_collection_items(1, "collect Inventor kernel declaration")?;
        admit_formatted(
            ctx,
            format_args!("{minor}"),
            "retain Inventor kernel save minor",
        )?;
    }
    ctx.charge_collection_items(1, "collect Inventor kernel declaration")?;
    ctx.charge_retained(1, "retain Inventor kernel reference width")?;
    if matches!(family, KernelFamily::Acis)
        && !cadmpeg_asm::dialect::acis_band_verified(header.metadata.save_format_major())
    {
        let recovery =
            cadmpeg_asm::dialect::nearest_verified_acis(header.metadata.save_format_major());
        let grammar = recovery
            .as_str()
            .split_once(':')
            .map_or(recovery.as_str(), |(_, grammar)| grammar);
        ctx.charge_retained(
            u64_len(ctx, grammar.len())?,
            "retain Inventor kernel recovery grammar",
        )?;
    }
    let header = match family {
        KernelFamily::Asm => cadmpeg_asm::dialect::KernelHeaderRef::Asm(header),
        KernelFamily::Acis => cadmpeg_asm::dialect::KernelHeaderRef::Acis(header),
    };
    Ok(cadmpeg_asm::dialect::classify(header))
}

/// The total kernel-layer row when the active carrier header does not parse.
fn unknown_kernel_layer() -> DialectMatch {
    cadmpeg_asm::dialect::classify(cadmpeg_asm::dialect::KernelHeaderRef::Unknown)
}

/// Classifies a kernel layer only when the active carrier provides kernel
/// evidence.
///
/// A selected carrier has an ASM or ACIS signature. Failure to parse its
/// header therefore maps to the total kernel row. `Unavailable` is a
/// host-selection state: it does not prove that a kernel stream exists, so
/// manufacturing `acis:unknown` for it would turn missing carrier evidence
/// into a false kernel identity. Inspection and decode both call this
/// function and therefore make the same distinction.
fn kernel_layer_for_state(
    ctx: &DecodeContext<'_>,
    state: &ActiveCarrierState<'_>,
) -> Result<Option<DialectMatch>, CodecError> {
    match state {
        ActiveCarrierState::Selected(carrier) => match carrier.header.as_ref() {
            Ok(header) => Ok(Some(kernel_layer(ctx, carrier.family, header)?)),
            Err(_) => Ok(Some(unknown_kernel_layer())),
        },
        ActiveCarrierState::NotApplicable | ActiveCarrierState::Unavailable(_) => Ok(None),
    }
}

/// The complete host and optional kernel identity reported by both inspection
/// and decode.
pub(crate) fn layers(
    ctx: &DecodeContext<'_>,
    primary: &DialectMatch,
    carrier: &ActiveCarrierState<'_>,
) -> Result<DialectLayers, CodecError> {
    ctx.charge_collection_items(
        u64_len(ctx, primary.declared().len())?,
        "copy Inventor primary dialect declarations",
    )?;
    for value in primary.declared().values() {
        ctx.charge_retained(
            u64_len(ctx, value.len())?,
            "copy Inventor primary dialect value",
        )?;
    }
    if let Some(instance) = primary.instance() {
        ctx.charge_retained(
            u64_len(ctx, instance.len())?,
            "copy Inventor primary dialect instance",
        )?;
    }
    if let cadmpeg_core::dialect::Admission::Unverified { using } = primary.admission() {
        ctx.charge_retained(
            u64_len(ctx, using.as_str().len())?,
            "copy Inventor primary dialect grammar",
        )?;
    }
    let mut layers = DialectLayers::of(primary.clone());
    if let Some(kernel) = kernel_layer_for_state(ctx, carrier)? {
        ctx.charge_collection_items(1, "collect Inventor kernel dialect layer")?;
        layers.insert(kernel).map_err(|rejected| {
            CodecError::malformed(format_args!(
                "duplicate Inventor dialect layer: {rejected:?}"
            ))
        })?;
    }
    Ok(layers)
}

/// The recovery loss the kernel layer charges, if it recovered.
pub(crate) fn kernel_dialect_loss(
    ctx: &DecodeContext<'_>,
    matched: &DialectMatch,
) -> Result<Option<LossNote>, CodecError> {
    match matched.admission() {
        cadmpeg_core::dialect::Admission::Refused
            if matched.format() == cadmpeg_asm::dialect::FORMAT =>
        {
            ctx.charge_collection_items(1, "collect Inventor kernel dialect loss")?;
            ctx.charge_retained(8, "retain Inventor kernel loss namespace")?;
            ctx.charge_retained(
                u64_len(ctx, InventorLossCode::KernelCarrierUnparseable.code().len())?,
                "retain Inventor kernel loss code",
            )?;
            ctx.charge_retained(
                u64_len(ctx, "the selected kernel carrier did not expose a parseable ACIS or ASM header; its native records remain retained".len())?,
                "retain Inventor kernel unparseable loss message",
            )?;
            Ok(Some(InventorLossCode::KernelCarrierUnparseable.note(
                "the selected kernel carrier did not expose a parseable ACIS or ASM header; its native records remain retained",
            )))
        }
        cadmpeg_core::dialect::Admission::Unverified { .. }
        | cadmpeg_core::dialect::Admission::Residual
            if matched.format() == cadmpeg_asm::dialect::FORMAT =>
        {
            ctx.charge_collection_items(1, "collect Inventor kernel dialect loss")?;
            ctx.charge_retained(8, "retain Inventor kernel loss namespace")?;
            ctx.charge_retained(
                u64_len(ctx, InventorLossCode::KernelDialectUnverified.code().len())?,
                "retain Inventor kernel loss code",
            )?;
            let declared = match (
                matched.declared().get("save_format_major"),
                matched.declared().get("save_format_minor"),
            ) {
                (Some(major), Some(minor)) => retained_format(
                    ctx,
                    format_args!("save format {major}.{minor}"),
                    "retain Inventor kernel declared save format",
                )?,
                (Some(major), None) => retained_format(
                    ctx,
                    format_args!("save format major {major}"),
                    "retain Inventor kernel declared save format",
                )?,
                (None, _) => {
                    ctx.charge_retained(14, "retain Inventor kernel declared save format")?;
                    "no save format".to_owned()
                }
            };
            let message = match matched.using() {
                Some(using) => retained_format(
                    ctx,
                    format_args!("the active kernel carrier declares {declared}, which no verified Spatial ACIS band declares; its records were read with the grammar `{using}` declares, and what they decoded is reported as it decoded"),
                    "retain Inventor kernel dialect loss message",
                )?,
                None => retained_format(
                    ctx,
                    format_args!("the active kernel carrier declares {declared}; its recovery names no declared save-band grammar as a substitute"),
                    "retain Inventor kernel dialect loss message",
                )?,
            };
            Ok(Some(
                InventorLossCode::KernelDialectUnverified.note(message),
            ))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests;
