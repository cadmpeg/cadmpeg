// SPDX-License-Identifier: Apache-2.0
//! The rollback position of a part's feature tree (`docs/formats/sldprt.md`
//! §3.4): the current mark of the active configuration's partition against
//! the mark each feature's `moPMarkRecord_c` names.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_parasolid::deltas::pmark_state::PmarkState;
use cadmpeg_parasolid::records::deltas::{EventContent, Metadata};
use cadmpeg_parasolid::records::DeltasRecords;

use crate::carchive::reader::MarkOwner;
use crate::container::{contains_ascii_case_insensitive, ContainerScan};
use crate::layout::pmark_record_head as head;
use crate::loss::SldprtLossCode;
use crate::records::FeatureInputLane;

#[cfg(test)]
mod tests;

const MARK_CLASS: &str = "moPMarkRecord_c";
const PARTITION_SUFFIX: &str = "-partition";
const RESOLVED_FEATURES_SUFFIX: &str = "-ResolvedFeatures";
/// The form word of the id-range form of `moPMarkRecord_c`.
const RANGE_FORM_WORD: u32 = 1;
/// The map count u32 and the u16 that open a `ResolvedFeatures` stream.
const RESOLVED_FEATURES_HEAD: usize = 6;
/// The `CArchive` word that a u32 class token follows.
const LONG_TAG: u16 = 0x7fff;
/// The `CArchive` word that opens a class declaration.
const DECLARATION_TAG: u16 = 0xffff;

/// The archive walk's `moPMarkRecord_c` holders of each configuration, keyed
/// by its lower-case `Contents/Config-<N>` name.
pub(crate) type MarkOwners = HashMap<String, Vec<MarkOwner>>;

/// What the stored marks state of the rollback position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reading {
    /// The current mark is the mark of the last feature record, which the
    /// feature named `last` holds. `recorded` names every feature that holds
    /// a record.
    End {
        last: String,
        recorded: BTreeSet<String>,
    },
    /// The current mark is the mark of the feature named `after`; `later`
    /// names the features of the records after it, in stream order.
    /// `recorded` names every feature that holds a record.
    RolledBack {
        after: String,
        later: Vec<String>,
        recorded: BTreeSet<String>,
    },
    /// The stored marks do not fix the position.
    Unread(Unread),
}

/// Why the stored marks do not fix the rollback position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Unread {
    /// No partition site of the active configuration.
    NoPartitionSite,
    /// The partition section holds no deltas transmit.
    NoDeltasTransmit,
    /// The deltas transmit does not frame its current mark.
    NoCurrentMark,
    /// The configuration's object or feature stream is absent.
    NoFeatureStreams,
    /// The configuration's class map does not reconcile with its feature
    /// stream, and no single class token opens the records of its marks.
    ClassMapUnreconciled,
    /// The class map declares no `moPMarkRecord_c`, and no single class token
    /// opens the records of the marks.
    NoMarkClass,
    /// The feature stream holds no `moPMarkRecord_c` record.
    NoMarkRecord,
    /// The current mark follows the last record's mark on the main line.
    AfterLastFeature { current: u32, last: u32 },
    /// The current mark is the mark of no record.
    NoFeatureMark { current: u32 },
    /// A record after the one at the current mark names a main-line mark.
    LaterMarkOnMainLine { current: u32 },
    /// The archive walk names no feature around a record from the current mark on.
    HolderUnresolved { current: u32 },
    /// A record's feature names no single timeline feature.
    FeatureUnresolved { name: String },
    /// A later record's feature precedes the rolled-back feature in the timeline.
    FeatureOutOfOrder { name: String },
    /// A feature between the feature at the current mark and the first
    /// unbuilt feature holds no record, so its mark does not place the bar
    /// before or after it.
    UnrecordedFeature { name: String },
}

