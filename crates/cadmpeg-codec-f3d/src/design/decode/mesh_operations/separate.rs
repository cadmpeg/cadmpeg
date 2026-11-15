use super::{MeshOperationBodyReferences, MeshOperationFraming, MeshOperationRecords};
use crate::bytes::{take_reference, Reference};
use crate::layout::mesh_operation_scope_prefix as prefix;
use crate::layout::mesh_separate_common_envelope as envelope;
use crate::layout::mesh_separate_scope_tail as tail;
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotValue<'a> {
    Null,
    Present {
        target: u32,
        inline_type: Option<&'a str>,
        registration: Result<usize, MeshOperationFraming>,
    },
    Invalid(MeshOperationFraming),
    Unread(MeshOperationFraming),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Slot<'a> {
    pub(crate) offset: usize,
    pub(crate) raw: &'a [u8],
    pub(crate) value: SlotValue<'a>,
}

impl Slot<'_> {
    pub(crate) fn target(self) -> Option<u32> {
        match self.value {
            SlotValue::Present { target, .. } => Some(target),
            _ => None,
        }
    }

    pub(crate) fn admitted(self) -> Option<u32> {
        match self.value {
            SlotValue::Present {
                target,
                registration: Ok(_),
                ..
            } => Some(target),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Run<'a> {
    pub(crate) declared: Option<u32>,
    pub(crate) slots: Vec<Slot<'a>>,
    pub(crate) unread: Option<(usize, usize, u32)>,
    pub(crate) framing: Option<MeshOperationFraming>,
}

#[derive(Debug)]
pub(crate) struct SeparateScope<'a> {
    pub(crate) frame: View<'a>,
    pub(crate) flag: Option<u8>,
    pub(crate) prefix_word: Option<u32>,
    pub(crate) outputs: Run<'a>,
    pub(crate) selector: Option<u32>,
    pub(crate) lane: &'a [u8],
    pub(crate) source: Option<Slot<'a>>,
    pub(crate) separator: Option<u8>,
    pub(crate) envelope: &'a [u8],
    pub(crate) common: Run<'a>,
    pub(crate) framing: Option<MeshOperationFraming>,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceDetails<'a> {
    pub(crate) frame: View<'a>,
    pub(crate) run: Run<'a>,
    pub(crate) references: Vec<Slot<'a>>,
    pub(crate) words: Vec<u32>,
    pub(crate) identifiers: Vec<&'a [u8]>,
    pub(crate) role: Option<u64>,
    pub(crate) framing: Option<MeshOperationFraming>,
}

pub(crate) struct SeparateSource<'a> {
    pub(crate) membership: MeshOperationBodyReferences,
    pub(crate) group: SourceDetails<'a>,
    pub(crate) members: Vec<Option<Result<SourceDetails<'a>, MeshOperationFraming>>>,
}

#[derive(Clone, Copy)]
enum Role {
    Body,
    Group,
    Member,
    Any,
}

impl MeshOperationRecords<'_> {
    fn separate_registration(
        &self,
        target: u32,
        inline: Option<&str>,
        role: Role,
    ) -> Result<usize, MeshOperationFraming> {
        let error = MeshOperationFraming("Separate reference registration");
        let ordinal = self
            .registrations
            .get(&u64::from(target))
            .copied()
            .flatten()
            .ok_or(error)?;
        let row = &self.meta.types[ordinal];
        let base = row.base_type_guid.value().ok_or(error)?;
        if inline.is_some_and(|guid| !guid.eq_ignore_ascii_case(row.type_guid.as_str())) {
            return Err(MeshOperationFraming("Separate inline reference type"));
        }
        let (guid, base_guid, module, version) = match role {
            Role::Body => (
                "EA90DA22-556C-4C61-89BB-20C2681B7A9D",
                "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
                "ParaMesh",
                None,
            ),
            Role::Group => (
                super::MESH_GROUP_GUID,
                "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
                "Fusion",
                Some(0),
            ),
            Role::Member => (
                "1AA9CC7B-5C34-45DA-AE14-0444E7F966F2",
                "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                "Fusion",
                Some(0),
            ),
            Role::Any => (
                row.type_guid.as_str(),
                base.as_str(),
                row.module.as_str(),
                Some(row.version),
            ),
        };
        self.record(
            target,
            "Separate reference target",
            guid,
            base_guid,
            module,
            version,
        )?;
        Ok(ordinal)
    }
}

