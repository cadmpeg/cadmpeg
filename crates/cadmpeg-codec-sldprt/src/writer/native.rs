// SPDX-License-Identifier: Apache-2.0
//! The native object structure of a part the writer generates: `moPart_c`
//! in `Contents/Config-0`, the default feature tree and a `moBaseBody_c`
//! stored body per body in `Contents/Config-0-ResolvedFeatures`, the
//! `swXmlContents` streams, and the fixed-form streams of a part of
//! `swVersion` 14000 (`docs/formats/sldprt.md` §2.2, §2.3, §8.2).
//!
//! Fields `sldprt.md` lays out carry the values chosen here. Byte runs it
//! gives as lengths only are written as zero bytes, which native parts do
//! not hold in most of them; `writer.native-skeleton-unlaid-runs` names them.

mod archive;
mod block;
mod config;
mod detailing;
pub(super) mod extrude;
pub(super) mod from_ir;
pub(super) mod sketch;
mod streams;
pub(super) mod tree;

#[cfg(test)]
mod tests;

use cadmpeg_core::CodecError;

/// The `swVersion` whose layout the skeleton follows.
const VERSION: u32 = 14000;

/// The date the skeleton's blocks state: year × 1000 + day of year.
const CREATION_DATE: u32 = 2_024_001;

/// The time stamp, in seconds since 1970-01-01 UTC, of 2024-01-01.
const TIME_STAMP: u32 = 1_704_067_200;

/// The FILETIME of [`TIME_STAMP`], high u32 then low u32.
const FILETIME: (u32, u32) = (31_079_493, 1_988_739_072);

/// Half the side of the default reference planes' display square, in metres.
const PLANE_HALF_EXTENT: f64 = 0.05;

/// The name class every named object opens with.
const NODE_NAME_CLASS: &str = "moNodeName_c";

/// The material folder of a part with no material.
const MATERIAL_NAME: &str = "Material <not specified>";

/// The object identifiers of the default tree's nodes: the first layout and
/// the first identifier set of sldprt.md §2.3, "Object identifiers". `15`
/// and `18` stay free: the set's third directional light, which the
/// skeleton does not write (open item EV-34), and an object the tree does
/// not hold.
pub(crate) mod ids {
    pub(super) const ANNOTATIONS: u32 = 1;
    pub(super) const FRONT_PLANE: u32 = 2;
    pub(super) const TOP_PLANE: u32 = 3;
    pub(super) const RIGHT_PLANE: u32 = 4;
    pub(super) const ORIGIN: u32 = 5;
    pub(super) const LIGHTS: u32 = 6;
    pub(super) const DESIGN_BINDER: u32 = 7;
    pub(super) const COMMENTS: u32 = 8;
    pub(super) const SOLID_BODIES: u32 = 9;
    pub(super) const SURFACE_BODIES: u32 = 10;
    pub(super) const MATERIAL: u32 = 11;
    pub(super) const AMBIENT: u32 = 12;
    pub(super) const DIRECTIONAL_1: u32 = 13;
    pub(super) const DIRECTIONAL_2: u32 = 14;
    pub(super) const EQUATIONS: u32 = 16;
    pub(super) const EXPLODED_VIEWS: u32 = 17;
    pub(super) const SENSORS: u32 = 19;
    pub(super) const FAVORITES: u32 = 20;
    pub(super) const HISTORY: u32 = 21;
    pub(super) const SELECTION_SETS: u32 = 22;
    /// The identifier of the first user feature. The history summary
    /// numbers a generated part's features from it, and the tree writes a
    /// user feature with the summary's identifier.
    pub(crate) const FIRST_USER: u32 = 23;
}

/// The object identifiers of the objects whose count varies with the part.
/// After the user features, in this order: the stored bodies' features, the
/// dimension parameters in write order, and the notes folder pair and the
/// markups folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Numbering {
    /// The first stored body's feature; the next bodies' follow it.
    pub(super) first_body: u32,
    /// The first dimension parameter; the next parameters follow it.
    pub(super) first_parameter: u32,
    /// The `Notes` folder.
    pub(super) notes: u32,
    /// The `Notes1___EndTag___` folder.
    pub(super) notes_end: u32,
    /// The `Markups` folder.
    pub(super) markups: u32,
}

impl Numbering {
    /// The numbering of a part whose history summary's largest identifier
    /// is `last_feature`, with `bodies` stored bodies and the user features
    /// `features`.
    pub(super) fn new(
        last_feature: Option<u32>,
        bodies: usize,
        features: &[Feature],
    ) -> Result<Self, CodecError> {
        let after = |id: u32, count: usize| {
            u32::try_from(count)
                .ok()
                .and_then(|count| id.checked_add(count))
        };
        let parameters = features.iter().map(Feature::parameters).sum();
        let first_body = last_feature
            .unwrap_or_default()
            .max(ids::FIRST_USER - 1)
            .checked_add(1);
        let first_parameter = first_body.and_then(|id| after(id, bodies));
        let notes = first_parameter.and_then(|id| after(id, parameters));
        let notes_end = notes.and_then(|id| id.checked_add(1));
        let markups = notes_end.and_then(|id| id.checked_add(1));
        match (first_body, first_parameter, notes, notes_end, markups) {
            (
                Some(first_body),
                Some(first_parameter),
                Some(notes),
                Some(notes_end),
                Some(markups),
            ) => Ok(Self {
                first_body,
                first_parameter,
                notes,
                notes_end,
                markups,
            }),
            _ => Err(CodecError::malformed(
                "SLDPRT native skeleton: the object count exceeds the identifier space",
            )),
        }
    }
}

