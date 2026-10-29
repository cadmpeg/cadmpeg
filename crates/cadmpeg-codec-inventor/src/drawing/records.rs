// SPDX-License-Identifier: Apache-2.0
//! Drawing sheet and view record prefixes.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize, Serializer};

use super::Family;
use crate::layout::{
    drawing_alternate_view_fixed as alternate_fixed, drawing_cached_view_prefix as cache_prefix,
    drawing_main_view_fixed as view_fixed, drawing_model_reference_prefix as model_prefix,
    drawing_sheet_controls as sheet_controls, drawing_sheet_prefix as sheet_prefix,
    drawing_sheet_property_list as sheet_list, drawing_sheet_tail_prefix as sheet_tail_prefix,
    drawing_sheet_terminal as sheet_terminal, drawing_view_operand_list as view_list,
    drawing_view_prefix as view_prefix,
};
use crate::pmdc::{reference_list_with_context, Cursor, PmDcReference, PmDcReferenceList};
use crate::record_identity::RecordTypeId;
use crate::records::RseRecordFrame;

const SHEET: [u8; 16] = [
    0xcd, 0x90, 0x7c, 0xd3, 0xd0, 0x11, 0x16, 0xfa, 0x60, 0x00, 0x0d, 0xbd, 0x86, 0x1c, 0x3c, 0xb0,
];
const VIEW: [u8; 16] = [
    0x59, 0x42, 0xcb, 0x22, 0xd1, 0x11, 0xc5, 0x57, 0x60, 0x00, 0x37, 0xbd, 0x86, 0x1c, 0x3c, 0xb0,
];
const ALTERNATE_VIEW: [u8; 16] = [
    0xc1, 0x96, 0x59, 0x62, 0xd4, 0x11, 0x4e, 0x8e, 0x60, 0x00, 0x95, 0x87, 0x3f, 0xa6, 0xfb, 0xb0,
];
const MODEL_REFERENCE: [u8; 16] = [
    0x69, 0xa5, 0x6f, 0x6b, 0xd2, 0x11, 0x2c, 0x3d, 0x60, 0x00, 0xb5, 0x8d, 0x31, 0x25, 0xc3, 0xb0,
];
const CACHED_VIEW: [u8; 16] = [
    0x81, 0x13, 0x6d, 0x8a, 0xd1, 0x11, 0x56, 0x6b, 0x60, 0x00, 0x38, 0xbd, 0x86, 0x1c, 0x3c, 0xb0,
];

const VIEW_OPERAND_TYPES: [&str; 19] = [
    "2e187506d2111f5e6000b3856c2387b0",
    "9ff49f1dd11101e96000a8ade3bc87b0",
    "3edf52ced011d0d20008ccbc0663dc09",
    "3edf52ced011d0d20008ccbc0663dc09",
    "4debf5fad41120726000bebead9287b0",
    "35df52ced011d0d20008ccbc0663dc09",
    "474dd06b2047478c9da481bcfae149d5",
    "229630f7d111a466600027a5602d6bb0",
    "5c42cb22d111c557600037bd861c3cb0",
    "284d8790d011f8d10008cabc0663dc09",
    "284d8790d011f8d10008cabc0663dc09",
    "ebf44026d111e457600037bd861c3cb0",
    "264d8790d011f8d10008cabc0663dc09",
    "264d8790d011f8d10008cabc0663dc09",
    "e9ee786bd11195a460009dade3bc87b0",
    "e9ee786bd11195a460009dade3bc87b0",
    "2e187506d2111f5e6000b3856c2387b0",
    "9ff49f1dd11101e96000a8ade3bc87b0",
    "35df52ced011d0d20008ccbc0663dc09",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    pub(crate) id: String,
    #[serde(with = "super::token_wire")]
    pub(crate) segment_token: IdentityKey,
    #[serde(rename = "record_ordinal")]
    pub(crate) ordinal: u32,
    pub(crate) segment_version_major: u8,
    #[serde(serialize_with = "serialize_type_id")]
    pub(crate) type_id: RecordTypeId,
    pub(crate) family: Family,
    pub(crate) payload: NativeBytes,
    pub(crate) fields: Fields,
}

