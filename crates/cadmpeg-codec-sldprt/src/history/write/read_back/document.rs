// SPDX-License-Identifier: Apache-2.0
//! Two decodes of one part compared whole: every record of every arena of the
//! geometry, model, design and process homes, the units, the tolerances and
//! the source attributes.
//!
//! A write re-lays the file, and the decoder's ids embed block and lane
//! offsets and history indices, so most ids of a written part differ from the
//! ids of the same records in the document it was written from. The records
//! are therefore paired by content: a record pairs with the one record of the
//! same arena of the other document whose wire form is the same once each id
//! in it is replaced by its pair, and each id not yet paired by its digit-free
//! shape. Pairing repeats until nothing more pairs. Paired records are then
//! compared leaf by leaf with the IR's comparator,
//! [`cadmpeg_ir::compare::values_agree`]: a string compares exactly once each
//! id in it is replaced by its pair, and an unpaired native reference by its
//! form with its locator digits replaced.
//!
//! The native namespaces are left out: they are the encoding, and every
//! written byte moves their payloads and offsets.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::{self, Write as _};
use std::hash::Hasher;

use cadmpeg_ir::document::CadIr;
use serde_json::Value;

/// The homes whose arenas are compared.
const HOMES: [&str; 4] = ["geometry", "model", "design", "process"];

/// The arena of the document-level records: units, tolerances and source
/// attributes.
const DOCUMENT: &str = "document";

/// The suffix of a source attribute that digests encoded bytes; the records
/// the bytes encode are compared instead.
const DIGEST_SUFFIX: &str = "_sha256";

/// The suffix of a configuration's stale mark (`swConfigurationNeedsUpdate`),
/// which the writer sets on a state it writes without regenerating it.
const NEEDS_UPDATE_SUFFIX: &str = "_needs_update";

/// What an unpaired id states in place of each run of digits.
const UNPAIRED_DIGITS: &str = "<n>";

/// The native reference forms without a `#` locator whose digits are file
/// offsets.
const OFFSET_FORMS: [&str; 1] = ["feature-input:single-face-ref:"];

/// One document's records in wire form, by arena.
#[derive(Default, Clone)]
pub(crate) struct Document {
    arenas: BTreeMap<String, Vec<Record>>,
}

/// One record in wire form, with its identity when it states one.
#[derive(Clone)]
struct Record {
    id: Option<String>,
    value: Value,
}

impl Record {
    fn new(value: Value) -> Self {
        let id = [
            value.get("id"),
            value.get("value").and_then(|inner| inner.get("id")),
            value.get("definition"),
        ]
        .into_iter()
        .flatten()
        .find_map(Value::as_str)
        .map(str::to_owned);
        Self { id, value }
    }

    fn name(&self) -> Option<&str> {
        self.value.get("name").and_then(Value::as_str)
    }

    /// The id of the record that owns this one: `owner`, or the value of a
    /// tagged `owner`.
    fn owner(&self) -> Option<&str> {
        let owner = self.value.get("owner")?;
        owner
            .as_str()
            .or_else(|| owner.get("value").and_then(Value::as_str))
    }
}

impl Document {
    /// The records of `ir`, with the arenas the wire omits; `None` when it
    /// does not serialize.
    pub(crate) fn of(ir: &CadIr) -> Option<Self> {
        let Value::Object(mut root) = ir.to_neutral_json_value().ok()? else {
            return None;
        };
        let Value::Object(omitted) = ir.model.omitted_arenas_value().ok()? else {
            return None;
        };
        if let Some(Value::Object(model)) = root.get_mut("model") {
            model.extend(omitted);
        }
        let mut arenas = BTreeMap::new();
        for home in HOMES {
            let Some(Value::Object(members)) = root.remove(home) else {
                continue;
            };
            for (field, member) in members {
                let records = match member {
                    Value::Array(items) => items.into_iter().map(Record::new).collect(),
                    // A member that is not an arena is one record named by
                    // its field.
                    other => vec![Record {
                        id: Some(field.clone()),
                        value: other,
                    }],
                };
                arenas.insert(format!("{home}.{field}"), records);
            }
        }
        let mut document = Vec::new();
        for field in ["units", "tolerances"] {
            if let Some(value) = root.remove(field) {
                document.push(Record {
                    id: Some(field.to_owned()),
                    value,
                });
            }
        }
        if let Some(Value::Object(attributes)) = root
            .get_mut("source")
            .and_then(|source| source.get_mut("attributes"))
            .map(Value::take)
        {
            let attributes = attributes
                .into_iter()
                .filter(|(key, _)| {
                    !key.ends_with(DIGEST_SUFFIX) && !key.ends_with(NEEDS_UPDATE_SUFFIX)
                })
                .collect();
            document.push(Record {
                id: Some("source attributes".to_owned()),
                value: Value::Object(attributes),
            });
        }
        arenas.insert(DOCUMENT.to_owned(), document);
        Some(Self { arenas })
    }

