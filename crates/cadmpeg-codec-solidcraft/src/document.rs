// SPDX-License-Identifier: Apache-2.0
//! The design document: its format tag and the facts the container layer
//! reads before the history is decoded.
//!
//! A design document is one JSON object. Its `format` member is
//! `solidcraft/N` or the earlier spelling `solvecraft/N`; a document without
//! `format` is format 1. `format_pending` lists the changes past format N
//! that the writing build already applied.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde_json::{Map, Value};

use crate::package::BlobName;

/// The tag prefix of current designs.
pub(crate) const FORMAT_PREFIX: &str = "solidcraft/";
/// The tag prefix of designs written before the application rename.
pub(crate) const LEGACY_FORMAT_PREFIX: &str = "solvecraft/";
/// The member that holds the format tag.
const KEY_FORMAT: &str = "format";
/// The member that lists the changes applied past the tagged format.
const KEY_FORMAT_PENDING: &str = "format_pending";
/// The document name.
const KEY_NAME: &str = "name";
/// The length unit of the document.
const KEY_UNITS: &str = "units";
/// The feature timeline.
pub(crate) const KEY_FEATURES: &str = "features";
/// The parameter table.
pub(crate) const KEY_PARAMS: &str = "params";
/// The identifier counter that readers recompute.
const KEY_NEXT_ID: &str = "next_id";
/// The table of linked designs, by component id.
const KEY_LINKS: &str = "links";
/// The linked design file of a link record.
const KEY_LINK_PATH: &str = "path";
/// The suffix of a member that names a blob.
const BLOB_FIELD_SUFFIX: &str = "_blob";

/// The spelling of a document's format tag prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagPrefix {
    /// `solidcraft/`.
    Solidcraft,
    /// `solvecraft/`.
    Solvecraft,
}

/// The format a document declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tag {
    /// No `format` member: format 1.
    Untagged,
    /// A design format tag with a nonzero version.
    Design {
        /// The prefix spelling.
        prefix: TagPrefix,
        /// The format number.
        version: u32,
    },
    /// A `format` value that is not a design format tag.
    Other(String),
}

impl Tag {
    /// Reads a `format` member value.
    fn of(value: Option<&Value>) -> Self {
        match value {
            None => Self::Untagged,
            Some(Value::String(text)) => design_version(text).map_or_else(
                || Self::Other(text.clone()),
                |(prefix, version)| Self::Design { prefix, version },
            ),
            Some(other) => Self::Other(other.to_string()),
        }
    }

    /// The format number: 1 for an untagged document.
    pub(crate) fn version(&self) -> Option<u32> {
        match self {
            Self::Untagged => Some(1),
            Self::Design { version, .. } => Some(*version),
            Self::Other(_) => None,
        }
    }
}

/// The prefix and version of a design format tag, or `None` for any other
/// text and for version 0.
pub(crate) fn design_version(text: &str) -> Option<(TagPrefix, u32)> {
    let (prefix, digits) = if let Some(digits) = text.strip_prefix(FORMAT_PREFIX) {
        (TagPrefix::Solidcraft, digits)
    } else {
        (
            TagPrefix::Solvecraft,
            text.strip_prefix(LEGACY_FORMAT_PREFIX)?,
        )
    };
    digits
        .parse::<u32>()
        .ok()
        .filter(|version| *version != 0)
        .map(|version| (prefix, version))
}

/// One member that names a blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlobReference {
    /// The member name, for example `step_blob`.
    pub(crate) field: String,
    /// The referenced member name.
    pub(crate) target: String,
    /// The parsed name, when it is a well-formed blob name.
    pub(crate) blob: Option<BlobName>,
}

/// The facts the container layer reads from a design document.
#[derive(Debug, Clone)]
pub(crate) struct Header {
    /// The declared format.
    pub(crate) tag: Tag,
    /// The `format_pending` names, in document order.
    pub(crate) pending: Vec<String>,
    /// The document name.
    pub(crate) name: Option<String>,
    /// The document length unit.
    pub(crate) units: Option<String>,
    /// The number of feature records.
    pub(crate) features: usize,
    /// The number of parameter records.
    pub(crate) params: usize,
    /// The top-level members outside the header and the timeline, in key
    /// order.
    pub(crate) sections: Vec<String>,
    /// Every blob reference, in key order within each object.
    pub(crate) blobs: Vec<BlobReference>,
    /// The linked design files, as (component id, path), in id order.
    pub(crate) links: Vec<(String, String)>,
}

/// A parsed design document.
pub(crate) struct Document {
    /// The header facts of the document object.
    pub(crate) header: Header,
    /// The document object.
    pub(crate) object: serde_json::Map<String, Value>,
}

/// Parses the document text of `view` and reads its header.
pub(crate) fn parse(ctx: &DecodeContext<'_>, view: View<'_>) -> Result<Document, CodecError> {
    let text = std::str::from_utf8(view.window())
        .map_err(|error| CodecError::malformed(format_args!("design document: {error}")))?;
    let value = ctx.parse_json_value(text, "SolidCraft design document")?;
    let Value::Object(object) = value else {
        return Err(CodecError::malformed(
            "design document: the top-level value is not a JSON object",
        ));
    };
    Ok(Document {
        header: Header::of(&object),
        object,
    })
}