impl std::fmt::Display for Unread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoPartitionSite => f.write_str("no partition site of the active configuration"),
            Self::NoDeltasTransmit => f.write_str("the partition section holds no deltas transmit"),
            Self::NoCurrentMark => f.write_str("the deltas transmit frames no current mark"),
            Self::NoFeatureStreams => {
                f.write_str("the configuration has no object stream or feature stream")
            }
            Self::ClassMapUnreconciled => f.write_str(
                "the configuration's class map does not reconcile with its feature stream, and no \
                 single class token opens records of its marks",
            ),
            Self::NoMarkClass => write!(
                f,
                "the class map declares no {MARK_CLASS}, and no single class token opens records of \
                 its marks"
            ),
            Self::NoMarkRecord => write!(f, "the feature stream holds no {MARK_CLASS} record"),
            Self::AfterLastFeature { current, last } => write!(
                f,
                "current mark {current} follows the last feature mark {last} on the main line"
            ),
            Self::NoFeatureMark { current } => {
                write!(f, "current mark {current} is the mark of no feature")
            }
            Self::LaterMarkOnMainLine { current } => write!(
                f,
                "a feature after the one at current mark {current} names a main-line mark"
            ),
            Self::HolderUnresolved { current } => write!(
                f,
                "the archive walk names no feature for a mark record from current mark {current} on"
            ),
            Self::FeatureUnresolved { name } => {
                write!(f, "feature {name} names no single timeline feature")
            }
            Self::FeatureOutOfOrder { name } => write!(
                f,
                "feature {name} precedes the rolled-back feature in the timeline"
            ),
            Self::UnrecordedFeature { name } => write!(
                f,
                "feature {name} after the feature at the current mark holds no mark record"
            ),
        }
    }
}

/// A `moPMarkRecord_c` found by its class tag: where its body starts and the
/// mark it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarkRecord {
    pub(crate) at: usize,
    pub(crate) mark: u32,
    pub(crate) valid: bool,
}

/// Where the class index of `moPMarkRecord_c` comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkClass {
    /// The configuration's class map names the class at this index.
    Mapped(u32),
    /// The class token whose records name the most main-line marks.
    Scanned(u32),
}

/// The current mark and the main line back from it, current first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkChain {
    pub(crate) current: u32,
    pub(crate) main: Vec<u32>,
}

/// The `moPMarkRecord_c` records of each partition site that stores a
/// deltas transmit, by its partition site key, or why they are
/// not read.
pub(crate) type StoredMarks<'a> = BTreeMap<String, StoredSite<'a>>;

pub(crate) struct StoredSite<'a> {
    pub(crate) frame: DeltasRecords<'a>,
    pub(crate) chain: Option<MarkChain>,
    pub(crate) marks: Result<SiteRecords, Unread>,
}

/// The `moPMarkRecord_c` records of one configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SiteRecords {
    /// The configuration's `Contents/Config-<N>` stream name.
    pub(crate) configuration: String,
    /// The records, in stream order.
    pub(crate) records: Vec<MarkRecord>,
    /// The class index comes from the token scan.
    pub(crate) inferred: bool,
    /// The id of the lane of the configuration's `ResolvedFeatures` stream.
    pub(crate) lane: Option<String>,
}

/// The mark records of every partition site of `scan` that stores a deltas
/// transmit. The decode moves the configuration streams into unknown
/// records after it reads them, so the records are read here, with the
/// lanes of the configurations' `ResolvedFeatures` streams.
pub(crate) fn stored_marks<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    lanes: &[FeatureInputLane],
    active_only: bool,
) -> Result<StoredMarks<'a>, CodecError> {
    let active = crate::container::select_active_parasolid_site(scan).map(|site| site.site_key());
    let mut stored = BTreeMap::new();
    for section in scan.sections() {
        let key = section.site_key();
        let name = section.name();
        let selected = active.as_ref() == Some(&key);
        if !selected && (active_only || !name.is_some_and(crate::container::is_partition_section)) {
            continue;
        }
        let Some(deltas) = section
            .ps_streams()
            .iter()
            .find(|stream| contains_ascii_case_insensitive(&stream.header.description, "deltas"))
        else {
            continue;
        };
        let frame = match DeltasRecords::read(&deltas.payload) {
            Ok(frame) => frame,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => continue,
        };
        let chain = mark_chain(&frame);
        let marks = match (&chain, name) {
            (Some(chain), Some(name)) => {
                site_marks(ctx, scan, name, chain)?.map(|marks| SiteRecords {
                    lane: lanes
                        .iter()
                        .find(|lane| lane.native_payload.as_slice() == marks.resolved)
                        .map(|lane| lane.id.clone()),
                    configuration: marks.configuration,
                    records: marks.records,
                    inferred: marks.inferred,
                })
            }
            (None, _) => Err(Unread::NoCurrentMark),
            (_, None) => Err(Unread::NoFeatureStreams),
        };
        stored.insert(
            key,
            StoredSite {
                frame,
                chain,
                marks,
            },
        );
    }
    Ok(stored)
}