    /// The record of arena `arena` with id `id`.
    pub(crate) fn record(&self, arena: &str, id: &str) -> Option<&Value> {
        self.arenas
            .get(arena)?
            .iter()
            .find(|record| record.id.as_deref() == Some(id))
            .map(|record| &record.value)
    }

    /// The record of arena `arena` with id `id`, to change.
    pub(crate) fn record_mut(&mut self, arena: &str, id: &str) -> Option<&mut Value> {
        self.arenas
            .get_mut(arena)?
            .iter_mut()
            .find(|record| record.id.as_deref() == Some(id))
            .map(|record| &mut record.value)
    }

    /// States feature `id`, named `old` in the source, by its name `new`
    /// where the decoder restates the name: the `name` of each record with
    /// that id, and each `name@feature` reference.
    pub(crate) fn rename_feature(&mut self, id: &str, old: &str, new: &str) {
        for record in self.arenas.values_mut().flatten() {
            if record.id.as_deref() == Some(id)
                && record.value.get("name").and_then(Value::as_str) == Some(old)
            {
                record.value["name"] = Value::String(new.to_owned());
            }
            rename_references(&mut record.value, old, new);
        }
    }

    /// The record with id `id`, in any arena.
    #[cfg(test)]
    pub(crate) fn find(&self, id: &str) -> Option<&Value> {
        self.arenas
            .values()
            .flatten()
            .find(|record| record.id.as_deref() == Some(id))
            .map(|record| &record.value)
    }

    /// The id of every record that states one.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> impl Iterator<Item = &str> {
        self.arenas
            .values()
            .flatten()
            .filter_map(|record| record.id.as_deref())
    }

    /// The ids of the model records that differ from, or are not in,
    /// `source`, the document this one was edited from.
    pub(crate) fn changed_model_records(&self, source: &Document) -> HashSet<String> {
        let mut changed = HashSet::new();
        for (arena, records) in &self.arenas {
            if !arena.starts_with("model.") {
                continue;
            }
            let original = source
                .arenas
                .get(arena)
                .into_iter()
                .flatten()
                .filter_map(|record| Some((record.id.as_deref()?, &record.value)))
                .collect::<HashMap<_, _>>();
            changed.extend(records.iter().filter_map(|record| {
                let id = record.id.as_deref()?;
                (original.get(id) != Some(&&record.value)).then(|| id.to_owned())
            }));
        }
        changed
    }

    /// The ids of the records of arena `arena` whose owner is one of `owners`.
    pub(crate) fn owned_by(&self, arena: &str, owners: &HashSet<String>) -> HashSet<String> {
        self.arenas
            .get(arena)
            .into_iter()
            .flatten()
            .filter(|record| record.owner().is_some_and(|owner| owners.contains(owner)))
            .filter_map(|record| record.id.clone())
            .collect()
    }
}

/// Which written record each stated record is.
#[derive(Default)]
pub(crate) struct Pairing {
    /// By arena, the paired (stated, written) record indices.
    pairs: BTreeMap<String, Vec<(usize, usize)>>,
    /// The stated ids of paired records.
    stated: HashSet<String>,
    /// The stated id of each paired written record, by written id.
    written: HashMap<String, String>,
    /// The written id of each paired stated record, by stated id.
    stated_to_written: HashMap<String, String>,
}

/// Which document a value is read from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Stated,
    Written,
}

impl Pairing {
    /// The label of id-like `text` read from `side`: the stated id of its
    /// pair, or `None` while it has none.
    fn label<'a>(&'a self, side: Side, text: &'a str) -> Option<&'a str> {
        match side {
            Side::Stated => self.stated.contains(text).then_some(text),
            Side::Written => self.written.get(text).map(String::as_str),
        }
    }

    /// The stated id of written id `id`, or `id` when it is not paired.
    pub(crate) fn stated_id<'a>(&'a self, id: &'a str) -> &'a str {
        self.written.get(id).map_or(id, String::as_str)
    }

    /// The written id of the record that stated id `id` pairs with.
    pub(crate) fn written_id(&self, id: &str) -> Option<&str> {
        self.stated_to_written.get(id).map(String::as_str)
    }

