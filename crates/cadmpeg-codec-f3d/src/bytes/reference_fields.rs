use super::{is_guid_hyphenated, lp_u32_bytes_at, lp_utf16_raw, Reference, Utf16View};
use cadmpeg_core::decode::View;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged,
)]
#[serde(
    tag = "state",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum ReadField<T> {
    Absent,
    #[default]
    Unread,
    Value(T),
}

impl<T> ReadField<T> {
    pub(crate) fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Absent | Self::Unread => None,
        }
    }

    fn map<U>(self, f: impl FnOnce(T) -> U) -> ReadField<U> {
        match self {
            Self::Absent => ReadField::Absent,
            Self::Unread => ReadField::Unread,
            Self::Value(value) => ReadField::Value(f(value)),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    bound(
        serialize = "G: Serialize, L: Serialize",
        deserialize = "G: ::serde::de::DeserializeOwned, L: ::serde::de::DeserializeOwned"
    )
)]
pub(crate) struct ReferenceFields<G, L> {
    pub(crate) complete: bool,
    pub(crate) present: ReadField<u8>,
    pub(crate) target: ReadField<u64>,
    pub(crate) inline_type_guid: ReadField<G>,
    pub(crate) cross_document: ReadField<u8>,
    pub(crate) different_segment: ReadField<u8>,
    pub(crate) segment: ReadField<u32>,
    pub(crate) asset_guid: ReadField<L>,
    pub(crate) same_document: ReadField<u8>,
    pub(crate) target_type_guid: ReadField<G>,
    pub(crate) link_name: ReadField<L>,
    pub(crate) versioned: ReadField<u8>,
    pub(crate) version_key_guid: ReadField<L>,
    pub(crate) version_urn: ReadField<L>,
}

impl ReferenceFields<&str, Utf16View<'_>> {
    pub(crate) fn into_owned(self) -> ReferenceFields<String, String> {
        ReferenceFields {
            complete: self.complete,
            present: self.present,
            target: self.target,
            inline_type_guid: self.inline_type_guid.map(str::to_owned),
            cross_document: self.cross_document,
            different_segment: self.different_segment,
            segment: self.segment,
            asset_guid: self.asset_guid.map(|text| text.to_string()),
            same_document: self.same_document,
            target_type_guid: self.target_type_guid.map(str::to_owned),
            link_name: self.link_name.map(|text| text.to_string()),
            versioned: self.versioned,
            version_key_guid: self.version_key_guid.map(|text| text.to_string()),
            version_urn: self.version_urn.map(|text| text.to_string()),
        }
    }
}

impl<G: AsRef<str>, L> ReferenceFields<G, L> {
    pub(crate) fn local(&self) -> Option<(u64, Option<&str>)> {
        if !self.complete
            || self.present != ReadField::Value(1)
            || self.cross_document != ReadField::Value(0)
            || self.different_segment != ReadField::Value(0)
            || !matches!(self.segment, ReadField::Absent)
            || !matches!(self.asset_guid, ReadField::Absent)
            || !matches!(self.same_document, ReadField::Absent)
            || !matches!(self.target_type_guid, ReadField::Absent)
            || !matches!(self.link_name, ReadField::Absent)
            || !matches!(self.versioned, ReadField::Absent)
            || !matches!(self.version_key_guid, ReadField::Absent)
            || !matches!(self.version_urn, ReadField::Absent)
        {
            return None;
        }
        let inline = match &self.inline_type_guid {
            ReadField::Absent => None,
            ReadField::Value(guid) if is_guid_hyphenated(guid.as_ref()) => Some(guid.as_ref()),
            ReadField::Unread | ReadField::Value(_) => return None,
        };
        Some((*self.target.value()?, inline))
    }
}

