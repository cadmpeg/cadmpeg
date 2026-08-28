// SPDX-License-Identifier: Apache-2.0
//! Schema-governed `RSe` database, registry, and revision tables.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RseSchema(u32);

impl RseSchema {
    pub(crate) const SCHEMA_31: Self = Self(31);

    pub(crate) const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VersionTuple {
    pub(crate) revision: u8,
    pub(crate) minor: u8,
    pub(crate) major: u8,
    pub(crate) state: [u8; 5],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RseDatabase {
    pub(crate) id: [u8; 16],
    pub(crate) schema: RseSchema,
    pub(crate) created_by: VersionTuple,
    pub(crate) created_filetime: u64,
    pub(crate) saved_by: VersionTuple,
    pub(crate) saved_filetime: u64,
    pub(crate) note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentObject {
    pub(crate) revision_id: [u8; 16],
    pub(crate) state: [u8; 9],
    pub(crate) segment_id: [u8; 16],
    pub(crate) value: u32,
    pub(crate) node_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentNode {
    pub(crate) index: u32,
    pub(crate) segment_list_indexes: [i16; 2],
    pub(crate) values: [u16; 6],
    pub(crate) number: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentRegistryEntry {
    pub(crate) display_name: String,
    pub(crate) segment_id: [u8; 16],
    pub(crate) revision_id: [u8; 16],
    pub(crate) value: u32,
    pub(crate) state: [u32; 5],
    pub(crate) secondary_count: u32,
    pub(crate) type_name: String,
    pub(crate) type_state: [u32; 2],
    pub(crate) version: VersionTuple,
    pub(crate) trailing_value: u32,
    pub(crate) objects: Vec<SegmentObject>,
    pub(crate) nodes: Vec<SegmentNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentRegistry {
    pub(crate) entries: Vec<SegmentRegistryEntry>,
    pub(crate) state: [u16; 2],
    pub(crate) primary_ids: Vec<[u8; 16]>,
    pub(crate) secondary_ids: Vec<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RevisionPayload {
    None,
    Short { enabled: bool, value: [u8; 8] },
    Long { enabled: bool, value: [u8; 16] },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RevisionEntry {
    pub(crate) id: [u8; 16],
    pub(crate) flags: u32,
    pub(crate) kind: u16,
    pub(crate) payload: RevisionPayload,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RevisionTable {
    pub(crate) version: u32,
    pub(crate) entries: Vec<RevisionEntry>,
}

/// The outcome of reading an `RSeDb` stream far enough to know its schema.
///
/// The schema is a version declaration, so it survives its own rejection: a
/// stream whose body the schema-31 grammar could not frame still tells the
/// dialect classifier what it declared. Folding that case into an error would
/// leave the declaration readable only by re-parsing the bytes at the report
/// boundary, which is exactly the drift this split exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DatabaseHeader {
    /// The body parsed under the schema-31 grammar. `schema` is what the stream
    /// declared, which is not always 31: a foreign schema whose body obeys the
    /// grammar is read, and the declaration it carries is what makes the
    /// document dialect-unverified.
    Supported(RseDatabase),
    /// A body the schema-31 grammar did not frame.
    Unframed {
        /// The schema the stream declared.
        schema: RseSchema,
        /// Where the substituted grammar stopped.
        detail: String,
    },
}

impl DatabaseHeader {
    /// The detail an unframed schema reports as a database issue.
    pub(crate) fn unframed_detail(schema: RseSchema, detail: &str) -> String {
        format!(
            "RSe database schema {} was read with the schema {} grammar, which did not frame it: \
             {detail}",
            schema.value(),
            RseSchema::SCHEMA_31.value()
        )
    }
}

pub(crate) fn parse_database(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<DatabaseHeader, CodecError> {
    let mut cursor = Cursor::new(bytes, "RSe database");
    let id = cursor.array("database id")?;
    let schema = RseSchema(cursor.u32("schema")?);
    // The schema is a declaration, never a gate (`docs/architecture.md`:
    // "Refusal is structural, never a version allowlist"). A foreign schema is
    // read with the schema-31 grammar, and only that attempt failing
    // structurally leaves the stream unavailable. The declaration still decides
    // the admission: `DialectRecovery` keys the unverified state and its charge
    // on what the stream said, not on what the parse managed.
    match schema_31_body(ctx, cursor, id, schema) {
        Ok(database) => Ok(DatabaseHeader::Supported(database)),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => Ok(DatabaseHeader::Unframed {
            schema,
            detail: error.to_string(),
        }),
    }
}

/// The `RSeDb` body as schema 31 declares it, from a cursor positioned after
/// the id and schema words.
fn schema_31_body(
    ctx: &DecodeContext<'_>,
    mut cursor: Cursor<'_>,
    id: [u8; 16],
    schema: RseSchema,
) -> Result<RseDatabase, CodecError> {
    let database = RseDatabase {
        id,
        schema,
        created_by: cursor.version("creation version")?,
        created_filetime: cursor.u64("creation FILETIME")?,
        saved_by: cursor.version("save version")?,
        saved_filetime: cursor.u64("save FILETIME")?,
        note: cursor.utf16("database note", 65_536)?,
    };
    cursor.finish()?;
    ctx.charge_collection_items(1, "admit Inventor RSe database")?;
    Ok(database)
}

/// Reads the segment registry with the schema-31 grammar.
///
/// The grammar is applied to every stream, whatever schema the `RSeDb` streams
/// declared: the registry is read, or it fails structurally and the caller
/// degrades it to [`ParsedState::Unavailable`]. Declining to try on a foreign
/// schema was a version allowlist, and it also made the document's
/// dialect-unverified message claim a grammar that had never been applied.
///
/// [`ParsedState::Unavailable`]: crate::rse::ParsedState::Unavailable
pub(crate) fn parse_registry(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<SegmentRegistry, CodecError> {
    let mut cursor = Cursor::new(bytes, "RSe segment registry");
    let count = cursor.count("segment count", 65_536)?;
    ctx.charge_collection_items(count as u64, "admit Inventor segment registry entries")?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let display_name = cursor.utf16("segment display name", 4_096)?;
        let segment_id = cursor.array("segment id")?;
        let revision_id = cursor.array("segment revision id")?;
        let value = cursor.u32("segment value")?;
        let object_count = cursor.count("segment object count", 1_000_000)?;
        let state = cursor.u32_array("segment state")?;
        let secondary_count = cursor.u32("segment secondary count")?;
        let type_name = cursor.utf16("segment type name", 4_096)?;
        let type_state = cursor.u32_array("segment type state")?;
        let version = cursor.version("segment version")?;
        let trailing_value = cursor.u32("segment trailing value")?;
        ctx.charge_collection_items(
            object_count as u64,
            "admit Inventor segment registry objects",
        )?;
        let mut objects = Vec::with_capacity(object_count);
        let mut node_count = None;
        for _ in 0..object_count {
            let object = SegmentObject {
                revision_id: cursor.array("object revision id")?,
                state: cursor.array("object state")?,
                segment_id: cursor.array("object segment id")?,
                value: cursor.u32("object value")?,
                node_count: cursor.u32("object node count")?,
            };
            node_count = Some(object.node_count);
            objects.push(object);
        }
        let node_count = node_count
            .unwrap_or(1)
            .checked_sub(1)
            .ok_or_else(|| CodecError::Malformed("RSe segment node count is zero".into()))?;
        let node_count = usize::try_from(node_count)
            .map_err(|_| CodecError::Malformed("RSe segment node count is too large".into()))?;
        if node_count > 1_000_000 {
            return Err(CodecError::Malformed(
                "RSe segment node count exceeds 1000000".into(),
            ));
        }
        ctx.charge_collection_items(node_count as u64, "admit Inventor segment registry nodes")?;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            nodes.push(SegmentNode {
                index: cursor.u32("node index")?,
                segment_list_indexes: [
                    cursor.i16("node segment-list index")?,
                    cursor.i16("node segment-list index")?,
                ],
                values: cursor.u16_array("node values")?,
                number: cursor.u16("node number")?,
            });
        }
        entries.push(SegmentRegistryEntry {
            display_name,
            segment_id,
            revision_id,
            value,
            state,
            secondary_count,
            type_name,
            type_state,
            version,
            trailing_value,
            objects,
            nodes,
        });
    }
    let state = cursor.u16_array("registry state")?;
    let primary_ids = cursor.id_list(ctx, "primary registry ids")?;
    let secondary_ids = cursor.id_list(ctx, "secondary registry ids")?;
    cursor.finish()?;
    Ok(SegmentRegistry {
        entries,
        state,
        primary_ids,
        secondary_ids,
    })
}

pub(crate) fn parse_revisions(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<RevisionTable, CodecError> {
    let mut cursor = Cursor::new(bytes, "RSe revision table");
    // The version word is evidence, kept on the table, and not a gate: the
    // version-3 grammar is attempted at any declared version, and a table that
    // does not obey it fails structurally at the cursor.
    let version = cursor.u32("version")?;
    let count = cursor.count("revision count", 1_000_000)?;
    ctx.charge_collection_items(count as u64, "admit Inventor revision entries")?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let id = cursor.array("revision id")?;
        let flags = cursor.u32("revision flags")?;
        let kind = cursor.u16("revision kind")?;
        let payload = if kind == u16::MAX {
            let enabled = cursor.u8("revision payload selector")? != 0;
            if enabled {
                RevisionPayload::Short {
                    enabled,
                    value: cursor.array("short revision payload")?,
                }
            } else {
                RevisionPayload::Long {
                    enabled,
                    value: cursor.array("long revision payload")?,
                }
            }
        } else {
            RevisionPayload::None
        };
        entries.push(RevisionEntry {
            id,
            flags,
            kind,
            payload,
        });
    }
    cursor.finish()?;
    Ok(RevisionTable { version, entries })
}

struct Cursor<'a> {
    source: View<'a>,
    scope: &'static str,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], scope: &'static str) -> Self {
        Self {
            source: View::over_retained(bytes),
            scope,
        }
    }

    #[allow(dead_code)] // Retained for framed RSe walks that still use the helper.
    fn take(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], CodecError> {
        Ok(self
            .source
            .req_take(len)
            .map_err(|error| error.during(field))?)
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, CodecError> {
        Ok(self.source.req_u8().map_err(|error| error.during(field))?)
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, CodecError> {
        Ok(self
            .source
            .req_u16_le()
            .map_err(|error| error.during(field))?)
    }

    fn i16(&mut self, field: &'static str) -> Result<i16, CodecError> {
        Ok(self
            .source
            .req_i16_le()
            .map_err(|error| error.during(field))?)
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, CodecError> {
        Ok(self
            .source
            .req_u32_le()
            .map_err(|error| error.during(field))?)
    }

    fn u64(&mut self, field: &'static str) -> Result<u64, CodecError> {
        Ok(self
            .source
            .req_u64_le()
            .map_err(|error| error.during(field))?)
    }

    fn array<const N: usize>(&mut self, field: &'static str) -> Result<[u8; N], CodecError> {
        self.source
            .array()
            .ok_or_else(|| CodecError::malformed(format_args!("truncated {} {field}", self.scope)))
    }

    fn u16_array<const N: usize>(&mut self, field: &'static str) -> Result<[u16; N], CodecError> {
        let mut values = [0; N];
        for value in &mut values {
            *value = self.u16(field)?;
        }
        Ok(values)
    }

    fn u32_array<const N: usize>(&mut self, field: &'static str) -> Result<[u32; N], CodecError> {
        let mut values = [0; N];
        for value in &mut values {
            *value = self.u32(field)?;
        }
        Ok(values)
    }

    fn version(&mut self, field: &'static str) -> Result<VersionTuple, CodecError> {
        Ok(VersionTuple {
            revision: self.u8(field)?,
            minor: self.u8(field)?,
            major: self.u8(field)?,
            state: self.array(field)?,
        })
    }

    fn count(&mut self, field: &'static str, maximum: usize) -> Result<usize, CodecError> {
        let count = usize::try_from(self.u32(field)?).map_err(|_| {
            CodecError::malformed(format_args!("{} {field} is too large", self.scope))
        })?;
        if count > maximum {
            return Err(CodecError::malformed(format_args!(
                "{} {field} exceeds {maximum}",
                self.scope
            )));
        }
        Ok(count)
    }

    fn utf16(&mut self, field: &'static str, maximum: usize) -> Result<String, CodecError> {
        let count = self.count(field, maximum)?;
        count.checked_mul(2).ok_or_else(|| {
            CodecError::malformed(format_args!("{} {field} length overflows", self.scope))
        })?;
        self.source.utf16_le(count).ok_or_else(|| {
            CodecError::malformed(format_args!("{} {field} is not UTF-16", self.scope))
        })
    }

    fn id_list(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
    ) -> Result<Vec<[u8; 16]>, CodecError> {
        let count = self.count(field, 1_000_000)?;
        ctx.charge_collection_items(count as u64, "admit Inventor registry identifier list")?;
        let mut ids = Vec::with_capacity(count);
        for _ in 0..count {
            ids.push(self.array(field)?);
        }
        Ok(ids)
    }

    fn finish(self) -> Result<(), CodecError> {
        if self.source.is_empty() {
            Ok(())
        } else {
            Err(CodecError::malformed(format_args!(
                "{} has {} trailing bytes",
                self.scope,
                self.source.remaining()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    use super::*;

    #[test]
    fn schema_31_database_reports_failed_exact_exhaustion_as_unframed() {
        let mut bytes = database_fixture();
        with_context(&bytes, |ctx| {
            let DatabaseHeader::Supported(database) =
                parse_database(ctx, &bytes).expect("schema 31 database parses")
            else {
                panic!("the fixture declares schema 31");
            };
            assert_eq!(database.schema.value(), 31);
            assert_eq!(database.created_by.major, 24);
            assert_eq!(database.saved_by.major, 25);
            assert_eq!(database.note, "synthetic database");
        });
        bytes.push(0);
        with_context(&bytes, |ctx| {
            let DatabaseHeader::Unframed { schema, detail } =
                parse_database(ctx, &bytes).expect("schema declaration survives trailing bytes")
            else {
                panic!("trailing bytes cannot frame");
            };
            assert_eq!(schema, RseSchema::SCHEMA_31);
            assert!(detail.contains("trailing bytes"), "{detail}");
        });
    }

    /// A foreign schema is read with the schema-31 grammar, and the declaration
    /// survives so the dialect classifier reads what the stream said.
    #[test]
    fn a_foreign_schema_is_read_with_the_schema_31_grammar() {
        let mut bytes = database_fixture();
        bytes[16..20].copy_from_slice(&12_u32.to_le_bytes());
        with_context(&bytes, |ctx| {
            let DatabaseHeader::Supported(database) =
                parse_database(ctx, &bytes).expect("a foreign schema is not an error")
            else {
                panic!("the schema-31 grammar frames this body");
            };
            // Read, and still labelled: the declaration is what the dialect
            // classifier keys the unverified admission on.
            assert_eq!(database.schema, RseSchema(12));
            assert_eq!(database.note, "synthetic database");
        });
    }

    /// A foreign schema whose body the substituted grammar cannot frame keeps
    /// its declaration and reports where the attempt stopped.
    #[test]
    fn a_foreign_schema_that_does_not_frame_reports_the_attempt() {
        let mut bytes = database_fixture();
        bytes[16..20].copy_from_slice(&12_u32.to_le_bytes());
        bytes.truncate(28);
        with_context(&bytes, |ctx| {
            let header = parse_database(ctx, &bytes).expect("a foreign schema is not an error");
            let DatabaseHeader::Unframed { schema, detail } = header else {
                panic!("a truncated body cannot frame");
            };
            assert_eq!(schema, RseSchema(12));
            let reported = DatabaseHeader::unframed_detail(schema, &detail);
            assert!(reported.contains("schema 12"), "{reported}");
            assert!(reported.contains("schema 31 grammar"), "{reported}");
        });
    }

    #[test]
    fn schema_31_that_does_not_frame_keeps_its_declaration() {
        let mut bytes = database_fixture();
        bytes.truncate(28);
        with_context(&bytes, |ctx| {
            let header = parse_database(ctx, &bytes).expect("schema 31 degrades after declaration");
            let DatabaseHeader::Unframed { schema, detail } = header else {
                panic!("a truncated body cannot frame");
            };
            assert_eq!(schema, RseSchema::SCHEMA_31);
            let reported = DatabaseHeader::unframed_detail(schema, &detail);
            assert!(reported.contains("schema 31"), "{reported}");
        });
    }

    #[test]
    fn schema_31_registry_uses_declared_object_and_node_counts() {
        let bytes = registry_fixture();
        with_context(&bytes, |ctx| {
            let registry = parse_registry(ctx, &bytes).expect("schema 31 registry parses");
            assert_eq!(registry.entries.len(), 1);
            assert_eq!(registry.entries[0].display_name, "PmBRepSegment");
            assert_eq!(registry.entries[0].objects.len(), 1);
            assert_eq!(registry.entries[0].nodes.len(), 1);
            assert_eq!(registry.primary_ids, vec![[0x61; 16]]);
        });
    }

    #[test]
    fn revision_table_frames_selector_payloads() {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 3);
        push_u32(&mut bytes, 2);
        bytes.extend_from_slice(&[0x11; 16]);
        push_u32(&mut bytes, 7);
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&[0x22; 16]);
        push_u32(&mut bytes, 8);
        bytes.extend_from_slice(&u16::MAX.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&[0x33; 8]);
        with_context(&bytes, |ctx| {
            let table = parse_revisions(ctx, &bytes).expect("revision table parses");
            assert_eq!(table.entries.len(), 2);
            assert!(matches!(
                table.entries[1].payload,
                RevisionPayload::Short { .. }
            ));
        });
    }

    fn with_context(bytes: &[u8], test: impl FnOnce(&DecodeContext<'_>)) {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic table fits policy");
        test(&ctx);
    }

    fn database_fixture() -> Vec<u8> {
        let mut bytes = vec![0x42; 16];
        push_u32(&mut bytes, 31);
        push_version(&mut bytes, 24);
        bytes.extend_from_slice(&17_u64.to_le_bytes());
        push_version(&mut bytes, 25);
        bytes.extend_from_slice(&18_u64.to_le_bytes());
        push_utf16(&mut bytes, "synthetic database");
        bytes
    }

    fn registry_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 1);
        push_utf16(&mut bytes, "PmBRepSegment");
        bytes.extend_from_slice(&[0x10; 16]);
        bytes.extend_from_slice(&[0x20; 16]);
        push_u32(&mut bytes, 3);
        push_u32(&mut bytes, 1);
        for value in 4..9 {
            push_u32(&mut bytes, value);
        }
        push_u32(&mut bytes, 9);
        push_utf16(&mut bytes, "PmBRepSegmentType");
        push_u32(&mut bytes, 10);
        push_u32(&mut bytes, 11);
        push_version(&mut bytes, 24);
        push_u32(&mut bytes, 12);
        bytes.extend_from_slice(&[0x20; 16]);
        bytes.extend_from_slice(&[0x30; 9]);
        bytes.extend_from_slice(&[0x10; 16]);
        push_u32(&mut bytes, 13);
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 14);
        bytes.extend_from_slice(&(-1_i16).to_le_bytes());
        bytes.extend_from_slice(&2_i16.to_le_bytes());
        for value in 15_u16..21 {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&21_u16.to_le_bytes());
        bytes.extend_from_slice(&22_u16.to_le_bytes());
        bytes.extend_from_slice(&23_u16.to_le_bytes());
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0x61; 16]);
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0x62; 16]);
        bytes
    }

    fn push_version(bytes: &mut Vec<u8>, major: u8) {
        bytes.extend_from_slice(&[1, 2, major, 4, 5, 6, 7, 8]);
    }

    fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        push_u32(bytes, units.len() as u32);
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