    /// `value`, read from the written document, with each paired id as the
    /// stated id it pairs with.
    pub(crate) fn stated_ids(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.stated_id(text).to_owned()),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.stated_ids(item)).collect())
            }
            Value::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(key, item)| (self.stated_id(key).to_owned(), self.stated_ids(item)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// The written ids that pair with a stated id, and that id.
    #[cfg(test)]
    pub(crate) fn renames(&self) -> &HashMap<String, String> {
        &self.written
    }

    fn bind(&mut self, arena: &str, stated: (usize, &Record), written: (usize, &Record)) {
        if let (Some(stated_id), Some(written_id)) = (&stated.1.id, &written.1.id) {
            self.stated.insert(stated_id.clone());
            self.written.insert(written_id.clone(), stated_id.clone());
            self.stated_to_written
                .insert(stated_id.clone(), written_id.clone());
        }
        self.pairs
            .entry(arena.to_owned())
            .or_default()
            .push((stated.0, written.0));
    }
}

/// How a fallback pairing round pairs records whose content differs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Round {
    /// One record on each side with a name and owner.
    Names,
    /// As many records with no name on each side whose ids have one native
    /// form, in arena order: their content differs, and their difference is
    /// reported leaf by leaf.
    Forms,
}

/// Pairs the records of `stated` and `written`, two decodes of one part.
pub(crate) fn pair(stated: &Document, written: &Document) -> Pairing {
    let mut pairing = Pairing::default();
    let mut open = stated
        .arenas
        .iter()
        .filter_map(|(arena, records)| {
            let other = written.arenas.get(arena)?;
            Some((
                arena.as_str(),
                (
                    (0..records.len()).collect::<Vec<_>>(),
                    (0..other.len()).collect::<Vec<_>>(),
                ),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    // An id with no digit embeds no offset or index: it names the same
    // record in both documents.
    for (arena, (stated_open, written_open)) in &mut open {
        let (left, right) = (&stated.arenas[*arena], &written.arenas[*arena]);
        stated_open.retain(|&index| {
            let Some(id) = left[index]
                .id
                .as_deref()
                .filter(|id| !id.bytes().any(|byte| byte.is_ascii_digit()))
            else {
                return true;
            };
            let Some(position) = written_open
                .iter()
                .position(|&other| right[other].id.as_deref() == Some(id))
            else {
                return true;
            };
            let other = written_open.remove(position);
            pairing.bind(arena, (index, &left[index]), (other, &right[other]));
            false
        });
    }
    loop {
        let bound = pair_unique_or_ties(stated, written, &mut open, &mut pairing);
        if bound > 0 {
            continue;
        }
        let bound = [Round::Names, Round::Forms]
            .into_iter()
            .map(|round| pair_round(stated, written, &mut open, &mut pairing, round))
            .find(|bound| *bound > 0);
        if bound.is_none() {
            return pairing;
        }
    }
}

type DigestGroups = BTreeMap<u64, (Vec<usize>, Vec<usize>)>;

/// Runs `Round::Unique` across all open arenas, and if nothing binds, applies
/// `Round::Ties` to the already-computed digest groups without re-hashing.
fn pair_unique_or_ties(
    stated: &Document,
    written: &Document,
    open: &mut BTreeMap<&str, (Vec<usize>, Vec<usize>)>,
    pairing: &mut Pairing,
) -> usize {
    let mut bound = 0;
    let mut cached_groups: Vec<(&str, DigestGroups)> = Vec::new();
    for (&arena, (stated_open, written_open)) in open.iter_mut() {
        if stated_open.is_empty() || written_open.is_empty() {
            continue;
        }
        let (left, right) = (&stated.arenas[arena], &written.arenas[arena]);
        let mut groups = BTreeMap::<u64, (Vec<usize>, Vec<usize>)>::new();
        for &index in stated_open.iter() {
            groups
                .entry(digest(&left[index], Side::Stated, pairing))
                .or_default()
                .0
                .push(index);
        }
        for &index in written_open.iter() {
            groups
                .entry(digest(&right[index], Side::Written, pairing))
                .or_default()
                .1
                .push(index);
        }
        let mut paired = Vec::new();
        for (lefts, rights) in groups.values() {
            if lefts.len() == 1 && rights.len() == 1 {
                paired.push((lefts[0], rights[0]));
            }
        }
        for &(index, other) in &paired {
            pairing.bind(arena, (index, &left[index]), (other, &right[other]));
        }
        if !paired.is_empty() {
            let paired_left: HashSet<usize> = paired.iter().map(|&(index, _)| index).collect();
            let paired_right: HashSet<usize> = paired.iter().map(|&(_, index)| index).collect();
            stated_open.retain(|index| !paired_left.contains(index));
            written_open.retain(|index| !paired_right.contains(index));
            bound += paired.len();
        } else if bound == 0 {
            cached_groups.push((arena, groups));
        }
    }
    if bound > 0 {
        return bound;
    }
    for (arena, groups) in cached_groups {
        let Some((stated_open, written_open)) = open.get_mut(arena) else {
            continue;
        };
        let (left, right) = (&stated.arenas[arena], &written.arenas[arena]);
        let mut paired = Vec::new();
        for (lefts, rights) in groups.values() {
            if lefts.len() == rights.len() && lefts.len() > 1 {
                paired.extend(lefts.iter().copied().zip(rights.iter().copied()));
            }
        }
        if paired.is_empty() {
            continue;
        }
        for &(index, other) in &paired {
            pairing.bind(arena, (index, &left[index]), (other, &right[other]));
        }
        let paired_left: HashSet<usize> = paired.iter().map(|&(index, _)| index).collect();
        let paired_right: HashSet<usize> = paired.iter().map(|&(_, index)| index).collect();
        stated_open.retain(|index| !paired_left.contains(index));
        written_open.retain(|index| !paired_right.contains(index));
        return paired.len();
    }
    0
}

/// Pairs the open records whose content agrees under `round`; the number
/// paired.
fn pair_round(
    stated: &Document,
    written: &Document,
    open: &mut BTreeMap<&str, (Vec<usize>, Vec<usize>)>,
    pairing: &mut Pairing,
    round: Round,
) -> usize {
    let mut bound = 0;
    for (arena, (stated_open, written_open)) in open.iter_mut() {
        if stated_open.is_empty() || written_open.is_empty() {
            continue;
        }
        let (left, right) = (&stated.arenas[*arena], &written.arenas[*arena]);
        let key = |side: Side, record: &Record| -> Option<u64> {
            match round {
                Round::Names => record.name().map(|name| {
                    let mut hasher = cadmpeg_core::finite_json::ContentHasher::new();
                    hasher.write(name.as_bytes());
                    hasher.write_u8(0xff);
                    if let Some(owner) = record.owner() {
                        hasher.write(relabel_text(owner, None, side, pairing).as_bytes());
                    }
                    hasher.finish()
                }),
                Round::Forms => record
                    .name()
                    .is_none()
                    .then(|| record.id.as_deref().and_then(native_form))
                    .flatten()
                    .map(|form| {
                        let mut hasher = cadmpeg_core::finite_json::ContentHasher::new();
                        hasher.write(form.as_bytes());
                        hasher.finish()
                    }),
            }
        };
        let mut groups = BTreeMap::<u64, (Vec<usize>, Vec<usize>)>::new();
        for &index in stated_open.iter() {
            if let Some(key) = key(Side::Stated, &left[index]) {
                groups.entry(key).or_default().0.push(index);
            }
        }
        for &index in written_open.iter() {
            if let Some(key) = key(Side::Written, &right[index]) {
                groups.entry(key).or_default().1.push(index);
            }
        }
        let mut paired = Vec::new();
        for (lefts, rights) in groups.values() {
            let agrees = match round {
                Round::Names => lefts.len() == 1 && rights.len() == 1,
                Round::Forms => lefts.len() == rights.len(),
            };
            if agrees {
                paired.extend(lefts.iter().copied().zip(rights.iter().copied()));
            }
        }
        for &(index, other) in &paired {
            pairing.bind(arena, (index, &left[index]), (other, &right[other]));
        }
        if !paired.is_empty() {
            let paired_left: HashSet<usize> = paired.iter().map(|&(index, _)| index).collect();
            let paired_right: HashSet<usize> = paired.iter().map(|&(_, index)| index).collect();
            stated_open.retain(|index| !paired_left.contains(index));
            written_open.retain(|index| !paired_right.contains(index));
        }
        bound += paired.len();
    }
    bound
}

/// The content of `record` read from `side` under the current pairing: its
/// own id, paired ids and the shape of unpaired ids replaced, and fractional
/// numbers to nine significant digits.
fn digest(record: &Record, side: Side, pairing: &Pairing) -> u64 {
    let mut hasher = cadmpeg_core::finite_json::ContentHasher::new();
    feed(
        &record.value,
        record.id.as_deref(),
        side,
        pairing,
        &mut hasher,
    );
    hasher.finish()
}

/// Fixed-capacity stack buffer for formatting floats without heap allocation.
struct StackBuf {
    bytes: [u8; 32],
    len: usize,
}

impl StackBuf {
    const fn new() -> Self {
        Self {
            bytes: [0; 32],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl fmt::Write for StackBuf {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let raw = text.as_bytes();
        let end = self.len.checked_add(raw.len()).ok_or(fmt::Error)?;
        let slot = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        slot.copy_from_slice(raw);
        self.len = end;
        Ok(())
    }
}

fn feed(value: &Value, own: Option<&str>, side: Side, pairing: &Pairing, hasher: &mut impl Hasher) {
    match value {
        Value::Null => hasher.write_u8(0),
        Value::Bool(flag) => {
            hasher.write_u8(1);
            hasher.write_u8(u8::from(*flag));
        }
        Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                hasher.write_u8(2);
                hasher.write_i64(integer);
            } else if let Some(integer) = number.as_u64() {
                hasher.write_u8(3);
                hasher.write_u64(integer);
            } else {
                let real = number.as_f64().unwrap_or_default();
                let real = if real == 0.0 { 0.0 } else { real };
                hasher.write_u8(4);
                let mut buf = StackBuf::new();
                if write!(&mut buf, "{real:.8e}").is_ok() {
                    hasher.write(buf.as_bytes());
                } else {
                    hasher.write(format!("{real:.8e}").as_bytes());
                }
            }
        }
        Value::String(text) => {
            hasher.write_u8(5);
            feed_text(text, own, side, pairing, hasher);
            hasher.write_u8(0xff);
        }
        Value::Array(items) => {
            hasher.write_u8(6);
            hasher.write_usize(items.len());
            for item in items {
                feed(item, own, side, pairing, hasher);
            }
        }
        Value::Object(entries) => {
            hasher.write_u8(7);
            hasher.write_usize(entries.len());
            let needs_relabel = entries
                .keys()
                .any(|key| key_relabels(key, own, side, pairing));
            if needs_relabel {
                let mut keyed = entries
                    .iter()
                    .map(|(key, value)| (relabel_text(key, own, side, pairing), value))
                    .collect::<Vec<_>>();
                keyed.sort_by(|left, right| left.0.cmp(&right.0));
                for (key, value) in keyed {
                    hasher.write(key.as_bytes());
                    hasher.write_u8(0xff);
                    feed(value, own, side, pairing, hasher);
                }
            } else {
                for (key, value) in entries {
                    hasher.write(key.as_bytes());
                    hasher.write_u8(0xff);
                    feed(value, own, side, pairing, hasher);
                }
            }
        }
    }
}

fn feed_text(
    text: &str,
    own: Option<&str>,
    side: Side,
    pairing: &Pairing,
    hasher: &mut impl Hasher,
) {
    hasher.write(relabel_text(text, own, side, pairing).as_bytes());
}

fn key_relabels(text: &str, own: Option<&str>, side: Side, pairing: &Pairing) -> bool {
    own == Some(text)
        || (side == Side::Written && pairing.written.contains_key(text))
        || (text.starts_with("sldprt:") && pairing.label(side, text).is_none())
}

/// `text` as two documents compare it: the record's own id as `@self`, a
/// paired id as its stated id, an unpaired native reference in its
/// [`native_form`], and any other text as it is.
fn relabel_text<'a>(
    text: &'a str,
    own: Option<&str>,
    side: Side,
    pairing: &'a Pairing,
) -> Cow<'a, str> {
    if own == Some(text) {
        return Cow::Borrowed("@self");
    }
    if let Some(label) = pairing.label(side, text) {
        return Cow::Borrowed(label);
    }
    if text.starts_with("sldprt:") {
        if let Some(form) = native_form(text) {
            return Cow::Owned(form);
        }
    }
    Cow::Borrowed(text)
}

/// The form of native reference `text`, with each run of digits of its
/// locator as [`UNPAIRED_DIGITS`]: a write moves the file offsets and
/// record indices a locator states. The locator is the text after the `#`
/// of a `sldprt:` id, or after an [`OFFSET_FORMS`] prefix. Digits a locator
/// states as content stay: a percent escape, and the line index that ends
/// the locator of a solver line. `None` for any other text, whose digits are
/// content.
fn native_form(text: &str) -> Option<String> {
    let rest = text.strip_prefix("sldprt:")?;
    let (kind, locator) = match rest.split_once('#') {
        Some((kind, locator)) => (format!("sldprt:{kind}#"), locator),
        None => OFFSET_FORMS.iter().find_map(|form| {
            rest.strip_prefix(form)
                .map(|locator| (format!("sldprt:{form}"), locator))
        })?,
    };
    let kept_last = locator
        .contains("solver-line")
        .then(|| {
            locator
                .rfind(|character: char| !character.is_ascii_digit())
                .map_or(0, |index| index + 1)
        })
        .filter(|start| *start < locator.len());
    let mut form = kind;
    let mut digits = false;
    // The end of the percent escape the scan is in: `%` and two hex digits.
    let mut escape_end = 0;
    for (index, character) in locator.char_indices() {
        let content = index < escape_end || kept_last.is_some_and(|start| index >= start);
        if character == '%' {
            escape_end = index + 3;
        }
        if character.is_ascii_digit() && !content {
            if !digits {
                form.push_str(UNPAIRED_DIGITS);
            }
            digits = true;
        } else {
            form.push(character);
            digits = false;
        }
    }
    Some(form)
}

/// Replaces each `@old` reference of `value`'s strings, a name followed by
/// no letter or digit, with `@new`.
fn rename_references(value: &mut Value, old: &str, new: &str) {
    match value {
        Value::String(text) => {
            let pattern = format!("@{old}");
            if !text.contains(&pattern) {
                return;
            }
            let mut renamed = String::with_capacity(text.len());
            let mut rest = text.as_str();
            while let Some(at) = rest.find(&pattern) {
                let after = &rest[at + pattern.len()..];
                renamed.push_str(&rest[..at]);
                if after.starts_with(|character: char| character.is_alphanumeric()) {
                    renamed.push_str(&pattern);
                } else {
                    renamed.push('@');
                    renamed.push_str(new);
                }
                rest = after;
            }
            renamed.push_str(rest);
            *text = renamed;
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| rename_references(item, old, new)),
        Value::Object(entries) => entries
            .values_mut()
            .for_each(|item| rename_references(item, old, new)),
        _ => {}
    }
}

/// `value` read from `side` with each id replaced as [`relabel_text`] does,
/// the record's own id kept.
fn relabel(value: &Value, side: Side, pairing: &Pairing) -> Value {
    match value {
        Value::String(text) => Value::String(relabel_text(text, None, side, pairing).into_owned()),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| relabel(item, side, pairing))
                .collect(),
        ),
        Value::Object(entries) => {
            let mut relabelled = serde_json::Map::new();
            for (key, value) in entries {
                // Two keys of one form stay two entries.
                let mut key = relabel_text(key, None, side, pairing).into_owned();
                while relabelled.contains_key(&key) {
                    key.push('\'');
                }
                relabelled.insert(key, relabel(value, side, pairing));
            }
            Value::Object(relabelled)
        }
        other => other.clone(),
    }
}