impl<G: AsRef<str>, L: AsRef<str>> ReferenceFields<G, L> {
    /// The bytes of a complete non-null reference as the reference encoding
    /// stores them, from its presence byte to its last field.
    pub(crate) fn encoded(&self) -> Option<Vec<u8>> {
        fn ascii<G: AsRef<str>>(out: &mut Vec<u8>, field: &ReadField<G>) -> Option<()> {
            match field {
                ReadField::Absent => {}
                ReadField::Value(guid) => {
                    let guid = guid.as_ref().as_bytes();
                    out.extend_from_slice(&u32::try_from(guid.len()).ok()?.to_le_bytes());
                    out.extend_from_slice(guid);
                }
                ReadField::Unread => return None,
            }
            Some(())
        }
        fn utf16<L: AsRef<str>>(out: &mut Vec<u8>, field: &ReadField<L>) -> Option<()> {
            let text = field.value()?.as_ref();
            let count = u32::try_from(text.encode_utf16().count()).ok()?;
            out.extend_from_slice(&count.to_le_bytes());
            out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            Some(())
        }
        if !self.complete || self.present != ReadField::Value(1) {
            return None;
        }
        let mut out = vec![1];
        out.extend_from_slice(&self.target.value()?.to_le_bytes());
        ascii(&mut out, &self.inline_type_guid)?;
        let cross_document = *self.cross_document.value()?;
        out.push(cross_document);
        if cross_document == 0 {
            let different_segment = *self.different_segment.value()?;
            out.push(different_segment);
            if different_segment == 1 {
                out.extend_from_slice(&self.segment.value()?.to_le_bytes());
            }
            return Some(out);
        }
        out.extend_from_slice(&self.segment.value()?.to_le_bytes());
        utf16(&mut out, &self.asset_guid)?;
        let same_document = *self.same_document.value()?;
        out.push(same_document);
        if same_document == 0 {
            ascii(&mut out, &self.target_type_guid)?;
            utf16(&mut out, &self.link_name)?;
            let versioned = *self.versioned.value()?;
            out.push(versioned);
            if versioned == 1 {
                utf16(&mut out, &self.version_key_guid)?;
                utf16(&mut out, &self.version_urn)?;
            }
        }
        Some(out)
    }
}

pub(crate) struct ReferenceRead<'a> {
    pub(crate) fields: ReferenceFields<&'a str, Utf16View<'a>>,
    pub(crate) reference: Option<Reference<&'a str, Utf16View<'a>>>,
    pub(crate) end: usize,
}

pub(crate) fn read_reference_fields(bytes: &[u8], at: usize) -> ReferenceRead<'_> {
    let mut fields = ReferenceFields {
        complete: false,
        present: ReadField::Unread,
        target: ReadField::Unread,
        inline_type_guid: ReadField::Unread,
        cross_document: ReadField::Unread,
        different_segment: ReadField::Unread,
        segment: ReadField::Unread,
        asset_guid: ReadField::Unread,
        same_document: ReadField::Unread,
        target_type_guid: ReadField::Unread,
        link_name: ReadField::Unread,
        versioned: ReadField::Unread,
        version_key_guid: ReadField::Unread,
        version_urn: ReadField::Unread,
    };
    let mut cursor = at;
    let reference = read(bytes, &mut cursor, &mut fields);
    fields.complete = reference.is_some();
    ReferenceRead {
        fields,
        reference,
        end: cursor,
    }
}

