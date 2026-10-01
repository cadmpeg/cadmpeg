// SPDX-License-Identifier: Apache-2.0
//! Source-format namespaces retained outside the format-neutral model.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::num::NonZeroUsize;

use cadmpeg_core::decode::DecodeContext;
#[cfg(feature = "schema")]
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

mod canon;
pub mod catalogue;
mod replay;

#[cfg(test)]
thread_local! {
    static TYPED_RECORD_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_ctx() -> DecodeContext<'static> {
    let arena = Box::leak(Box::new(cadmpeg_core::decode::DecodeArena::new()));
    DecodeContext::from_root_bytes(&[], arena, &cadmpeg_core::decode::DecodePolicy::service())
        .expect("empty test input fits the service policy")
        .0
}

/// Deepest container chain one native record field may hold.
///
/// Every descent of a stored field recurses: `serde_json::from_value` carries
/// no recursion counter, [`replay::emit`] starts one parse per container,
/// `canon::CanonValue` enters one frame per container, and `Serialize`,
/// [`NativeRecord::fields`], [`NativeRecord::field`] and `Drop` walk the
/// stored `Value` itself. The bound is therefore stated where a field enters
/// the record, so every reader of a constructed record is already inside it.
/// It is twice the 128 containers a `serde_json` text parse admits, so every
/// field a CADIR document can state is read back.
const MAX_NATIVE_NESTING_DEPTH: usize = 256;

/// The text every refusal of [`MAX_NATIVE_NESTING_DEPTH`] carries.
///
/// The write path, the replay path and the constructor all answer to one
/// number, so they say the same thing when they refuse.
fn nests_too_deep_message() -> String {
    format!("native value nests deeper than {MAX_NATIVE_NESTING_DEPTH} containers")
}