struct Cursor<'a>(View<'a>);

impl<'a> Cursor<'a> {
    fn word(&mut self, field: &'static str) -> Result<u32, MeshOperationFraming> {
        self.0.u32_le().ok_or(MeshOperationFraming(field))
    }

    fn byte(&mut self, field: &'static str) -> Result<u8, MeshOperationFraming> {
        self.0.u8().ok_or(MeshOperationFraming(field))
    }

    fn slot(&mut self, records: &MeshOperationRecords<'_>, role: Role) -> (Slot<'a>, bool) {
        let offset = self.0.position();
        let start = offset - self.0.start();
        let mut end = start;
        let Some(reference) = take_reference(self.0.window(), &mut end) else {
            let raw = &self.0.window()[start..];
            let after_target = 1 + std::mem::size_of::<u64>();
            let typed = View::u32_le_at(raw, after_target) == Some(36);
            let envelope = if typed {
                crate::bytes::TYPED_LOCAL_REFERENCE_LEN - 2
            } else {
                after_target
            };
            let type_prefix = raw.get(after_target..).unwrap_or(&[]);
            let partial_type = !type_prefix.is_empty()
                && type_prefix.len() < std::mem::size_of::<u32>()
                && 36u32.to_le_bytes().starts_with(type_prefix);
            let invalid_type = typed
                && raw
                    .get(after_target + std::mem::size_of::<u32>()..envelope)
                    .is_some_and(|guid| {
                        !std::str::from_utf8(guid).is_ok_and(crate::bytes::is_guid_hyphenated)
                    });
            let invalid = raw.first().is_some_and(|byte| *byte > 1)
                || invalid_type
                || (!partial_type
                    && (raw.get(envelope).is_some_and(|byte| *byte != 0)
                        || (raw.get(envelope) == Some(&0)
                            && raw.get(envelope + 1).is_some_and(|byte| *byte != 0))));
            let value = if invalid {
                SlotValue::Invalid(MeshOperationFraming("Separate rejected reference envelope"))
            } else {
                SlotValue::Unread(MeshOperationFraming(
                    "Separate truncated reference envelope",
                ))
            };
            return (Slot { offset, raw, value }, false);
        };
        let raw = &self.0.window()[start..end];
        if self.0.seek(self.0.start() + end).is_none() {
            return (
                Slot {
                    offset,
                    raw,
                    value: SlotValue::Invalid(MeshOperationFraming("Separate reference extent")),
                },
                false,
            );
        }
        let value = match reference {
            Reference::Null => SlotValue::Null,
            Reference::Local {
                target,
                inline_type_guid,
            } => match u32::try_from(target).ok().filter(|target| *target != 0) {
                Some(target) => SlotValue::Present {
                    target,
                    inline_type: inline_type_guid,
                    registration: records.separate_registration(target, inline_type_guid, role),
                },
                None => SlotValue::Invalid(MeshOperationFraming("Separate local target range")),
            },
            _ => SlotValue::Invalid(MeshOperationFraming("Separate nonlocal reference")),
        };
        (Slot { offset, raw, value }, true)
    }

