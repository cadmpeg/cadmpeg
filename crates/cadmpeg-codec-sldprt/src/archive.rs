// SPDX-License-Identifier: Apache-2.0
//! The MFC `CArchive` class map of a `Contents/Config-N` object stream.
//!
//! A class is declared once, as `ff ff`, a u16 schema, a u16 length and the
//! class name, and an object of an already declared class opens with the tag
//! `0x8000 | index`. Classes and objects share one map counter: a declaration
//! takes an index for the class and the next one for the object it opens, and
//! a class tag takes the next index for the object it opens. The stream's
//! `Config-N-ResolvedFeatures` stream opens with the count at which the map of
//! `Config-N` ended (`docs/formats/sldprt.md` § 2.2).
//!
//! A body carries no length, so the reader scans every byte pair for a tag of
//! a declared class and decides from the bytes after it whether the pair opens
//! an object or lies inside a body. The count it reaches agrees with the
//! `ResolvedFeatures` count when every such decision was right; a
//! configuration whose count disagrees has a class map that is wrong from its
//! first wrong decision on.
//!
//! The decisions:
//! - A tag opens an object when the word after it is not data-like, or when
//!   the class's declared body also opens with data. A class whose bodies
//!   open with data (a colour, a float) instead has an anchor: the first
//!   offset in its body at which its declaration and its first accepted
//!   objects all carry an archive event. A tag the first test rejects opens
//!   an object when an event lies at the class's anchor.
//! - A declaration whose six-byte body opening no other declaration shares is
//!   read against the tags that recur with that opening later in the stream.
//!   When the recurring tags name one index other than the counter's, the
//!   counter moves to it, and every class declared since the last confirmed
//!   declaration moves with it when its own recurrences favour the move.
//! - A tag naming an index the map does not hold, followed by the unique body
//!   opening of a declared class, names that class's index. The class and the
//!   classes declared after it move to it on the same evidence.
//! - The root class has one object, so a tag naming it never opens one.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::decode::{Coverage, CoverageKey};

use crate::container::ContainerScan;

/// Configuration object streams read with a `ResolvedFeatures` stream.
const WALKED_CONFIGURATION_COUNT: CoverageKey =
    CoverageKey::new("object_stream_class_map_configuration_count");
/// Configuration object streams whose map count equals the `ResolvedFeatures` count.
const RECONCILED_CONFIGURATION_COUNT: CoverageKey =
    CoverageKey::new("object_stream_class_map_reconciled_configuration_count");
/// Class declarations read in the configuration object streams.
const CLASS_DECLARATION_COUNT: CoverageKey =
    CoverageKey::new("object_stream_class_declaration_count");

/// Length of a class declaration's fixed header: `ff ff`, schema, name length.
const DECLARATION_HEADER_LEN: usize = 6;
/// Shortest class name a declaration carries.
const SHORTEST_CLASS_NAME: u8 = 3;
/// Longest class name a declaration carries.
const LONGEST_CLASS_NAME: u8 = 0x3f;
/// Body bytes that identify a class's objects.
const OPENING_LEN: usize = 6;
/// Highest index read for the name class from name tags (`k 80 ff fe ff`).
const HIGHEST_NAME_TAG_INDEX: u8 = 0x10;
/// The name class's index is this far above the map's first index.
const NAME_CLASS_ABOVE_FIRST_INDEX: i64 = 2;
/// First map index when no name tag is present.
const FALLBACK_FIRST_INDEX: i64 = 2;
/// The class whose declaration opens the first object name.
const NAME_CLASS: &str = "moNodeName_c";
/// Indices a declaration and the object it opens take.
const DECLARATION_INDICES: i64 = 2;
/// Highest first map index read from the name class's declaration; a higher
/// reading is not a name tag.
const HIGHEST_DECLARED_FIRST_INDEX: i64 = 0x7e;
/// A word at or above this value, without bit 15, is data inside a body.
const DATA_WORD_FLOOR: u16 = 0x1000;
/// Largest downward move of the counter at a declaration.
const LARGEST_MOVE_DOWN: i64 = 50;
/// Largest upward move of the counter at a declaration.
const LARGEST_MOVE_UP: i64 = 1500;
/// Recurrences the winning index needs before the counter moves.
const MIN_MOVE_SUPPORT: usize = 3;
/// The winning index's recurrences outnumber the runner-up's by this factor.
const MOVE_DOMINANCE: usize = 2;
/// Occurrences of a body opening read for one declaration.
const MAX_OPENING_OCCURRENCES: usize = 20_000;
/// First body offset an anchor can lie at.
const FIRST_ANCHOR_OFFSET: usize = 4;
/// Last body offset an anchor can lie at.
const LAST_ANCHOR_OFFSET: usize = 24;
/// Accepted objects of a class that refine its anchor.
const ANCHOR_SAMPLES: u8 = 5;
/// Occurrences of a tag with a class's opening that name the class's index.
const MIN_REFERENCE_SITE_OCCURRENCES: usize = 2;
/// Bytes after a handle class's tag in the cell form whose first word is `0x777f`.
const EXTENDED_HANDLE_CELL: [u8; 4] = [0x7f, 0x77, 0xfe, 0xff];
/// Classes whose body openings recur after tags of other indices; a
/// reference site does not move them.
const AMBIGUOUS_OPENING_CLASSES: [&str; 4] = [
    "moAtom_c",
    "moDirectionLight_c",
    "moFaceRef_c",
    "moSketchRegion_c",
];
/// Distinct 15-bit tag indices.
const TAG_INDICES: usize = 1 << 15;
/// Bytes of class-map tag index charged as one work unit.
const CLASS_MAP_INDEX_BYTES_PER_CHARGE: usize = 1024;
/// A Unicode `CString` marker.
const STRING_MARKER: [u8; 3] = [0xff, 0xfe, 0xff];

