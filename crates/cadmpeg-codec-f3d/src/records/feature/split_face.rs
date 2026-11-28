use crate::records::identity::Located;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_control, DesignSplitFaceMethodControl, "control");
cadmpeg_core::named_optional_field!(
    deserialize_carrier_frame,
    DesignSplitFaceCarrierFrame,
    "carrier_frame"
);
cadmpeg_core::named_optional_field!(
    deserialize_direction,
    Located<Option<[FiniteReal; 3]>>,
    "direction"
);
cadmpeg_core::named_optional_field!(deserialize_input_count, Located<u32>, "input_count");
cadmpeg_core::named_optional_field!(
    deserialize_same_document_asset,
    String,
    "same_document_asset"
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignSplitFaceOperation {
    pub(crate) method_offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_control"
    )]
    pub(crate) control: Option<DesignSplitFaceMethodControl>,
    pub(crate) direction_reference: DesignSplitFaceDirectionReference,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_carrier_frame"
    )]
    pub(crate) carrier_frame: Option<DesignSplitFaceCarrierFrame>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_direction"
    )]
    pub(crate) direction: Option<Located<Option<[FiniteReal; 3]>>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) inputs: Vec<Located<DesignSplitFaceDirectionReference>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_input_count"
    )]
    pub(crate) input_count: Option<Located<u32>>,
    #[serde(default)]
    pub(crate) inputs_complete: bool,
}

impl DesignSplitFaceOperation {
    pub(crate) fn unresolved(method_offset: u64) -> Self {
        Self {
            method_offset,
            control: None,
            direction_reference: DesignSplitFaceDirectionReference::Unavailable,
            carrier_frame: None,
            direction: None,
            inputs: Vec::new(),
            input_count: None,
            inputs_complete: false,
        }
    }

    pub(crate) fn method(&self) -> DesignSplitFaceMethod {
        let Some(control) = self.control else {
            return DesignSplitFaceMethod::Unresolved;
        };
        let null = matches!(
            self.direction_reference,
            DesignSplitFaceDirectionReference::Null
        );
        let available = !matches!(
            self.direction_reference,
            DesignSplitFaceDirectionReference::Unavailable
        );
        match (
            control.family,
            control.projection,
            control.extension,
            null,
            available,
        ) {
            (1, 0, extension @ 0..=1, true, true) => DesignSplitFaceMethod::SurfaceIntersection {
                extend: extension != 0,
            },
            (1, 1, 0, true, true) => DesignSplitFaceMethod::ClosestPoint,
            (0, 1, 0, false, true) => DesignSplitFaceMethod::AlongVector,
            _ => DesignSplitFaceMethod::Unresolved,
        }
    }

