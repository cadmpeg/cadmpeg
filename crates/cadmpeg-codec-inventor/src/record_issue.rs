// SPDX-License-Identifier: Apache-2.0
//! Located parser failures shared by the Inventor record families.

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Default)]
struct ByteCounter(usize);

impl Write for ByteCounter {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(std::fmt::Error)?;
        Ok(())
    }
}

pub(crate) fn admit_issue_detail(
    ctx: &DecodeContext<'_>,
    error: &CodecError,
    operation: &'static str,
) -> Result<(), CodecError> {
    admit_formatted(ctx, format_args!("{error}"), operation)
}

pub(crate) fn admit_formatted(
    ctx: &DecodeContext<'_>,
    args: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let mut detail_len = ByteCounter::default();
    detail_len.write_fmt(args).map_err(|_| {
        ctx.refuse_codec_limit("Inventor issue detail byte count", u64::MAX - 1, u64::MAX)
    })?;
    ctx.charge_retained(detail_len.0 as u64, operation)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecordIssueFamily {
    Assembly,
    Presentation,
    Design { type_id: String },
    Sketch { type_id: String },
    Feature { type_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RecordIssueWire")]
pub(crate) struct RecordIssue {
    pub(crate) family: RecordIssueFamily,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) detail: String,
}

struct RecordIssueId<'a>(&'a RecordIssue);

impl std::fmt::Display for RecordIssueId<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = match self.0.family {
            RecordIssueFamily::Assembly => "inventor:assembly:record-issue",
            RecordIssueFamily::Presentation => "inventor:presentation:record-issue",
            RecordIssueFamily::Design { .. } => "inventor:pmdc:record-issue",
            RecordIssueFamily::Sketch { .. } => "inventor:pmdc:sketch-record-issue",
            RecordIssueFamily::Feature { .. } => "inventor:pmdc:feature-record-issue",
        };
        formatter.write_fmt(format_args!(
            "{prefix}#{}-{}",
            self.0.segment_token, self.0.record_ordinal
        ))
    }
}

impl Serialize for RecordIssueId<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl Serialize for RecordIssue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &RecordIssueId(self))?;
        match &self.family {
            RecordIssueFamily::Assembly | RecordIssueFamily::Presentation => {}
            RecordIssueFamily::Design { type_id }
            | RecordIssueFamily::Sketch { type_id }
            | RecordIssueFamily::Feature { type_id } => {
                map.serialize_entry("type_id", type_id)?;
            }
        }
        map.serialize_entry("segment_token", &self.segment_token)?;
        map.serialize_entry("record_ordinal", &self.record_ordinal)?;
        map.serialize_entry("detail", &self.detail)?;
        map.end()
    }
}

impl RecordIssue {
    pub(crate) fn id(&self) -> String {
        RecordIssueId(self).to_string()
    }
}

#[derive(Serialize, Deserialize)]
struct RecordIssueWire {
    id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_type_id"
    )]
    type_id: Option<String>,
    segment_token: String,
    record_ordinal: u32,
    detail: String,
}

impl From<RecordIssue> for RecordIssueWire {
    fn from(value: RecordIssue) -> Self {
        Self {
            id: value.id(),
            type_id: match value.family {
                RecordIssueFamily::Assembly | RecordIssueFamily::Presentation => None,
                RecordIssueFamily::Design { type_id }
                | RecordIssueFamily::Sketch { type_id }
                | RecordIssueFamily::Feature { type_id } => Some(type_id),
            },
            segment_token: value.segment_token,
            record_ordinal: value.record_ordinal,
            detail: value.detail,
        }
    }
}

impl TryFrom<RecordIssueWire> for RecordIssue {
    type Error = String;

