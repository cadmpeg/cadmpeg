// SPDX-License-Identifier: Apache-2.0
//! Surface construction branch framing domains.

use super::branch_items::BranchItems;
use super::discriminators::SurfaceBranchMode;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub(crate) enum SurfaceFamily {
    Form14 = 0x14,
    Form50 = 0x50,
}

impl From<SurfaceFamily> for u8 {
    fn from(value: SurfaceFamily) -> Self {
        match value {
            SurfaceFamily::Form14 => 0x14,
            SurfaceFamily::Form50 => 0x50,
        }
    }
}

impl TryFrom<u8> for SurfaceFamily {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x14 => Ok(Self::Form14),
            0x50 => Ok(Self::Form50),
            _ => Err("surface family must be 0x14 or 0x50"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct SurfaceSuffix(Vec<u8>);

impl SurfaceSuffix {
    pub(crate) fn new(bytes: Vec<u8>) -> Result<Self, &'static str> {
        if !(1..=5).contains(&bytes.len()) {
            return Err("surface suffix must contain 1 through 5 bytes");
        }
        Ok(Self(bytes))
    }

    #[cfg(test)]
    pub(crate) fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl<'de> Deserialize<'de> for SurfaceSuffix {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// One branch whose reference positions follow from its frame and token widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceBranch<B> {
    offset: u64,
    terminal_relative: u64,
    mode: SurfaceBranchMode,
    witnessed: bool,
    members: BranchItems<(PayloadIndexToken, B)>,
    terminal: (PayloadIndexToken, B),
    suffix: SurfaceSuffix,
}

impl<B> SurfaceBranch<B> {
    pub(crate) fn new(
        offset: u64,
        mode: SurfaceBranchMode,
        witnessed: bool,
        members: BranchItems<(PayloadIndexToken, B)>,
        terminal: (PayloadIndexToken, B),
        suffix: SurfaceSuffix,
    ) -> Result<Self, &'static str> {
        let terminal_relative =
            match Self::extent(offset, witnessed, &members, &terminal, &suffix, |members| {
                Ok::<_, std::convert::Infallible>(members.iter())
            }) {
                Ok(offset) => offset?,
                Err(error) => match error {},
            };
        Ok(Self {
            offset,
            terminal_relative,
            mode,
            witnessed,
            members,
            terminal,
            suffix,
        })
    }

    fn from_wire(
        ctx: &DecodeContext<'_>,
        offset: u64,
        mode: SurfaceBranchMode,
        witnessed: bool,
        members: BranchItems<(PayloadIndexToken, B)>,
        terminal: (PayloadIndexToken, B),
        suffix: SurfaceSuffix,
    ) -> Result<Result<Self, &'static str>, CodecError> {
        Ok(
            Self::extent(offset, witnessed, &members, &terminal, &suffix, |members| {
                ctx.admit_iter(members, "NX surface branch token widths")
            })?
            .map(|terminal_relative| Self {
                offset,
                terminal_relative,
                mode,
                witnessed,
                members,
                terminal,
                suffix,
            }),
        )
    }

    fn extent<'a, E, I: Iterator<Item = &'a (PayloadIndexToken, B)>>(
        offset: u64,
        witnessed: bool,
        members: &'a BranchItems<(PayloadIndexToken, B)>,
        terminal: &(PayloadIndexToken, B),
        suffix: &SurfaceSuffix,
        admit: impl FnOnce(&'a [(PayloadIndexToken, B)]) -> Result<I, E>,
    ) -> Result<Result<u64, &'static str>, E>
    where
        B: 'a,
    {
        let state_bytes = if witnessed {
            u64::from(members.declared_count()) + 5
        } else {
            5
        };
        let relative = admit(members.as_slice())?
            .try_fold(3_u64, |length, (token, _)| {
                length.checked_add(u64_from_index(token.raw().len()))
            })
            .and_then(|length| length.checked_add(state_bytes))
            .and_then(|length| length.checked_add(3));
        let valid = relative.and_then(|relative| {
            offset
                .checked_add(relative)?
                .checked_add(u64_from_index(terminal.0.raw().len()))?
                .checked_add(1)?
                .checked_add(u64_from_index(suffix.0.len()))?;
            Some(relative)
        });
        Ok(valid.ok_or("source_offset: surface branch frame overflows"))
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }
    pub(crate) fn mode(&self) -> SurfaceBranchMode {
        self.mode
    }
    pub(crate) fn witnessed(&self) -> bool {
        self.witnessed
    }
    pub(crate) fn members(&self) -> &BranchItems<(PayloadIndexToken, B)> {
        &self.members
    }
    pub(crate) fn terminal(&self) -> &(PayloadIndexToken, B) {
        &self.terminal
    }
    pub(crate) fn suffix(&self) -> &SurfaceSuffix {
        &self.suffix
    }

    pub(crate) fn member_offsets(&self) -> impl Iterator<Item = u64> + '_ {
        let mut at = self.offset + 3;
        self.members.as_slice().iter().map(move |(token, _)| {
            let offset = at;
            at += cadmpeg_core::decode::u64_from_index(token.raw().len());
            offset
        })
    }

