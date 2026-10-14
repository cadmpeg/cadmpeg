// SPDX-License-Identifier: Apache-2.0
//! XML and JSON parsing with nesting depth charged to the decode session.
//!
//! Each parser charges the document's nesting depth through
//! [`DecodeContext::enter_nested`], so a deep document refuses with the
//! session's recursion-depth dimension before code walks it recursively.

use super::{DecodeContext, DepthGuard};
use crate::CodecError;

impl DecodeContext<'_> {
    /// Holds `depth` nesting guards for the duration of `run`.
    fn at_depth<T>(
        &self,
        depth: u64,
        operation: &'static str,
        run: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let mut guards: Vec<DepthGuard<'_>> = Vec::new();
        for _ in 0..depth {
            guards.push(self.enter_nested(operation)?);
        }
        let result = run();
        drop(guards);
        result
    }

    fn tree_malformed(error: impl std::fmt::Display) -> CodecError {
        CodecError::malformed(format_args!("{error}"))
    }

    /// Parses XML with DTD processing disabled and charges its element depth.
    pub fn parse_xml<'input>(
        &self,
        text: &'input str,
        operation: &'static str,
    ) -> Result<roxmltree::Document<'input>, CodecError> {
        let document = roxmltree::Document::parse(text).map_err(Self::tree_malformed)?;
        // Node ids follow document order, so a parent's depth is known
        // before its children's.
        let mut depths: Vec<u64> = Vec::new();
        let mut maximum = 0_u64;
        for node in document.root().descendants() {
            let depth = node
                .parent()
                .and_then(|parent| depths.get(parent.id().get_usize()))
                .map_or(0, |parent| parent + 1);
            maximum = maximum.max(depth);
            depths.push(depth);
        }
        self.at_depth(maximum, operation, || Ok(document))
    }

    /// Parses a JSON value tree after charging its nesting depth.
    pub fn parse_json_value(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<serde_json::Value, CodecError> {
        self.at_depth(json_depth(text), operation, || {
            serde_json::from_str(text).map_err(Self::tree_malformed)
        })
    }

    /// Parses a type with a derived `Deserialize` after charging the source's
    /// nesting depth.
    pub fn parse_json<T: serde::de::DeserializeOwned>(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<T, CodecError> {
        self.at_depth(json_depth(text), operation, || {
            serde_json::from_str(text).map_err(Self::tree_malformed)
        })
    }
}

/// The nesting depth a JSON parse can reach.
///
/// Counts `[` and `{` outside strings. With the `raw_value` feature, a string
/// that spells the private raw-value token is parsed again, so its escaped
/// delimiters can nest deeper than the visible brackets; when such a string
/// occurs, every opening delimiter and escape counts.
fn json_depth(text: &str) -> u64 {
    let mut depth = 0_u64;
    let mut maximum = 0_u64;
    let mut openings = 0_u64;
    let mut escapes = 0_u64;
    let mut raw = false;
    let mut string = JsonStringScan::default();
    for &byte in text.as_bytes() {
        openings += u64::from(matches!(byte, b'[' | b'{'));
        escapes += u64::from(byte == b'\\');
        if string.active {
            raw |= string.byte(byte);
            continue;
        }
        match byte {
            b'"' => {
                string = JsonStringScan {
                    active: true,
                    possible: true,
                    ..JsonStringScan::default()
                }
            }
            b'[' | b'{' => {
                depth += 1;
                maximum = maximum.max(depth);
            }
            b']' | b'}' if depth != 0 => depth -= 1,
            _ => {}
        }
    }
    if raw {
        openings + escapes
    } else {
        maximum
    }
}

const RAW_VALUE_TOKEN: &[u8] = b"$serde_json::private::RawValue";

#[derive(Default)]
struct JsonStringScan {
    active: bool,
    escaped: bool,
    unicode_digits: u8,
    unicode_value: u32,
    matched: usize,
    possible: bool,
}

impl JsonStringScan {
    fn decoded(&mut self, byte: Option<u8>) {
        if byte.is_some() && byte == RAW_VALUE_TOKEN.get(self.matched).copied() {
            self.matched += 1;
        } else {
            self.possible = false;
        }
    }

    /// Returns true only when a completed string spells the raw-value token.
    fn byte(&mut self, byte: u8) -> bool {
        if self.unicode_digits != 0 {
            if let Some(digit) = char::from(byte).to_digit(16) {
                self.unicode_value = self.unicode_value * 16 + digit;
            } else {
                self.possible = false;
            }
            self.unicode_digits -= 1;
            if self.unicode_digits == 0 {
                self.decoded(u8::try_from(self.unicode_value).ok());
            }
        } else if self.escaped {
            self.escaped = false;
            if byte == b'u' {
                self.unicode_digits = 4;
                self.unicode_value = 0;
            } else {
                self.decoded(match byte {
                    b'"' | b'\\' | b'/' => Some(byte),
                    b'b' => Some(8),
                    b'f' => Some(12),
                    b'n' => Some(b'\n'),
                    b'r' => Some(b'\r'),
                    b't' => Some(b'\t'),
                    _ => None,
                });
            }
        } else if byte == b'\\' {
            self.escaped = true;
        } else if byte == b'"' {
            self.active = false;
            return self.possible && self.matched == RAW_VALUE_TOKEN.len();
        } else {
            self.decoded(Some(byte));
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::json_depth;
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct JsonRecord {
        name: String,
        values: Vec<u64>,
    }

    fn with_depth<T>(depth: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = depth;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        run(&ctx)
    }

    fn is_depth_refusal<T: std::fmt::Debug>(result: &Result<T, CodecError>) -> bool {
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth)
    }

    #[test]
    fn xml_parses_and_rejects_malformed_text() {
        with_depth(256, |ctx| {
            let document = ctx
                .parse_xml("<r a=\"1\"><c/></r>", "XML")
                .expect("valid XML");
            assert_eq!(document.root_element().attribute("a"), Some("1"));
            assert!(matches!(
                ctx.parse_xml("<r>", "XML"),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn xml_depth_is_charged_to_the_session() {
        let nested = "<a><b><c/></b></a>";
        with_depth(3, |ctx| assert!(ctx.parse_xml(nested, "XML").is_ok()));
        with_depth(2, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_xml(nested, "XML")));
        });
    }

    #[test]
    fn json_value_and_typed_parse() {
        with_depth(256, |ctx| {
            let value = ctx
                .parse_json_value(r#"{"a":[1,2]}"#, "JSON")
                .expect("valid JSON");
            assert_eq!(value, serde_json::json!({"a": [1, 2]}));
            let record: JsonRecord = ctx
                .parse_json(r#"{"name":"r","values":[3]}"#, "JSON")
                .expect("valid JSON");
            assert_eq!(
                record,
                JsonRecord {
                    name: "r".into(),
                    values: vec![3],
                }
            );
            assert!(matches!(
                ctx.parse_json_value("{", "JSON"),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn typed_json_keeps_duplicate_field_errors() {
        with_depth(256, |ctx| {
            let text = r#"{"name":"a","name":"b","values":[]}"#;
            assert!(matches!(
                ctx.parse_json::<JsonRecord>(text, "JSON"),
                Err(CodecError::Malformed(_))
            ));
            let value = ctx.parse_json_value(text, "JSON").expect("valid JSON");
            assert_eq!(value["name"], "b");
        });
    }

    #[test]
    fn typed_json_ignores_raw_carrier_extensions() {
        with_depth(256, |ctx| {
            let text = r#"{"name":"r","values":[],"extension":{"$serde_json::private::RawValue":"not-json"}}"#;
            let record: JsonRecord = ctx.parse_json(text, "JSON").expect("valid JSON");
            assert_eq!(record.name, "r");
        });
    }

    #[test]
    fn json_depth_counts_brackets_outside_strings_and_raw_replay() {
        assert_eq!(json_depth(r#"["[[["]"#), 1);
        assert_eq!(json_depth(r#"{"a":[[1]]}"#), 3);
        let raw = r#"{"$serde_json::private::RawValue":"\u005b0\u005d"}"#;
        assert!(json_depth(raw) >= 2);
        let escaped_key = r#"{"\u0024serde_json::private::RawValue":"[0]"}"#;
        assert!(json_depth(escaped_key) >= 2);
        with_depth(1, |ctx| {
            assert!(is_depth_refusal(&ctx.parse_json_value(raw, "JSON")));
        });
        with_depth(256, |ctx| {
            let value = ctx
                .parse_json_value(raw, "JSON")
                .expect("valid raw carrier");
            assert_eq!(value, serde_json::json!([0]));
        });
    }
}
