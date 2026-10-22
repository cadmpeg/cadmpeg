// SPDX-License-Identifier: Apache-2.0
//! Decode-local lane Views and pending fidelity pointers for admitted records.

use crate::container::ContainerScan;
use crate::records::FeatureInputLane;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct BodySources<'a> {
    views: BTreeMap<String, View<'a>>,
    pointers: BTreeMap<String, BTreeMap<String, u64>>,
}

impl<'a> BodySources<'a> {
    pub(crate) fn from_views(
        views: impl IntoIterator<Item = (String, View<'a>)>,
    ) -> Result<Self, CodecError> {
        let mut result = Self::default();
        for (id, view) in views {
            if result.views.insert(id, view).is_some() {
                return Err(CodecError::malformed(
                    "duplicate body-selection lane source",
                ));
            }
        }
        Ok(result)
    }

    /// Split a real root containing the exact concatenation the caller authored.
    /// This does not register an alias or turn a retained copy into source data.
    pub(crate) fn from_concatenated(
        lanes: &[FeatureInputLane],
        source: View<'a>,
    ) -> Result<Self, CodecError> {
        let mut cursor = source.start();
        let mut views = Vec::new();
        for lane in lanes {
            let end = cursor
                .checked_add(lane.native_payload.len())
                .ok_or_else(|| CodecError::malformed("body lane source length overflow"))?;
            let view = source
                .child(cursor, end)
                .ok_or_else(|| CodecError::malformed("body lane source exceeds root"))?;
            if view.window() != lane.native_payload.as_slice() {
                return Err(CodecError::malformed(
                    "body lane source concatenation differs",
                ));
            }
            views.push((lane.id.clone(), view));
            cursor = end;
        }
        if cursor != source.end() {
            return Err(CodecError::malformed(
                "body lane source concatenation has extra bytes",
            ));
        }
        Self::from_views(views)
    }

    pub(crate) fn from_scan(
        scan: &ContainerScan<'a>,
        primary: &[FeatureInputLane],
        supplemental: &[FeatureInputLane],
    ) -> Result<Self, CodecError> {
        let mut views = Vec::new();
        for section in scan.sections() {
            for family in ["resolved-features", "config-objects"] {
                let id = super::assembly::source_id(section, family);
                if primary.iter().chain(supplemental).any(|lane| lane.id == id) {
                    let view = scan.section_view(section).ok_or_else(|| {
                        CodecError::malformed("body-selection lane lacks its source View")
                    })?;
                    views.push((id, view));
                }
            }
        }
        Self::from_views(views)
    }

    pub(crate) fn view(&self, lane: &FeatureInputLane) -> Result<View<'a>, CodecError> {
        let source = self
            .views
            .get(&lane.id)
            .copied()
            .ok_or_else(|| CodecError::malformed("body-selection lane source is missing"))?;
        if source.window() != lane.native_payload.as_slice() {
            return Err(CodecError::malformed(
                "body-selection lane differs from its source View",
            ));
        }
        Ok(source)
    }

    pub(crate) fn pointers(&self) -> &BTreeMap<String, BTreeMap<String, u64>> {
        &self.pointers
    }

    pub(crate) fn pointer(&self, id: &str) -> Option<u64> {
        self.pointers
            .get(id)
            .and_then(|fields| fields.get("offset"))
            .copied()
    }

    pub(crate) fn insert(&mut self, id: String, offset: u64) -> Result<(), CodecError> {
        if self
            .pointers
            .insert(id, BTreeMap::from([("offset".into(), offset)]))
            .is_some()
        {
            return Err(CodecError::malformed(
                "duplicate feature-input source pointer",
            ));
        }
        Ok(())
    }

    pub(crate) fn into_pointers(self) -> BTreeMap<String, BTreeMap<String, u64>> {
        self.pointers
    }
}

#[cfg(test)]
mod tests;
