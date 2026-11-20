// SPDX-License-Identifier: Apache-2.0
//! `SolidCraft` dialect identity: which registry row a file is, and how it was
//! admitted.
//!
//! The discriminants are the encoding (a version 1 container or a plain JSON
//! document), the container marker, the document format number, and
//! `format_pending`. [`classify`] is the single construction path for a
//! [`DialectMatch`] in this codec.

use cadmpeg_core::dialect::{DialectId, DialectMatch};
use cadmpeg_ir::report::loss::LossNote;

use crate::document::{Header, Tag};
use crate::loss::SolidcraftLossCode;
use crate::package::Marker;

include!("dialect/registry_ids.rs");

/// The newest released design format.
/// The synthesis catalog: the two current dialects the builder generates. The
/// first is the default of a source-less model. Preserving a source in any
/// other dialect is not listed here: an inherited target replays it.
pub(crate) const TARGETS: &[cadmpeg_core::target::TargetDescriptor] = &[
    cadmpeg_core::target::TargetDescriptor {
        id: SOLIDCRAFT_CONTAINER_1_V57,
        aliases: &[],
    },
    cadmpeg_core::target::TargetDescriptor {
        id: SOLIDCRAFT_JSON_V57,
        aliases: &[],
    },
];

pub(crate) const CURRENT_FORMAT: u32 = 57;

/// Key of the container marker format in [`DialectMatch::declared`].
const DECLARED_CONTAINER_FORMAT: &str = "container_format";
/// Key of the document format tag in [`DialectMatch::declared`].
const DECLARED_DOCUMENT_FORMAT: &str = "document_format";
/// Key of the `format_pending` names in [`DialectMatch::declared`].
const DECLARED_FORMAT_PENDING: &str = "format_pending";

/// The encoding of a design file.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Encoding<'a> {
    /// A plain JSON document.
    Json,
    /// A ZIP container with this marker.
    Container(&'a Marker),
}

/// A completed classification whose recovery state cannot drift from its match.
pub(crate) struct DialectClassification(State);

enum State {
    Admitted(DialectMatch),
    Residual(DialectMatch, String),
}

/// The document format band of a design document.
enum Band {
    Current,
    Earlier,
    /// The next format, which lists its pending changes; each is named here.
    Pending,
}

/// The changes `SolidCraft` lists as pending after format 57. A design of the
/// next format admits only when every change it lists is one of these, so an
/// unknown change keeps the file on the residual path.
const KNOWN_PENDING: &[&str] = &[
    "bounding-solid-side-margins",
    "joint-lateral-offsets",
    "move-rotate-about-edge",
    "render-coat-flakes",
    "sketch-3d-constraints",
    "sketch-3d-dimensions",
    "sketch-3d-relations",
    "sketch-reference-placements",
    "step-import-solid-items",
    "surface-sweep-exact-spline-path",
    "whole-profile-holes-share-a-curve",
];

fn band(header: &Header) -> Result<Band, String> {
    if !header.pending.is_empty() {
        let next = CURRENT_FORMAT + 1;
        let known = header
            .pending
            .iter()
            .all(|name| KNOWN_PENDING.contains(&name.as_str()));
        if known && header.tag.version() == Some(next) {
            return Ok(Band::Pending);
        }
        return Err(format!(
            "the document lists changes past its format in format_pending: {}",
            header.pending.join(", ")
        ));
    }
    match header.tag.version() {
        Some(CURRENT_FORMAT) => Ok(Band::Current),
        Some(version) if version < CURRENT_FORMAT => Ok(Band::Earlier),
        Some(version) => Err(format!(
            "the document declares format {version}, past the newest released format \
             {CURRENT_FORMAT}"
        )),
        None => Err("the document declares no design format".to_owned()),
    }
}