    fn terminal_relative_offset(&self) -> u64 {
        self.terminal_relative
    }

    pub(crate) fn terminal_offset(&self) -> u64 {
        self.offset + self.terminal_relative_offset()
    }
}

impl SurfaceBranch<()> {
    pub(crate) fn resolve<B>(
        self,
        ctx: &DecodeContext<'_>,
        file_base: u64,
        mut target: impl FnMut(PayloadIndexToken) -> Result<B, CodecError>,
    ) -> Result<Option<SurfaceBranch<B>>, CodecError> {
        let Some(offset) = self.offset.checked_add(file_base) else {
            return Ok(None);
        };
        let members = self
            .members
            .try_map_indexed_charged(ctx, |_, (token, ())| Ok((token, target(token)?)))?;
        let terminal = (self.terminal.0, target(self.terminal.0)?);
        Ok(SurfaceBranch::from_wire(
            ctx,
            offset,
            self.mode,
            self.witnessed,
            members,
            terminal,
            self.suffix,
        )?
        .ok())
    }
}

/// A uniquely framed group containing the declared 1 through 255 branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceFeaturePayloadBranches {
    pub(crate) family: SurfaceFamily,
    pub(crate) header_code: u8,
    branches: Vec<SurfaceBranch<()>>,
}

impl SurfaceFeaturePayloadBranches {
    pub(crate) fn into_branches(self) -> Vec<SurfaceBranch<()>> {
        self.branches
    }
}

#[derive(Clone, Copy)]
struct SurfaceBranchShape<'a> {
    offset: u64,
    terminal_relative: u64,
    mode: SurfaceBranchMode,
    witnessed: bool,
    declared_count: u8,
    member_bytes: &'a [u8],
    terminal: PayloadIndexToken,
    suffix: &'a [u8],
}

struct SurfacePathSearch<'a> {
    path: Vec<SurfaceBranchShape<'a>>,
    first_path: Option<Vec<SurfaceBranchShape<'a>>>,
    matches: u8,
}

fn surface_feature_branch_paths<'a>(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'a>,
    at: usize,
    remaining: u8,
    terminator: &[u8],
    search: &mut SurfacePathSearch<'a>,
    selected_storage: &mut ScopedReservation<'_>,
) -> Result<(), CodecError> {
    let payload = record.payload();
    let payload_offset = record.payload_offset();
    let _depth = ctx.enter_nested("NX surface branch path")?;
    ctx.charge_work(1, "scan NX surface branch path")?;
    if remaining == 0 {
        return Ok(());
    }
    let Some(mode) = payload
        .get(at)
        .copied()
        .and_then(|value| SurfaceBranchMode::try_from(value).ok())
    else {
        return Ok(());
    };
    if payload.get(at + 1) != Some(&1) {
        return Ok(());
    }
    let Some(&declared_count @ 2..) = payload.get(at + 2) else {
        return Ok(());
    };
    let mut cursor = at + 3;
    let member_start = cursor;
    let mut rows = 1..declared_count;
    while ctx
        .next_charged(&mut rows, "NX surface feature branch paths range traversal")?
        .is_some()
    {
        let Some(token) = payload.get(cursor..).and_then(PayloadIndexToken::read) else {
            return Ok(());
        };
        cursor += token.raw().len();
    }
    let member_bytes = &payload[member_start..cursor];
    let witnessed = payload.get(cursor..cursor + 2) == Some(&[1, declared_count]);
    if witnessed {
        cursor += 2;
    }
    let zero_count = if witnessed {
        usize::from(declared_count) + 3
    } else {
        5
    };
    let Some(zero_lane) = payload.get(cursor..cursor + zero_count) else {
        return Ok(());
    };
    if (witnessed
        && !ctx.all_by(
            zero_lane,
            |&byte| Ok(byte == 0),
            "NX surface branch zero lane",
        )?)
        || (!witnessed && zero_lane != [0; 5])
    {
        return Ok(());
    }
    cursor += zero_count;
    if payload.get(cursor..cursor + 3) != Some(&[0xff, 1, 2]) {
        return Ok(());
    }
    cursor += 3;
    let terminal_relative = u64_from_index(cursor - at);
    let Some(terminal) = payload.get(cursor..).and_then(PayloadIndexToken::read) else {
        return Ok(());
    };
    cursor += terminal.raw().len();
    if payload.get(cursor) != Some(&0) {
        return Ok(());
    }
    cursor += 1;
    let Some(offset) = u64_from_index(payload_offset).checked_add(u64_from_index(at)) else {
        return Ok(());
    };
    for suffix_len in 1..=5 {
        let Some(suffix) = payload.get(cursor..cursor + suffix_len) else {
            continue;
        };
        let next = cursor + suffix_len;
        if offset.checked_add(u64_from_index(next - at)).is_none() {
            continue;
        }
        let branch = SurfaceBranchShape {
            offset,
            terminal_relative,
            mode,
            witnessed,
            declared_count,
            member_bytes,
            terminal,
            suffix,
        };
        ctx.push_vec(&mut search.path, branch, "NX surface branch search stack")?;
        if remaining == 1 {
            if payload.get(next..next + terminator.len()) == Some(terminator) {
                search.matches += 1;
                if search.matches == 1 {
                    search.first_path = Some(selected_storage.with_storage(|| {
                        let mut first_path = ctx
                            .collection_vec(search.path.len(), "NX surface selected path shapes")?;
                        for &shape in
                            ctx.admit_iter(&search.path, "NX surface selected path shape copies")?
                        {
                            first_path.push(shape);
                        }
                        Ok::<_, CodecError>(first_path)
                    })?);
                }
            }
        } else {
            surface_feature_branch_paths(
                ctx,
                record,
                next,
                remaining - 1,
                terminator,
                search,
                selected_storage,
            )?;
        }
        search.path.pop();
        if search.matches == 2 {
            return Ok(());
        }
    }
    Ok(())
}

