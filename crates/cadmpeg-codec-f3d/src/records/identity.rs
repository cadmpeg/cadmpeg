// SPDX-License-Identifier: Apache-2.0
//! Record primitives every design-record family states: entity identity, located values, counted runs and the shared affine transform.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
pub(super) const IDENTITY_MATRIX: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// A secondary selection identity and its optional curve identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct DesignSecondaryIdentity<Id> {
    #[serde(rename = "secondary_identity")]
    pub(crate) identity: Id,
    #[serde(
        rename = "curve_secondary_identity",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) curve_identity: Option<Id>,
}

impl<Id> DesignSecondaryIdentity<Id> {
    pub(super) fn from_wire(
        identity: Option<Id>,
        curve_identity: Option<Id>,
    ) -> Result<Option<Self>, String> {
        match (identity, curve_identity) {
            (Some(identity), curve_identity) => Ok(Some(Self {
                identity,
                curve_identity,
            })),
            (None, None) => Ok(None),
            (None, Some(_)) => Err("curve_secondary_identity requires secondary_identity".into()),
        }
    }
}

/// Design entity identity with a decimal u64 suffix after its final underscore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignEntityId {
    pub(super) text: String,
    suffix: u64,
}

impl TryFrom<String> for DesignEntityId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let (_, suffix) = value
            .rsplit_once('_')
            .ok_or("entity_id requires a decimal suffix")?;
        let suffix = suffix
            .parse::<u64>()
            .map_err(|_| "entity_id requires a decimal u64 suffix")?;
        Ok(Self {
            text: value,
            suffix,
        })
    }
}