/// Whether `stated`, read from the stated document, and `written`, read from
/// the written one, agree leaf by leaf under `pairing`.
pub(crate) fn agree(stated: &Value, written: &Value, pairing: &Pairing) -> bool {
    disagreement(stated, written, pairing).is_none()
}

/// The places at which `stated`, read from the stated document, and
/// `written`, read from the written one, disagree under `pairing`; `None`
/// where they agree.
pub(crate) fn disagreement(stated: &Value, written: &Value, pairing: &Pairing) -> Option<String> {
    let mut changed = Vec::new();
    leaves_paired(
        Some(stated),
        Some(written),
        pairing,
        &mut String::new(),
        &mut changed,
    );
    let places = changed
        .into_iter()
        .map(|(pointer, _, _)| pointer)
        .collect::<Vec<_>>();
    (!places.is_empty()).then(|| places.join(", "))
}

/// Whether `stated`, read from the stated document, agrees with `reported`,
/// a written leaf as a [`Difference`] reports it.
pub(crate) fn agrees_with_reported(stated: &Value, reported: &Value, pairing: &Pairing) -> bool {
    let mut changed = Vec::new();
    leaves(
        Some(&relabel(stated, Side::Stated, pairing)),
        Some(reported),
        &mut String::new(),
        &mut changed,
    );
    changed.is_empty()
}