/// The class map one configuration object stream reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassMap {
    /// The map count after the stream's last object.
    end: i64,
    /// Class declarations read.
    declarations: usize,
    /// Whether each class lies at least two indices above the class declared
    /// before it.
    consistent: bool,
}

impl ClassMap {
    /// Whether the map count equals `resolved_count`, the leading u32 of the
    /// configuration's `ResolvedFeatures` stream, and the map is consistent.
    fn reconciles(&self, resolved_count: u32) -> bool {
        self.consistent && self.end == i64::from(resolved_count)
    }
}

/// Class-map coverage of every configuration object stream with a
/// `ResolvedFeatures` stream.
pub(crate) fn coverage(
    scan: &ContainerScan<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Coverage, CodecError> {
    let mut streams = HashMap::<String, &[u8]>::new();
    for section in scan.sections() {
        if let Some(name) = section.name() {
            streams.insert(
                name.replace('\\', "/").to_ascii_lowercase(),
                section.payload(),
            );
        }
    }
    let mut walked = 0_usize;
    let mut reconciled = 0_usize;
    let mut declarations = 0_usize;
    for (name, payload) in &streams {
        if !is_configuration_object_stream(name) {
            continue;
        }
        let Some(resolved_count) = streams
            .get(&format!("{name}-resolvedfeatures"))
            .and_then(|resolved| resolved_count(resolved))
        else {
            continue;
        };
        let map = walk(payload, Some(resolved_count), ctx)?;
        walked = walked.checked_add(1).ok_or_else(count_overflow)?;
        if map.reconciles(resolved_count) {
            reconciled = reconciled.checked_add(1).ok_or_else(count_overflow)?;
        }
        declarations = declarations
            .checked_add(map.declarations)
            .ok_or_else(count_overflow)?;
    }
    let mut coverage = Coverage::default();
    if walked > 0 {
        coverage.record(WALKED_CONFIGURATION_COUNT, walked);
        coverage.record(RECONCILED_CONFIGURATION_COUNT, reconciled);
        coverage.record(CLASS_DECLARATION_COUNT, declarations);
    }
    Ok(coverage)
}

/// The map count a `ResolvedFeatures` stream opens with. A stream whose first
/// declaration follows a u16, as in a compound-file part, carries none.
fn resolved_count(resolved: &[u8]) -> Option<u32> {
    if resolved.get(2..4) == Some(&[0xff, 0xff][..]) {
        return None;
    }
    View::u32_le_at(resolved, 0)
}

/// `contents/config-N` with a decimal `N`, in normalised lower case.
fn is_configuration_object_stream(name: &str) -> bool {
    name.strip_prefix("contents/config-")
        .is_some_and(|slot| !slot.is_empty() && slot.bytes().all(|byte| byte.is_ascii_digit()))
}

fn count_overflow() -> CodecError {
    CodecError::malformed("SLDPRT object-stream class map count overflows")
}

/// Walks the class map of one configuration object stream.
///
/// `resolved_count`, when known, bounds the indices a reference site can name.
pub(crate) fn walk(
    payload: &[u8],
    resolved_count: Option<u32>,
    ctx: &DecodeContext<'_>,
) -> Result<ClassMap, CodecError> {
    let walker = walked(payload, resolved_count.map(i64::from), ctx)?;
    Ok(ClassMap {
        end: walker.next,
        declarations: walker.declarations.len(),
        consistent: walker.consistent()?,
    })
}

/// The class-index map one configuration object stream ends with, when its
/// walk reconciles with `resolved_count` (the leading u32 of the stream's
/// `ResolvedFeatures`). That stream continues the map from this one, so its
/// class tags name these indices. `None` when the walk does not reconcile:
/// a map wrong from its first wrong decision on names wrong classes.
pub(crate) fn class_index_map(
    payload: &[u8],
    resolved_count: u32,
    ctx: &DecodeContext<'_>,
) -> Result<Option<std::collections::BTreeMap<u32, String>>, CodecError> {
    let walker = walked(payload, Some(i64::from(resolved_count)), ctx)?;
    if !walker.reaches(i64::from(resolved_count))? {
        return Ok(None);
    }
    Ok(Some(
        walker
            .known
            .iter()
            .filter_map(|(&index, &name)| Some((u32::try_from(index).ok()?, name.to_owned())))
            .collect(),
    ))
}

/// Walks `payload` from its first map index. When the name tags do not give
/// that index, the walk starts from the name class's declaration, and from
/// [`FALLBACK_FIRST_INDEX`] when that walk does not reach `bound`.
fn walked<'p, 'a, 'c>(
    payload: &'p [u8],
    bound: Option<i64>,
    ctx: &'c DecodeContext<'a>,
) -> Result<Walker<'p, 'a, 'c>, CodecError> {
    let (first, alternate) = match name_tag_first_index(payload) {
        Some(first) => (first, None),
        None => match declared_first_index(payload) {
            Some(first) => (first, Some(FALLBACK_FIRST_INDEX)),
            None => (FALLBACK_FIRST_INDEX, None),
        },
    };
    let mut walker = Walker::new(payload, bound, first, ctx)?;
    walker.run()?;
    if let (Some(alternate), Some(bound)) = (alternate, bound) {
        if !walker.reaches(bound)? {
            let mut other = Walker::new(payload, Some(bound), alternate, ctx)?;
            other.run()?;
            if other.reaches(bound)? {
                walker = other;
            }
        }
    }
    Ok(walker)
}