/// What the skeleton states about the part.
pub(super) struct Facts<'a> {
    /// The identifiers of the objects whose count varies.
    pub(super) numbering: Numbering,
    /// The part's name.
    pub(super) name: &'a str,
    /// The name of the configuration the skeleton writes.
    pub(super) active: &'a str,
    /// The user features, in source order, written after the origin.
    pub(super) features: &'a [Feature],
    /// One stored body per body of the configuration's partition.
    pub(super) bodies: &'a [Body],
}

/// A user feature the tree writes.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Feature {
    Sketch(sketch::Sketch),
    Extrusion(extrude::Extrusion),
}

impl Feature {
    /// The feature's object identifier.
    pub(super) fn id(&self) -> u32 {
        match self {
            Self::Sketch(sketch) => sketch.feature_id,
            Self::Extrusion(extrusion) => extrusion.feature_id,
        }
    }

    /// The count of dimension parameters the feature writes: one per sketch
    /// dimension, and an extrusion's depth.
    fn parameters(&self) -> usize {
        match self {
            Self::Sketch(sketch) => sketch.dimensions.len(),
            Self::Extrusion(_) => 1,
        }
    }
}

/// One stored body.
pub(super) struct Body {
    /// The body identifier: [`config::FIRST_BODY_ID`] and up.
    pub(super) id: u32,
    /// The object identifier of its `moBaseBody_c` feature.
    pub(super) feature_id: u32,
    /// The feature's name.
    pub(super) name: String,
    /// The body's GUID text.
    pub(super) guid: String,
    /// Whether the body is a sheet body, recorded as a `moCompSurfaceBody_c`.
    pub(super) sheet: bool,
}

impl Body {
    /// The `index`-th body (from zero): `Imported<index + 1>`, its feature
    /// numbered by `numbering`, and a GUID derived from the part name and
    /// index.
    pub(super) fn numbered(
        part: &str,
        numbering: &Numbering,
        index: usize,
        sheet: bool,
    ) -> Result<Self, CodecError> {
        let ordinal = u32::try_from(index).ok();
        let id = ordinal.and_then(|ordinal| config::FIRST_BODY_ID.checked_add(ordinal));
        let feature_id = ordinal.and_then(|ordinal| numbering.first_body.checked_add(ordinal));
        let (Some(id), Some(feature_id)) = (id, feature_id) else {
            return Err(CodecError::malformed(
                "SLDPRT native skeleton: the body count exceeds the identifier space",
            ));
        };
        Ok(Self {
            id,
            feature_id,
            name: format!("Imported{}", index + 1),
            guid: guid(part, id),
            sheet,
        })
    }
}

/// The digits of a GUID's text form.
const GUID_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// A GUID in the `8-4-4-4-12` text form from the SHA-256 of `part` and the
/// body identifier `id`.
fn guid(part: &str, id: u32) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(part.as_bytes());
    hasher.update(id.to_le_bytes());
    let digest = hasher.finalize();
    let mut hex = String::new();
    for &byte in digest.iter().take(16) {
        hex.push(char::from(GUID_DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(GUID_DIGITS[usize::from(byte & 0x0f)]));
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The count and total length of the detailing settings' byte runs, which
/// the skeleton writes as zero bytes.
pub(super) fn detailing_zero_runs() -> (usize, usize) {
    detailing::zero_runs()
}

/// The skeleton's other zero byte runs in which native parts of the version
/// hold values.
pub(super) const OTHER_ZERO_RUNS: &str = "the twenty bytes after the part's visual properties, \
     the 64 bytes of the moAtom_c, the twelve identifier bytes of the sketch-block manager and the \
     30 bytes of its moPMarkRecord_c, the 26 bytes after the exploded-view manager, and the \
     origin sketch's 76-byte and 42-byte settings runs";

/// The streams every native part of the version holds that the skeleton
/// leaves out.
pub(super) const OMITTED_STREAMS: [&str; 22] = [
    "Header2",
    "ModelStamps",
    "Preview",
    "Contents/3DExperienceExchange2",
    "Contents/CMgr",
    "Contents/CMgrHdr2",
    "Contents/Config-0-LWDATA",
    "Contents/Config-0-ModelHeader",
    "Contents/CusProps",
    "Contents/Definition",
    "SwDocContentMgr/SwDocContentMgrInfo",
    "ThirdPtyStore/VisualStates",
    "_MO_VERSION_14000/Biography",
    "_MO_VERSION_14000/History",
    "[Content_Types].xml",
    "_rels/.rels",
    "docProps/app.xml",
    "docProps/core.xml",
    "docProps/custom.xml",
    "docProps/Config-0-Properties.xml",
    "docProps/ISolidWorksInformation.xml",
    "docProps/OpenTime.xml",
];

/// The streams of the skeleton, by name.
pub(super) fn skeleton(facts: &Facts<'_>) -> Result<Vec<(String, Vec<u8>)>, CodecError> {
    let (config, map, part) = config::config(facts)?;
    let resolved = tree::resolved_features(facts, map, &part)?;
    Ok(vec![
        ("Contents/Config-0".into(), config),
        ("Contents/Config-0-ResolvedFeatures".into(), resolved),
        ("Contents/CnfgObjs".into(), streams::configuration_objects()),
        ("Contents/OleItems".into(), vec![0; 4]),
        ("Contents/eModelLic".into(), vec![0; 4]),
        ("Contents/MBLinkData".into(), vec![0; 6]),
        ("Contents/User Units Table".into(), Vec::new()),
        (
            "Contents/View Orientation Data".into(),
            streams::view_orientations(),
        ),
        (format!("_MO_VERSION_{VERSION}/AssyVisualData"), vec![0; 4]),
        ("swXmlContents/Features".into(), streams::features(facts)),
        ("swXmlContents/KeyWords".into(), streams::keywords(facts)),
    ])
}
