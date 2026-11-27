// SPDX-License-Identifier: Apache-2.0
//! Assembly component table: the `members` record of `NeuAsmSld`
//! ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#9-assembly-component-table)).
//!
//! [`member_table`] reads one table from a section payload. Rows decode in
//! source order; the first row the grammar does not admit ends the table and
//! is recorded as its stop.

use cadmpeg_core::decode::View;

/// Named field that opens the component table.
const MEMBERS_FIELD: &[u8] = b"\xe0\x00members\0";

/// `NeuAsmSld.members` rows in source order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MemberTable {
    /// Byte offset of the `members` field header.
    pub(crate) offset: usize,
    /// Row count declared by the array header.
    pub(crate) declared: u32,
    /// Rows the grammar admitted, in source order.
    pub(crate) members: Vec<Member>,
    /// The first row the grammar did not admit, or a row count that differs
    /// from `declared`.
    pub(crate) stop: Option<MemberTableStop>,
}

/// Where and why a component table ended before its declared rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemberTableStop {
    pub(crate) offset: usize,
    pub(crate) reason: &'static str,
    /// Why the row that starts at `offset` does not read, or `None` where
    /// the table closes with a row count other than the declared one.
    pub(crate) row: Option<RowFault>,
}

/// Why a component row does not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowFault {
    /// The bytes break the row grammar: a fixed token differs, a field runs
    /// past the section, a repeat token opens a run, or a name is not UTF-8.
    Broken,
    /// The row holds a value form with no reading here: a frame-slot token
    /// outside the slot table, a `tha_ref` that is not null, or a name that
    /// is empty or holds a control character.
    Unread,
}

use RowFault::{Broken, Unread};

/// One component placed by the assembly.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Member {
    /// Byte offset of the row's first byte.
    pub(crate) offset: usize,
    /// Component identifier within the assembly.
    pub(crate) id: u32,
    pub(crate) kind: MemberKind,
    /// Child model name.
    pub(crate) name: String,
    pub(crate) family_name: String,
    /// `e1`, `e2`, `e3`, then `origin`: the child x, y, and z axes and its
    /// origin in assembly coordinates.
    pub(crate) frame: [f64; 12],
}

/// The `type` field of a component row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberKind {
    Assembly,
    Part,
    Other(u8),
}

impl MemberKind {
    fn from_byte(byte: u8) -> Self {
        match byte {
            1 => Self::Assembly,
            2 => Self::Part,
            other => Self::Other(other),
        }
    }

    /// Model file extension of the child.
    pub(crate) fn extension(self) -> Option<&'static str> {
        match self {
            Self::Assembly => Some("asm"),
            Self::Part => Some("prt"),
            Self::Other(_) => None,
        }
    }
}

impl MemberTable {
    /// The same table with every offset moved by `base`.
    pub(crate) fn at(mut self, base: usize) -> Option<Self> {
        self.offset = self.offset.checked_add(base)?;
        for member in &mut self.members {
            member.offset = member.offset.checked_add(base)?;
        }
        if let Some(stop) = &mut self.stop {
            stop.offset = stop.offset.checked_add(base)?;
        }
        Some(self)
    }
}

/// Read the unique `members` table in `payload`.
///
/// `None` means `payload` holds no `members` field, more than one, or an
/// array header that does not frame one.
pub(crate) fn member_table(payload: &[u8]) -> Option<MemberTable> {
    let start = unique_field(payload)?;
    let mut view = View::over_retained(payload);
    view.seek(start.checked_add(MEMBERS_FIELD.len())?)?;
    expect(&mut view, &[0xf1, 0xf8])?;
    let declared = compact(&mut view)?;
    expect(&mut view, &[0xf7])?;
    let element = compact(&mut view)?;
    expect(&mut view, &[0xfb, 0xe3])?;
    let mut table = MemberTable {
        offset: start,
        declared,
        members: Vec::new(),
        stop: None,
    };
    if declared == 0 {
        return Some(table);
    }
    let mut row_start = view.position();
    let mut terminator = match schema_row(&mut view, element) {
        Ok((member, terminator)) => {
            table.members.push(Member {
                offset: row_start,
                ..member
            });
            terminator
        }
        Err(fault) => {
            table.stop = Some(MemberTableStop {
                offset: row_start,
                reason: match fault {
                    Broken => "the named first row does not match the component row grammar",
                    Unread => {
                        "the named first row holds a value form the row grammar does not read"
                    }
                },
                row: Some(fault),
            });
            return Some(table);
        }
    };
    while terminator == ROW_CONTINUES {
        row_start = view.position();
        match positional_row(&mut view) {
            Ok((member, next)) => {
                table.members.push(Member {
                    offset: row_start,
                    ..member
                });
                terminator = next;
            }
            Err(fault) => {
                table.stop = Some(MemberTableStop {
                    offset: row_start,
                    reason: match fault {
                        Broken => "a positional row does not match the component row grammar",
                        Unread => {
                            "a positional row holds a value form the row grammar does not read"
                        }
                    },
                    row: Some(fault),
                });
                return Some(table);
            }
        }
    }
    if u32::try_from(table.members.len()).ok() != Some(declared) {
        table.stop = Some(MemberTableStop {
            offset: view.position(),
            reason: "the table ends with a row count that differs from its declared count",
            row: None,
        });
    }
    Some(table)
}