/// States whether `value` holds a container chain longer than `limit`.
///
/// The pending containers live in this function's own vector, so measuring a
/// value cannot itself overflow the machine stack. Only containers enter it: a
/// scalar carries no chain, and a key is not a value.
fn nests_past(value: &Value, limit: usize) -> bool {
    let mut pending: Vec<(&Value, usize)> = Vec::new();
    push_container(&mut pending, value, 1);
    while let Some((value, depth)) = pending.pop() {
        if depth > limit {
            return true;
        }
        match value {
            Value::Array(items) => {
                for item in items {
                    push_container(&mut pending, item, depth + 1);
                }
            }
            Value::Object(entries) => {
                for entry in entries.values() {
                    push_container(&mut pending, entry, depth + 1);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    false
}

/// Records `value` at `depth` when it is a container.
fn push_container<'a>(pending: &mut Vec<(&'a Value, usize)>, value: &'a Value, depth: usize) {
    if matches!(value, Value::Array(_) | Value::Object(_)) {
        pending.push((value, depth));
    }
}

/// One non-empty native arena reported as an exporter loss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LossCount {
    /// Source-format namespace this arena belongs to.
    pub format: String,
    /// Arena name within that namespace.
    pub kind: String,
    /// Number of records in the arena.
    pub count: NonZeroUsize,
}

/// Conversion failure between codec-owned typed records and generic records.
#[derive(Debug, thiserror::Error)]
pub enum NativeConvertError {
    /// The caller's decode budget refused native record storage.
    #[error(transparent)]
    Resource(#[from] cadmpeg_core::CodecError),
    /// A serialized typed record has no string `id` field.
    #[error("native record is missing a string id")]
    MissingId,
    /// A native identity does not follow the entity identity grammar.
    #[error("native record has an invalid identity: {0}")]
    InvalidIdentity(#[from] crate::ids::IdentityError),
    /// A typed record did not serialize as a JSON object.
    #[error("native record did not serialize as an object")]
    NonObject,
    /// A caller-owned field nests deeper than a native record may hold.
    #[error(
        "native record {id}: field {field} nests deeper than {} containers",
        MAX_NATIVE_NESTING_DEPTH
    )]
    FieldNestsTooDeep {
        /// Identity the refused field was offered under.
        id: crate::ids::Identity,
        /// Name of the refused field.
        field: String,
    },
    /// A typed record holds a NaN or infinite number, which a stored record
    /// cannot state.
    #[error("native record field {field} holds a non-finite number")]
    NonFiniteNumber {
        /// Member path of the refused number: keys joined by `.`, sequence
        /// elements written `[index]`.
        field: String,
    },
    /// JSON conversion failed.
    #[error("native record conversion failed: {0}")]
    Serde(#[from] serde_json::Error),
    /// Canonical conversion refused an input member without a JSON parse error.
    #[error("native record conversion failed: {0}")]
    ConversionMessage(String),
    /// A stored record does not satisfy its codec-owned reader.
    #[error("native record {id}: {source}")]
    ReadRecord {
        /// Identity of the refused stored record.
        id: crate::ids::Identity,
        /// Codec-owned field admission error.
        #[source]
        source: serde_json::Error,
    },
    /// A producer's record cannot enter a native arena.
    #[error("native input record at ordinal {ordinal}: {source}")]
    WriteRecord {
        /// Zero-based position in the producer's input iterator.
        ordinal: usize,
        /// Record conversion error before an identity is necessarily available.
        #[source]
        source: Box<NativeConvertError>,
    },
    /// A named arena contains a refused record.
    #[error("native arena {arena}: {source}")]
    Arena {
        /// Owning arena name.
        arena: String,
        /// Located record conversion error.
        #[source]
        source: Box<NativeConvertError>,
    },
    /// A typed child record references no record in its owning arena.
    #[error("native record has an invalid owner: {0}")]
    InvalidOwner(String),
    /// A native collection violates its codec-owned admission contract.
    #[error("native collection is invalid: {0}")]
    InvalidCollection(String),
    /// A source-independent unknown record has no retained source counterpart.
    #[error("native unknown record has no retained source record: {0}")]
    MissingRetainedSourceRecord(String),
}

impl From<NativeConvertError> for cadmpeg_core::CodecError {
    fn from(error: NativeConvertError) -> Self {
        if let Some(limit) = error.resource_limit() {
            Self::ResourceLimit(limit)
        } else {
            Self::Malformed(error.to_string())
        }
    }
}

impl NativeConvertError {
    fn resource_limit(&self) -> Option<cadmpeg_core::decode::ResourceLimit> {
        match self {
            Self::Resource(cadmpeg_core::CodecError::ResourceLimit(limit)) => Some(*limit),
            Self::WriteRecord { source, .. } | Self::Arena { source, .. } => {
                source.resource_limit()
            }
            _ => None,
        }
    }
}

/// Charges serialized writes without retaining their bytes.
struct ChargingJsonWriter<'a, 'b> {
    ctx: &'a DecodeContext<'b>,
    refusal: Option<cadmpeg_core::CodecError>,
}

impl Write for ChargingJsonWriter<'_, '_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Err(error) = self.ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "serialize native record",
        ) {
            self.refusal = Some(error);
            return Err(std::io::Error::other("native record resource limit"));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Schema descriptor for a native record's identity and open field map.
#[derive(Serialize)]
#[cfg(feature = "schema")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename = "NativeRecord")]
struct RecordShape<'a> {
    /// Globally unique record identity.
    id: crate::ids::Identity,
    /// Codec-owned record fields.
    #[serde(flatten)]
    fields: &'a Map<String, Value>,
}

/// One native record field a producer states without nesting it.
///
/// A record assembled from these cannot reach [`MAX_NATIVE_NESTING_DEPTH`], so
/// [`NativeRecord::from_identity`] admits them with nothing to measure and
/// nothing to refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeField {
    /// A JSON string.
    Text(String),
    /// A JSON array of strings.
    TextList(Vec<String>),
}

impl NativeField {
    /// This field as the value a record stores.
    fn into_value(self) -> Value {
        match self {
            Self::Text(text) => Value::String(text),
            Self::TextList(items) => Value::Array(items.into_iter().map(Value::String).collect()),
        }
    }
}

/// One source-native record with a stable identity and codec-owned fields.
///
/// Every field is at most [`MAX_NATIVE_NESTING_DEPTH`] containers deep: the
/// four ways to build one are [`NativeRecord::new`], which measures the
/// caller's map, `Deserialize`, which calls it, [`NativeRecord::from_typed`],
/// whose serializer counts the containers it enters, and
/// [`NativeRecord::from_identity`], whose fields cannot nest. Readers of a
/// stored record therefore descend a bounded value and measure nothing.
///
/// The record holds the value it was constructed from. `serde_json::Map` is
/// key-sorted, so the stored map is already in canonical order and the record
/// renders its canonical text — `id` first, then the other keys sorted — on
/// demand through [`Serialize`]. Reading a field is a map lookup and has no
/// failing branch.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeRecord {
    /// Globally unique record identity, serialized as the leading `id` member.
    id: crate::ids::Identity,
    /// Codec-owned fields in canonical key order, never holding `id`.
    fields: Map<String, Value>,
}

