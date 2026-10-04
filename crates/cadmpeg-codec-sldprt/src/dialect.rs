// SPDX-License-Identifier: Apache-2.0
//! SLDPRT dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, registry-generated
//! [`DialectId`] constants are the boundary, [`classify_layers`] is the
//! one construction path, and the vocabulary is closed.
//!
//! # The axis is `swVersion`, and it is the only one that selects a layout
//!
//! `[format.sldprt]` declares `complete = false`: the vendor's release space
//! is not enumerable from any published document, so the rows are grammar
//! classes plus the mandatory `unknown` row.
//!
//! Two document-wide discriminants exist and only one is a grammar boundary.
//! The container branch — compound file versus native block envelope
//! ([`crate::container::looks_like_sldprt`]) — is the pre-parse dispatch, but
//! the outer version word it yields is never compared to anything: `scan` hard
//! -sets it to `0` on the compound-file branch and reads a big-endian `u32` at
//! offset 4 on the native branch, and both values only reach the
//! `outer_version` attribute. It is provenance, not evidence, and for that
//! reason it is not a declared key here either: a value cadmpeg synthesizes on
//! one of the two branches is not something the source declared.
//!
//! `swVersion` is the axis. It is not in the container header: it is an
//! attribute of a `swSolidWorks` XML payload extracted after the scan, and it
//! selects the dialect row, whose [`SldprtDialect::form_code_padding`] method owns the byte width of
//! the feature-operation form-code padding — four bytes below 12000, eight at
//! 12000 and above. That padding shifts every feature-operation read, so the
//! boundary is B1.
//!
//! # The declaration is evidence; the id is identity
//!
//! [`DialectMatch::declared`] records the `swVersion` attribute verbatim, as
//! the source wrote it. [`DialectMatch::dialect`] records which registry row
//! the document satisfies. They are different statements and a consumer must
//! not join them: `swVersion="SW2019"` is recorded verbatim and classifies as
//! `sldprt:unknown`, because the row's discriminant is a usable numeric
//! declaration and that string is not one. Parse a version out of an id and
//! the answer is wrong for exactly the files whose declarations are wrong.
//!
//! # The residual row is never `Admitted`
//!
//! Admission verifies a *declared* identity, and `sldprt:unknown` is the
//! absence of one. So the two versioned rows are [`Admission::Admitted`] and
//! `sldprt:unknown` is [`Admission::Residual`]: read with no declared grammar.
//! Its residual fallback does not claim another row's strategy.
//!
//! That fallback is well-defined — the padding filter is not applied and the
//! ambiguity resolver requires the two candidate offsets to agree before it
//! binds an operation code — and it is tempting to call the result verified on
//! that basis. It is not: agreement between candidates is *consistency*, not a
//! declaration. Nothing in the file said which padding it was written with, so
//! nothing was verified against a declaration, and a part that declares
//! nothing must stay distinguishable in the ladder from a part whose
//! declaration was checked. Every golden fixture in this crate is synthetic
//! and version-less, so every one of them sits on this row; making that
//! visible is exactly what the totality row is for.
//!
//! [`Admission::Refused`] is unreachable: this codec refuses only on container
//! framing, I/O, and entity-budget grounds, all of which return
//! [`cadmpeg_core::CodecError`] before any report exists, and none of which is
//! a dialect judgement.
//!
//! Where the padding filter is absent and the candidates disagree, the
//! resolver binds nothing. That is a loss inside a dialect, expressed through
//! the ordinary loss vocabulary, not an admission state.

use crate::container::{ContainerScan, Section};
use crate::loss::SldprtLossCode;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::{Admission, DialectId, DialectLayers, DialectMatch};
use cadmpeg_core::target::TargetDescriptor;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeMap;

include!("dialect/registry_ids.rs");

#[cfg(test)]
const PARASOLID_FORMAT: &str = "parasolid";

/// The one dialect this writer synthesizes.
///
/// `writer::generated_solidworks_xml` emits a `swSolidWorks` block with no
/// `swVersion` attribute, so a synthesized part carries no version declaration
/// and classifies into the registry's totality row. Every versioned row is
/// reachable only by preserving a retained part.
pub(crate) const TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
    id: SldprtDialect::Unknown.id(),
    aliases: &[],
}];

/// Key of the `swSolidWorks` `swVersion` attribute in
/// [`DialectMatch::declared`].
///
/// Absent when the document declares no `swVersion`, which is every document
/// carrying no `swSolidWorks` XML payload at all. The value is the attribute
/// text exactly as written, including declarations that do not read as a
/// number.
const DECLARED_SW_VERSION: &str = "sw_version";

/// One row of `docs/dialects.toml` under the `sldprt` namespace.
///
/// Three rows, and the classification is total over them: the two grammar
/// classes selected by the padding boundary, plus the mandatory `unknown` row that
/// absorbs every declaration it cannot use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SldprtDialect {
    SwVersionPre12000,
    SwVersion12000Plus,
    Unknown,
}