/// Terminator after a row that another row follows.
const ROW_CONTINUES: u8 = 0xe3;
/// Terminator after the final row.
const TABLE_ENDS: u8 = 0xf4;

fn unique_field(payload: &[u8]) -> Option<usize> {
    let first = cadmpeg_core::bytes::find(payload, MEMBERS_FIELD)?;
    cadmpeg_core::bytes::find_from(payload, MEMBERS_FIELD, first + 1)
        .is_none()
        .then_some(first)
}

fn expect(view: &mut View<'_>, bytes: &[u8]) -> Option<()> {
    (view.take(bytes.len())? == bytes).then_some(())
}

/// [`expect`] inside a row, where a differing token breaks the grammar.
fn token(view: &mut View<'_>, bytes: &[u8]) -> Result<(), RowFault> {
    expect(view, bytes).ok_or(Broken)
}

fn compact(view: &mut View<'_>) -> Option<u32> {
    let (value, end) = crate::psb::complete_compact_int(view.window(), view.position())?;
    view.seek(end)?;
    Some(value)
}

/// A NUL-terminated UTF-8 model name. A name without its NUL in the section,
/// or that is not UTF-8, breaks the grammar; an empty name or one with a
/// control character has no reading here.
fn name(view: &mut View<'_>) -> Result<String, RowFault> {
    let length = view
        .unread()
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(Broken)?;
    let bytes = view.take(length).ok_or(Broken)?;
    view.skip(1).ok_or(Broken)?;
    let text = std::str::from_utf8(bytes).map_err(|_| Broken)?;
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(Unread);
    }
    Ok(text.to_owned())
}

fn terminator(view: &mut View<'_>) -> Result<u8, RowFault> {
    let byte = view.u8().ok_or(Broken)?;
    matches!(byte, ROW_CONTINUES | TABLE_ENDS)
        .then_some(byte)
        .ok_or(Broken)
}

fn named(view: &mut View<'_>, kind: u8, field: &[u8]) -> Result<(), RowFault> {
    token(view, &[0xe0, kind])?;
    token(view, field)?;
    token(view, &[0])
}

/// The null `tha_ref`; any other value has no reading here.
fn null_reference(view: &mut View<'_>) -> Result<(), RowFault> {
    match view.u8() {
        Some(NULL_REFERENCE) => Ok(()),
        Some(_) => Err(Unread),
        None => Err(Broken),
    }
}

fn schema_row(view: &mut View<'_>, element: u32) -> Result<(Member, u8), RowFault> {
    named(view, 0x01, b"id")?;
    let id = compact(view).ok_or(Broken)?;
    named(view, 0x01, b"type")?;
    let kind = MemberKind::from_byte(view.u8().ok_or(Broken)?);
    named(view, 0x0a, b"name")?;
    let child = name(view)?;
    named(view, 0x0a, b"family_name")?;
    let family_name = name(view)?;
    named(view, 0x00, b"tha_ref")?;
    null_reference(view)?;
    let mut frame = [0.0; 12];
    for (field, slots) in [&b"e1"[..], b"e2", b"e3", b"origin"]
        .into_iter()
        .zip(frame.chunks_exact_mut(3))
    {
        named(view, 0x02, field)?;
        token(view, &[0xf8, 0x03])?;
        frame_slots(view, slots)?;
    }
    token(view, &[0xf1, 0xf7])?;
    if compact(view).ok_or(Broken)? != element {
        return Err(Broken);
    }
    let terminator = terminator(view)?;
    Ok((
        Member {
            offset: 0,
            id,
            kind,
            name: child,
            family_name,
            frame,
        },
        terminator,
    ))
}

fn positional_row(view: &mut View<'_>) -> Result<(Member, u8), RowFault> {
    let id = compact(view).ok_or(Broken)?;
    let kind = MemberKind::from_byte(view.u8().ok_or(Broken)?);
    let child = name(view)?;
    let family_name = name(view)?;
    null_reference(view)?;
    let mut frame = [0.0; 12];
    frame_slots(view, &mut frame)?;
    let terminator = terminator(view)?;
    Ok((
        Member {
            offset: 0,
            id,
            kind,
            name: child,
            family_name,
            frame,
        },
        terminator,
    ))
}