    fn run(
        &mut self,
        ctx: &DecodeContext<'_>,
        records: &MeshOperationRecords<'_>,
        role: Role,
    ) -> Result<Run<'a>, CodecError> {
        let mut run = Run::default();
        let declared = match self.word("Separate reference count") {
            Ok(count) => count,
            Err(error) => {
                run.framing = Some(error);
                return Ok(run);
            }
        };
        run.declared = Some(declared);
        let complete = bounded_len(u64::from(declared), 1, self.0.remaining());
        let count = complete.unwrap_or(self.0.remaining());
        let mut end = self.0.position() - self.0.start();
        let mut attempts = 0;
        while attempts < count {
            attempts += 1;
            if take_reference(self.0.window(), &mut end).is_none() {
                break;
            }
        }
        ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(attempts),
            "F3D Separate reference slots",
        )?;
        let empty = Slot {
            offset: self.0.position(),
            raw: &[],
            value: SlotValue::Unread(MeshOperationFraming("Separate unread slot")),
        };
        run.slots = ctx.alloc_filled(attempts, empty, "F3D Separate reference slots")?;
        for ordinal in 0..attempts {
            let (slot, framed) = self.slot(records, role);
            run.slots[ordinal] = slot;
            if !framed {
                run.slots.truncate(ordinal + 1);
                run.unread = Some((
                    ordinal,
                    slot.offset,
                    declared - u32::try_from(ordinal).map_err(CodecError::malformed)?,
                ));
                run.framing = Some(MeshOperationFraming("Separate reference run"));
                return Ok(run);
            }
        }
        if complete.is_none() {
            let ordinal = u32::try_from(count).map_err(CodecError::malformed)?;
            run.unread = Some((count, self.0.position(), declared - ordinal));
            run.framing = Some(MeshOperationFraming("Separate impossible reference count"));
        }
        Ok(run)
    }

    fn identifier(&mut self) -> Result<&'a [u8], MeshOperationFraming> {
        let count = self.word("Separate identifier length")?;
        let count = bounded_len(u64::from(count), 2, self.0.remaining())
            .ok_or(MeshOperationFraming("Separate identifier extent"))?;
        let at = self.0.position() - self.0.start();
        self.0
            .skip(count * 2)
            .ok_or(MeshOperationFraming("Separate identifier extent"))?;
        Ok(&self.0.window()[at..at + count * 2])
    }
}

impl<'a> MeshOperationRecords<'a> {
    pub(crate) fn separate_scope(
        &self,
        ctx: &DecodeContext<'_>,
        index: u32,
    ) -> Result<Result<SeparateScope<'a>, MeshOperationFraming>, CodecError> {
        let frame = match self.record(
            index,
            "Separate scope record",
            "6B6FDE2E-D9B9-4866-9D07-3123C919B0D2",
            "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF",
            "Fusion",
            Some(2),
        ) {
            Ok(frame) => frame,
            Err(error) => return Ok(Err(error)),
        };
        read_scope(ctx, self, frame).map(Ok)
    }

    pub(crate) fn separate_source(
        &self,
        ctx: &DecodeContext<'_>,
        index: u32,
    ) -> Result<Result<SeparateSource<'a>, MeshOperationFraming>, CodecError> {
        let frame = match self.record(
            index,
            "Separate source group",
            super::MESH_GROUP_GUID,
            "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
            "Fusion",
            Some(0),
        ) {
            Ok(frame) => frame,
            Err(error) => return Ok(Err(error)),
        };
        let group = self.separate_details(ctx, frame, false)?;
        let membership = self.group_body_references(ctx, index)?;
        let mut members =
            ctx.alloc_filled(group.run.slots.len(), None, "F3D Separate member details")?;
        for (position, slot) in group.run.slots.iter().enumerate() {
            let Some(member) = slot.target() else {
                continue;
            };
            members[position] = Some(
                match self.record(
                    member,
                    "Separate source member",
                    "1AA9CC7B-5C34-45DA-AE14-0444E7F966F2",
                    "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                    "Fusion",
                    Some(0),
                ) {
                    Ok(frame) => Ok(self.separate_details(ctx, frame, true)?),
                    Err(error) => Err(error),
                },
            );
        }
        Ok(Ok(SeparateSource {
            membership,
            group,
            members,
        }))
    }

    fn separate_details(
        &self,
        ctx: &DecodeContext<'_>,
        frame: View<'a>,
        member: bool,
    ) -> Result<SourceDetails<'a>, CodecError> {
        let mut details = SourceDetails {
            frame,
            run: Run::default(),
            references: Vec::new(),
            words: Vec::new(),
            identifiers: Vec::new(),
            role: None,
            framing: None,
        };
        let mut cursor = Cursor(frame);
        let read = (|| -> Result<Result<(), MeshOperationFraming>, CodecError> {
            if frame
                .window()
                .get(prefix::ZERO_RUN_9..prefix::FLAG + 1)
                .is_none_or(|bytes| bytes.iter().any(|byte| *byte != 0))
            {
                return Ok(Err(MeshOperationFraming("Separate source prefix")));
            }
            if cursor.0.seek(frame.start() + prefix::FLAG + 1).is_none() {
                return Ok(Err(MeshOperationFraming("Separate source prefix")));
            }
            if !member {
                details.run = cursor.run(ctx, self, Role::Member)?;
                if let Some(error) = details.run.framing {
                    return Ok(Err(error));
                }
            }
            Ok((|| {
                for role in if member {
                    &[Role::Body, Role::Any][..]
                } else {
                    &[Role::Any, Role::Any][..]
                } {
                    let (slot, framed) = cursor.slot(self, *role);
                    details.references.push(slot);
                    if !framed {
                        return Err(MeshOperationFraming("Separate source reference"));
                    }
                }
                details.words.push(cursor.word("Separate source word")?);
                if member {
                    for _ in 0..2 {
                        details.identifiers.push(cursor.identifier()?);
                    }
                    for _ in 0..2 {
                        details.words.push(cursor.word("Separate member word")?);
                    }
                } else {
                    let (slot, framed) = cursor.slot(self, Role::Any);
                    details.references.push(slot);
                    if !framed {
                        return Err(MeshOperationFraming("Separate auxiliary reference"));
                    }
                    details.role = Some(
                        cursor
                            .0
                            .u64_le()
                            .ok_or(MeshOperationFraming("Separate group role"))?,
                    );
                }
                Ok(())
            })())
        })()?;
        details.framing = read.err();
        Ok(details)
    }
}