impl Header {
    fn of(object: &Map<String, Value>) -> Self {
        let pending = match object.get(KEY_FORMAT_PENDING) {
            Some(Value::Array(names)) => names
                .iter()
                .map(|name| match name {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect(),
            Some(other) => vec![other.to_string()],
            None => Vec::new(),
        };
        let text = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
        let count = |key: &str| {
            object
                .get(key)
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        };
        let sections = object
            .keys()
            .filter(|key| {
                !matches!(
                    key.as_str(),
                    KEY_FORMAT
                        | KEY_FORMAT_PENDING
                        | KEY_NAME
                        | KEY_UNITS
                        | KEY_FEATURES
                        | KEY_PARAMS
                        | KEY_NEXT_ID
                )
            })
            .cloned()
            .collect();
        Self {
            tag: Tag::of(object.get(KEY_FORMAT)),
            pending,
            name: text(KEY_NAME),
            units: text(KEY_UNITS),
            features: count(KEY_FEATURES),
            params: count(KEY_PARAMS),
            sections,
            blobs: blob_references(object),
            links: object
                .get(KEY_LINKS)
                .and_then(Value::as_object)
                .map(|links| {
                    links
                        .iter()
                        .filter_map(|(id, link)| {
                            let path = link.get(KEY_LINK_PATH)?.as_str()?;
                            Some((id.clone(), path.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// Every string member whose name ends in `_blob`, depth first. The
/// walk keeps its own stack, so its depth is that of the parsed tree.
fn blob_references(object: &Map<String, Value>) -> Vec<BlobReference> {
    let mut references = Vec::new();
    for (key, value) in object {
        collect(key, value, &mut references);
    }
    let mut stack: Vec<&Value> = object.values().rev().collect();
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    collect(key, child, &mut references);
                }
                stack.extend(map.values().rev());
            }
            Value::Array(items) => stack.extend(items.iter().rev()),
            _ => {}
        }
    }
    references
}

fn collect(key: &str, value: &Value, references: &mut Vec<BlobReference>) {
    if !key.ends_with(BLOB_FIELD_SUFFIX) {
        return;
    }
    if let Value::String(target) = value {
        references.push(BlobReference {
            field: key.to_owned(),
            target: target.clone(),
            blob: BlobName::parse(target),
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{design_version, Header, Tag, TagPrefix};
    use crate::package::BlobKind;

    #[test]
    fn design_tags_read_both_prefixes_and_refuse_version_zero() {
        assert_eq!(
            design_version("solidcraft/57"),
            Some((TagPrefix::Solidcraft, 57))
        );
        assert_eq!(
            design_version("solvecraft/13"),
            Some((TagPrefix::Solvecraft, 13))
        );
        assert_eq!(design_version("solidcraft/0"), None);
        assert_eq!(design_version("solidcraft-drawing/3"), None);
        assert_eq!(design_version("solvecraftx/13"), None);
        assert_eq!(design_version("solidcraft/x"), None);
    }

    #[test]
    fn header_reads_tag_counts_sections_and_blob_references() {
        let hash = "a".repeat(64);
        let document = json!({
            "format": "solidcraft/57",
            "format_pending": ["late-change"],
            "name": "Plate",
            "units": "mm",
            "next_id": 9,
            "params": [{"name": "w", "expr": "10 mm", "unit": "mm", "model": false}],
            "features": [
                {"id": 1, "type": "import", "step": "", "step_blob": format!("blobs/{hash}.step")},
                {"id": 2, "type": "mesh_import", "meshes": [{"name": "m", "mesh_blob": "blobs/bad.mesh"}]}
            ],
            "canvases": [],
            "appearances": {},
            "links": {"3": {"path": "parts/arm.sc", "feature": 4}, "5": {"feature": 6}}
        });
        let header = Header::of(document.as_object().unwrap());
        assert_eq!(
            header.tag,
            Tag::Design {
                prefix: TagPrefix::Solidcraft,
                version: 57
            }
        );
        assert_eq!(header.pending, ["late-change"]);
        assert_eq!(header.name.as_deref(), Some("Plate"));
        assert_eq!(header.units.as_deref(), Some("mm"));
        assert_eq!((header.features, header.params), (2, 1));
        assert_eq!(header.sections, ["appearances", "canvases", "links"]);
        assert_eq!(header.links, [("3".to_owned(), "parts/arm.sc".to_owned())]);
        assert_eq!(header.blobs.len(), 2);
        assert_eq!(header.blobs[0].field, "step_blob");
        assert_eq!(
            header.blobs[0].blob.as_ref().map(|blob| blob.kind),
            Some(BlobKind::Step)
        );
        assert_eq!(header.blobs[1].target, "blobs/bad.mesh");
        assert!(header.blobs[1].blob.is_none());
    }

    #[test]
    fn an_untagged_document_is_format_one() {
        let header = Header::of(json!({"name": "A", "features": []}).as_object().unwrap());
        assert_eq!(header.tag, Tag::Untagged);
        assert_eq!(header.tag.version(), Some(1));
    }
}