/// The map count a `ResolvedFeatures` stream opens with (see [`resolved_count`]).
pub(crate) fn resolved_features_count(resolved: &[u8]) -> Option<u32> {
    resolved_count(resolved)
}

/// One class declaration.
#[derive(Debug, Clone, Copy)]
struct Declaration<'a> {
    offset: usize,
    name: &'a str,
    body: usize,
}

/// What the word at an offset looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Word {
    /// End of the payload.
    End,
    /// `ff ff`.
    Declaration,
    /// `ff fe ff`.
    String,
    /// Bit 15 set: a class tag.
    Tag,
    /// Below `0x1000`: a count, flag or small integer.
    Small,
    /// Anything else: data inside a body.
    Data,
}

/// The walker's record of one class name.
#[derive(Debug, Clone)]
struct Class {
    index: i64,
    body: usize,
    opening: Word,
    /// Bit `s` set: an archive event lies at body offset `s`.
    anchors: u32,
    /// Whether the anchors were read at the class's first declaration.
    anchored: bool,
    samples: u8,
}

struct Walker<'p, 'a, 'c> {
    payload: &'p [u8],
    /// Charges the class-map work and its retained index.
    ctx: &'c DecodeContext<'a>,
    declarations: Vec<Declaration<'p>>,
    /// Occurrences of each class tag word, by 15-bit index, in stream order.
    tags: Vec<Vec<usize>>,
    /// Occurrences of each unique body opening, in stream order.
    openings: HashMap<&'p [u8], Vec<usize>>,
    /// How many declarations open with each body opening.
    opening_counts: HashMap<&'p [u8], usize>,
    /// Unique body openings that a reference site reads, by class.
    site_openings: HashMap<&'p [u8], &'p str>,
    bound: Option<i64>,
    next: i64,
    /// Map index to class name.
    known: HashMap<i64, &'p str>,
    classes: HashMap<&'p str, Class>,
    /// Class names in declaration order.
    order: Vec<&'p str>,
    /// Classes declared since the last confirmed declaration.
    since: Vec<&'p str>,
    /// For each index a class left, the offset at which it left.
    released: HashMap<i64, usize>,
}

