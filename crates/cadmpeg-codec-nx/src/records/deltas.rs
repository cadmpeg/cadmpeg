// SPDX-License-Identifier: Apache-2.0

use super::instance::{read_status_instance, StatusInstance};
use super::{base, own_definition, positive, read_definition, BaseVersion, SchemaPlan};
use crate::deltas::partition_prefix::PartitionPrefixState;
use crate::deltas::pmark_state::PmarkState;
use crate::deltas::section::{MapEntry, MapRead, SectionKind, TerminalWord};
use crate::deltas::transmit_state::TransmitState;
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Definition {
    declaration: Range<usize>,
    plan: Rc<SchemaPlan>,
}

impl Definition {
    pub(crate) fn declaration(&self) -> Range<usize> {
        self.declaration.clone()
    }
    pub(crate) fn plan(&self) -> &SchemaPlan {
        &self.plan
    }
    pub(crate) fn shared_plan(&self) -> Rc<SchemaPlan> {
        Rc::clone(&self.plan)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Instance {
    definition: Rc<Definition>,
    data: StatusInstance,
}

impl Instance {
    pub(crate) fn definition(&self) -> &Definition {
        &self.definition
    }
    pub(crate) fn identity_at(&self) -> usize {
        self.data.identity_at
    }
    pub(crate) fn xmt(&self) -> NonNullXmt {
        self.data.xmt
    }
    pub(crate) fn variable_length(&self) -> Option<u32> {
        self.data.variable_length
    }
    pub(crate) fn fields(&self) -> &[Range<usize>] {
        &self.data.fields
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Metadata {
    Map {
        entries: Vec<MapEntry>,
        terminal_word: TerminalWord,
    },
    Pmark(PmarkState),
    Terminator {
        closes: SectionKind,
        section_start: usize,
    },
    FinalTerminator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EventContent {
    Header(TransmitState),
    PartitionPrefix(PartitionPrefixState),
    Instance(Instance),
    Metadata(Metadata),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Event {
    range: Range<usize>,
    content: EventContent,
}

impl Event {
    pub(crate) fn range(&self) -> Range<usize> {
        self.range.clone()
    }
    pub(crate) fn content(&self) -> &EventContent {
        &self.content
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StopReason {
    MissingHeader,
    UnsupportedBase,
    InvalidHeader,
    InvalidDeclaration,
    InvalidInstance,
    UnsupportedPacket,
    UnclosedSection,
    MissingSection,
    InvalidMap,
    UnresolvedMapWordWidth,
    InvalidPmark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum DeltasEnd {
    Complete,
    Stopped { offset: usize, reason: StopReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeltasRecords<'s> {
    source: &'s [u8],
    definitions: Vec<Rc<Definition>>,
    events: Vec<Event>,
    end: DeltasEnd,
}

struct Header {
    state: TransmitState,
    embedded: bool,
    maximum: u16,
}

fn header(view: &mut View<'_>) -> Result<Header, StopReason> {
    if view.take(2) != Some(b"PS") {
        return Err(StopReason::MissingHeader);
    }
    let Some((description, schema, embedded, maximum, user_fields)) = (|| {
        let length = usize::try_from(view.u32_be()?).ok()?;
        let description = std::str::from_utf8(view.take(length)?).ok()?;
        let length = usize::try_from(view.u32_be()?).ok()?;
        let schema = std::str::from_utf8(view.take(length)?).ok()?;
        let token = cadmpeg_parasolid::find_schema_token(schema.as_bytes())?;
        (token.value() == schema).then_some(())?;
        let embedded = match schema.split('_').count() {
            3 => false,
            4 => true,
            _ => return None,
        };
        let maximum = if embedded {
            view.u16_be()?
        } else {
            base::maximum(BaseVersion::V13006)
        };
        let user_fields = view.u32_be()?;
        Some((description, schema, embedded, maximum, user_fields))
    })() else {
        return Err(StopReason::InvalidHeader);
    };
    if schema.rsplit('_').next() != Some("13006") {
        return Err(StopReason::UnsupportedBase);
    }
    if maximum < 4 || user_fields != 0 {
        return Err(StopReason::InvalidHeader);
    }
    TransmitState::new(
        description.to_owned(),
        schema.to_owned(),
        maximum,
        user_fields,
    )
    .map(|state| Header {
        state,
        embedded,
        maximum,
    })
    .map_err(|_| StopReason::InvalidHeader)
}

fn first_definition(
    view: &mut View<'_>,
    kind: u16,
    embedded: bool,
    start: usize,
) -> Result<Option<Definition>, CodecError> {
    let plan = if embedded {
        let Some(count) = view.u8() else {
            return Ok(None);
        };
        read_definition(view, BaseVersion::V13006, kind, count)?
    } else {
        base::fields(BaseVersion::V13006, kind)
            .map(|fields| own_definition(kind, fields))
            .transpose()?
    };
    if let Some(plan) = plan {
        if plan
            .fields
            .iter()
            .any(|field| matches!(field.data_type, super::DataType::Unestablished(_)))
        {
            return Ok(None);
        }
        Ok(Some(Definition {
            declaration: start..view.position(),
            plan: Rc::new(plan),
        }))
    } else {
        Ok(None)
    }
}

fn matches_base(plan: &SchemaPlan) -> bool {
    base::fields(BaseVersion::V13006, plan.kind).is_some_and(|fields| {
        plan.fields.len() == fields.len()
            && plan.fields.iter().zip(fields).all(|(actual, expected)| {
                actual.name.as_str() == expected.name
                    && actual.data_type == expected.data_type
                    && actual.pointer_class == expected.pointer_class
                    && actual.n_elements == expected.n_elements
                    && actual.transmit == expected.transmit
            })
    })
}

fn prefix(
    view: &mut View<'_>,
    embedded: bool,
) -> Result<Option<(PartitionPrefixState, [Definition; 2])>, CodecError> {
    let start = view.position();
    if view.u16_be() != Some(3) {
        return Ok(None);
    }
    let Some(partition) = first_definition(view, 3, embedded, start)? else {
        return Ok(None);
    };
    if !matches_base(&partition.plan) {
        return Ok(None);
    }
    let Some((xmt, current_pmark, highest_id)) = (|| {
        let xmt = NonNullXmt::try_from(positive(view)?.checked_add(1)?).ok()?;
        let current_pmark = NonNullXmt::try_from(positive(view)?.checked_add(1)?).ok()?;
        Some((xmt, current_pmark, view.u32_be()?))
    })() else {
        return Ok(None);
    };
    let start = view.position();
    if view.u16_be() != Some(4) {
        return Ok(None);
    }
    let Some(pmark) = first_definition(view, 4, embedded, start)? else {
        return Ok(None);
    };
    if !matches_base(&pmark.plan) {
        return Ok(None);
    }
    let Some(mark) = PmarkState::read(view) else {
        return Ok(None);
    };
    let Some(state) = PartitionPrefixState::new(xmt, current_pmark, highest_id, mark) else {
        return Ok(None);
    };
    Ok(Some((state, [partition, pmark])))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeState {
    Leading,
    Map,
    PmarkHead,
    PmarkNodes,
}

impl NodeState {
    fn kind(self) -> SectionKind {
        match self {
            Self::Leading => SectionKind::Leading,
            Self::Map => SectionKind::Map,
            Self::PmarkHead | Self::PmarkNodes => SectionKind::Pmark,
        }
    }

    fn after(self) -> Expect {
        match self {
            Self::Leading => Expect::FirstMap,
            Self::Map => Expect::PmarkOrFinal,
            Self::PmarkHead | Self::PmarkNodes => Expect::MapOrEnd,
        }
    }
}

#[derive(Clone, Copy)]
enum Expect {
    FirstMap,
    PmarkOrFinal,
    MapOrEnd,
}

struct OpenSection {
    state: NodeState,
    start: usize,
    events: Vec<Event>,
    definitions: Vec<Rc<Definition>>,
}

impl OpenSection {
    fn new(state: NodeState, start: usize) -> Self {
        Self {
            state,
            start,
            events: Vec::new(),
            definitions: Vec::new(),
        }
    }

    fn push(&mut self, event: Event) {
        self.events.push(event);
    }

    fn first_use(&mut self, definition: Definition) -> Rc<Definition> {
        let definition = Rc::new(definition);
        self.definitions.push(Rc::clone(&definition));
        definition
    }

    fn read_node(
        &mut self,
        view: &mut View<'_>,
        embedded: bool,
        established: &BTreeMap<u16, Rc<Definition>>,
        kind: u16,
        start: usize,
    ) -> Result<Result<(), StopReason>, CodecError> {
        let existing = if let Some(definition) = established.get(&kind) {
            Some(Rc::clone(definition))
        } else {
            self.definitions
                .iter()
                .find(|definition| definition.plan.kind == kind)
                .map(Rc::clone)
        };
        let new = if existing.is_none() {
            match first_definition(view, kind, embedded, start)? {
                Some(definition) => Some(definition),
                None => return Ok(Err(StopReason::InvalidDeclaration)),
            }
        } else {
            None
        };
        let plan = new
            .as_ref()
            .map(|definition| &definition.plan)
            .or_else(|| existing.as_ref().map(|definition| &definition.plan));
        let Some(plan) = plan else {
            return Err(CodecError::Malformed(
                "missing established NX Deltas schema".into(),
            ));
        };
        let Some(data) = read_status_instance(view, plan) else {
            return Ok(Err(StopReason::InvalidInstance));
        };
        let definition = match (existing, new) {
            (Some(definition), _) => definition,
            (_, Some(definition)) => self.first_use(definition),
            _ => return Err(CodecError::Malformed("missing NX Deltas definition".into())),
        };
        self.push(Event {
            range: start..view.position(),
            content: EventContent::Instance(Instance { definition, data }),
        });
        if self.state == NodeState::PmarkHead {
            self.state = NodeState::PmarkNodes;
        }
        Ok(Ok(()))
    }
}

fn read_map(view: &mut View<'_>, maximum: u16) -> Result<Metadata, StopReason> {
    let mut entries = Vec::new();
    loop {
        match MapEntry::read(view, maximum) {
            Some(MapRead::Entry(entry)) => entries.push(entry),
            Some(MapRead::End) => break,
            None => return Err(StopReason::InvalidMap),
        }
    }
    if view.u16_be() != Some(0) {
        return Err(StopReason::InvalidMap);
    }
    let word = view.u16_be().ok_or(StopReason::InvalidMap)?;
    let terminal_word =
        TerminalWord::try_from(word).map_err(|_| StopReason::UnresolvedMapWordWidth)?;
    Ok(Metadata::Map {
        entries,
        terminal_word,
    })
}

impl<'s> DeltasRecords<'s> {
    pub(crate) fn source(&self) -> &'s [u8] {
        self.source
    }
    pub(crate) fn definitions(&self) -> &[Rc<Definition>] {
        &self.definitions
    }
    pub(crate) fn events(&self) -> &[Event] {
        &self.events
    }
    pub(crate) fn end(&self) -> &DeltasEnd {
        &self.end
    }
    pub(crate) fn suffix(&self) -> Range<usize> {
        match self.end {
            DeltasEnd::Complete => self.source.len()..self.source.len(),
            DeltasEnd::Stopped { offset, .. } => offset..self.source.len(),
        }
    }
    pub(crate) fn census(&self) -> crate::deltas::census::Census<'_, 's> {
        crate::deltas::census::Census::new(self)
    }

    pub(crate) fn read(source: &'s [u8]) -> Result<Self, CodecError> {
        let mut result = Self {
            source,
            definitions: Vec::new(),
            events: Vec::new(),
            end: DeltasEnd::Complete,
        };
        let mut view = View::over_retained(source);
        let header = match header(&mut view) {
            Ok(header) => header,
            Err(reason) => {
                result.end = DeltasEnd::Stopped { offset: 0, reason };
                return Ok(result);
            }
        };
        result.events.push(Event {
            range: 0..view.position(),
            content: EventContent::Header(header.state),
        });
        let start = view.position();
        let Some((state, definitions)) = prefix(&mut view, header.embedded)? else {
            result.end = DeltasEnd::Stopped {
                offset: start,
                reason: StopReason::UnsupportedPacket,
            };
            return Ok(result);
        };
        let mut established = BTreeMap::new();
        for definition in definitions {
            result.establish(&mut established, definition);
        }
        result.events.push(Event {
            range: start..view.position(),
            content: EventContent::PartitionPrefix(state),
        });
        let mut section = OpenSection::new(NodeState::Leading, view.position());
        loop {
            let at = view.position();
            let outcome = match view.u16_be() {
                None => Err(StopReason::UnclosedSection),
                Some(1) => match view.u16_be() {
                    None => Err(StopReason::UnclosedSection),
                    Some(1) => {
                        let expect = section.state.after();
                        let closes = section.state.kind();
                        let section_start = section.start;
                        for definition in &section.definitions {
                            established.insert(definition.plan.kind, Rc::clone(definition));
                        }
                        result.definitions.append(&mut section.definitions);
                        result.events.append(&mut section.events);
                        result.events.push(Event {
                            range: at..view.position(),
                            content: EventContent::Metadata(Metadata::Terminator {
                                closes,
                                section_start,
                            }),
                        });
                        let start = view.position();
                        match expect {
                            Expect::MapOrEnd if view.is_empty() => break,
                            Expect::FirstMap | Expect::MapOrEnd => {
                                if view.is_empty() {
                                    result.end = DeltasEnd::Stopped {
                                        offset: start,
                                        reason: StopReason::MissingSection,
                                    };
                                    break;
                                }
                                section = OpenSection::new(NodeState::Map, start);
                                match read_map(&mut view, header.maximum) {
                                    Ok(map) => section.push(Event {
                                        range: start..view.position(),
                                        content: EventContent::Metadata(map),
                                    }),
                                    Err(reason) => {
                                        result.end = DeltasEnd::Stopped {
                                            offset: start,
                                            reason,
                                        };
                                        break;
                                    }
                                }
                            }
                            Expect::PmarkOrFinal => {
                                if view.unread() == [0, 1, 0, 1] {
                                    result.events.push(Event {
                                        range: start..source.len(),
                                        content: EventContent::Metadata(Metadata::FinalTerminator),
                                    });
                                    break;
                                }
                                if view.is_empty() {
                                    result.end = DeltasEnd::Stopped {
                                        offset: start,
                                        reason: StopReason::MissingSection,
                                    };
                                    break;
                                }
                                if view.u16_be() != Some(4) {
                                    result.end = DeltasEnd::Stopped {
                                        offset: start,
                                        reason: StopReason::UnsupportedPacket,
                                    };
                                    break;
                                }
                                section = OpenSection::new(NodeState::PmarkHead, start);
                                let Some(state) = PmarkState::read(&mut view) else {
                                    result.end = DeltasEnd::Stopped {
                                        offset: start,
                                        reason: StopReason::InvalidPmark,
                                    };
                                    break;
                                };
                                section.push(Event {
                                    range: start..view.position(),
                                    content: EventContent::Metadata(Metadata::Pmark(state)),
                                });
                            }
                        }
                        continue;
                    }
                    Some(_) => Err(StopReason::UnsupportedPacket),
                },
                Some(4) if section.state == NodeState::PmarkHead => {
                    match PmarkState::read(&mut view) {
                        Some(state) => {
                            section.push(Event {
                                range: at..view.position(),
                                content: EventContent::Metadata(Metadata::Pmark(state)),
                            });
                            Ok(())
                        }
                        None => Err(StopReason::InvalidPmark),
                    }
                }
                Some(kind) if (kind == 2 || kind > 4) && kind <= header.maximum => {
                    section.read_node(&mut view, header.embedded, &established, kind, at)?
                }
                Some(_) => Err(StopReason::UnsupportedPacket),
            };
            if let Err(reason) = outcome {
                result.end = DeltasEnd::Stopped {
                    offset: section.start,
                    reason,
                };
                break;
            }
        }
        Ok(result)
    }

    fn establish(
        &mut self,
        established: &mut BTreeMap<u16, Rc<Definition>>,
        definition: Definition,
    ) {
        let definition = Rc::new(definition);
        established.insert(definition.plan.kind, Rc::clone(&definition));
        self.definitions.push(definition);
    }
}

#[cfg(test)]
mod tests;