/// Id `id` of an unpaired record as a [`Difference`] reports it in a key or
/// a reference.
pub(crate) fn reported_form(id: &str) -> String {
    native_form(id).unwrap_or_else(|| id.to_owned())
}

/// One difference between two decodes of one part.
#[derive(Clone)]
pub(crate) struct Difference {
    pub(crate) arena: String,
    /// The stated record's id; for an added record, its written id paired
    /// where it can be.
    pub(crate) id: Option<String>,
    /// The record's name.
    pub(crate) name: Option<String>,
    /// The record's owner, as a stated id where it pairs.
    pub(crate) owner: Option<String>,
    pub(crate) change: Change,
}

/// What differs about one record.
#[derive(Clone)]
pub(crate) enum Change {
    /// The written document has no record for it.
    Removed,
    /// The stated document has no record for it; `content` is its digest
    /// under the pairing.
    Added { content: u64 },
    /// The leaf at JSON pointer `pointer` differs; `None` where one side
    /// states none.
    Leaf {
        pointer: String,
        stated: Option<Value>,
        written: Option<Value>,
    },
}

impl Difference {
    /// What a loss calls it: the arena, the record's name or id, and the
    /// place of a changed leaf.
    pub(crate) fn label(&self) -> String {
        let record = match (self.name.as_deref(), self.id.as_deref()) {
            (Some(name), Some(id)) => format!("{name} ({id})"),
            (Some(record), None) | (None, Some(record)) => record.to_owned(),
            (None, None) => "<record>".to_owned(),
        };
        match &self.change {
            Change::Removed => format!("{} {record} (missing)", self.arena),
            Change::Added { .. } => format!("{} {record} (added)", self.arena),
            Change::Leaf { pointer, .. } => format!("{} {record} at {pointer}", self.arena),
        }
    }