fn read_scope<'a>(
    ctx: &DecodeContext<'_>,
    records: &MeshOperationRecords<'_>,
    frame: View<'a>,
) -> Result<SeparateScope<'a>, CodecError> {
    let mut parsed = SeparateScope {
        frame,
        flag: None,
        prefix_word: None,
        outputs: Run::default(),
        selector: None,
        lane: &[],
        source: None,
        separator: None,
        envelope: &[],
        common: Run::default(),
        framing: None,
    };
    let mut cursor = Cursor(frame);
    let read = (|| -> Result<Result<(), MeshOperationFraming>, CodecError> {
        if frame
            .window()
            .get(prefix::ZERO_RUN_9..prefix::FLAG)
            .is_none_or(|bytes| bytes.iter().any(|byte| *byte != 0))
        {
            return Ok(Err(MeshOperationFraming("Separate scope prefix")));
        }
        if cursor.0.seek(frame.start() + prefix::FLAG).is_none() {
            return Ok(Err(MeshOperationFraming("Separate scope prefix")));
        }
        let head = (|| {
            let flag = cursor.byte("Separate flag")?;
            parsed.flag = Some(flag);
            match flag {
                0 => (),
                1 => parsed.prefix_word = Some(cursor.word("Separate prefix word")?),
                _ => return Err(MeshOperationFraming("Separate flag")),
            }
            Ok(())
        })();
        if let Err(error) = head {
            return Ok(Err(error));
        }
        parsed.outputs = cursor.run(ctx, records, Role::Body)?;
        if let Some(error) = parsed.outputs.framing {
            return Ok(Err(error));
        }
        let tail = (|| {
            parsed.selector = Some(cursor.word("Separate selector")?);
            let at = cursor.0.position() - frame.start();
            let lane_width = tail::SOURCE_GROUP - tail::RAW_LANE;
            let count = cursor.0.remaining().min(lane_width);
            parsed.lane = &frame.window()[at..at + count];
            cursor
                .0
                .skip(lane_width)
                .ok_or(MeshOperationFraming("Separate raw lane"))?;
            let (slot, framed) = cursor.slot(records, Role::Group);
            parsed.source = Some(slot);
            if !framed {
                return Err(MeshOperationFraming("Separate source group reference"));
            }
            parsed.separator = Some(cursor.byte("Separate source separator")?);
            let start = cursor.0.position() - frame.start();
            parsed.envelope = &frame.window()[start..];
            let guid = cursor.identifier()?;
            if guid.len() != envelope::ZEROS - envelope::GUID_UTF16 {
                return Err(MeshOperationFraming("Separate common GUID extent"));
            }
            cursor
                .0
                .skip(envelope::LEN - envelope::ZEROS)
                .ok_or(MeshOperationFraming("Separate common envelope"))?;
            parsed.envelope = &frame.window()[start..cursor.0.position() - frame.start()];
            Ok(())
        })();
        if let Err(error) = tail {
            return Ok(Err(error));
        }
        parsed.common = cursor.run(ctx, records, Role::Any)?;
        Ok(match parsed.common.framing {
            Some(error) => Err(error),
            None => Ok(()),
        })
    })()?;
    parsed.framing = read.err();
    Ok(parsed)
}

#[cfg(test)]
mod tests;