impl<'p, 'a, 'c> Walker<'p, 'a, 'c> {
    fn new(
        payload: &'p [u8],
        bound: Option<i64>,
        first: i64,
        ctx: &'c DecodeContext<'a>,
    ) -> Result<Self, CodecError> {
        let declarations = declarations(payload);
        let mut opening_counts = HashMap::<&[u8], usize>::new();
        for declaration in &declarations {
            *opening_counts
                .entry(opening(payload, declaration.body))
                .or_default() += 1;
        }
        let unique = |bytes: &[u8]| opening_counts.get(bytes) == Some(&1) && distinct_bytes(bytes);
        let mut site_openings = HashMap::new();
        let mut openings = HashMap::<&[u8], Vec<usize>>::new();
        for declaration in &declarations {
            let bytes = opening(payload, declaration.body);
            if unique(bytes) {
                openings.entry(bytes).or_default();
                if !AMBIGUOUS_OPENING_CLASSES.contains(&declaration.name) {
                    site_openings.insert(bytes, declaration.name);
                }
            }
        }
        if !openings.is_empty() {
            for (at, window) in payload.windows(OPENING_LEN).enumerate() {
                if let Some(found) = openings.get_mut(window) {
                    found.push(at);
                }
            }
        }
        let index_bytes = TAG_INDICES
            .checked_mul(std::mem::size_of::<Vec<usize>>())
            .and_then(|bytes| bytes.checked_add(payload.len()))
            .ok_or_else(count_overflow)?;
        let index_charges = index_bytes / CLASS_MAP_INDEX_BYTES_PER_CHARGE
            + usize::from(index_bytes % CLASS_MAP_INDEX_BYTES_PER_CHARGE != 0);
        ctx.charge_entities(
            u64_from_index(index_charges),
            "walk SLDPRT object-stream class map",
        )?;
        let mut tags = (0..TAG_INDICES)
            .map(|_| Vec::new())
            .collect::<Vec<Vec<usize>>>();
        for (at, pair) in payload.windows(2).enumerate() {
            if pair[1] & 0x80 != 0 {
                tags[tag_key(pair[0], pair[1])].push(at);
            }
        }
        Ok(Self {
            payload,
            ctx,
            declarations,
            tags,
            openings,
            opening_counts,
            site_openings,
            bound,
            next: first,
            known: HashMap::new(),
            classes: HashMap::new(),
            order: Vec::new(),
            since: Vec::new(),
            released: HashMap::new(),
        })
    }

    /// Charges class-map work against the decode context.
    fn charge(&self, work: u64) -> Result<(), CodecError> {
        self.ctx
            .charge_entities(work, "walk SLDPRT object-stream class map")
    }

    fn run(&mut self) -> Result<(), CodecError> {
        let payload = self.payload;
        let mut declaration = 0_usize;
        let mut at = 0_usize;
        while at + 1 < payload.len() {
            // A declaration a tag stepped over is data inside that tag's body.
            while self
                .declarations
                .get(declaration)
                .is_some_and(|found| found.offset < at)
            {
                declaration += 1;
            }
            if let Some(found) = self
                .declarations
                .get(declaration)
                .copied()
                .filter(|found| found.offset == at)
            {
                declaration += 1;
                self.declare(found)?;
                at = found.body;
                continue;
            }
            let Some(word) = View::u16_le_at(payload, at) else {
                break;
            };
            if word & 0x8000 != 0 && word != u16::MAX {
                let index = i64::from(word & 0x7fff);
                let held = self
                    .known
                    .get(&index)
                    .copied()
                    .filter(|name| self.order.first() != Some(name));
                if let Some(name) = held {
                    if self.opens_object(at, name) {
                        self.next = add(self.next, 1)?;
                        at += 2;
                        continue;
                    }
                } else if at + 2 + OPENING_LEN <= payload.len() && self.reference_site(at, index)? {
                    self.next = add(self.next, 1)?;
                    at += 2;
                    continue;
                }
            }
            at += 1;
        }
        Ok(())
    }