/// Read the rollback position of the active configuration's feature tree
/// from the mark records `stored` holds. A class index taken from the token
/// scan adds its note to `losses`.
pub(crate) fn read(
    scan: &ContainerScan<'_>,
    owners: &MarkOwners,
    stored: &StoredMarks<'_>,
    losses: &mut Vec<LossNote>,
) -> Reading {
    let Some(site) = crate::container::select_active_parasolid_site(scan) else {
        return Reading::Unread(Unread::NoPartitionSite);
    };
    let Some(site) = stored.get(&site.site_key()) else {
        let reason = if site
            .section
            .ps_streams()
            .iter()
            .any(|stream| contains_ascii_case_insensitive(&stream.header.description, "deltas"))
        {
            Unread::NoCurrentMark
        } else {
            Unread::NoDeltasTransmit
        };
        return Reading::Unread(reason);
    };
    let marks = match &site.marks {
        Ok(marks) => marks,
        Err(unread) => return Reading::Unread(unread.clone()),
    };
    let Some(chain) = &site.chain else {
        return Reading::Unread(Unread::NoCurrentMark);
    };
    if marks.inferred {
        losses.push(mark_class_inferred(&marks.configuration));
    }
    let owners = owners
        .get(&marks.configuration.to_ascii_lowercase())
        .map(Vec::as_slice)
        .unwrap_or_default();
    join(chain, &marks.records, owners)
}

/// The `moPMarkRecord_c` records of one configuration, in its stream.
struct SiteMarks<'s> {
    /// The configuration's `Contents/Config-<N>` stream name.
    configuration: String,
    /// The configuration's `ResolvedFeatures` stream.
    resolved: &'s [u8],
    /// The records, in stream order.
    records: Vec<MarkRecord>,
    /// The class index comes from the token scan.
    inferred: bool,
}

/// The `moPMarkRecord_c` records of the configuration whose partition
/// section is named `partition`, and whose deltas transmit has the main line
/// `chain`.
fn site_marks<'s>(
    ctx: &DecodeContext<'_>,
    scan: &'s ContainerScan<'_>,
    partition: &str,
    chain: &MarkChain,
) -> Result<Result<SiteMarks<'s>, Unread>, CodecError> {
    let Some(configuration) = configuration_stream(partition) else {
        return Ok(Err(Unread::NoFeatureStreams));
    };
    let section = |name: &str| {
        scan.sections()
            .find(|section| {
                section
                    .name()
                    .is_some_and(|other| other.replace('\\', "/").eq_ignore_ascii_case(name))
            })
            .map(crate::container::Section::payload)
    };
    let (Some(config), Some(resolved)) = (
        section(&configuration),
        section(&format!("{configuration}{RESOLVED_FEATURES_SUFFIX}")),
    ) else {
        return Ok(Err(Unread::NoFeatureStreams));
    };
    let count = crate::archive::resolved_count(resolved);
    let map = match count {
        Some(count) => crate::archive::class_index_map(config, count, ctx)?,
        None => None,
    };
    let (index, inferred) = match mark_class_index(map.as_ref(), count, resolved, chain) {
        Ok(MarkClass::Mapped(index)) => (index, false),
        Ok(MarkClass::Scanned(index)) => (index, true),
        Err(unread) => return Ok(Err(unread)),
    };
    Ok(Ok(SiteMarks {
        records: mark_records(resolved, index),
        configuration,
        resolved,
        inferred,
    }))
}

