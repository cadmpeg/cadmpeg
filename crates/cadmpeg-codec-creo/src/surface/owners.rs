// SPDX-License-Identifier: Apache-2.0
//! Owners named by `srf_array` rows through `topol_ref_data`.

use crate::psb::token;

/// `list_type` of a row owned by a body.
const LIST_TYPE_BODY: u8 = 0x62;
/// `list_type` of a row owned by a quilt.
const LIST_TYPE_QUILT: u8 = 0x39;
/// Null `srf_flip_dat` member that precedes `list_id`.
const NULL_MEMBER: u8 = 0xe1;
/// Null compact value.
const NULL_VALUE: u8 = 0xf6;
/// Opener of a three-byte identifier.
const WIDE_IDENTIFIER: u8 = 0xf5;
/// Opener of a positional row in a counted frame.
const ROW_OPEN: u8 = 0xe3;

/// The owner named by a surface row's `list_id` and `list_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SurfaceOwner {
    /// `list_type` `62`: the body whose identifier is `list_id`.
    Body(u32),
    /// `list_type` `39`: the quilt whose identifier is `list_id`.
    Quilt(u32),
    /// Null `list_id` and `list_type`.
    Unowned,
}

/// The owner of one row in a complete counted `srf_array` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceRowOwner {
    /// The row's `geom_id`.
    pub(crate) surface_id: u32,
    /// The owner the row names.
    pub(crate) owner: SurfaceOwner,
    /// Byte offset of the row's `geom_id` field.
    pub(crate) offset: usize,
}

/// Owners of the rows of every complete counted `srf_array` frame.
///
/// A named prototype states `list_id` and `list_type` by name. A positional
/// row ends with `e1 <list_id> <list_type>` before the next row's `e3` or,
/// for the last row, before the next array's named header.
pub(crate) fn row_owners(payload: &[u8]) -> Vec<SurfaceRowOwner> {
    let rows = super::counted_row_bounds(payload);
    let mut owners = Vec::new();
    for (index, (row, frame_end)) in rows.iter().enumerate() {
        let next_row = rows
            .get(index + 1)
            .filter(|(_, next_end)| next_end == frame_end)
            .map(|(next, _)| next.offset);
        let owner = if is_named_row(payload, row.offset) {
            named_owner(payload, row.offset, next_row.unwrap_or(*frame_end))
        } else {
            let boundary = match next_row {
                Some(next) => next
                    .checked_sub(1)
                    .filter(|at| payload.get(*at) == Some(&ROW_OPEN)),
                None => frame_end
                    .checked_sub(2)
                    .filter(|at| payload.get(*at) == Some(&token::NAMED_RECORD)),
            };
            boundary.and_then(|boundary| positional_owner(payload, row.offset, boundary))
        };
        if let Some(owner) = owner {
            owners.push(SurfaceRowOwner {
                surface_id: row.id,
                owner,
                offset: row.offset,
            });
        }
    }
    owners
}

fn is_named_row(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + b"geom_id\0".len()) == Some(b"geom_id\0".as_slice())
}

fn named_owner(payload: &[u8], start: usize, end: usize) -> Option<SurfaceOwner> {
    let region = payload.get(start..end)?;
    let id_label = exactly_one_match(region, b"list_id\0")?;
    let (list_id, after_id) = list_id_value(region, id_label + b"list_id\0".len())?;
    let type_header = [token::NAMED_RECORD, 0x01];
    let type_label = after_id + type_header.len();
    (region.get(after_id..type_label) == Some(type_header.as_slice())
        && region.get(type_label..type_label + b"list_type\0".len())
            == Some(b"list_type\0".as_slice()))
    .then_some(())?;
    owner(list_id, *region.get(type_label + b"list_type\0".len())?)
}

fn positional_owner(payload: &[u8], start: usize, boundary: usize) -> Option<SurfaceOwner> {
    let kind_at = boundary.checked_sub(1)?;
    let mut matches = (1..=3).filter_map(|width: usize| {
        let id_at = kind_at.checked_sub(width)?;
        let flip_at = id_at.checked_sub(1)?;
        (flip_at > start && payload.get(flip_at) == Some(&NULL_MEMBER)).then_some(())?;
        let (list_id, after) = list_id_value(payload, id_at)?;
        (after == kind_at).then_some(list_id)
    });
    let list_id = matches.next()?;
    matches.next().is_none().then_some(())?;
    owner(list_id, *payload.get(kind_at)?)
}