/// Decode the unique exactly framed counted branch group in a bounded `SKIN`
/// or `Studio Surface` payload.
pub(crate) fn surface_feature_payload_branches<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<
    (
        Option<SurfaceFeaturePayloadBranches>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    const SKIN_TERMINATOR: [u8; 11] = [
        0x00, 0x00, 0x00, 0x01, 0x03, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01,
    ];
    const STUDIO_TERMINATOR: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01];
    let mut storage = ctx.reserve_scoped(0, "NX surface branch scratch")?;
    let terminator = match record.name() {
        "SKIN" => &SKIN_TERMINATOR[..],
        "Studio Surface" => &STUDIO_TERMINATOR[..],
        _ => return Ok((None, storage)),
    };
    let mut candidate = None;
    if let Some(range_end) = record.payload().len().checked_sub(6) {
        let mut starts = 0..range_end;
        while let Some(start) = ctx.next_charged(&mut starts, "scan NX surface branch groups")? {
            if record.payload().get(start..start + 2) != Some(&[0xa0, 0x5a]) {
                continue;
            }
            let Some(family) = record
                .payload()
                .get(start + 2)
                .copied()
                .and_then(|byte| SurfaceFamily::try_from(byte).ok())
            else {
                continue;
            };
            let Some(header_code) = record.payload().get(start + 3).copied() else {
                continue;
            };
            if record.payload().get(start + 4) != Some(&0x01) {
                continue;
            }
            let Some(declared_group_count @ 1..) = record.payload().get(start + 5).copied() else {
                continue;
            };
            let mut search_storage = ctx.reserve_scoped(0, "NX surface branch search storage")?;
            let mut selected_storage =
                ctx.reserve_scoped(0, "NX surface selected shape storage")?;
            let (shapes, matches) = search_storage.with_storage(|| {
                let mut search = SurfacePathSearch {
                    path: Vec::new(),
                    first_path: None,
                    matches: 0,
                };
                surface_feature_branch_paths(
                    ctx,
                    record,
                    start + 6,
                    declared_group_count,
                    terminator,
                    &mut search,
                    &mut selected_storage,
                )?;
                Ok::<_, CodecError>((search.first_path, search.matches))
            })?;
            drop(search_storage);
            if matches != 1 {
                continue;
            }
            let Some(shapes) = shapes else {
                continue;
            };
            if candidate.is_some() {
                return Ok((None, storage));
            }
            candidate = Some((family, header_code, shapes, selected_storage));
        }
    }
    let Some((family, header_code, shapes, _selected_storage)) = candidate else {
        return Ok((None, storage));
    };
    let mut branches =
        storage.with_storage(|| ctx.collection_vec(shapes.len(), "NX surface branches"))?;
    for shape in ctx.admit_iter(shapes, "NX surface selected branch materialization")? {
        let mut members = storage.with_storage(|| {
            ctx.collection_vec(
                usize::from(shape.declared_count - 1),
                "NX surface branch members",
            )
        })?;
        let mut at = 0;
        for _ in ctx.admit_iter(
            &(1..shape.declared_count),
            "NX surface branch member materialization",
        )? {
            let Some(token) = shape
                .member_bytes
                .get(at..)
                .and_then(PayloadIndexToken::read)
            else {
                return Ok((None, storage));
            };
            at += token.raw().len();
            members.push((token, ()));
        }
        let Ok(members) = BranchItems::new(members) else {
            return Ok((None, storage));
        };
        let suffix = ctx.copy_retained(shape.suffix, "NX surface suffix bytes")?;
        let Ok(suffix) = SurfaceSuffix::new(suffix) else {
            return Ok((None, storage));
        };
        branches.push(SurfaceBranch {
            offset: shape.offset,
            terminal_relative: shape.terminal_relative,
            mode: shape.mode,
            witnessed: shape.witnessed,
            members,
            terminal: (shape.terminal, ()),
            suffix,
        });
    }
    Ok((
        Some(SurfaceFeaturePayloadBranches {
            family,
            header_code,
            branches,
        }),
        storage,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn surface_branch_search_uses_scoped_shapes_before_owned_members() {
        let mut bytes = vec![0xa0, 0x5a, 0x14, 0x13, 1, 1, 0x40, 1, 2, 0xf0, 1];
        bytes.extend([0; 5]);
        bytes.extend([0xff, 1, 2, 0xf0, 2, 0, 0x81, 0x58]);
        bytes.extend([0, 0, 0, 1, 3, 0, 0, 0, 0xff, 0xff, 1]);
        let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            super::surface_feature_payload_branches(
                ctx,
                super::OperationPayload::new(&bytes, 0, "SKIN").unwrap(),
            )
            .map(|(value, _storage)| value)
        };
        let group = crate::test_support::with_decode_context(decode)
            .unwrap()
            .unwrap();
        assert_eq!(group.branches.len(), 1);
        assert_eq!(group.branches[0].members.as_slice()[0].0.value(), 1);
        assert_eq!(group.branches[0].terminal.0.value(), 2);
        crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            "NX surface branch search stack",
            decode,
        );
        crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            "NX surface selected path shapes",
            decode,
        );
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                // Four search slots and one selected shape set the scratch peak.
                policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(
                    5 * std::mem::size_of::<super::SurfaceBranchShape<'_>>(),
                );
                // Only the two suffix bytes survive native branch projection.
                policy.limits.max_retained_bytes = 2;
            },
            |ctx| {
                let group = decode(ctx).unwrap().unwrap();
                assert_eq!(group.branches.len(), 1);
                assert_eq!(group.branches[0].members.as_slice()[0].0.value(), 1);
                assert_eq!(group.branches[0].terminal.0.value(), 2);
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
        crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "NX surface suffix bytes",
            decode,
        );
        bytes.pop();
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_retained_bytes = 0,
            |ctx| {
                assert!(super::surface_feature_payload_branches(
                    ctx,
                    super::OperationPayload::new(&bytes, 0, "SKIN").unwrap()
                )
                .map(|(value, _storage)| value)
                .unwrap()
                .is_none());
            },
        );
    }

    use super::{SurfaceFamily, SurfaceSuffix};

    #[test]
    fn surface_family_preserves_exact_numeric_domain() {
        for byte in u8::MIN..=u8::MAX {
            let wire = byte.to_string();
            let decoded = serde_json::from_str::<SurfaceFamily>(&wire);
            if matches!(byte, 0x14 | 0x50) {
                assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), wire);
            } else {
                assert!(decoded.unwrap_err().to_string().contains("family"));
            }
        }
    }

    #[test]
    fn surface_suffix_preserves_opaque_bytes_and_rejects_invalid_lengths() {
        for wire in [
            "[255]",
            "[0,255]",
            "[1,2,3]",
            "[0,1,2,255]",
            "[255,0,1,2,3]",
        ] {
            let decoded: SurfaceSuffix = serde_json::from_str(wire).unwrap();
            assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
            assert!((1..=5).contains(&decoded.into_vec().len()));
        }
        for wire in ["[]", "[0,1,2,3,4,5]"] {
            assert!(serde_json::from_str::<SurfaceSuffix>(wire)
                .unwrap_err()
                .to_string()
                .contains("suffix"));
        }
    }
}
