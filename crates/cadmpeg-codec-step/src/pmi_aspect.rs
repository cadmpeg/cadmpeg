// SPDX-License-Identifier: Apache-2.0
//! Content-derived identity for unresolved shape-aspect links.
//!
//! A `PmiTarget::ShapeAspect` keyed by Part 21 instance number breaks every
//! rewrite: the writer allocates new numbers, so the re-decode keys never
//! match (#209 §2.4: an instance number is source layout, not content). The
//! key is the aspect's content instead: its entity kind, name, datum
//! identification, and datum-target form. The writer emits one entity per
//! key, so the re-decode computes the same keys.
//!
//! The key is a JSON array string. STEP names and identifications hold
//! arbitrary text, so no separator spelling is collision-free.

use cadmpeg_core::text::NonBlankString;

/// Build the link key for an aspect of `kind` with the given content.
#[must_use]
pub(crate) fn aspect_key(
    kind: &str,
    name: &str,
    identification: &str,
    form: &str,
) -> NonBlankString {
    let key = serde_json::to_string(&[kind, name, identification, form])
        .expect("string array serializes");
    NonBlankString::new(key).expect("a JSON array holds non-whitespace")
}

/// The content a link key names.
pub(crate) struct AspectKey {
    /// The aspect entity kind (`DATUM`, `DATUM_FEATURE`, `DATUM_TARGET`,
    /// or `SHAPE_ASPECT`; other subtypes collapse to their base).
    pub(crate) kind: String,
    /// The aspect name.
    pub(crate) name: String,
    /// The datum or datum-target identification.
    pub(crate) identification: String,
    /// The datum-target form string.
    pub(crate) form: String,
}

/// Parse a link key, or `None` when it is not a key this module built.
#[must_use]
pub(crate) fn parse_aspect_key(key: &str) -> Option<AspectKey> {
    let [kind, name, identification, form]: [String; 4] = serde_json::from_str(key).ok()?;
    Some(AspectKey {
        kind,
        name,
        identification,
        form,
    })
}

#[cfg(test)]
mod tests {
    use super::{aspect_key, parse_aspect_key};

    #[test]
    fn keys_round_trip_arbitrary_text() {
        let key = aspect_key("DATUM_TARGET", "a:b\\c\"d", "A:1", "circular curve");
        let parsed = parse_aspect_key(key.as_str()).expect("own key parses");
        assert_eq!(parsed.kind, "DATUM_TARGET");
        assert_eq!(parsed.name, "a:b\\c\"d");
        assert_eq!(parsed.identification, "A:1");
        assert_eq!(parsed.form, "circular curve");
    }

    #[test]
    fn foreign_strings_do_not_parse() {
        assert!(parse_aspect_key("#6").is_none());
        assert!(parse_aspect_key("datum:A").is_none());
        assert!(parse_aspect_key("[\"DATUM\"]").is_none());
    }
}
