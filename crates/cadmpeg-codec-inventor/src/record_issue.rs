// SPDX-License-Identifier: Apache-2.0
//! Located parser failures shared by the Inventor record families.

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};

use crate::record_identity::RecordTypeId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecordIssueFamily {
    Assembly,
    Presentation,
    Design { type_id: RecordTypeId },
    Sketch { type_id: RecordTypeId },
    Feature { type_id: RecordTypeId },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RecordIssueWire")]
pub(crate) struct RecordIssue {
    pub(crate) family: RecordIssueFamily,
    pub(crate) segment_token: cadmpeg_ir::ids::IdentityKey,
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
                map.serialize_entry("type_id", type_id.as_str())?;
            }
        }
        map.serialize_entry("segment_token", self.segment_token.as_str())?;
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

#[cfg(test)]
impl From<RecordIssue> for RecordIssueWire {
    fn from(value: RecordIssue) -> Self {
        Self {
            id: value.id(),
            type_id: match value.family {
                RecordIssueFamily::Assembly | RecordIssueFamily::Presentation => None,
                RecordIssueFamily::Design { type_id }
                | RecordIssueFamily::Sketch { type_id }
                | RecordIssueFamily::Feature { type_id } => Some(type_id.as_str().to_owned()),
            },
            segment_token: value.segment_token.as_str().to_owned(),
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
                RecordIssueFamily::Design { type_id: type_id.try_into().map_err(str::to_owned)? }
            }
            (Some("inventor:pmdc:sketch-record-issue"), Some(type_id)) => {
                RecordIssueFamily::Sketch { type_id: type_id.try_into().map_err(str::to_owned)? }
            }
            (Some("inventor:pmdc:feature-record-issue"), Some(type_id)) => {
                RecordIssueFamily::Feature { type_id: type_id.try_into().map_err(str::to_owned)? }
            }
            _ => return Err("record issue id family and type_id do not agree".into()),
        };
        let issue = Self {
            family,
            segment_token: cadmpeg_ir::ids::IdentityKey::try_new(wire.segment_token)
                .map_err(|error| error.to_string())?,
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
    fn record_issues_reject_invalid_type_guids() {
        for prefix in ["inventor:pmdc:record-issue", "inventor:pmdc:sketch-record-issue", "inventor:pmdc:feature-record-issue"] {
            for type_id in ["not-a-guid", "", "0001", "ABCDEF0123456789abcdef0123456789ab"] {
                let wire = serde_json::json!({"id": format!("{prefix}#segment-0"), "type_id": type_id, "segment_token": "segment", "record_ordinal": 0, "detail": "invalid"});
                assert!(serde_json::from_value::<RecordIssue>(wire).is_err());
            }
        }
    }

    #[test]
    fn record_issues_reject_invalid_segment_tokens() {
        for token in ["", "has space", "has#separator"] {
            let wire = serde_json::json!({"id": format!("inventor:assembly:record-issue#{token}-0"), "segment_token": token, "record_ordinal": 0, "detail": "invalid"});
            assert!(serde_json::from_value::<RecordIssue>(wire).is_err());
        }
    }

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
                    type_id: "0123456789abcdef0123456789abcdef".to_owned().try_into().expect("GUID"),
                },
                "inventor:pmdc:record-issue",
                true,
            ),
            (
                RecordIssueFamily::Sketch {
                    type_id: "0123456789abcdef0123456789abcdef".to_owned().try_into().expect("GUID"),
                },
                "inventor:pmdc:sketch-record-issue",
                true,
            ),
            (
                RecordIssueFamily::Feature {
                    type_id: "0123456789abcdef0123456789abcdef".to_owned().try_into().expect("GUID"),
                },
                "inventor:pmdc:feature-record-issue",
                true,
            ),
        ] {
            let issue = RecordIssue {
                family,
                segment_token: cadmpeg_ir::ids::IdentityKey::try_new("segment").expect("token"),
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
                type_id: "0123456789abcdef0123456789abcdef".to_owned().try_into().expect("GUID"),
            },
            segment_token: cadmpeg_ir::ids::IdentityKey::try_new("segment").expect("token"),
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