    pub(crate) fn scope_offsets_valid(&self, start: u64, end: u64) -> bool {
        let Some(prologue) = start.checked_add(cadmpeg_core::decode::u64_from_index(
            crate::layout::split_face_scope_prefix::LEN,
        )) else {
            return false;
        };
        if self.method_offset < prologue || self.method_offset > end {
            return false;
        }
        if self.control.is_some()
            && (prologue
                .checked_add(3)
                .is_none_or(|minimum| self.method_offset < minimum)
                || self
                    .method_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(
                        crate::layout::split_face_method_flags::LEN,
                    ))
                    .is_none_or(|after| after > end))
        {
            return false;
        }
        if let Some(target) = self.direction_reference.target() {
            if target.offset <= prologue
                || self
                    .direction_reference
                    .encoded_end(target.offset)
                    .is_none_or(|after| after != self.method_offset)
            {
                return false;
            }
        }
        if matches!(
            self.direction_reference,
            DesignSplitFaceDirectionReference::Unavailable
        ) && self.control.is_some()
        {
            return false;
        }
        let Some(frame) = self.carrier_frame else {
            return self.direction.is_none()
                && self.input_count.is_none()
                && self.inputs.is_empty()
                && !self.inputs_complete;
        };
        if !(frame.primary_offset < frame.secondary_offset
            && frame.secondary_offset < frame.end_offset)
            || frame
                .secondary_offset
                .checked_add(11)
                .is_none_or(|after| after > frame.end_offset)
            || !matches!(frame.version, 1 | 2 | 4)
        {
            return false;
        }
        let carrier_start = frame.primary_offset;
        let carrier_end = frame.secondary_offset;
        let Some(direction) = &self.direction else {
            return false;
        };
        if self.direction_reference.local_record_index().is_none()
            || carrier_start
                .checked_add(cadmpeg_core::decode::u64_from_index(
                    crate::layout::design_hole_point_data_v1_prefix::DIRECTION,
                ))
                .is_none_or(|minimum| direction.offset < minimum)
            || direction
                .offset
                .checked_add(24)
                .is_none_or(|after| after > carrier_end)
        {
            return false;
        }
        let Some(count) = self.input_count else {
            return self.inputs.is_empty() && !self.inputs_complete;
        };
        let Some(mut cursor) = count
            .offset
            .checked_add(4)
            .filter(|after| *after <= carrier_end)
        else {
            return false;
        };
        let count_after_direction = crate::layout::design_hole_point_data_v1_prefix::INPUT_COUNT
            - crate::layout::design_hole_point_data_v1_prefix::DIRECTION
            + if frame.version == 1 {
                0
            } else {
                std::mem::size_of::<[f64; 3]>()
            }
            + usize::from(frame.version == 4);
        if direction
            .offset
            .checked_add(cadmpeg_core::decode::u64_from_index(count_after_direction))
            .is_none_or(|after| after != count.offset)
            || u64::try_from(self.inputs.len()).ok().is_none_or(|length| {
                length > u64::from(count.value)
                    || self.inputs_complete && length != u64::from(count.value)
            })
        {
            return false;
        }
        for input in &self.inputs {
            if input.offset != cursor || input.offset >= carrier_end {
                return false;
            }
            if input
                .value
                .target()
                .is_some_and(|target| input.offset.checked_add(1) != Some(target.offset))
            {
                return false;
            }
            let Some(after) = input
                .value
                .encoded_len()
                .and_then(|length| input.offset.checked_add(length))
                .filter(|after| *after <= carrier_end)
            else {
                return false;
            };
            cursor = after;
        }
        !self.inputs_complete || cursor == carrier_end
    }

    pub(crate) fn carrier_type_assumed(&self) -> bool {
        self.carrier_frame.is_some()
            && matches!(&self.direction_reference,
                DesignSplitFaceDirectionReference::Local { inline_type_guid: Some(guid), .. }
                if !guid.eq_ignore_ascii_case(crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignSplitFaceCarrierFrame {
    pub(crate) primary_offset: u64,
    pub(crate) secondary_offset: u64,
    pub(crate) end_offset: u64,
    pub(crate) version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignSplitFaceMethodControl {
    pub(crate) family: u32,
    pub(crate) projection: u8,
    pub(crate) extension: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DesignSplitFaceDirectionReference {
    Unavailable,
    Null,
    Local {
        target: Located<u64>,
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        inline_type_guid: Option<String>,
    },
    CrossSegment {
        target: Located<u64>,
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        inline_type_guid: Option<String>,
        segment: u32,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_same_document_asset"
        )]
        same_document_asset: Option<String>,
    },
    CrossDocument {
        target: Located<u64>,
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        inline_type_guid: Option<String>,
        segment: u32,
        asset_guid: String,
        target_type_guid: String,
        external_type_guid_present: bool,
        link_name: String,
        #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
        version: Option<(String, String)>,
    },
}

impl DesignSplitFaceDirectionReference {
    fn target(&self) -> Option<&Located<u64>> {
        match self {
            Self::Local { target, .. }
            | Self::CrossSegment { target, .. }
            | Self::CrossDocument { target, .. } => Some(target),
            Self::Unavailable | Self::Null => None,
        }
    }
    fn encoded_len(&self) -> Option<u64> {
        let text_units = |text: &str, maximum: usize| {
            let count = text.encode_utf16().count();
            (count <= maximum).then(|| cadmpeg_core::decode::u64_from_index(count))
        };
        let inline = match self {
            Self::Local {
                inline_type_guid, ..
            }
            | Self::CrossSegment {
                inline_type_guid, ..
            }
            | Self::CrossDocument {
                inline_type_guid, ..
            } => inline_type_guid.as_deref(),
            Self::Null => return Some(1),
            Self::Unavailable => return None,
        };
        if inline.is_some_and(|guid| !crate::bytes::is_guid_hyphenated(guid)) {
            return None;
        }
        let typed = if inline.is_some() { 40 } else { 0 };
        match self {
            Self::Local { .. } => Some(11 + typed),
            Self::CrossSegment {
                same_document_asset: None,
                ..
            } => Some(15 + typed),
            Self::CrossSegment {
                same_document_asset: Some(asset),
                ..
            } => (19_u64 + typed).checked_add(text_units(asset, 64)?.checked_mul(2)?),
            Self::CrossDocument {
                asset_guid,
                target_type_guid,
                external_type_guid_present,
                link_name,
                version,
                ..
            } => {
                if !crate::bytes::is_guid_hyphenated(target_type_guid)
                    || inline.is_none() && !external_type_guid_present
                    || !external_type_guid_present
                        && inline.is_some_and(|guid| !guid.eq_ignore_ascii_case(target_type_guid))
                {
                    return None;
                }
                let mut size = (24_u64 + typed)
                    .checked_add(text_units(asset_guid, 64)?.checked_mul(2)?)?
                    .checked_add(text_units(link_name, 256)?.checked_mul(2)?)?
                    .checked_add(if *external_type_guid_present { 40 } else { 0 })?;
                if let Some((key, value)) = version {
                    if key.encode_utf16().count() != 36 {
                        return None;
                    }
                    size = size
                        .checked_add(80)?
                        .checked_add(text_units(value, 256)?.checked_mul(2)?)?;
                }
                Some(size)
            }
            Self::Unavailable | Self::Null => None,
        }
    }

    fn encoded_end(&self, target_offset: u64) -> Option<u64> {
        target_offset
            .checked_sub(1)?
            .checked_add(self.encoded_len()?)
    }

    pub(crate) fn local_record_index(&self) -> Option<u32> {
        match self {
            Self::Local { target, .. } => {
                u32::try_from(target.value).ok().filter(|value| *value != 0)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DesignSplitFaceMethod {
    Unresolved,
    SurfaceIntersection { extend: bool },
    ClosestPoint,
    AlongVector,
}

rewrite_native_record!(DesignSplitFaceOperation, []; {method_offset, control, direction_reference, carrier_frame, direction, inputs, input_count, inputs_complete});
rewrite_native_record!(DesignSplitFaceCarrierFrame, []; {primary_offset, secondary_offset, end_offset, version});
rewrite_native_record!(DesignSplitFaceMethodControl, []; {family, projection, extension});
rewrite_native_enum!(DesignSplitFaceDirectionReference, []; {
    Unavailable,
    Null,
    Local { target, inline_type_guid },
    CrossSegment { target, inline_type_guid, segment, same_document_asset },
    CrossDocument { target, inline_type_guid, segment, asset_guid, target_type_guid, external_type_guid_present, link_name, version },
});

#[cfg(test)]
mod tests;
