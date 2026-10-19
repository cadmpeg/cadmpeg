// SPDX-License-Identifier: Apache-2.0
//! Decode source string values before storing typed native records.

use serde::{Serialize, Serializer};

/// Transient source encoding. Only decoded text or length/hash evidence is
/// serialized into CADIR; the source image in fidelity owns the encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeText<T = Vec<u8>>(T);

impl<T> From<T> for NativeText<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T: AsRef<[u8]>> NativeText<T> {
    /// Read the semantic string when its source encoding is admitted UTF-8.
    pub(crate) fn text(&self) -> Option<&str> {
        std::str::from_utf8(self.0.as_ref()).ok()
    }
}

impl<T: AsRef<[u8]>> Serialize for NativeText<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match std::str::from_utf8(self.0.as_ref()) {
            Ok(text) => serializer.serialize_str(text),
            Err(_) => {
                #[derive(Serialize)]
                struct UnreadableText {
                    #[serde(flatten)]
                    evidence: cadmpeg_ir::source_fidelity::SourceEvidence,
                    diagnostic: &'static str,
                }
                UnreadableText {
                    evidence: cadmpeg_ir::source_fidelity::SourceEvidence::of(self.0.as_ref()),
                    diagnostic: "native text is not UTF-8",
                }
                .serialize(serializer)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NativeText;

    #[test]
    fn decoded_text_is_stored_as_text_instead_of_its_source_encoding() {
        assert_eq!(
            serde_json::to_value(NativeText::from("Rib α".as_bytes())).unwrap(),
            serde_json::json!("Rib α")
        );
    }

    #[test]
    fn unreadable_text_keeps_evidence_without_serializing_a_payload() {
        let source = [0xff, 0xfe, 0x80];
        let value = serde_json::to_value(NativeText::from(source)).unwrap();
        assert_eq!(value["length"], 3);
        assert_eq!(
            value["sha256"],
            serde_json::to_value(cadmpeg_ir::source_fidelity::SourceEvidence::of(&source).sha256)
                .unwrap()
        );
        assert_eq!(value["diagnostic"], "native text is not UTF-8");
        assert_eq!(value.as_object().unwrap().len(), 3);
    }
}