impl DesignEntityId {
    pub(crate) fn from_parts(prefix: &str, suffix: u64) -> Self {
        Self {
            text: format!("{prefix}_{suffix}"),
            suffix,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn suffix(&self) -> u64 {
        self.suffix
    }
}

/// A source value and the byte offset of its encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Located<T, O = u64> {
    pub(crate) value: T,
    pub(crate) offset: O,
}

impl<T, O> Located<T, O> {
    pub(super) fn from_wire(
        value: Option<T>,
        offset: Option<O>,
        field: &str,
    ) -> Result<Option<Self>, String> {
        match (value, offset) {
            (None, None) => Ok(None),
            (Some(value), Some(offset)) => Ok(Some(Self { value, offset })),
            _ => Err(format!("{field} and {field}_offset must occur together")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReferenceRunData<T, O> {
    Unlocated(Vec<T>),
    Located(Vec<Located<T, O>>),
}

/// An ordered run whose encoding locations are either complete or absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReferenceRun<T, O = u64>(ReferenceRunData<T, O>);

impl<T, O> ReferenceRun<T, O> {
    /// A run whose values carry no encoding locations. The empty run has no
    /// locations in either wire form, so it is always `Located(vec![])`.
    pub(crate) fn unlocated(values: Vec<T>) -> Self {
        if values.is_empty() {
            Self(ReferenceRunData::Located(Vec::new()))
        } else {
            Self(ReferenceRunData::Unlocated(values))
        }
    }

    /// A run whose values each carry an encoding location.
    pub(crate) fn located(rows: Vec<Located<T, O>>) -> Self {
        Self(ReferenceRunData::Located(rows))
    }

    pub(crate) fn located_rows(&self) -> Option<&[Located<T, O>]> {
        match &self.0 {
            ReferenceRunData::Located(rows) => Some(rows),
            ReferenceRunData::Unlocated(_) => None,
        }
    }

    pub(crate) fn values(&self) -> impl ExactSizeIterator<Item = &T> + DoubleEndedIterator + Clone {
        (0..self.len()).map(|index| match &self.0 {
            ReferenceRunData::Unlocated(values) => &values[index],
            ReferenceRunData::Located(values) => &values[index].value,
        })
    }

    pub(crate) fn values_in(
        &self,
        range: std::ops::Range<usize>,
    ) -> Option<impl ExactSizeIterator<Item = &T> + DoubleEndedIterator + Clone> {
        if range.start > range.end || range.end > self.len() {
            return None;
        }
        Some(
            self.values()
                .skip(range.start)
                .take(range.end - range.start),
        )
    }

    pub(crate) fn values_array<const N: usize>(&self) -> Option<[&T; N]> {
        match &self.0 {
            ReferenceRunData::Unlocated(values) => {
                let values: &[T; N] = values.as_slice().try_into().ok()?;
                Some(values.each_ref())
            }
            ReferenceRunData::Located(values) => {
                let values: &[Located<T, O>; N] = values.as_slice().try_into().ok()?;
                Some(values.each_ref().map(|row| &row.value))
            }
        }
    }

    pub(crate) fn offsets(
        &self,
    ) -> impl ExactSizeIterator<Item = &O> + DoubleEndedIterator + Clone {
        let rows: &[Located<T, O>] = match &self.0 {
            ReferenceRunData::Unlocated(_) => &[],
            ReferenceRunData::Located(rows) => rows,
        };
        rows.iter().map(|row| &row.offset)
    }

    #[cfg(test)]
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let (unlocated, located): (&mut [T], &mut [Located<T, O>]) = match &mut self.0 {
            ReferenceRunData::Unlocated(values) => (values, &mut []),
            ReferenceRunData::Located(values) => (&mut [], values),
        };
        unlocated
            .iter_mut()
            .chain(located.iter_mut().map(|row| &mut row.value))
    }

    pub(crate) fn len(&self) -> usize {
        match &self.0 {
            ReferenceRunData::Unlocated(values) => values.len(),
            ReferenceRunData::Located(values) => values.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match &self.0 {
            ReferenceRunData::Unlocated(values) => values.is_empty(),
            ReferenceRunData::Located(values) => values.is_empty(),
        }
    }

    pub(crate) fn from_columns(
        values: Vec<T>,
        offsets: Vec<O>,
        field: &str,
    ) -> Result<Self, String> {
        if offsets.is_empty() {
            return Ok(Self::unlocated(values));
        }
        if values.len() != offsets.len() {
            return Err(format!(
                "{field} offsets must be absent or match every value"
            ));
        }
        Ok(Self::located(
            values
                .into_iter()
                .zip(offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
        ))
    }

    pub(super) fn into_wire(self) -> (Vec<T>, Vec<O>) {
        match self.0 {
            ReferenceRunData::Unlocated(values) => (values, Vec::new()),
            ReferenceRunData::Located(values) => values
                .into_iter()
                .map(|row| (row.value, row.offset))
                .unzip(),
        }
    }
}

/// A vector that always holds at least one element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NonEmptyVec<T>(Vec<T>);

impl<T> NonEmptyVec<T> {
    /// Wrap `items`, or return `None` when `items` is empty.
    pub(super) fn new(items: Vec<T>) -> Option<Self> {
        (!items.is_empty()).then_some(Self(items))
    }

    /// Borrow the elements in order.
    pub(super) fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Take the elements in order.
    pub(super) fn into_vec(self) -> Vec<T> {
        self.0
    }
}

/// A non-empty half-open interval of source bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NonEmptyByteSpan {
    start: u64,
    end: u64,
}

impl NonEmptyByteSpan {
    pub(crate) fn new(start: u64, end: u64) -> Option<Self> {
        if start >= end {
            return None;
        }
        Some(Self { start, end })
    }

    pub(crate) fn start(&self) -> u64 {
        self.start
    }

    pub(crate) fn end(&self) -> u64 {
        self.end
    }

    pub(super) fn byte_len(&self) -> u64 {
        self.end - self.start
    }
}

/// A value with its source encoding location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordedValue<T> {
    pub(crate) value: T,
    pub(crate) offset: u64,
}

impl<T> RecordedValue<T> {
    pub(super) fn from_wire(
        value: Option<T>,
        offset: Option<u64>,
        field: &str,
    ) -> Result<Option<Self>, String> {
        match (value, offset) {
            (Some(value), Some(offset)) => Ok(Some(Self { value, offset })),
            (Some(_), None) => Err(format!("{field} requires {field}_offset")),
            (None, None) => Ok(None),
            (None, Some(_)) => Err(format!("{field}_offset requires {field}")),
        }
    }
}

/// A value that its record form may leave without a source encoding location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaybeRecordedValue<T> {
    /// The form stores the value at a known offset.
    Located(RecordedValue<T>),
    /// The form fixes the value in its envelope and stores no member for it.
    Unlocated(T),
}

impl<T: Copy> MaybeRecordedValue<T> {
    /// The value, located or not.
    pub(super) fn value(&self) -> T {
        match self {
            Self::Located(recorded) => recorded.value,
            Self::Unlocated(value) => *value,
        }
    }

    /// The source encoding location, when the form stores one.
    pub(super) fn offset(&self) -> Option<u64> {
        match self {
            Self::Located(recorded) => Some(recorded.offset),
            Self::Unlocated(_) => None,
        }
    }

    pub(super) fn from_wire(
        value: Option<T>,
        offset: Option<u64>,
        field: &str,
    ) -> Result<Option<Self>, String> {
        match (value, offset) {
            (Some(value), Some(offset)) => Ok(Some(Self::Located(RecordedValue { value, offset }))),
            (Some(value), None) => Ok(Some(Self::Unlocated(value))),
            (None, None) => Ok(None),
            (None, Some(_)) => Err(format!("{field}_offset requires {field}")),
        }
    }
}

// The wire adapter receives the optional field by reference, including its absence.
#[allow(clippy::ref_option)]
pub(super) fn serialize_absent_u64_offset<S: Serializer>(
    value: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(value.unwrap_or(0))
}

pub(super) fn deserialize_absent_u64_offset<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let offset = u64::deserialize(deserializer)?;
    Ok((offset != 0).then_some(offset))
}

/// A finite row-major affine placement.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(super) struct DesignAffineTransform(pub(super) [[f64; 4]; 4]);

impl DesignAffineTransform {
    /// Four row-major rows.
    pub(super) fn rows(self) -> [[f64; 4]; 4] {
        self.0
    }
}

impl TryFrom<[[f64; 4]; 4]> for DesignAffineTransform {
    type Error = String;
    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        (rows.iter().flatten().all(|value| value.is_finite()) && rows[3] == [0.0, 0.0, 0.0, 1.0])
            .then_some(Self(rows))
            .ok_or_else(|| "transform must be finite and affine".into())
    }
}

impl From<DesignAffineTransform> for [[f64; 4]; 4] {
    fn from(value: DesignAffineTransform) -> Self {
        value.0
    }
}

impl std::ops::Deref for DesignAffineTransform {
    type Target = [[f64; 4]; 4];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct NativeRecordId {
    pub(super) text: String,
    stream_end: usize,
}

impl NativeRecordId {
    pub(super) fn try_new(
        text: String,
        kind: &str,
        key: impl std::fmt::Display,
    ) -> Result<Self, String> {
        let stream = crate::ids::native_stream(&text).ok_or("id must contain a native stream")?;
        if text != format!("{stream}:{kind}#{key}") {
            return Err(format!("id must identify {kind} at {key}"));
        }
        let stream_end = stream.len();
        Ok(Self { text, stream_end })
    }
    pub(super) fn stream(&self) -> &str {
        &self.text[..self.stream_end]
    }
}