fn read<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    fields: &mut ReferenceFields<&'a str, Utf16View<'a>>,
) -> Option<Reference<&'a str, Utf16View<'a>>> {
    macro_rules! scalar {
        ($field:ident, $value:expr, $width:expr) => {{
            let value = $value?;
            fields.$field = ReadField::Value(value);
            *cursor = cursor.checked_add($width)?;
            value
        }};
    }
    macro_rules! text {
        ($field:ident, $value:expr) => {{
            let (value, end) = $value?;
            fields.$field = ReadField::Value(value);
            *cursor = end;
            value
        }};
    }
    let ascii = |at| {
        let (raw, end) = lp_u32_bytes_at(bytes, at)?;
        (raw.len() == 36).then_some(())?;
        Some((std::str::from_utf8(raw).ok()?, end))
    };
    let utf16 = |at, bounds| {
        let (raw, end) = lp_utf16_raw(bytes, at, bounds)?;
        Some((Utf16View::new(raw)?, end))
    };
    let present = scalar!(present, bytes.get(*cursor).copied(), 1);
    if present == 0 {
        fields.target = ReadField::Absent;
        fields.inline_type_guid = ReadField::Absent;
        fields.cross_document = ReadField::Absent;
        fields.different_segment = ReadField::Absent;
        fields.segment = ReadField::Absent;
        fields.asset_guid = ReadField::Absent;
        fields.same_document = ReadField::Absent;
        fields.target_type_guid = ReadField::Absent;
        fields.link_name = ReadField::Absent;
        fields.versioned = ReadField::Absent;
        fields.version_key_guid = ReadField::Absent;
        fields.version_urn = ReadField::Absent;
        return Some(Reference::Null);
    }
    if present != 1 {
        return None;
    }
    let target = scalar!(target, View::u64_le_at(bytes, *cursor), 8);
    let inline_type_guid = if View::u32_le_at(bytes, *cursor) == Some(36) {
        let guid = text!(inline_type_guid, ascii(*cursor));
        if !is_guid_hyphenated(guid) {
            return None;
        }
        Some(guid)
    } else if matches!(bytes.get(*cursor), Some(0 | 1)) {
        fields.inline_type_guid = ReadField::Absent;
        None
    } else {
        return None;
    };
    let cross_document = scalar!(cross_document, bytes.get(*cursor).copied(), 1);
    let reference = match cross_document {
        0 => {
            fields.asset_guid = ReadField::Absent;
            fields.same_document = ReadField::Absent;
            fields.target_type_guid = ReadField::Absent;
            fields.link_name = ReadField::Absent;
            fields.versioned = ReadField::Absent;
            fields.version_key_guid = ReadField::Absent;
            fields.version_urn = ReadField::Absent;
            match scalar!(different_segment, bytes.get(*cursor).copied(), 1) {
                0 => {
                    fields.segment = ReadField::Absent;
                    Reference::Local {
                        target,
                        inline_type_guid,
                    }
                }
                1 => {
                    let segment = scalar!(segment, View::u32_le_at(bytes, *cursor), 4);
                    Reference::CrossSegment {
                        target,
                        inline_type_guid,
                        segment,
                        asset_guid: None,
                    }
                }
                _ => return None,
            }
        }
        1 => {
            fields.different_segment = ReadField::Absent;
            let segment = scalar!(segment, View::u32_le_at(bytes, *cursor), 4);
            let asset_guid = text!(asset_guid, utf16(*cursor, 0..=64));
            match scalar!(same_document, bytes.get(*cursor).copied(), 1) {
                1 => {
                    fields.target_type_guid = ReadField::Absent;
                    fields.link_name = ReadField::Absent;
                    fields.versioned = ReadField::Absent;
                    fields.version_key_guid = ReadField::Absent;
                    fields.version_urn = ReadField::Absent;
                    Reference::CrossSegment {
                        target,
                        inline_type_guid,
                        segment,
                        asset_guid: Some(asset_guid),
                    }
                }
                0 => {
                    let external = ascii(*cursor).filter(|(guid, _)| is_guid_hyphenated(guid));
                    let (target_type_guid, link_name) = if let Some((guid, end)) = external {
                        fields.target_type_guid = ReadField::Value(guid);
                        *cursor = end;
                        (Some(guid), text!(link_name, utf16(*cursor, 0..=256)))
                    } else if inline_type_guid.is_some() {
                        let (link, end) = utf16(*cursor, 0..=256)?;
                        fields.target_type_guid = ReadField::Absent;
                        fields.link_name = ReadField::Value(link);
                        *cursor = end;
                        (None, link)
                    } else {
                        if let Some((guid, end)) = ascii(*cursor) {
                            fields.target_type_guid = ReadField::Value(guid);
                            *cursor = end;
                        }
                        return None;
                    };
                    let version = match scalar!(versioned, bytes.get(*cursor).copied(), 1) {
                        0 => {
                            fields.version_key_guid = ReadField::Absent;
                            fields.version_urn = ReadField::Absent;
                            None
                        }
                        1 => {
                            let key = text!(version_key_guid, utf16(*cursor, 36..=36));
                            let urn = text!(version_urn, utf16(*cursor, 0..=256));
                            Some((key, urn))
                        }
                        _ => return None,
                    };
                    Reference::CrossDocument {
                        target,
                        inline_type_guid,
                        segment,
                        asset_guid,
                        target_type_guid,
                        link_name,
                        version,
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(reference)
}

#[cfg(test)]
mod tests {
    use super::*;
    const INLINE: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    const SEPARATE: &str = "bbbbbbbb-bbbb-cccc-dddd-eeeeeeeeeeee";
    const ASSET: &str = "11111111-2222-3333-4444-555555555555";

    #[test]
    fn local_adapter_requires_known_compatible_route_fields() {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&42u64.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        let fields = read_reference_fields(&bytes, 0).fields.into_owned();
        assert_eq!(fields.local(), Some((42, None)));
        for case in 0..6 {
            let mut changed = fields.clone();
            match case {
                0 => changed.inline_type_guid = ReadField::Unread,
                1 => changed.inline_type_guid = ReadField::Value("not-a-guid".into()),
                2 => changed.segment = ReadField::Value(0),
                3 => changed.target_type_guid = ReadField::Value(INLINE.into()),
                4 => changed.asset_guid = ReadField::Value(ASSET.into()),
                _ => changed.versioned = ReadField::Value(0),
            }
            assert!(changed.local().is_none());
        }
    }

    fn external(inline: bool, separate: bool) -> (Vec<u8>, usize, usize) {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&u64::MAX.to_le_bytes());
        if inline {
            bytes.extend_from_slice(&36u32.to_le_bytes());
            bytes.extend_from_slice(INLINE.as_bytes());
        }
        bytes.push(1);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        crate::test_support::lp_utf16(&mut bytes, ASSET);
        bytes.push(0);
        if separate {
            bytes.extend_from_slice(&36u32.to_le_bytes());
            bytes.extend_from_slice(SEPARATE.as_bytes());
        }
        let link_at = bytes.len();
        crate::test_support::lp_utf16(&mut bytes, "12345678-1234-1234-1234-123456789012");
        bytes.push(1);
        crate::test_support::lp_utf16(&mut bytes, ASSET);
        let urn_at = bytes.len();
        crate::test_support::lp_utf16(&mut bytes, "urn:test:version");
        (bytes, link_at, urn_at)
    }
    #[test]
    fn complete_paths_keep_all_fields_and_strict_conflicts_are_atomic() {
        for (inline, separate) in [(true, false), (false, true), (true, true)] {
            let (bytes, _, _) = external(inline, separate);
            let read = read_reference_fields(&bytes, 0);
            assert!(read.fields.complete);
            assert_eq!(read.end, bytes.len());
            assert_eq!(read.fields.target, ReadField::Value(u64::MAX));
            assert_eq!(
                read.fields.inline_type_guid,
                if inline {
                    ReadField::Value(INLINE)
                } else {
                    ReadField::Absent
                }
            );
            assert_eq!(
                read.fields.target_type_guid,
                if separate {
                    ReadField::Value(SEPARATE)
                } else {
                    ReadField::Absent
                }
            );
            let fields = read.fields.into_owned();
            assert_eq!(fields.segment, ReadField::Value(7));
            assert_eq!(fields.asset_guid, ReadField::Value(ASSET.into()));
            assert_eq!(
                fields.link_name,
                ReadField::Value("12345678-1234-1234-1234-123456789012".into())
            );
            assert_eq!(fields.version_key_guid, ReadField::Value(ASSET.into()));
            assert_eq!(
                fields.version_urn,
                ReadField::Value("urn:test:version".into())
            );
            assert!(fields.local().is_none());
            let mut cursor = 0;
            assert_eq!(
                crate::bytes::take_reference(&bytes, &mut cursor).is_some(),
                !(inline && separate)
            );
            assert_eq!(cursor, if inline && separate { 0 } else { bytes.len() });
        }
    }
    #[test]
    fn every_truncation_retains_its_prefix_without_advancing_strict_cursor() {
        let (bytes, link_at, urn_at) = external(true, true);
        for end in 0..bytes.len() {
            let read = read_reference_fields(&bytes[..end], 0);
            assert!(!read.fields.complete, "end={end}");
            assert!(read.end <= end);
            if end >= 9 {
                assert_eq!(read.fields.target, ReadField::Value(u64::MAX));
            }
            if end >= link_at {
                assert_eq!(read.fields.target_type_guid, ReadField::Value(SEPARATE));
            }
            if end >= urn_at {
                assert_eq!(
                    read.fields.version_key_guid.value().unwrap().to_string(),
                    ASSET
                );
            }
            let mut cursor = 0;
            assert!(crate::bytes::take_reference(&bytes[..end], &mut cursor).is_none());
            assert_eq!(cursor, 0);
        }
    }
    #[test]
    fn short_optional_type_prefixes_keep_the_type_and_later_fields_unread() {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&1600u64.to_le_bytes());
        bytes.extend_from_slice(&36u32.to_le_bytes());
        for end in 9..13 {
            let read = read_reference_fields(&bytes[..end], 0);
            assert_eq!(read.fields.target, ReadField::Value(1600));
            assert_eq!(read.fields.inline_type_guid, ReadField::Unread);
            assert_eq!(read.fields.cross_document, ReadField::Unread);
            assert_eq!(read.fields.different_segment, ReadField::Unread);
            assert!(!read.fields.complete);
            let mut cursor = 0;
            assert!(crate::bytes::take_reference(&bytes[..end], &mut cursor).is_none());
            assert_eq!(cursor, 0);
        }
        let (bytes, link_at, _) = external(true, true);
        let separate_at = link_at - 4 - SEPARATE.len();
        for end in separate_at..separate_at + 4 {
            let read = read_reference_fields(&bytes[..end], 0);
            assert_eq!(read.fields.inline_type_guid, ReadField::Value(INLINE));
            assert_eq!(read.fields.same_document, ReadField::Value(0));
            assert_eq!(read.fields.target_type_guid, ReadField::Unread);
            assert_eq!(read.fields.link_name, ReadField::Unread);
            assert_eq!(read.fields.versioned, ReadField::Unread);
            assert!(!read.fields.complete);
        }
    }
    #[test]
    fn every_complete_reference_encodes_back_to_its_stored_bytes() {
        let mut forms = Vec::new();
        for inline in [false, true] {
            let mut head = vec![1];
            head.extend_from_slice(&42u64.to_le_bytes());
            if inline {
                head.extend_from_slice(&36u32.to_le_bytes());
                head.extend_from_slice(INLINE.as_bytes());
            }
            let mut local = head.clone();
            local.extend_from_slice(&[0, 0]);
            forms.push(local);
            let mut segment = head.clone();
            segment.extend_from_slice(&[0, 1]);
            segment.extend_from_slice(&7u32.to_le_bytes());
            forms.push(segment);
            let mut qualified = head.clone();
            qualified.push(1);
            qualified.extend_from_slice(&7u32.to_le_bytes());
            crate::test_support::lp_utf16(&mut qualified, ASSET);
            qualified.push(1);
            forms.push(qualified);
            let mut unversioned = head;
            unversioned.push(1);
            unversioned.extend_from_slice(&7u32.to_le_bytes());
            crate::test_support::lp_utf16(&mut unversioned, ASSET);
            unversioned.push(0);
            if !inline {
                unversioned.extend_from_slice(&36u32.to_le_bytes());
                unversioned.extend_from_slice(SEPARATE.as_bytes());
            }
            crate::test_support::lp_utf16(&mut unversioned, "Rôle 1");
            unversioned.push(0);
            forms.push(unversioned);
        }
        for (inline, separate) in [(true, false), (false, true), (true, true)] {
            forms.push(external(inline, separate).0);
        }
        for bytes in forms {
            let read = read_reference_fields(&bytes, 0);
            assert!(read.fields.complete);
            assert_eq!(read.end, bytes.len());
            assert_eq!(read.fields.into_owned().encoded(), Some(bytes.clone()));
            let truncated = read_reference_fields(&bytes[..bytes.len() - 1], 0);
            assert_eq!(truncated.fields.into_owned().encoded(), None);
        }
        assert_eq!(
            read_reference_fields(&[0], 0).fields.into_owned().encoded(),
            None
        );
    }

    #[test]
    fn null_local_and_qualified_same_document_remain_distinct() {
        let null = read_reference_fields(&[0], 0);
        assert!(null.fields.complete);
        assert_eq!(null.fields.target, ReadField::Absent);
        let mut local = vec![1];
        local.extend_from_slice(&u64::MAX.to_le_bytes());
        local.extend_from_slice(&[0, 0]);
        assert_eq!(
            read_reference_fields(&local, 0).fields.local(),
            Some((u64::MAX, None))
        );
        local.truncate(9);
        local.push(0);
        local.push(1);
        local.extend_from_slice(&7u32.to_le_bytes());
        let segment = read_reference_fields(&local, 0);
        assert!(segment.fields.complete);
        assert!(segment.fields.local().is_none());
        let mut qualified = vec![1];
        qualified.extend_from_slice(&9u64.to_le_bytes());
        qualified.push(1);
        qualified.extend_from_slice(&7u32.to_le_bytes());
        crate::test_support::lp_utf16(&mut qualified, ASSET);
        qualified.push(1);
        let qualified = read_reference_fields(&qualified, 0).fields.into_owned();
        assert!(qualified.complete);
        assert!(qualified.local().is_none());
        assert_eq!(qualified.asset_guid, ReadField::Value(ASSET.into()));
    }
}
