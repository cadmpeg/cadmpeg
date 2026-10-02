// SPDX-License-Identifier: Apache-2.0
//! Borrow provenance location and tag data during serialization.

use super::{AnnotationLocation, Provenance, SourceLocation, StreamName};
use serde::{Serialize, Serializer};

impl Serialize for StreamName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serializer.serialize_str(self.as_str()) }
}

impl Serialize for Provenance<AnnotationLocation> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> { stream: &'a StreamName, offset: u64, #[serde(skip_serializing_if = "Option::is_none")] tag: Option<&'a str> }
        Wire { stream: &self.location.stream, offset: self.offset, tag: self.tag.as_deref() }.serialize(serializer)
    }
}

impl Serialize for Provenance<SourceLocation> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> { format: &'a str, #[serde(skip_serializing_if = "Option::is_none")] stream: Option<&'a StreamName>, offset: u64, #[serde(skip_serializing_if = "Option::is_none")] tag: Option<&'a str> }
        Wire { format: &self.location.format, stream: self.location.stream.as_ref(), offset: self.offset, tag: self.tag.as_deref() }.serialize(serializer)
    }
}