    /// The segments of a changed leaf's pointer, unescaped.
    pub(crate) fn segments(&self) -> Vec<String> {
        match &self.change {
            Change::Leaf { pointer, .. } => pointer
                .split('/')
                .skip(1)
                .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// Every difference between `stated` and `written` under `pairing`.
pub(crate) fn differences(
    stated: &Document,
    written: &Document,
    pairing: &Pairing,
) -> Vec<Difference> {
    let mut differences = Vec::new();
    let arenas = stated
        .arenas
        .keys()
        .chain(written.arenas.keys())
        .collect::<BTreeSet<_>>();
    for arena in arenas {
        let (left, right) = (
            stated.arenas.get(arena).map_or(&[][..], Vec::as_slice),
            written.arenas.get(arena).map_or(&[][..], Vec::as_slice),
        );
        let pairs = pairing.pairs.get(arena).map_or(&[][..], Vec::as_slice);
        let stated_paired = pairs
            .iter()
            .map(|(index, _)| *index)
            .collect::<HashSet<_>>();
        let written_paired = pairs
            .iter()
            .map(|(_, index)| *index)
            .collect::<HashSet<_>>();
        for (index, record) in left.iter().enumerate() {
            if !stated_paired.contains(&index) {
                differences.push(Difference {
                    arena: arena.clone(),
                    id: record.id.clone(),
                    name: record.name().map(str::to_owned),
                    owner: record.owner().map(str::to_owned),
                    change: Change::Removed,
                });
            }
        }
        for (index, record) in right.iter().enumerate() {
            if !written_paired.contains(&index) {
                differences.push(Difference {
                    arena: arena.clone(),
                    id: record
                        .id
                        .as_deref()
                        .map(|id| pairing.stated_id(id).to_owned()),
                    name: record.name().map(str::to_owned),
                    owner: record
                        .owner()
                        .map(|owner| pairing.stated_id(owner).to_owned()),
                    change: Change::Added {
                        content: digest(record, Side::Written, pairing),
                    },
                });
            }
        }
        let mut pointer = String::new();
        for &(index, other) in pairs {
            let (record, read) = (&left[index], &right[other]);
            let mut changed = Vec::new();
            pointer.clear();
            leaves_paired(
                Some(&record.value),
                Some(&read.value),
                pairing,
                &mut pointer,
                &mut changed,
            );
            differences.extend(
                changed
                    .into_iter()
                    .map(|(pointer, stated, written)| Difference {
                        arena: arena.clone(),
                        id: record.id.clone(),
                        name: record.name().map(str::to_owned),
                        owner: record.owner().map(str::to_owned),
                        change: Change::Leaf {
                            pointer,
                            stated,
                            written,
                        },
                    }),
            );
        }
    }
    differences
}

type Leaf = (String, Option<Value>, Option<Value>);

fn push_pointer_key(pointer: &mut String, key: &str) {
    pointer.push('/');
    if key.contains('~') || key.contains('/') {
        for character in key.chars() {
            match character {
                '~' => pointer.push_str("~0"),
                '/' => pointer.push_str("~1"),
                other => pointer.push(other),
            }
        }
    } else {
        pointer.push_str(key);
    }
}

fn push_pointer_index(pointer: &mut String, index: usize) {
    pointer.push('/');
    if write!(pointer, "{index}").is_err() {
        pointer.push_str(&index.to_string());
    }
}

/// Pushes each leaf at which `stated` and `written` disagree under `pairing`,
/// borrowing both trees without cloning agreeing records.
fn leaves_paired(
    stated: Option<&Value>,
    written: Option<&Value>,
    pairing: &Pairing,
    pointer: &mut String,
    out: &mut Vec<Leaf>,
) {
    match (stated, written) {
        (Some(Value::String(left)), Some(Value::String(right))) => {
            let left_rel = relabel_text(left, None, Side::Stated, pairing);
            let right_rel = relabel_text(right, None, Side::Written, pairing);
            if left_rel != right_rel {
                out.push((
                    pointer.clone(),
                    Some(Value::String(left_rel.into_owned())),
                    Some(Value::String(right_rel.into_owned())),
                ));
            }
        }
        (Some(Value::Object(left)), Some(Value::Object(right))) => {
            let relabels = left
                .keys()
                .any(|key| key_relabels(key, None, Side::Stated, pairing))
                || right
                    .keys()
                    .any(|key| key_relabels(key, None, Side::Written, pairing));
            if relabels {
                let left_rel = relabel(&Value::Object(left.clone()), Side::Stated, pairing);
                let right_rel = relabel(&Value::Object(right.clone()), Side::Written, pairing);
                leaves(Some(&left_rel), Some(&right_rel), pointer, out);
                return;
            }
            let same_keys = left.len() == right.len()
                && left
                    .keys()
                    .zip(right.keys())
                    .all(|(left_key, right_key)| left_key == right_key);
            if same_keys {
                for ((key, left_value), (_, right_value)) in left.iter().zip(right.iter()) {
                    let length = pointer.len();
                    push_pointer_key(pointer, key);
                    leaves_paired(Some(left_value), Some(right_value), pairing, pointer, out);
                    pointer.truncate(length);
                }
            } else {
                for key in left.keys().chain(right.keys()).collect::<BTreeSet<_>>() {
                    let length = pointer.len();
                    push_pointer_key(pointer, key);
                    leaves_paired(left.get(key), right.get(key), pairing, pointer, out);
                    pointer.truncate(length);
                }
            }
        }
        (Some(Value::Array(left)), Some(Value::Array(right))) if left.len() == right.len() => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                let length = pointer.len();
                push_pointer_index(pointer, index);
                leaves_paired(Some(left), Some(right), pairing, pointer, out);
                pointer.truncate(length);
            }
        }
        (
            Some(left @ (Value::Null | Value::Bool(_) | Value::Number(_))),
            Some(right @ (Value::Null | Value::Bool(_) | Value::Number(_))),
        ) => {
            if cadmpeg_ir::compare::values_agree(left, right).is_err() {
                out.push((pointer.clone(), Some(left.clone()), Some(right.clone())));
            }
        }
        (Some(left), Some(right)) => {
            let left_rel = relabel(left, Side::Stated, pairing);
            let right_rel = relabel(right, Side::Written, pairing);
            if cadmpeg_ir::compare::values_agree(&left_rel, &right_rel).is_err() {
                out.push((pointer.clone(), Some(left_rel), Some(right_rel)));
            }
        }
        (None, None) => {}
        (left, right) => out.push((
            pointer.clone(),
            left.map(|value| relabel(value, Side::Stated, pairing)),
            right.map(|value| relabel(value, Side::Written, pairing)),
        )),
    }
}

/// Pushes each leaf at which `stated` and `written` disagree.
fn leaves(
    stated: Option<&Value>,
    written: Option<&Value>,
    pointer: &mut String,
    out: &mut Vec<Leaf>,
) {
    match (stated, written) {
        (Some(Value::Object(left)), Some(Value::Object(right))) => {
            for key in left.keys().chain(right.keys()).collect::<BTreeSet<_>>() {
                let length = pointer.len();
                push_pointer_key(pointer, key);
                leaves(left.get(key), right.get(key), pointer, out);
                pointer.truncate(length);
            }
        }
        (Some(Value::Array(left)), Some(Value::Array(right))) if left.len() == right.len() => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                let length = pointer.len();
                push_pointer_index(pointer, index);
                leaves(Some(left), Some(right), pointer, out);
                pointer.truncate(length);
            }
        }
        (Some(left), Some(right)) => {
            if cadmpeg_ir::compare::values_agree(left, right).is_err() {
                out.push((pointer.clone(), Some(left.clone()), Some(right.clone())));
            }
        }
        (None, None) => {}
        (left, right) => out.push((pointer.clone(), left.cloned(), right.cloned())),
    }
}
