// SPDX-License-Identifier: Apache-2.0
//! Located parser failures shared by the Inventor record families.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecordIssueFamily {
    Assembly,
    Presentation,
    Design { type_id: String },
    Sketch { type_id: String },
    Feature { type_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RecordIssueWire", into = "RecordIssueWire")]
pub(crate) struct RecordIssue {
    pub(crate) family: RecordIssueFamily,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) detail: String,
}

impl RecordIssue {
    pub(crate) fn id(&self) -> String {
        let prefix = match self.family {
            RecordIssueFamily::Assembly => "inventor:assembly:record-issue",
            RecordIssueFamily::Presentation => "inventor:presentation:record-issue",
            RecordIssueFamily::Design { .. } => "inventor:pmdc:record-issue",
            RecordIssueFamily::Sketch { .. } => "inventor:pmdc:sketch-record-issue",
            RecordIssueFamily::Feature { .. } => "inventor:pmdc:feature-record-issue",
        };
        format!("{prefix}#{}-{}", self.segment_token, self.record_ordinal)
    }
}

#[derive(Serialize, Deserialize)]
struct RecordIssueWire {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    use super::{RecordIssue, RecordIssueFamily};

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
            assert_eq!(serde_json::to_value(&issue).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<RecordIssue>(expected.clone()).unwrap(),
                issue
            );
            let mut wrong_location = expected.clone();
            wrong_location["record_ordinal"] = serde_json::json!(8);
            assert!(serde_json::from_value::<RecordIssue>(wrong_location).is_err());
            if typed {
                expected.as_object_mut().unwrap().remove("type_id");
            } else {
                expected["type_id"] = serde_json::json!("0123456789abcdef0123456789abcdef");
            }
            assert!(serde_json::from_value::<RecordIssue>(expected).is_err());
        }
    }
}