/// Dialect layers admitted from one container and classification damage that
/// could not be represented in the unique layer identity set.
pub(crate) struct LayerClassification {
    host: SldprtDialect,
    layers: DialectLayers,
    losses: Vec<LossNote>,
}

impl LayerClassification {
    pub(crate) fn host(&self) -> SldprtDialect {
        self.host
    }

    pub(crate) fn layers(&self) -> &DialectLayers {
        &self.layers
    }

    pub(crate) fn append_losses(
        &self,
        ctx: &DecodeContext<'_>,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        for loss in dialect_losses(ctx, &self.layers)? {
            ctx.reserve_vec(losses, 1, "append SLDPRT dialect losses")?;
            losses.push(loss);
        }
        for loss in ctx.admit_iter(&self.losses, "scan SLDPRT append_losses values")? {
            let message = ctx.format_retained(
                format_args!("{}", loss.message),
                "copy SLDPRT dialect collision loss",
            )?;
            ctx.reserve_vec(losses, 1, "append SLDPRT dialect losses")?;
            losses.push(SldprtLossCode::DialectLayerCollision.note(message));
        }
        Ok(())
    }
}

/// Embedded Parasolid schema rows this codec reads with their own declared
/// grammar. Every other schema is recovered on the kernel's residual path.
const VERIFIED_KERNELS: [DialectId; 3] = [
    cadmpeg_core::dialect_id!("parasolid:sch-sw-33103"),
    cadmpeg_core::dialect_id!("parasolid:sch-sw-32001"),
    cadmpeg_core::dialect_id!("parasolid:format-13006"),
];

/// Classify the host document and every framed Parasolid stream it carries.
pub(crate) fn classify_layers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<LayerClassification, CodecError> {
    let mut kernels = Vec::new();
    for section in scan.sections(ctx)? {
        let (site_prefix, site_ordinal) = match section {
            Section::Block(block) => ("block", cadmpeg_core::decode::u64_from_index(block.offset)),
            Section::Compound(stream) => ("compound", u64::from(stream.directory_id)),
        };
        for stream in ctx.admit_iter(section.ps_streams(), "scan SLDPRT topology members")? {
            // A nameless section states its absence by omission: the site
            // key and the stream offset already separate two of them.
            let carrier = match section.name() {
                Some(name) => ctx.format_retained(
                    format_args!("{site_prefix}@{site_ordinal}:{name}+{}", stream.offset),
                    "retain SLDPRT Parasolid carrier",
                )?,
                None => ctx.format_retained(
                    format_args!("{site_prefix}@{site_ordinal}+{}", stream.offset),
                    "retain SLDPRT Parasolid carrier",
                )?,
            };
            let schema = ctx.format_retained(
                format_args!("{}", stream.header.schema.value()),
                "retain SLDPRT Parasolid schema",
            )?;
            let schema = cadmpeg_parasolid::OwnedSchemaToken::try_from(schema)
                .map_err(|_| CodecError::Malformed("invalid admitted Parasolid schema".into()))?;
            ctx.reserve_vec(&mut kernels, 1, "collect SLDPRT Parasolid layers")?;
            kernels.push((schema, cadmpeg_parasolid::Carrier::new(carrier)));
        }
    }
    let declaration = crate::container::declared_sw_version(scan);
    let host = SldprtDialect::from_declaration(declaration);
    let mut layers = DialectLayers::of(host.matched(ctx, declaration)?);
    let extra = cadmpeg_parasolid::extra_layers(ctx, kernels, &VERIFIED_KERNELS)?;
    let mut losses = Vec::new();
    for message in cadmpeg_parasolid::push_extras(ctx, &mut layers, extra)? {
        ctx.reserve_vec(&mut losses, 1, "collect SLDPRT dialect collision losses")?;
        losses.push(SldprtLossCode::DialectLayerCollision.note(message));
    }
    Ok(LayerClassification {
        host,
        layers,
        losses,
    })
}

impl SldprtDialect {
    /// Every dialect identity this enum can name.
    const ALL: [Self; 3] = [
        Self::SwVersionPre12000,
        Self::SwVersion12000Plus,
        Self::Unknown,
    ];

    /// The registry-generated id for this variant.
    pub(crate) const fn id(self) -> DialectId {
        match self {
            Self::SwVersionPre12000 => SLDPRT_SW_VERSION_PRE_12000,
            Self::SwVersion12000Plus => SLDPRT_SW_VERSION_12000_PLUS,
            Self::Unknown => SLDPRT_UNKNOWN,
        }
    }

    /// The row a `swVersion` declaration selects.
    ///
    /// The padding this row owns is the discriminant, so a classification bug
    /// and a decode bug cannot be different bugs. The
    /// boundary value 12000 belongs to the `Eight` arm, and every declaration
    /// the padding rule cannot use — absent, non-numeric, negative,
    /// wider than `u32`, or zero — lands on [`Self::Unknown`].
    pub(crate) fn from_declaration(sw_version: Option<&str>) -> Self {
        match sw_version.and_then(|value| value.parse::<u32>().ok()) {
            Some(1..12_000) => Self::SwVersionPre12000,
            Some(12_000..) => Self::SwVersion12000Plus,
            _ => Self::Unknown,
        }
    }