impl NativeRecord {
    /// Build a record from a stable identity and an arbitrary field map.
    ///
    /// Any `id` member of `fields` is dropped in favour of `id`.
    /// The identity is checked before this call; document validation checks uniqueness.
    /// A field nested past [`MAX_NATIVE_NESTING_DEPTH`] is refused by name:
    /// the map is the caller's own, so nothing about it is bounded until it
    /// is measured here.
    pub fn new(
        id: crate::ids::Identity,
        mut fields: Map<String, Value>,
    ) -> Result<Self, NativeConvertError> {
        fields.remove("id");
        for (field, value) in &fields {
            if nests_past(value, MAX_NATIVE_NESTING_DEPTH) {
                return Err(NativeConvertError::FieldNestsTooDeep {
                    id,
                    field: field.clone(),
                });
            }
        }
        Ok(Self { id, fields })
    }

    /// Build a record from an admitted identity and fields that do not nest.
    ///
    /// A `NativeField` states its own shape, so this path has nothing to
    /// measure and no failing branch. An `id` entry is dropped in favour of
    /// `id`, and a repeated name keeps the last entry.
    pub fn from_identity(
        id: impl Into<crate::ids::Identity>,
        fields: impl IntoIterator<Item = (String, NativeField)>,
    ) -> Self {
        let id = id.into();
        let mut stored = Map::new();
        for (name, value) in fields {
            if name != "id" {
                stored.insert(name, value.into_value());
            }
        }
        Self { id, fields: stored }
    }

    /// Build a record by serializing one codec-owned typed record.
    ///
    /// The canonical serializer admits what the plain value serializer does
    /// not: a NaN or infinite number is refused by its member path, object
    /// keys must be distinct, and a `RawValue` payload is read through
    /// one-container replay rather than a recursion-limited parse.
    pub(crate) fn from_typed<T: Serialize>(record: &T) -> Result<Self, NativeConvertError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )?;
        Self::from_typed_for_decode(&ctx, record, None)
    }

    fn from_typed_for_decode<T: Serialize>(
        ctx: &DecodeContext<'_>,
        record: &T,
        sink: Option<&dyn canon::ByteSink>,
    ) -> Result<Self, NativeConvertError> {
        let serialized = record.serialize(canon::CanonValue::for_record_with_sink(ctx, sink));
        ctx.charge_work(0, "construct canonical native value")?;
        let serialized = serialized.map_err(|error| error.into_native(ctx))?;
        let canon::Node::Object(mut fields) = serialized else {
            return Err(NativeConvertError::NonObject);
        };
        let Some(Value::String(id)) = fields.remove("id") else {
            return Err(NativeConvertError::MissingId);
        };
        Ok(Self {
            id: crate::ids::Identity::new(id)?,
            fields,
        })
    }

    /// Globally unique record identity.
    #[must_use]
    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    /// The codec-owned fields, excluding `id`.
    ///
    /// This clones the stored map; read it once and reuse it when inspecting
    /// more than one field, and prefer [`field`](Self::field) when one field is
    /// all that is wanted.
    #[must_use]
    pub fn fields(&self) -> Map<String, Value> {
        self.fields.clone()
    }

    /// Clone codec-owned fields after admitting the value tree and its bytes.
    pub fn fields_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Map<String, Value>, NativeConvertError> {
        let Value::Object(fields) = copy_value_for_decode(ctx, &self.fields)? else {
            return Err(NativeConvertError::NonObject);
        };
        Ok(fields)
    }

    /// One codec-owned field.
    ///
    /// `id` is not a codec-owned field and is never answered here.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<Value> {
        self.fields.get(name).cloned()
    }

    fn to_typed<T: DeserializeOwned>(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<T, NativeConvertError> {
        let record = copy_value_for_decode(ctx, self)?;
        #[cfg(test)]
        TYPED_RECORD_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        match serde_json::from_value(record) {
            Ok(value) => Ok(value),
            Err(source) => Err(NativeConvertError::ReadRecord {
                id: self
                    .id
                    .try_clone_for_decode(ctx, "retain native record error identity")?,
                source,
            }),
        }
    }
}