    /// Records a declaration, moving the counter when the declaration's
    /// recurrences name another index.
    fn declare(&mut self, found: Declaration<'p>) -> Result<(), CodecError> {
        let bytes = opening(self.payload, found.body);
        let mut index = self.next;
        let mut confirmed = false;
        if self.opening_counts.get(bytes) == Some(&1) && distinct_bytes(bytes) {
            let readings = self.readings(found.body, index)?;
            if readings.get(&0).is_some_and(|count| *count > 0) {
                confirmed = true;
            } else if let Some(shift) = dominant_shift(&readings) {
                index = add(index, shift)?;
                confirmed = true;
                let since = std::mem::take(&mut self.since);
                self.move_classes(&since, shift, found.offset)?;
            }
        }
        self.known.insert(index, found.name);
        self.order.push(found.name);
        let opening_word = word_at(self.payload, found.body);
        let class = self.classes.entry(found.name).or_insert_with(|| Class {
            index,
            body: found.body,
            opening: opening_word,
            anchors: 0,
            anchored: false,
            samples: 0,
        });
        class.index = index;
        self.next = add(index, DECLARATION_INDICES)?;
        if confirmed {
            self.since.clear();
        } else {
            self.since.push(found.name);
        }
        if self
            .classes
            .get(found.name)
            .is_some_and(|class| !class.anchored)
        {
            let anchors = self.anchors(found.body);
            if let Some(class) = self.classes.get_mut(found.name) {
                class.anchors = anchors;
                class.anchored = true;
            }
        }
        Ok(())
    }

    /// Whether the tag of held class `name` at `at` opens an object.
    fn opens_object(&mut self, at: usize, name: &'p str) -> bool {
        let Some(class) = self.classes.get(name) else {
            return false;
        };
        let after = word_at(self.payload, at + 2);
        let mut opens = after != Word::Data || class.opening == Word::Data;
        if opens && class.samples < ANCHOR_SAMPLES {
            let anchors = self.anchors(at + 2);
            if let Some(class) = self.classes.get_mut(name) {
                class.anchors &= anchors;
                class.samples += 1;
            }
        }
        let anchors = self.classes.get(name).map_or(0, |class| class.anchors);
        if !opens && anchors != 0 {
            opens = self.event(at + 2 + first_anchor(anchors));
        }
        if !opens && name.ends_with("Handle") {
            opens = self.payload.get(at + 2..at + 6) == Some(&EXTENDED_HANDLE_CELL[..]);
        }
        opens
    }

    /// Whether the tag at `at`, naming `index`, which the map does not hold,
    /// is followed by a declared class's unique opening and so names that
    /// class's index. Moves the map when it is.
    fn reference_site(&mut self, at: usize, index: i64) -> Result<bool, CodecError> {
        self.charge(1)?;
        let payload = self.payload;
        let Some(&name) = payload
            .get(at + 2..at + 2 + OPENING_LEN)
            .and_then(|bytes| self.site_openings.get(bytes))
        else {
            return Ok(false);
        };
        let Some(held) = self.classes.get(name).map(|class| class.index) else {
            return Ok(false);
        };
        if self.bound.is_some_and(|bound| index >= bound) {
            return Ok(false);
        }
        let shift = sub(index, held)?;
        if shift == 0 || !(-LARGEST_MOVE_DOWN..=LARGEST_MOVE_UP).contains(&shift) {
            return Ok(false);
        }
        if self.site_occurrences(at)? < MIN_REFERENCE_SITE_OCCURRENCES {
            return Ok(false);
        }
        self.known.remove(&held);
        self.released.insert(held, at);
        self.known.insert(index, name);
        if let Some(class) = self.classes.get_mut(name) {
            class.index = index;
        }
        if let Some(position) = self.order.iter().position(|declared| *declared == name) {
            let later = self.order[position + 1..].to_vec();
            self.move_classes(&later, shift, at)?;
        }
        if self.since.contains(&name) {
            self.next = add(self.next, shift)?;
        }
        if self.order.first() != Some(&name) {
            self.count_passed(name, index, at)?;
        }
        Ok(true)
    }