fn serialize_type_id<S: Serializer>(
    value: &RecordTypeId,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value.as_str())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum Fields {
    CachedView {
        prefix: NativeBytes<[u8; cache_prefix::VIEW_REFERENCE]>,
        view: PmDcReference,
        tail: Tail,
    },
    ModelReference {
        prefix: NativeBytes<[u8; model_prefix::LEN]>,
        path: String,
        tail: Tail,
    },
    Sheet {
        header: NativeBytes<[u8; sheet_prefix::IDENTIFIER]>,
        identifier: NativeBytes<[u8; 16]>,
        label: String,
        properties: PmDcReferenceList,
        controls: NativeBytes<[u8; sheet_controls::LEN]>,
        views: PmDcReferenceList,
        tail_prefix: NativeBytes<[u8; sheet_tail_prefix::LEN]>,
        auxiliary_lists: Box<[PmDcReferenceList; 3]>,
        terminal: NativeBytes<[u8; sheet_terminal::LEN]>,
        tail: Tail,
    },
    View {
        prefix: NativeBytes<[u8; view_prefix::LEN]>,
        operands: PmDcReferenceList,
        word: u16,
        values: [u32; 2],
        extension: Option<u32>,
        text: String,
        tail: Tail,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Tail {
    offset: u64,
    len: u64,
    sha256: Sha256Digest,
}

impl Tail {
    fn of(view: View<'_>) -> Self {
        Self {
            offset: cadmpeg_core::decode::u64_from_index(view.read_len()),
            len: cadmpeg_core::decode::u64_from_index(view.remaining()),
            sha256: Sha256Digest::digest(view.unread()),
        }
    }
}

pub(super) fn recognizes(family: Family, type_id: [u8; 16]) -> bool {
    (family == Family::Sheet && matches!(type_id, SHEET | VIEW | ALTERNATE_VIEW))
        || (family == Family::Browser && type_id == MODEL_REFERENCE)
        || (family == Family::SheetCache && type_id == CACHED_VIEW)
}

impl Record {
    pub(super) fn local_references(&self) -> impl Iterator<Item = PmDcReference> + '_ {
        let groups = match &self.fields {
            Fields::Sheet {
                properties,
                views,
                auxiliary_lists,
                ..
            } => [
                Some(properties),
                Some(views),
                Some(&auxiliary_lists[0]),
                Some(&auxiliary_lists[1]),
                Some(&auxiliary_lists[2]),
            ],
            Fields::View { operands, .. } => [Some(operands), None, None, None, None],
            Fields::CachedView { .. } | Fields::ModelReference { .. } => [None; 5],
        };
        groups
            .into_iter()
            .flatten()
            .flat_map(|list| list.references().iter().copied())
    }

    pub(super) fn operands_are_valid<'a>(&self, lookup: impl Fn(u32) -> Option<&'a str>) -> bool {
        let local = |reference: &PmDcReference, expected: &str| {
            reference.qualified()
                && reference
                    .record_ordinal()
                    .is_some_and(|ordinal| lookup(ordinal) == Some(expected))
        };
        match &self.fields {
            Fields::View { operands, .. } => {
                operands.references().len() == VIEW_OPERAND_TYPES.len()
                    && operands
                        .references()
                        .iter()
                        .zip(VIEW_OPERAND_TYPES)
                        .all(|(reference, expected)| local(reference, expected))
            }
            Fields::Sheet {
                properties,
                auxiliary_lists,
                ..
            } => {
                properties
                    .references()
                    .iter()
                    .all(|reference| local(reference, "754d8790d011f8d10008cabc0663dc09"))
                    && auxiliary_lists[..2]
                        .iter()
                        .flat_map(PmDcReferenceList::references)
                        .all(|reference| local(reference, "b2206557d11196a460001b8aeb49cdb0"))
            }
            Fields::CachedView { .. } | Fields::ModelReference { .. } => true,
        }
    }

    pub(super) fn definition_header(&self) -> Option<(u32, u16)> {
        if self.family != Family::Sheet
            || self.type_id.as_str() != "5942cb22d111c557600037bd861c3cb0"
        {
            return None;
        }
        let Fields::View { prefix, .. } = &self.fields else {
            return None;
        };
        let mut view = View::over_retained(prefix.as_ref());
        Some((view.u32_le()?, view.u16_le()?))
    }

    pub(super) fn read(
        ctx: &DecodeContext<'_>,
        token: &IdentityKey,
        family: Family,
        version: u8,
        frame: &RseRecordFrame<'_>,
    ) -> Result<Self, CodecError> {
        let fields = parse(ctx, family, version, frame.type_id, frame.payload)?;
        let count = frame
            .payload
            .counted(u64::from(frame.payload_len()?), 1)
            .ok_or_else(|| CodecError::malformed("drawing payload length exceeds its frame"))?;
        let mut payload = ctx.alloc_filled(count.get(), 0_u8, "retain Inventor drawing payload")?;
        payload.copy_from_slice(frame.payload.window());
        Ok(Self {
            id: Self::identity(token, frame.ordinal),
            segment_token: token.clone(),
            ordinal: frame.ordinal,
            segment_version_major: version,
            type_id: RecordTypeId::from_bytes(frame.type_id),
            family,
            payload: payload.into(),
            fields,
        })
    }

    pub(crate) fn valid(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        if self.id != Self::identity(&self.segment_token, self.ordinal) {
            return Ok(false);
        }
        let type_id = match self.type_id.as_str() {
            "cd907cd3d01116fa60000dbd861c3cb0" => SHEET,
            "5942cb22d111c557600037bd861c3cb0" => VIEW,
            "c1965962d4114e8e600095873fa6fbb0" => ALTERNATE_VIEW,
            "69a56f6bd2112c3d6000b58d3125c3b0" => MODEL_REFERENCE,
            "81136d8ad111566b600038bd861c3cb0" => CACHED_VIEW,
            _ => return Ok(false),
        };
        match parse(
            ctx,
            self.family,
            self.segment_version_major,
            type_id,
            View::over_retained(&self.payload),
        ) {
            Ok(fields) => Ok(fields == self.fields),
            Err(error @ CodecError::ResourceLimit(_)) => Err(error),
            Err(_) => Ok(false),
        }
    }

    fn identity(token: &IdentityKey, ordinal: u32) -> String {
        format!("inventor:drawing:record#{token}-{ordinal}")
    }
}