/// Copies a value through the canonical serializer's charged allocations.
fn copy_value_for_decode<T: Serialize + ?Sized>(
    ctx: &DecodeContext<'_>,
    value: &T,
) -> Result<Value, NativeConvertError> {
    let copied = value.serialize(canon::CanonValue::for_record_with_sink(ctx, None));
    ctx.charge_work(0, "construct canonical native value")?;
    Ok(copied.map_err(|error| error.into_native(ctx))?.into_value())
}

/// Adds an arena name to a semantic typed-record refusal.
fn read_record<T: DeserializeOwned>(
    ctx: &DecodeContext<'_>,
    arena: &str,
    record: &NativeRecord,
) -> Result<T, NativeConvertError> {
    match record.to_typed(ctx) {
        Ok(value) => Ok(value),
        Err(source) if source.resource_limit().is_some() => Err(source),
        Err(source) => {
            let arena = ctx.copy_retained_text(arena, "retain native arena error name")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<NativeConvertError>()),
                "retain native arena error",
            )?;
            Err(NativeConvertError::Arena {
                arena,
                source: Box::new(source),
            })
        }
    }
}

impl Serialize for NativeRecord {
    /// Writes the record member for member through `serializer`, so it honours
    /// the caller's formatting: `to_string_pretty` must indent a native record
    /// the same way it indents every other document entity. It writes the
    /// constructors' order: `id` first, then the codec-owned fields by key.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.fields.len() + 1))?;
        serde::ser::SerializeMap::serialize_entry(&mut map, "id", self.id.as_str())?;
        for (key, value) in &self.fields {
            serde::ser::SerializeMap::serialize_entry(&mut map, key, value)?;
        }
        serde::ser::SerializeMap::end(map)
    }
}

impl<'de> Deserialize<'de> for NativeRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = cadmpeg_core::distinct_keys::json_object(deserializer)?;
        let Some(Value::String(id)) = fields.remove("id") else {
            return Err(<D::Error as serde::de::Error>::custom(
                NativeConvertError::MissingId,
            ));
        };
        Self::new(
            crate::ids::Identity::new(id).map_err(serde::de::Error::custom)?,
            fields,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for NativeRecord {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "NativeRecord".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::NativeRecord").into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        RecordShape::json_schema(generator)
    }
}