/// `list_id`: a compact integer, a three-byte `f5 <XX> <YY>` identifier, or
/// null.
fn list_id_value(payload: &[u8], at: usize) -> Option<(Option<u32>, usize)> {
    match *payload.get(at)? {
        NULL_VALUE => Some((None, at + 1)),
        WIDE_IDENTIFIER => {
            let [high, low] = [*payload.get(at + 1)?, *payload.get(at + 2)?];
            Some((
                Some(u32::from(WIDE_IDENTIFIER) << 16 | u32::from(high) << 8 | u32::from(low)),
                at + 3,
            ))
        }
        0x00..=0xbf => {
            let (value, after) = crate::psb::complete_compact_int(payload, at)?;
            Some((Some(value), after))
        }
        _ => None,
    }
}

fn owner(list_id: Option<u32>, list_type: u8) -> Option<SurfaceOwner> {
    match (list_id, list_type) {
        (None, NULL_VALUE) => Some(SurfaceOwner::Unowned),
        (Some(id), LIST_TYPE_BODY) => Some(SurfaceOwner::Body(id)),
        (Some(id), LIST_TYPE_QUILT) => Some(SurfaceOwner::Quilt(id)),
        _ => None,
    }
}

fn exactly_one_match(region: &[u8], needle: &[u8]) -> Option<usize> {
    let mut matches = region
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::{positional_owner, row_owners, SurfaceOwner};

    #[test]
    fn positional_tail_names_a_body_quilt_or_no_owner() {
        let row = |tail: &[u8]| {
            let mut bytes = vec![
                0x81, 0x39, 0x22, 0x81, 0x36, 0x01, 0x00, 0x00, 0xe3, 0x80, 0xf4,
            ];
            bytes.extend_from_slice(tail);
            bytes.push(0xe3);
            let boundary = bytes.len() - 1;
            positional_owner(&bytes, 0, boundary)
        };
        assert_eq!(
            row(&[0xe1, 0x81, 0x70, 0x62]),
            Some(SurfaceOwner::Body(368))
        );
        assert_eq!(row(&[0xe1, 0x05, 0x62]), Some(SurfaceOwner::Body(5)));
        assert_eq!(
            row(&[0xe1, 0xf5, 0x96, 0x92, 0x62]),
            Some(SurfaceOwner::Body(0xf5_9692))
        );
        assert_eq!(
            row(&[0xe1, 0x83, 0x28, 0x39]),
            Some(SurfaceOwner::Quilt(808))
        );
        assert_eq!(row(&[0xe1, 0xf6, 0xf6]), Some(SurfaceOwner::Unowned));
        assert_eq!(row(&[0x81, 0x70, 0x62]), None);
        assert_eq!(row(&[0xe1, 0x81, 0x70, 0x40]), None);
        assert_eq!(row(&[0xe1, 0xf6, 0x62]), None);
        assert_eq!(row(&[0xe1, 0x05, 0xf6]), None);
    }

    #[test]
    fn counted_frame_rows_name_their_owners() {
        let mut payload = b"\xe0\x00srf_array\0\xf8\x03".to_vec();
        let named = b"\xe3\xe0\x01geom_id\0\x0a\xe0\x01geom_type\0\x22\xe0\x01feat_id\0\x05\
\xe0\x01orient\0\x01\xe0\x01boundary_type\0\x00\xe0\x01next_geom_ptr\0\x0b\
\xe0\x00topol_ref_data\0\xe3\xe0\x01owner_feat_revnum\0\x02\xe0\x00srf_flip_dat\0\xe1\
\xe0\x01list_id\0\x81\x70\xe0\x01list_type\0\x62\xf2\xf7\x03";
        payload.extend_from_slice(named);
        payload.extend_from_slice(&[0xe3, 0x0b, 0x22, 0x05, 0x01, 0x00, 0x0c]);
        payload.extend_from_slice(&[0xe3, 0x02, 0xe1, 0x81, 0x70, 0x62]);
        payload.extend_from_slice(&[0xe3, 0x0c, 0x22, 0x05, 0xf6, 0x00, 0x00]);
        payload.extend_from_slice(&[0xe3, 0x02, 0xe1, 0xf6, 0xf6]);
        payload.extend_from_slice(b"\xe0\x00crv_array\0\xf8\x00");
        let owners: Vec<_> = row_owners(&payload)
            .into_iter()
            .map(|row| (row.surface_id, row.owner))
            .collect();
        assert_eq!(
            owners,
            [
                (10, SurfaceOwner::Body(368)),
                (11, SurfaceOwner::Body(368)),
                (12, SurfaceOwner::Unowned)
            ]
        );
    }
}