fn parse(
    ctx: &DecodeContext<'_>,
    family: Family,
    version: u8,
    type_id: [u8; 16],
    payload: View<'_>,
) -> Result<Fields, CodecError> {
    if version != 19 || !recognizes(family, type_id) {
        return Err(CodecError::malformed(
            "drawing record has an unsupported family, type, or version",
        ));
    }
    let mut view = payload;
    if type_id == CACHED_VIEW {
        let prefix = crate::reader::array(&mut view, "cached drawing view prefix")?.into();
        let reference = crate::reader::pmdc_reference(&mut view, "cached drawing view definition")?;
        if reference.qualified() || reference.index() == 0 {
            return Err(CodecError::malformed(
                "cached drawing view requires an external reference",
            ));
        }
        let tail = Tail::of(view);
        Ok(Fields::CachedView {
            prefix,
            view: reference,
            tail,
        })
    } else if type_id == MODEL_REFERENCE {
        let prefix =
            crate::reader::array(&mut view, "drawing modeled-document reference prefix")?.into();
        let path = text(ctx, &mut view, "drawing modeled-document path")?;
        let supported_suffix = path
            .rsplit_once('.')
            .is_some_and(|(_, suffix)| matches!(suffix, "ipt" | "iam"));
        if !supported_suffix {
            return Err(CodecError::malformed(
                "drawing modeled-document path has an unsupported suffix",
            ));
        }
        let tail = Tail::of(view);
        Ok(Fields::ModelReference { prefix, path, tail })
    } else if type_id == SHEET {
        let header = crate::reader::array(&mut view, "drawing sheet header")?.into();
        let identifier = crate::reader::array(&mut view, "drawing sheet identifier")?.into();
        let label = text(ctx, &mut view, "drawing sheet label")?;
        let properties = list(ctx, &mut view, "drawing sheet properties")?;
        if cadmpeg_core::decode::u64_from_index(properties.references().len())
            != u64::from(sheet_list::COUNT_VALUE)
        {
            return Err(CodecError::malformed(
                "drawing sheet requires six property references",
            ));
        }
        let controls = crate::reader::array(&mut view, "drawing sheet controls")?.into();
        let views = list(ctx, &mut view, "drawing sheet views")?;
        let tail = Tail::of(view);
        let tail_prefix = crate::reader::array(&mut view, "drawing sheet tail prefix")?.into();
        let auxiliary_lists = Box::new([
            list(ctx, &mut view, "drawing sheet auxiliary list 0")?,
            list(ctx, &mut view, "drawing sheet auxiliary list 1")?,
            list(ctx, &mut view, "drawing sheet auxiliary views")?,
        ]);
        let terminal = crate::reader::array(&mut view, "drawing sheet terminal")?.into();
        if !view.is_empty() {
            return Err(CodecError::malformed("drawing sheet has trailing bytes"));
        }
        Ok(Fields::Sheet {
            header,
            identifier,
            label,
            properties,
            controls,
            views,
            tail_prefix,
            auxiliary_lists,
            terminal,
            tail,
        })
    } else {
        let fixed_len = if type_id == ALTERNATE_VIEW {
            alternate_fixed::LEN
        } else {
            view_fixed::LEN
        };
        let mut fixed = view;
        crate::reader::take(&mut fixed, fixed_len, "drawing view fixed fields")?;
        let prefix = crate::reader::array(&mut view, "drawing view prefix")?.into();
        let operands = list(ctx, &mut view, "drawing view operands")?;
        if cadmpeg_core::decode::u64_from_index(operands.references().len())
            != u64::from(view_list::COUNT_VALUE)
        {
            return Err(CodecError::malformed(
                "drawing view requires nineteen operand references",
            ));
        }
        let word = crate::reader::u16(&mut view, "drawing view word")?;
        let values = crate::reader::u32_array(&mut view, "drawing view values")?;
        let extension = if type_id == ALTERNATE_VIEW {
            Some(crate::reader::u32(
                &mut view,
                "alternate drawing view value",
            )?)
        } else {
            None
        };
        let text = text(ctx, &mut view, "drawing view text")?;
        let tail = Tail::of(view);
        Ok(Fields::View {
            prefix,
            operands,
            word,
            values,
            extension,
            text,
            tail,
        })
    }
}

fn text(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    field: &'static str,
) -> Result<String, CodecError> {
    let count = crate::reader::u32(view, "drawing text length")?;
    let bounded = view
        .counted(u64::from(count), 2)
        .ok_or_else(|| CodecError::malformed("drawing text count exceeds its payload"))?;
    crate::reader::utf16_text(ctx, view, bounded.get(), field, "retain drawing text")
}

fn list(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    field: &str,
) -> Result<PmDcReferenceList, CodecError> {
    let mut cursor = Cursor::new(*view);
    let list = reference_list_with_context(ctx, &mut cursor, sheet_list::MARKER_VALUE, field)?;
    *view = cursor.into_view();
    Ok(list)
}

#[cfg(test)]
mod tests;