/// Serialize codec-owned typed records into one arena in canonical record order.
///
/// The arena is returned rather than stored, so a caller that only needs the
/// canonical records never has to read them back out of a namespace. Each
/// record is converted before the next is read, and a record the source could
/// not state stops the walk with that record's own error. Equal identities can
/// occur before document validation, so ordering must preserve their input
/// order. A fallibly reserved index permutation orders records by identity and
/// original ordinal. Its storage is admitted against the caller's temporary-byte
/// budget before sorting.
pub fn arena_from<T, E, I>(ctx: &DecodeContext<'_>, records: I) -> Result<Vec<NativeRecord>, E>
where
    T: Serialize,
    E: From<NativeConvertError>,
    I: IntoIterator<Item = Result<T, E>>,
{
    let mut converted = Vec::new();
    for (ordinal, record) in records.into_iter().enumerate() {
        let record = record?;
        ctx.reserve_retained_vec(&mut converted, 1, "store native record")
            .map_err(|error| E::from(NativeConvertError::Resource(error)))?;
        let writer = RefCell::new(ChargingJsonWriter { ctx, refusal: None });
        let sink = |bytes: &[u8]| writer.borrow_mut().write_all(bytes);
        let result = NativeRecord::from_typed_for_decode(ctx, &record, Some(&sink));
        let record = match result {
            Ok(record) => record,
            Err(error) => {
                let source = writer
                    .borrow_mut()
                    .refusal
                    .take()
                    .map_or(error, NativeConvertError::Resource);
                if source.resource_limit().is_some() {
                    return Err(E::from(source));
                }
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<NativeConvertError>()),
                    "retain native write error",
                )
                .map_err(|error| E::from(NativeConvertError::Resource(error)))?;
                return Err(E::from(NativeConvertError::WriteRecord {
                    ordinal,
                    source: Box::new(source),
                }));
            }
        };
        converted.push(record);
    }
    let scratch_bytes = converted
        .len()
        .checked_mul(std::mem::size_of::<usize>())
        .map(cadmpeg_core::decode::u64_from_index)
        .ok_or_else(|| {
            E::from(NativeConvertError::Resource(ctx.refuse_codec_limit(
                "sort native records scratch size",
                u64::MAX - 1,
                u64::MAX,
            )))
        })?;
    let _sort_scratch = ctx
        .reserve_scoped(scratch_bytes, "sort native records")
        .map_err(|error| E::from(NativeConvertError::Resource(error)))?;
    let operation = "sort native records";
    let count = u64::try_from(converted.len()).map_err(|_| {
        E::from(NativeConvertError::Resource(ctx.refuse_codec_limit(
            operation,
            u64::MAX - 1,
            u64::MAX,
        )))
    })?;
    ctx.charge_collection_items(count, operation)
        .map_err(|error| E::from(NativeConvertError::Resource(error)))?;
    let mut order = Vec::new();
    order.try_reserve_exact(converted.len()).map_err(|_| {
        E::from(NativeConvertError::Resource(ctx.refuse_codec_limit(
            operation,
            u64::MAX - 1,
            u64::MAX,
        )))
    })?;
    order.extend(0..converted.len());
    ctx.sort_unstable_by(
        &mut order,
        |left, right| {
            converted[*left]
                .id()
                .cmp(converted[*right].id())
                .then_with(|| left.cmp(right))
        },
        |index| converted[*index].id().len(),
        operation,
    )
    .map_err(|error| E::from(NativeConvertError::Resource(error)))?;
    for start in 0..order.len() {
        let mut cursor = start;
        while order[cursor] != start {
            let next = order[cursor];
            converted.swap(cursor, next);
            order[cursor] = cursor;
            cursor = next;
        }
        order[cursor] = cursor;
    }
    Ok(converted)
}

/// Source-format arena collection.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct NativeNamespace {
    /// Record arenas keyed by stable arena name.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    arenas: BTreeMap<String, Vec<NativeRecord>>,
}