/// The loss of a `moPMarkRecord_c` class index that the token scan gives.
pub(crate) fn mark_class_inferred(configuration: &str) -> LossNote {
    SldprtLossCode::HistoryRollbackMarkClassInferred.note(format!(
        "The {MARK_CLASS} class of {configuration} is the class token whose records name the \
         most main-line marks; the configuration's class map does not state it."
    ))
}

/// The `Contents/Config-<N>` stream name of a `Contents/Config-<N>-Partition`
/// section.
fn configuration_stream(partition: &str) -> Option<String> {
    let normalized = partition.replace('\\', "/");
    let stem = normalized.len().checked_sub(PARTITION_SUFFIX.len())?;
    (normalized.is_char_boundary(stem) && normalized[stem..].eq_ignore_ascii_case(PARTITION_SUFFIX))
        .then(|| normalized[..stem].to_owned())
}

/// The current mark of a deltas transmit and the main line back from it.
fn mark_chain(records: &DeltasRecords<'_>) -> Option<MarkChain> {
    let mut current = None;
    let mut marks = BTreeMap::<u32, PmarkState>::new();
    for event in records.events() {
        match event.content() {
            EventContent::PartitionPrefix(prefix) => current = Some(prefix.current_pmark()),
            EventContent::Metadata(Metadata::Pmark(mark)) => {
                marks.insert(mark.xmt(), *mark);
            }
            _ => {}
        }
    }
    let current = current?;
    let mut main = vec![current.identifier()];
    let mut at = current;
    let mut visited = HashSet::from([current.xmt()]);
    while let Some(preceding) = at
        .preceding()
        .filter(|preceding| visited.insert(*preceding))
    {
        let Some(mark) = marks.get(&preceding) else {
            break;
        };
        main.push(mark.identifier());
        at = *mark;
    }
    Some(MarkChain {
        current: current.identifier(),
        main,
    })
}

/// The class index of `moPMarkRecord_c` in a configuration's feature stream:
/// the class map's entry where the map reconciles and names the class, else
/// the token scan's index. A reconciled map bounds the token scan as an
/// unreconciled one does: a token at or past the count names no class of the
/// configuration.
fn mark_class_index(
    map: Option<&BTreeMap<u32, String>>,
    count: Option<u32>,
    resolved: &[u8],
    chain: &MarkChain,
) -> Result<MarkClass, Unread> {
    let mapped = map.and_then(|map| {
        map.iter()
            .find_map(|(index, name)| (name == MARK_CLASS).then_some(*index))
    });
    if let Some(index) = mapped {
        return Ok(MarkClass::Mapped(index));
    }
    token_naming_marks(resolved, count, chain)
        .map(MarkClass::Scanned)
        .ok_or(if map.is_some() {
            Unread::NoMarkClass
        } else {
            Unread::ClassMapUnreconciled
        })
}

/// The class index of `moPMarkRecord_c` where the class map does not name it:
/// among the indices below the stream's map `count`, where one bounds them,
/// and whose record heads mostly name main-line marks after the initial one,
/// the one whose record heads name the most distinct such marks. `None` where
/// no index or several do.
fn token_naming_marks(stream: &[u8], count: Option<u32>, chain: &MarkChain) -> Option<u32> {
    let marks = match chain.main.split_last() {
        Some((_, later)) if !later.is_empty() => later,
        _ => chain.main.as_slice(),
    };
    let marks = marks.iter().copied().collect::<HashSet<_>>();
    let mut named = BTreeMap::<u32, Named>::new();
    for at in RESOLVED_FEATURES_HEAD..stream.len() {
        let Some(word) = View::u16_le_at(stream, at) else {
            break;
        };
        let (index, body) = match word {
            LONG_TAG => match View::u32_le_at(stream, at + 2) {
                Some(long) if long & 0x8000_0000 != 0 => (long & 0x7fff_ffff, at + 6),
                _ => continue,
            },
            DECLARATION_TAG => continue,
            word if word & 0x8000 != 0 => (u32::from(word & 0x7fff), at + 2),
            _ => continue,
        };
        if index == 0 || count.is_some_and(|count| index >= count) {
            continue;
        }
        if let Some(mark) = record_mark(stream, body) {
            let named = named.entry(index).or_default();
            named.heads += 1;
            if marks.contains(&mark) {
                named.heads_on_main += 1;
                named.marks.insert(mark);
            }
        }
    }
    named.retain(|_, named| 2 * named.heads_on_main > named.heads);
    let most = named.values().map(|named| named.marks.len()).max()?;
    let mut leaders = named
        .iter()
        .filter(|(_, named)| named.marks.len() == most)
        .map(|(index, _)| *index);
    match (leaders.next(), leaders.next()) {
        (Some(index), None) => Some(index),
        _ => None,
    }
}