/// Classifies a design file from its encoding and its document header.
pub(crate) fn classify(encoding: Encoding<'_>, header: Option<&Header>) -> DialectClassification {
    let selected = match (encoding, header) {
        (Encoding::Container(Marker::Drawing), _) => Ok(SOLIDCRAFT_DRAWING_CONTAINER_1),
        (Encoding::Container(Marker::Design | Marker::DraftDesign), Some(header)) => band(header)
            .map(|band| match band {
                Band::Current => SOLIDCRAFT_CONTAINER_1_V57,
                Band::Earlier => SOLIDCRAFT_CONTAINER_1_V1_56,
                Band::Pending => SOLIDCRAFT_CONTAINER_1_V58_PENDING,
            }),
        (Encoding::Container(Marker::Other(text)), _) => Err(format!(
            "the container marker {text:?} is not a version 1 marker"
        )),
        (Encoding::Container(Marker::Absent), _) => {
            Err("the container has no container.json marker".to_owned())
        }
        (Encoding::Container(_), None) => {
            Err("the container has no readable design document".to_owned())
        }
        (Encoding::Json, Some(header)) => band(header).map(|band| match band {
            Band::Current => SOLIDCRAFT_JSON_V57,
            Band::Earlier => SOLIDCRAFT_JSON_V1_56,
            Band::Pending => SOLIDCRAFT_JSON_V58_PENDING,
        }),
        (Encoding::Json, None) => Err("the document is unreadable".to_owned()),
    };
    let declare = |mut matched: DialectMatch| {
        if let Encoding::Container(marker) = encoding {
            if let Some(format) = marker.declared() {
                matched = matched.with_declared_entry(
                    cadmpeg_core::nonblank_const!(DECLARED_CONTAINER_FORMAT),
                    format,
                );
            }
        }
        if let Some(header) = header {
            let tag = match &header.tag {
                Tag::Untagged => None,
                Tag::Design { prefix, version } => Some(match prefix {
                    crate::document::TagPrefix::Solidcraft => {
                        format!("{}{version}", crate::document::FORMAT_PREFIX)
                    }
                    crate::document::TagPrefix::Solvecraft => {
                        format!("{}{version}", crate::document::LEGACY_FORMAT_PREFIX)
                    }
                }),
                Tag::Other(text) => Some(text.clone()),
            };
            if let Some(tag) = tag {
                matched = matched.with_declared_entry(
                    cadmpeg_core::nonblank_const!(DECLARED_DOCUMENT_FORMAT),
                    &tag,
                );
            }
            if !header.pending.is_empty() {
                matched = matched.with_declared_entry(
                    cadmpeg_core::nonblank_const!(DECLARED_FORMAT_PENDING),
                    &header.pending.join(","),
                );
            }
        }
        matched
    };
    match selected {
        Ok(id) => DialectClassification(State::Admitted(declare(DialectMatch::admitted(id)))),
        Err(reason) => DialectClassification(State::Residual(
            declare(DialectMatch::residual(SOLIDCRAFT_UNKNOWN)),
            reason,
        )),
    }
}

impl DialectClassification {
    pub(crate) fn into_matched(self) -> DialectMatch {
        match self.0 {
            State::Admitted(matched) | State::Residual(matched, _) => matched,
        }
    }

    /// The loss charged exactly for the residual state.
    pub(crate) fn loss(&self) -> Option<LossNote> {
        match &self.0 {
            State::Admitted(_) => None,
            State::Residual(_, reason) => Some(SolidcraftLossCode::SourceDialectUnverified.note(
                format!("{reason}; the file was read without a declared document grammar"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::dialect::Admission;
    use serde_json::json;

    use super::{
        classify, Encoding, SOLIDCRAFT_CONTAINER_1_V1_56, SOLIDCRAFT_CONTAINER_1_V57,
        SOLIDCRAFT_DRAWING_CONTAINER_1, SOLIDCRAFT_JSON_V1_56, SOLIDCRAFT_JSON_V57,
        SOLIDCRAFT_UNKNOWN,
    };
    use crate::document::Header;
    use crate::package::Marker;

    fn header(value: &serde_json::Value) -> Header {
        let text = value.to_string();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            text.as_bytes(),
            &arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .unwrap();
        let document = crate::document::parse(&ctx, root).unwrap();
        ctx.finish_session().unwrap();
        document.header
    }

    #[test]
    fn rows_follow_encoding_and_format_band() {
        let current = header(&json!({"format": "solidcraft/57", "features": []}));
        let earlier = header(&json!({"format": "solvecraft/13", "features": []}));
        let untagged = header(&json!({"features": []}));
        for (encoding, header, dialect) in [
            (
                Encoding::Container(&Marker::Design),
                &current,
                SOLIDCRAFT_CONTAINER_1_V57,
            ),
            (
                Encoding::Container(&Marker::DraftDesign),
                &earlier,
                SOLIDCRAFT_CONTAINER_1_V1_56,
            ),
            (Encoding::Json, &current, SOLIDCRAFT_JSON_V57),
            (Encoding::Json, &untagged, SOLIDCRAFT_JSON_V1_56),
            (
                Encoding::Container(&Marker::Drawing),
                &current,
                SOLIDCRAFT_DRAWING_CONTAINER_1,
            ),
        ] {
            let classification = classify(encoding, Some(header));
            assert!(classification.loss().is_none());
            let matched = classification.into_matched();
            assert_eq!(matched.dialect(), &dialect);
            assert_eq!(matched.admission(), &Admission::Admitted);
        }
    }

    #[test]
    fn newer_pending_and_foreign_files_are_residual() {
        let newer = header(&json!({"format": "solidcraft/58", "features": []}));
        let pending = header(&json!({
            "format": "solidcraft/57",
            "format_pending": ["a-change"],
            "features": []
        }));
        let foreign = header(&json!({"format": "another/1"}));
        let other_marker = Marker::Other("{\"format\":\"solidcraft-container/2\"}".into());
        for (encoding, header) in [
            (Encoding::Json, Some(&newer)),
            (Encoding::Container(&Marker::Design), Some(&pending)),
            (Encoding::Json, Some(&foreign)),
            (Encoding::Container(&other_marker), Some(&newer)),
            (Encoding::Container(&Marker::Absent), None),
            (Encoding::Json, None),
        ] {
            let classification = classify(encoding, header);
            assert!(classification.loss().is_some());
            let matched = classification.into_matched();
            assert_eq!(matched.dialect(), &SOLIDCRAFT_UNKNOWN);
            assert_eq!(matched.admission(), &Admission::Residual);
        }
    }
}
