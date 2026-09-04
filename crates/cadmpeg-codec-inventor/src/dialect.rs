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
//! grammars cannot frame degrades to `ParsedState::Unavailable` or
//! `SegmentMetaState::Malformed` with its own issue record, which is a
//! structural outcome. So the loss message here states a grammar that was
//! actually applied, and a foreign declaration that parses is still unverified:
//! [`SegmentMetaState::declaration`] reports what the stream said, not what the
//! parse used.
//!
//! [`SegmentMetaState::declaration`]: crate::rse::SegmentMetaState::declaration

use std::collections::BTreeMap;

use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::dialect::{DialectId, DialectMatch, Grammar};
use cadmpeg_ir::report::LossNote;

use crate::container::InventorContainer;
use crate::database::RseSchema;
use crate::kernel::{ActiveCarrierState, KernelFamily};
use crate::loss::InventorLossCode;
use crate::rse::{MetaStreamDeclaration, ParsedState};

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

/// Joins declaration values into one `declared` entry.
fn join(values: impl IntoIterator<Item = String>) -> String {
    values.into_iter().collect::<Vec<_>>().join(",")
}

/// One row of `docs/dialects.toml` under the `inventor` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InventorDialect {
    /// `RSeDb` schema 31 and `RSe` Meta Stream version 8, both declared.
    Cfb3Rse31Meta8,
    /// The mandatory totality row: any other declaration, and
    /// the absence of one. Admitted and degraded, never refused.
    Unknown,
}

impl InventorDialect {
    /// Every dialect identity this enum can name.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 2] = [Self::Cfb3Rse31Meta8, Self::Unknown];