    /// Counts the objects of `name` that the walk passed before `at` while
    /// the map did not hold `index` for it: tags of `index` after the class's
    /// declaration and after `index` left another class, outside declarations,
    /// that open an object.
    fn count_passed(&mut self, name: &'p str, index: i64, at: usize) -> Result<(), CodecError> {
        let body = self.classes.get(name).map_or(at, |class| class.body);
        let start = self
            .released
            .get(&index)
            .copied()
            .map_or(body, |released| released.max(body));
        let passed = usize::try_from(index)
            .ok()
            .and_then(|key| self.tags.get(key))
            .map(|sites| {
                let first = sites.partition_point(|&site| site <= start);
                let end = sites.partition_point(|&site| site < at);
                sites.get(first..end).unwrap_or_default().to_vec()
            })
            .unwrap_or_default();
        self.charge(u64_from_index(passed.len()))?;
        let mut free = start;
        for site in passed {
            if site < free || self.in_declaration(site) {
                continue;
            }
            if self.opens_object(site, name) {
                self.next = add(self.next, 1)?;
                free = site.checked_add(2).ok_or_else(count_overflow)?;
            }
        }
        Ok(())
    }

    /// Whether `at` lies in a class declaration's header or name.
    fn in_declaration(&self, at: usize) -> bool {
        let after = self
            .declarations
            .partition_point(|found| found.offset <= at);
        after
            .checked_sub(1)
            .and_then(|last| self.declarations.get(last))
            .is_some_and(|found| at < found.body)
    }

    /// Whether the walk reached `bound` with a consistent map.
    fn reaches(&self, bound: i64) -> Result<bool, CodecError> {
        Ok(self.next == bound && self.consistent()?)
    }

    /// Whether each class, in declaration order, lies at least
    /// [`DECLARATION_INDICES`] above the class declared before it.
    fn consistent(&self) -> Result<bool, CodecError> {
        let mut seen = HashSet::new();
        let mut floor: Option<i64> = None;
        for name in &self.order {
            if !seen.insert(*name) {
                continue;
            }
            let Some(index) = self.classes.get(name).map(|class| class.index) else {
                continue;
            };
            if floor.is_some_and(|floor| index < floor) {
                return Ok(false);
            }
            floor = Some(add(index, DECLARATION_INDICES)?);
        }
        Ok(true)
    }