/// The record heads one class token opens.
#[derive(Default)]
struct Named {
    heads: usize,
    heads_on_main: usize,
    marks: BTreeSet<u32>,
}

/// The mark of the record head at `body`, where the bytes there have its form.
fn record_mark(stream: &[u8], body: usize) -> Option<u32> {
    let record = stream.get(body..body.checked_add(head::LEN)?)?;
    let mark = View::u32_le_at(record, head::MARK)?;
    let form = View::u32_le_at(record, head::FORM)?;
    let form_word = View::u32_le_at(record, head::FORM_WORD)?;
    (form <= 1 || form_word == RANGE_FORM_WORD).then_some(mark)
}

/// Every `moPMarkRecord_c` of a `ResolvedFeatures` stream whose class has map
/// index `index`, in stream order.
fn mark_records(stream: &[u8], index: u32) -> Vec<MarkRecord> {
    let tag = match u16::try_from(index) {
        Ok(short) if short < LONG_TAG => (0x8000 | short).to_le_bytes().to_vec(),
        _ => {
            let mut tag = LONG_TAG.to_le_bytes().to_vec();
            tag.extend_from_slice(&(0x8000_0000 | index).to_le_bytes());
            tag
        }
    };
    let Some(body) = stream.get(RESOLVED_FEATURES_HEAD..) else {
        return Vec::new();
    };
    let finder = memchr::memmem::Finder::new(tag.as_slice());
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = finder.find(&body[cursor..]) {
        let offset = cursor + rel;
        cursor = offset + 1;
        let at = RESOLVED_FEATURES_HEAD + offset + tag.len();
        if let Some(mark) = at
            .checked_add(head::MARK)
            .and_then(|start| View::u32_le_at(stream, start))
        {
            out.push(MarkRecord {
                at,
                mark,
                valid: record_mark(stream, at).is_some(),
            });
        }
    }
    out
}

/// Join the current mark to the feature records, whose holders the archive
/// walk gives in `owners`.
fn join(chain: &MarkChain, records: &[MarkRecord], owners: &[MarkOwner]) -> Reading {
    let current = chain.current;
    let records = records
        .iter()
        .filter(|record| record.valid)
        .collect::<Vec<_>>();
    let Some(&last) = records.last() else {
        return Reading::Unread(Unread::NoMarkRecord);
    };
    let owners = owners
        .iter()
        .map(|owner| (owner.at, &owner.owner))
        .collect::<HashMap<_, _>>();
    let main = chain.main.iter().copied().collect::<HashSet<_>>();
    let holder = |record: &&MarkRecord| owners.get(&record.at).and_then(|owner| (*owner).clone());
    let recorded = records.iter().filter_map(holder).collect::<BTreeSet<_>>();
    if last.mark == current {
        return match holder(&last) {
            Some(last) => Reading::End { last, recorded },
            None => Reading::Unread(Unread::HolderUnresolved { current }),
        };
    }
    let Some(index) = records.iter().rposition(|record| record.mark == current) else {
        return Reading::Unread(if main.contains(&last.mark) {
            Unread::AfterLastFeature {
                current,
                last: last.mark,
            }
        } else {
            Unread::NoFeatureMark { current }
        });
    };
    let later = &records[index + 1..];
    if later.iter().any(|record| main.contains(&record.mark)) {
        return Reading::Unread(Unread::LaterMarkOnMainLine { current });
    }
    let (Some(after), Some(later)) = (
        holder(&records[index]),
        later.iter().map(holder).collect::<Option<Vec<_>>>(),
    ) else {
        return Reading::Unread(Unread::HolderUnresolved { current });
    };
    Reading::RolledBack {
        after,
        later,
        recorded,
    }
}