/// The null `tha_ref` value.
const NULL_REFERENCE: u8 = 0xe1;

/// Fill `slots` from consecutive member-frame scalar tokens.
///
/// `e4`, `e5`, and `e6` repeat the preceding slot of the same run one, two,
/// or three times. A token of a form the slot table does not state has no
/// reading here; a stated token that runs past the section, decodes to a
/// non-finite value, or repeats at the start of a run breaks the grammar.
fn frame_slots(view: &mut View<'_>, slots: &mut [f64]) -> Result<(), RowFault> {
    let mut filled = 0;
    while filled < slots.len() {
        let head = view.u8().ok_or(Broken)?;
        if let 0xe4..=0xe6 = head {
            let previous = *slots
                .get(filled.checked_sub(1).ok_or(Broken)?)
                .ok_or(Broken)?;
            let end = filled.checked_add(usize::from(head - 0xe3)).ok_or(Broken)?;
            slots.get_mut(filled..end).ok_or(Broken)?.fill(previous);
            filled = end;
            continue;
        }
        if !stated_slot(head, view.unread()) {
            return Err(Unread);
        }
        *slots.get_mut(filled).ok_or(Broken)? = frame_scalar(head, view).ok_or(Broken)?;
        filled += 1;
    }
    Ok(())
}

/// Whether the slot table states the token that `head` opens and `rest`
/// continues. A token cut short by the end of `rest` counts as stated.
fn stated_slot(head: u8, rest: &[u8]) -> bool {
    match head {
        0x00..=0x02 => rest
            .first()
            .is_none_or(|selector| constant_bits(*selector).is_some()),
        0x19..=0x4a => (head - if head < 0x32 { 0x19 } else { 0x32 }) % 5 != 3,
        0xa4 => false,
        0xee => rest.get(2).is_none_or(|byte| *byte == 0xff),
        0x04 | 0x0f | 0x10 | 0x18 | 0x4b..=0xdf | 0xea | 0xec | 0xed => true,
        _ => false,
    }
}

/// The largest double below one.
const ONE_BELOW: u64 = 0x3fef_ffff_ffff_ffff;

/// IEEE bits of a constant selector in the member-frame lane.
fn constant_bits(selector: u8) -> Option<u64> {
    match selector {
        0x04 => Some(ONE_BELOW),
        0x0f => Some(1.0_f64.to_bits()),
        0x10 => Some((-1.0_f64).to_bits()),
        _ => None,
    }
}

/// Decode one member-frame scalar whose first byte `head` was already read.
pub(crate) fn frame_scalar(head: u8, view: &mut View<'_>) -> Option<f64> {
    let mut raw = [0_u8; 8];
    match head {
        0x00..=0x02 => {
            raw = constant_bits(view.u8()?)?.to_be_bytes();
            let low = usize::from(head) + 1;
            raw.get_mut(8 - low..)?.copy_from_slice(view.take(low)?);
        }
        0x04 | 0x0f | 0x10 => raw = constant_bits(head)?.to_be_bytes(),
        0x18 => {}
        0x19..=0x4a => grouped(head, view, &mut raw)?,
        0x4b..=0xdf => {
            raw[..2].copy_from_slice(&crate::scalar::dict_head(head)?.to_be_bytes());
            raw[2..].copy_from_slice(view.take(6)?);
        }
        0xea => raw[..3].copy_from_slice(view.take(3)?),
        0xec => raw[..7].copy_from_slice(view.take(7)?),
        0xed => raw.copy_from_slice(view.take(8)?),
        0xee => {
            let explicit = view.array::<3>()?;
            (explicit[2] == 0xff).then_some(())?;
            raw = [0xff; 8];
            raw[..3].copy_from_slice(&explicit);
        }
        _ => return None,
    }
    // endian-exception: reconstructed-scalar
    let value = f64::from_be_bytes(raw);
    value.is_finite().then_some(value)
}

/// The lattice `19..4a`: five forms for each leading IEEE byte `3c..40`
/// (`19..31`) and `bc..c0` (`32..4a`).
fn grouped(head: u8, view: &mut View<'_>, raw: &mut [u8; 8]) -> Option<()> {
    let (base, first_lead) = if head < 0x32 {
        (0x19, 0x3c)
    } else {
        (0x32, 0xbc)
    };
    let rank = head - base;
    raw[0] = first_lead + rank / 5;
    match rank % 5 {
        0 => raw[1..].copy_from_slice(view.take(7)?),
        1 => {
            let [second, fill] = view.array::<2>()?;
            raw[1] = second;
            raw[2..].fill(fill);
        }
        2 => raw[1..3].copy_from_slice(view.take(2)?),
        4 => raw[1..7].copy_from_slice(view.take(6)?),
        _ => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests;