    /// Moves each of `names` by `shift` when its own recurrences favour the move.
    fn move_classes(&mut self, names: &[&'p str], shift: i64, at: usize) -> Result<(), CodecError> {
        for name in names {
            self.charge(1)?;
            let Some((index, body)) = self
                .classes
                .get(name)
                .map(|class| (class.index, class.body))
            else {
                continue;
            };
            let moved = add(index, shift)?;
            if moved < 0 || self.known.contains_key(&moved) {
                continue;
            }
            let bytes = opening(self.payload, body);
            if self.recurrences(moved, body, bytes)? > self.recurrences(index, body, bytes)? {
                self.known.remove(&index);
                self.released.insert(index, at);
                self.known.insert(moved, name);
                if let Some(class) = self.classes.get_mut(name) {
                    class.index = moved;
                }
            }
        }
        Ok(())
    }

    /// For each shift, the tags `0x8000 | (index + shift)` after `body` that
    /// are followed by the opening at `body`.
    fn readings(&self, body: usize, index: i64) -> Result<HashMap<i64, usize>, CodecError> {
        let payload = self.payload;
        let bytes = opening(payload, body);
        let mut readings = HashMap::<i64, usize>::new();
        let Some(occurrences) = self.openings.get(bytes) else {
            return Ok(readings);
        };
        let first = occurrences.partition_point(|&at| at < body);
        let considered = occurrences[first..].len().min(MAX_OPENING_OCCURRENCES);
        self.charge(u64_from_index(considered))?;
        for &at in occurrences[first..].iter().take(MAX_OPENING_OCCURRENCES) {
            let Some(tag_at) = at.checked_sub(2).filter(|&tag_at| tag_at > body) else {
                continue;
            };
            let Some(word) = View::u16_le_at(payload, tag_at) else {
                continue;
            };
            if word & 0x8000 == 0 || word == u16::MAX {
                continue;
            }
            let shift = sub(i64::from(word & 0x7fff), index)?;
            let named = add(index, shift)?;
            if (-LARGEST_MOVE_DOWN..=LARGEST_MOVE_UP).contains(&shift)
                && named >= 0
                && (shift == 0 || !self.known.contains_key(&named))
            {
                *readings.entry(shift).or_default() += 1;
            }
        }
        Ok(readings)
    }

    /// Tags `0x8000 | index` after `body` followed by `bytes`.
    fn recurrences(&self, index: i64, body: usize, bytes: &[u8]) -> Result<usize, CodecError> {
        let Some(sites) = usize::try_from(index)
            .ok()
            .and_then(|key| self.tags.get(key))
        else {
            return Ok(0);
        };
        let first = sites.partition_point(|&at| at <= body);
        self.charge(u64_from_index(sites.len() - first))?;
        Ok(sites[first..]
            .iter()
            .filter(|&&at| clipped(self.payload, at + 2, OPENING_LEN) == bytes)
            .count())
    }

    /// Non-overlapping occurrences, from `at` on, of the tag and opening at `at`.
    fn site_occurrences(&self, at: usize) -> Result<usize, CodecError> {
        let payload = self.payload;
        let pattern = clipped(payload, at, 2 + OPENING_LEN);
        let key = tag_key(payload[at], payload[at + 1]);
        let mut count = 0;
        let mut scanned = 0_usize;
        let mut free = at;
        for &site in &self.tags[key] {
            scanned = scanned.checked_add(1).ok_or_else(count_overflow)?;
            if site < free || payload.get(site..site + pattern.len()) != Some(pattern) {
                continue;
            }
            count += 1;
            if count >= MIN_REFERENCE_SITE_OCCURRENCES {
                break;
            }
            free = site + pattern.len();
        }
        self.charge(u64_from_index(scanned))?;
        Ok(count)
    }

    /// Body offsets 4..=24 after `body` at which an archive event lies.
    fn anchors(&self, body: usize) -> u32 {
        (FIRST_ANCHOR_OFFSET..=LAST_ANCHOR_OFFSET)
            .filter(|&offset| self.event(body + offset))
            .fold(0, |mask, offset| mask | 1 << offset)
    }

    /// Whether a `CString`, a declaration or a held class's tag lies at `at`.
    fn event(&self, at: usize) -> bool {
        let payload = self.payload;
        if at + 3 > payload.len() {
            return false;
        }
        if payload[at..at + 3] == STRING_MARKER
            || self
                .declarations
                .binary_search_by_key(&at, |declaration| declaration.offset)
                .is_ok()
        {
            return true;
        }
        View::u16_le_at(payload, at).is_some_and(|word| {
            word & 0x8000 != 0
                && word != u16::MAX
                && self.known.contains_key(&i64::from(word & 0x7fff))
        })
    }
}

/// The smallest body offset in an anchor mask.
fn first_anchor(anchors: u32) -> usize {
    // A mask of 32 bits has at most 32 trailing zeros, which `usize` holds.
    usize::try_from(anchors.trailing_zeros()).unwrap_or(LAST_ANCHOR_OFFSET + 1)
}

/// The shift that names the counter's index when it wins clearly.
fn dominant_shift(readings: &HashMap<i64, usize>) -> Option<i64> {
    let mut counts = readings
        .iter()
        .map(|(&shift, &count)| (count, shift))
        .collect::<Vec<_>>();
    counts.sort_unstable_by(|left, right| right.cmp(left));
    let &(best, shift) = counts.first()?;
    let runner_up = counts.get(1).map_or(0, |&(count, _)| count);
    (best >= MIN_MOVE_SUPPORT && best >= runner_up.checked_mul(MOVE_DOMINANCE)?).then_some(shift)
}

/// The map's first index from the name class's declaration: the index of
/// the first name tag after the declaration's own name, less two for each
/// declaration before it.
fn declared_first_index(payload: &[u8]) -> Option<i64> {
    let declarations = declarations(payload);
    let position = declarations
        .iter()
        .position(|declaration| declaration.name == NAME_CLASS)?;
    let mut at = declarations.get(position)?.body;
    if payload.get(at..at.checked_add(STRING_MARKER.len())?)? != STRING_MARKER {
        return None;
    }
    at = at.checked_add(STRING_MARKER.len())?;
    let mut units = usize::from(*payload.get(at)?);
    at = at.checked_add(1)?;
    if units == usize::from(u8::MAX) {
        units = usize::from(View::u16_le_at(payload, at)?);
        at = at.checked_add(2)?;
    }
    at = at.checked_add(units.checked_mul(2)?)?;
    let before = i64::try_from(position)
        .ok()?
        .checked_mul(DECLARATION_INDICES)?;
    while at.checked_add(2 + STRING_MARKER.len())? <= payload.len() {
        let word = View::u16_le_at(payload, at)?;
        if word & 0x8000 != 0
            && word != u16::MAX
            && payload.get(at + 2..at + 2 + STRING_MARKER.len()) == Some(&STRING_MARKER[..])
        {
            let first = i64::from(word & 0x7fff).checked_sub(before)?;
            return (0..=HIGHEST_DECLARED_FIRST_INDEX)
                .contains(&first)
                .then_some(first);
        }
        at += 1;
    }
    None
}

/// The map's first index read from name tags with an index of at most
/// [`HIGHEST_NAME_TAG_INDEX`], or `None` when there are none.
fn name_tag_first_index(payload: &[u8]) -> Option<i64> {
    let mut counts = Vec::<(u8, usize)>::new();
    let mut at = 0;
    while at + 5 <= payload.len() {
        let index = payload[at];
        if (1..=HIGHEST_NAME_TAG_INDEX).contains(&index)
            && payload[at + 1] == 0x80
            && payload[at + 2..at + 5] == STRING_MARKER
        {
            match counts.iter_mut().find(|(found, _)| *found == index) {
                Some((_, count)) => *count += 1,
                None => counts.push((index, 1)),
            }
            at += 5;
        } else {
            at += 1;
        }
    }
    // The first of the most frequent indices, in order of first occurrence.
    let mut best: Option<(u8, usize)> = None;
    for &(index, count) in &counts {
        if best.is_none_or(|(_, most)| count > most) {
            best = Some((index, count));
        }
    }
    best.map(|(index, _)| i64::from(index) - NAME_CLASS_ABOVE_FIRST_INDEX)
}

/// Every class declaration, in stream order.
fn declarations(payload: &[u8]) -> Vec<Declaration<'_>> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + DECLARATION_HEADER_LEN <= payload.len() {
        let header = &payload[at..at + DECLARATION_HEADER_LEN];
        let length = header[4];
        if header[..2] != [0xff, 0xff]
            || header[3] > 1
            || !(1..=LONGEST_CLASS_NAME).contains(&length)
            || header[5] != 0
        {
            at += 1;
            continue;
        }
        let start = at + DECLARATION_HEADER_LEN;
        let body = start + usize::from(length);
        let name = clipped(payload, start, usize::from(length));
        if let Some(name) = class_name(name) {
            found.push(Declaration {
                offset: at,
                name,
                body,
            });
        }
        at = start;
    }
    found
}