impl NativeNamespace {
    /// Admit codec-owned aggregate state from this raw namespace.
    pub fn admit<'a, T>(&'a self) -> Result<T, NativeConvertError>
    where
        T: TryFrom<&'a Self, Error = NativeConvertError>,
    {
        T::try_from(self)
    }

    /// Return the record arenas keyed by stable arena name.
    #[must_use]
    pub fn arenas(&self) -> &BTreeMap<String, Vec<NativeRecord>> {
        &self.arenas
    }

    /// Return the record arenas for modification.
    pub fn arenas_mut(&mut self) -> &mut BTreeMap<String, Vec<NativeRecord>> {
        &mut self.arenas
    }

    /// Replace an arena by serializing codec-owned typed records.
    pub fn set_arena<T: Serialize>(
        &mut self,
        ctx: &DecodeContext<'_>,
        name: impl AsRef<str>,
        records: &[T],
    ) -> Result<(), NativeConvertError> {
        self.set_arena_from(ctx, name, records.iter())
    }

    /// Replace an arena by serializing codec-owned typed records one at a time.
    ///
    /// Codecs whose typed records must be reshaped before storage should build
    /// the reshaped record inside the iterator rather than collecting a full
    /// second copy of the population first.
    pub fn set_arena_from<T: Serialize, I: IntoIterator<Item = T>>(
        &mut self,
        ctx: &DecodeContext<'_>,
        name: impl AsRef<str>,
        records: I,
    ) -> Result<(), NativeConvertError> {
        let name = name.as_ref();
        let name = ctx.copy_retained_text(name, "retain native arena name")?;
        let converted = match arena_from(ctx, records.into_iter().map(Ok::<T, NativeConvertError>))
        {
            Ok(converted) => converted,
            Err(source) if source.resource_limit().is_some() => return Err(source),
            Err(source) => {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<NativeConvertError>()),
                    "retain native arena error",
                )?;
                return Err(NativeConvertError::Arena {
                    arena: name,
                    source: Box::new(source),
                });
            }
        };
        ctx.insert_btree_map(&mut self.arenas, name, converted, "store native arena")?;
        Ok(())
    }

    /// Admit an arena through a codec-owned collection constructor.
    pub fn arena_as_collection<T, C>(&self, name: &str) -> Result<C, NativeConvertError>
    where
        T: DeserializeOwned,
        C: TryFrom<Vec<T>, Error = NativeConvertError>,
    {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        self.arena_as_collection_for_decode(&ctx, name)
    }

    /// Admits a typed collection using the caller's decode budget.
    pub fn arena_as_collection_for_decode<T, C>(
        &self,
        ctx: &DecodeContext<'_>,
        name: &str,
    ) -> Result<C, NativeConvertError>
    where
        T: DeserializeOwned,
        C: TryFrom<Vec<T>, Error = NativeConvertError>,
    {
        C::try_from(self.arena_as_for_decode(ctx, name)?)
    }

    /// Deserialize an arena into codec-owned typed records.
    pub fn arena_as<T: DeserializeOwned>(&self, name: &str) -> Result<Vec<T>, NativeConvertError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        self.arena_as_for_decode(&ctx, name)
    }

    /// Deserialize one arena with admission before each retained typed copy.
    pub fn arena_as_for_decode<T: DeserializeOwned>(
        &self,
        ctx: &DecodeContext<'_>,
        name: &str,
    ) -> Result<Vec<T>, NativeConvertError> {
        let mut typed = Vec::new();
        if let Some((arena, records)) = self.arenas.get_key_value(name) {
            for record in records {
                ctx.reserve_retained_vec(&mut typed, 1, "load typed native record")?;
                typed.push(read_record(ctx, arena, record)?);
            }
        }
        Ok(typed)
    }

    /// Deserialize an arena into codec-owned typed records one at a time.
    ///
    /// A record is parsed only when the iterator is advanced onto it, so a
    /// caller that consumes and releases each one — reshaping an arena, or
    /// reducing it for hashing — never holds the typed population alongside
    /// whatever it produces from it.
    pub fn arena_iter_as<'a, T: DeserializeOwned + 'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = Result<T, NativeConvertError>> + 'a {
        self.arenas
            .get_key_value(name)
            .into_iter()
            .flat_map(|(name, records)| {
                records.iter().map(move |record| {
                    let arena = cadmpeg_core::decode::DecodeArena::new();
                    let policy = cadmpeg_core::decode::DecodePolicy::default();
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    read_record(&ctx, name, record)
                })
            })
    }

    /// Reads typed records lazily using the caller's decode budget.
    pub fn arena_iter_as_for_decode<'a, T: DeserializeOwned + 'a>(
        &'a self,
        ctx: &'a DecodeContext<'_>,
        name: &str,
    ) -> impl Iterator<Item = Result<T, NativeConvertError>> + 'a {
        self.arenas
            .get_key_value(name)
            .into_iter()
            .flat_map(move |(arena, records)| {
                records
                    .iter()
                    .map(move |record| read_record(ctx, arena, record))
            })
    }
}

/// Native records grouped by source-format namespace id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct Native(
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub  BTreeMap<String, NativeNamespace>,
);

impl Native {
    /// Return a source-format namespace.
    pub fn namespace(&self, format: &str) -> Option<&NativeNamespace> {
        self.0.get(format)
    }

    /// Return or create a source-format namespace.
    pub fn namespace_mut(&mut self, format: impl Into<String>) -> &mut NativeNamespace {
        self.0.entry(format.into()).or_default()
    }

    /// Sort every arena into canonical identity order.
    pub(crate) fn finalize(&mut self, ctx: &DecodeContext<'_>) -> Result<(), cadmpeg_core::CodecError> {
        for namespace in self.0.values_mut() {
            for records in namespace.arenas.values_mut() {
                ctx.stable_sort_by(records, |left, right| left.id().cmp(right.id()), |record| record.id().len(), "finalize native arena")?;
            }
        }
        Ok(())
    }

    /// Return one count for each non-empty native arena.
    pub fn loss_counts(&self) -> Vec<LossCount> {
        self.0
            .iter()
            .flat_map(|(format, namespace)| {
                namespace.arenas.iter().filter_map(move |(kind, records)| {
                    let count = NonZeroUsize::new(records.len())?;
                    Some(LossCount {
                        format: format.clone(),
                        kind: kind.clone(),
                        count,
                    })
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