    /// The registry-generated id for this variant.
    pub(crate) const fn id(self) -> DialectId {
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
    pub(crate) fn of(container: &InventorContainer<'_>) -> Self {
        let mut schemas = container
            .rse
            .databases
            .iter()
            .filter_map(|descriptor| descriptor.declared_schema)
            .collect::<Vec<_>>();
        schemas.sort_unstable_by_key(|schema| schema.value());
        schemas.dedup();
        let mut unframed_schemas = container
            .rse
            .databases
            .iter()
            .filter(|descriptor| matches!(descriptor.state, ParsedState::Unavailable(_)))
            .filter_map(|descriptor| descriptor.declared_schema)
            .collect::<Vec<_>>();
        unframed_schemas.sort_unstable_by_key(|schema| schema.value());
        unframed_schemas.dedup();
        let mut meta_streams = container
            .rse
            .segments
            .iter()
            .filter_map(|segment| segment.meta.declaration())
            .collect::<Vec<_>>();
        meta_streams.sort();
        meta_streams.dedup();
        let mut unframed_meta_streams = container
            .rse
            .segments
            .iter()
            .filter_map(|segment| match &segment.meta {
                crate::rse::SegmentMetaState::Malformed {
                    declared: Some(declared),
                    ..
                } => Some(declared.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        unframed_meta_streams.sort();
        unframed_meta_streams.dedup();
        Self {
            cfb_major_version: container.snapshot.major_version(),
            schemas,
            unframed_schemas,
            meta_streams,
            unframed_meta_streams,
        }
    }

    /// Evaluate identity and admission once from the parsed facts.
    pub(crate) fn classify(&self) -> DialectMatch {
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
        declared.insert(
            DECLARED_CFB_MAJOR_VERSION.into(),
            self.cfb_major_version.to_string(),
        );
        if !self.schemas.is_empty() {
            declared.insert(
                DECLARED_RSE_DB_SCHEMA.into(),
                join(self.schemas.iter().map(|schema| schema.value().to_string())),
            );
        }
        if !self.meta_streams.is_empty() {
            declared.insert(
                DECLARED_META_STREAM_MARKER.into(),
                join(
                    self.meta_streams
                        .iter()
                        .map(|declared| declared.marker.clone()),
                ),
            );
            declared.insert(
                DECLARED_META_STREAM_VERSION.into(),
                join(
                    self.meta_streams
                        .iter()
                        .map(|declared| declared.version.to_string()),
                ),
            );
        }
        if admitted {
            DialectMatch::admitted(dialect.id())
        } else {
            DialectMatch::unverified(
                dialect.id(),
                Grammar::of(&InventorDialect::Cfb3Rse31Meta8.id()),
            )
        }
        .with_declared(declared)
    }

    /// The loss charged when the document's declarations do not select the
    /// grammar this codec read it with.
    fn unverified_loss(&self) -> LossNote {
        let mut reasons = Vec::new();
        if !self.unframed_schemas.is_empty() {
            reasons.push(format!(
                "RSe database schema {} is declared but its body does not frame under the schema-31 grammar",
                join(
                    self.unframed_schemas
                        .iter()
                        .map(|schema| schema.value().to_string())
                )
            ));
        }
        if self.schemas.is_empty() {
            reasons.push("no RSe database stream declares a schema".to_owned());
        } else {
            let foreign = self
                .schemas
                .iter()
                .copied()
                .filter(|schema| *schema != RseSchema::SCHEMA_31)
                .collect::<Vec<_>>();
            if !foreign.is_empty() {
                reasons.push(format!(
                    "RSe database schema {} is declared",
                    join(foreign.iter().map(|schema| schema.value().to_string()))
                ));
            }
        }
        if !self.unframed_meta_streams.is_empty() {
            reasons.push(format!(
                "RSe segment metadata marker {} version {} is declared but its body does not frame under the version-8 grammar",
                join(
                    self.unframed_meta_streams
                        .iter()
                        .map(|declared| format!("{:?}", declared.marker))
                ),
                join(
                    self.unframed_meta_streams
                        .iter()
                        .map(|declared| declared.version.to_string())
                )
            ));
        }
        if self.meta_streams.is_empty() {
            reasons.push("no RSe segment metadata stream declares a marker and version".to_owned());
        } else {
            let foreign = self
                .meta_streams
                .iter()
                .filter(|declared| !declared.is_verified())
                .collect::<Vec<_>>();
            if !foreign.is_empty() {
                reasons.push(format!(
                    "RSe segment metadata marker {} version {} is declared",
                    join(
                        foreign
                            .iter()
                            .map(|declared| format!("{:?}", declared.marker))
                    ),
                    join(foreign.iter().map(|declared| declared.version.to_string()))
                ));
            }
        }
        InventorLossCode::SourceDialectUnverified.note(format!(
            "{}; this decode applied the only Inventor grammars this codec implements — RSe \
             database schema {} and RSe segment metadata marker {:?} version {} — to those \
             streams, and what they did not frame is reported as an unavailable stream with its \
             own issue record",
            reasons.join("; "),
            RseSchema::SCHEMA_31.value(),
            MetaStreamDeclaration::VERIFIED_MARKER,
            MetaStreamDeclaration::VERIFIED_VERSION
        ))
    }
}

/// The dialect-unverified loss required by a classified admission.
///
/// Presence is derived from `matched`; recovery evidence supplies only the
/// message detail and cannot independently select whether a loss exists.
pub(crate) fn dialect_loss(matched: &DialectMatch, recovery: &DialectRecovery) -> Option<LossNote> {
    (!matches!(
        matched.admission(),
        cadmpeg_core::dialect::Admission::Admitted
    ))
    .then(|| recovery.unverified_loss())
}

/// The `acis:` kernel-layer match for one parsed active carrier.
fn kernel_layer(
    family: KernelFamily,
    header: &cadmpeg_asm::kernel_header::KernelHeader,
) -> DialectMatch {
    let header = match family {
        KernelFamily::Asm => cadmpeg_asm::dialect::KernelHeaderRef::Asm(header),
        KernelFamily::Acis => cadmpeg_asm::dialect::KernelHeaderRef::Acis(header),
    };
    cadmpeg_asm::dialect::classify(header)
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
pub(crate) fn kernel_layer_for_state(state: &ActiveCarrierState<'_>) -> Option<DialectMatch> {
    match state {
        ActiveCarrierState::Selected(carrier) => Some(carrier.header.as_ref().map_or_else(
            |_| unknown_kernel_layer(),
            |header| kernel_layer(carrier.family, header),
        )),
        ActiveCarrierState::NotApplicable | ActiveCarrierState::Unavailable(_) => None,
    }
}

/// The complete host and optional kernel identity reported by both inspection
/// and decode.
pub(crate) fn layers(primary: DialectMatch, carrier: &ActiveCarrierState<'_>) -> DialectLayers {
    let mut layers = DialectLayers::of(primary);
    if let Some(kernel) = kernel_layer_for_state(carrier) {
        let _ = layers.insert(kernel);
    }
    layers
}

/// The recovery loss the kernel layer charges, if it recovered.
pub(crate) fn kernel_dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    match matched.admission() {
        cadmpeg_core::dialect::Admission::Refused
            if matched.format() == cadmpeg_asm::dialect::FORMAT =>
        {
            Some(InventorLossCode::KernelCarrierUnparseable.note(
                "the selected kernel carrier did not expose a parseable ACIS or ASM header; its native records remain retained",
            ))
        }
        _ => cadmpeg_asm::dialect::unverified_message("the active kernel carrier", matched)
            .map(|message| InventorLossCode::KernelDialectUnverified.note(message)),
    }
}

#[cfg(test)]
mod tests;