/// A run-time class name: a letter or underscore, then letters, digits or
/// underscores.
fn class_name(bytes: &[u8]) -> Option<&str> {
    let (&first, rest) = bytes.split_first()?;
    let valid = bytes.len() >= usize::from(SHORTEST_CLASS_NAME)
        && (first.is_ascii_alphabetic() || first == b'_')
        && rest
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
    valid.then(|| std::str::from_utf8(bytes).ok()).flatten()
}

fn word_at(payload: &[u8], at: usize) -> Word {
    let Some(word) = View::u16_le_at(payload, at) else {
        return Word::End;
    };
    if word == u16::MAX {
        Word::Declaration
    } else if payload.get(at..at + 3) == Some(&STRING_MARKER[..]) {
        Word::String
    } else if word & 0x8000 != 0 {
        Word::Tag
    } else if word < DATA_WORD_FLOOR {
        Word::Small
    } else {
        Word::Data
    }
}

/// The body opening at `body`, shorter at the end of the payload.
fn opening(payload: &[u8], body: usize) -> &[u8] {
    clipped(payload, body, OPENING_LEN)
}

/// Up to `len` bytes from `at`.
fn clipped(payload: &[u8], at: usize, len: usize) -> &[u8] {
    let start = at.min(payload.len());
    let end = at
        .checked_add(len)
        .map_or(payload.len(), |end| end.min(payload.len()));
    &payload[start..end]
}

/// Whether an opening has at least two distinct bytes.
fn distinct_bytes(bytes: &[u8]) -> bool {
    bytes
        .first()
        .is_some_and(|first| bytes.iter().any(|byte| byte != first))
}

/// The 15-bit index of the tag word `low, high`.
fn tag_key(low: u8, high: u8) -> usize {
    usize::from(high & 0x7f) << 8 | usize::from(low)
}

fn add(left: i64, right: i64) -> Result<i64, CodecError> {
    left.checked_add(right).ok_or_else(count_overflow)
}

fn sub(left: i64, right: i64) -> Result<i64, CodecError> {
    left.checked_sub(right).ok_or_else(count_overflow)
}

#[cfg(test)]
mod tests;