    /// Feature-operation form-code padding width selected by this row.
    pub(crate) const fn form_code_padding(self) -> Option<usize> {
        match self {
            Self::SwVersionPre12000 => Some(4),
            Self::SwVersion12000Plus => Some(8),
            Self::Unknown => None,
        }
    }

    /// The typed row carried by an existing classification.
    pub(crate) fn from_match(matched: &DialectMatch) -> Option<Self> {
        let id = matched.dialect();
        Self::ALL.into_iter().find(|dialect| dialect.id() == *id)
    }

    /// Classifies one document from its `swVersion` declaration. The single
    /// construction path for a [`DialectMatch`] in this codec, so a
    /// classification bug and the report can never disagree.
    #[cfg(test)]
    pub(crate) fn classify(sw_version: Option<&str>) -> DialectMatch {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("test dialect context fits service policy");
        Self::from_declaration(sw_version)
            .matched(&ctx, sw_version)
            .expect("test declaration fits service policy")
    }

    /// Build the wire identity for this typed row and its source declaration.
    fn matched(
        self,
        ctx: &DecodeContext<'_>,
        sw_version: Option<&str>,
    ) -> Result<DialectMatch, CodecError> {
        let mut declared = BTreeMap::new();
        if let Some(value) = sw_version {
            let value =
                ctx.format_retained(format_args!("{value}"), "retain SLDPRT dialect declaration")?;
            ctx.insert_btree_map(
                &mut declared,
                cadmpeg_core::nonblank_const!(DECLARED_SW_VERSION),
                value,
                "index SLDPRT dialect declaration",
            )?;
        }
        Ok(match self {
            Self::SwVersionPre12000 | Self::SwVersion12000Plus => DialectMatch::admitted(self.id()),
            Self::Unknown => DialectMatch::residual(self.id()),
        }
        .with_declared(declared))
    }

    /// Classifies one scanned document, reading the declaration from the scan.
    ///
    /// The read is [`crate::container::declared_sw_version`], so the retained
    /// declaration has one extraction and one report location.
    #[cfg(test)]
    fn classify_scan(scan: &ContainerScan<'_>) -> DialectMatch {
        Self::classify(crate::container::declared_sw_version(scan))
    }
}

/// The dialect-unverified loss for a classified layer.
///
/// `None` exactly when `matched.admission` is [`Admission::Admitted`], because
/// this reads that field rather than reclassifying. The biconditional the
/// decode policy requires is therefore structural: the note charged and the
/// admission reported come from one value, not from two authors agreeing.
fn dialect_loss(
    ctx: &DecodeContext<'_>,
    matched: &DialectMatch,
) -> Result<Option<LossNote>, CodecError> {
    match matched.admission() {
        Admission::Admitted | Admission::Refused => Ok(None),
        Admission::Unverified { .. } | Admission::Residual => {
            if let Some(message) = cadmpeg_parasolid::unverified_message(ctx, matched)? {
                return Ok(Some(SldprtLossCode::KernelDialectUnverified.note(message)));
            }
            if matched.format() != FORMAT {
                return Ok(None);
            }
            let message = match matched.declared().get(DECLARED_SW_VERSION) {
                Some(value) => ctx.format_retained(format_args!(
                        "the swSolidWorks swVersion declaration {value:?} does not read as a version \
                         above zero, so no declared identity was verified. The document is read on \
                         the `{}` residual path without substituting a declared dialect grammar: the \
                         feature-operation form-code padding filter is not applied, and an \
                         operation code binds only where the four- and eight-byte candidates agree. \
                         Agreement is consistency, not a declaration.",
                        matched.dialect()
                    ), "retain SLDPRT source dialect loss")?,
                None => ctx.format_retained(format_args!(
                        "the document carries no swSolidWorks swVersion declaration, so no declared identity was verified. The document is read on \
                         the `{}` residual path without substituting a declared dialect grammar: the \
                         feature-operation form-code padding filter is not applied, and an \
                         operation code binds only where the four- and eight-byte candidates agree. \
                         Agreement is consistency, not a declaration.",
                        matched.dialect()
                    ), "retain SLDPRT source dialect loss")?,
            };
            Ok(Some(SldprtLossCode::SourceDialectUnverified.note(message)))
        }
    }
}

/// Losses charged by every unverified layer in a classified document.
fn dialect_losses(
    ctx: &DecodeContext<'_>,
    layers: &DialectLayers,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    for layer in layers.iter() {
        if let Some(loss) = dialect_loss(ctx, layer)? {
            ctx.reserve_vec(&mut losses, 1, "collect SLDPRT dialect losses")?;
            losses.push(loss);
        }
    }
    Ok(losses)
}

#[cfg(test)]
mod tests;