    fn try_from(wire: RecordIssueWire) -> Result<Self, Self::Error> {
        let prefix = wire.id.split_once('#').map(|(prefix, _)| prefix);
        let family = match (prefix, wire.type_id) {
            (Some("inventor:assembly:record-issue"), None) => RecordIssueFamily::Assembly,
            (Some("inventor:presentation:record-issue"), None) => RecordIssueFamily::Presentation,
            (Some("inventor:pmdc:record-issue"), Some(type_id)) => {
                RecordIssueFamily::Design { type_id }
            }
            (Some("inventor:pmdc:sketch-record-issue"), Some(type_id)) => {
                RecordIssueFamily::Sketch { type_id }
            }
            (Some("inventor:pmdc:feature-record-issue"), Some(type_id)) => {
                RecordIssueFamily::Feature { type_id }
            }
            _ => return Err("record issue id family and type_id do not agree".into()),
        };
        let issue = Self {
            family,
            segment_token: wire.segment_token,
            record_ordinal: wire.record_ordinal,
            detail: wire.detail,
        };
        if issue.id() != wire.id {
            return Err("record issue id does not match its record location".into());
        }
        Ok(issue)
    }
}

#[cfg(test)]
mod tests {
    use super::{RecordIssue, RecordIssueFamily, RecordIssueWire};
    use cadmpeg_test_support::refusal::{refusal, states_the_key};

    #[test]
    fn each_family_preserves_its_legacy_wire_and_rejects_mixed_fields() {
        for (family, prefix, typed) in [
            (
                RecordIssueFamily::Assembly,
                "inventor:assembly:record-issue",
                false,
            ),
            (
                RecordIssueFamily::Presentation,
                "inventor:presentation:record-issue",
                false,
            ),
            (
                RecordIssueFamily::Design {
                    type_id: "0123456789abcdef0123456789abcdef".into(),
                },
                "inventor:pmdc:record-issue",
                true,
            ),
            (
                RecordIssueFamily::Sketch {
                    type_id: "0123456789abcdef0123456789abcdef".into(),
                },
                "inventor:pmdc:sketch-record-issue",
                true,
            ),
            (
                RecordIssueFamily::Feature {
                    type_id: "0123456789abcdef0123456789abcdef".into(),
                },
                "inventor:pmdc:feature-record-issue",
                true,
            ),
        ] {
            let issue = RecordIssue {
                family,
                segment_token: "segment".into(),
                record_ordinal: 7,
                detail: "truncated field".into(),
            };
            let mut expected = serde_json::json!({
                "id": format!("{prefix}#segment-7"), "segment_token": "segment", "record_ordinal": 7, "detail": "truncated field"
            });
            if typed {
                expected["type_id"] = serde_json::json!("0123456789abcdef0123456789abcdef");
            }
            assert_eq!(
                serde_json::to_value(&issue).expect("valid test fixture"),
                expected
            );
            assert_eq!(
                serde_json::from_value::<RecordIssue>(expected.clone())
                    .expect("valid test fixture"),
                issue
            );
            let mut wrong_location = expected.clone();
            wrong_location["record_ordinal"] = serde_json::json!(8);
            assert!(serde_json::from_value::<RecordIssue>(wrong_location).is_err());
            if typed {
                expected
                    .as_object_mut()
                    .expect("valid test fixture")
                    .remove("type_id");
            } else {
                expected["type_id"] = serde_json::json!("0123456789abcdef0123456789abcdef");
            }
            assert!(serde_json::from_value::<RecordIssue>(expected).is_err());
        }
    }

    /// A top-level optional key on a record issue names itself in its refusal.
    #[test]
    fn a_top_level_record_issue_key_names_itself_in_its_refusal() {
        states_the_key("type_id", &refusal::<super::RecordIssueWire>("type_id"));
    }

    #[test]
    fn record_issue_borrowed_wire_refuses_retained_limit_before_text_copy() {
        let issue = RecordIssue {
            family: RecordIssueFamily::Design {
                type_id: "0123456789abcdef0123456789abcdef".to_owned(),
            },
            segment_token: "segment".to_owned(),
            record_ordinal: 1,
            detail: "invalid indexed record".to_owned(),
        };
        let wire = RecordIssueWire::from(issue.clone());
        assert_eq!(
            serde_json::to_vec(&issue).expect("borrowed issue"),
            serde_json::to_vec(&wire).expect("owned issue")
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &issue,
            serde_json::to_value(wire).expect("owned issue value"),
        );
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_type_id, String, "type_id");
