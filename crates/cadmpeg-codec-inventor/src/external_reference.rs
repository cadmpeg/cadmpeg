// SPDX-License-Identifier: Apache-2.0
//! Bounded `UFRxDoc` document and external-reference inventory.

use cadmpeg_container::compound::{CompoundSnapshot, CompoundStreamId};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

#[derive(Debug)]
pub(crate) enum UfrxState<'a> {
    Absent,
    Parsed(UfrxDocument<'a>),
    Unsupported {
        stream: CompoundStreamId,
        schema: u16,
        section_versions: Vec<u16>,
        source: View<'a>,
        detail: String,
    },
    Malformed {
        stream: CompoundStreamId,
        detail: String,
    },
}

#[derive(Debug)]
pub(crate) struct UfrxDocument<'a> {
    pub(crate) stream: CompoundStreamId,
    pub(crate) schema: u16,
    pub(crate) section_versions: Vec<u16>,
    pub(crate) original_file_name: String,
    pub(crate) caption: String,
    pub(crate) representation: Option<UfrxRepresentationState>,
    pub(crate) model_states: Vec<UfrxModelState<'a>>,
    pub(crate) references: Vec<InventorExternalReference>,
    pub(crate) unparsed_tail: View<'a>,
}

#[derive(Debug)]
pub(crate) struct UfrxModelState<'a> {
    pub(crate) prefix: u8,
    pub(crate) name: String,
    pub(crate) state: [u16; 2],
    pub(crate) prefix_count: u32,
    pub(crate) parameters: Vec<UfrxModelStateParameter>,
    pub(crate) suffix: View<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxRepresentationState {
    pub(crate) prefix: u16,
    pub(crate) active_representation: String,
    pub(crate) active_representation_kind: String,
    pub(crate) secondary_active_lod_state: [u16; 2],
    pub(crate) active_model_state: String,
    pub(crate) active_model_state_state: [u16; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxModelStateParameter {
    pub(crate) name: String,
    pub(crate) tag: u8,
    pub(crate) kind: u16,
    pub(crate) state: u16,
    pub(crate) value: String,
    pub(crate) trailer: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InventorExternalReference {
    pub(crate) path: String,
    pub(crate) library_id: i32,
    pub(crate) library_name: String,
    pub(crate) display_name: String,
    pub(crate) state_groups: Vec<[u16; 3]>,
    pub(crate) state: [u16; 2],
    pub(crate) document_id: [u8; 16],
    pub(crate) database_id: [u8; 16],
    pub(crate) reference_id: u32,
    pub(crate) occurrence_count: u32,
    pub(crate) version: u32,
    pub(crate) flags: u32,
}

pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
) -> Result<UfrxState<'a>, CodecError> {
    let Some(stream) = snapshot.stream("UFRxDoc") else {
        return Ok(UfrxState::Absent);
    };
    let source = snapshot.open(ctx, stream)?;
    Ok(match parse_stream(ctx, source) {
        Ok(document) => UfrxState::Parsed(UfrxDocument {
            stream: stream.id(),
            schema: document.schema,
            section_versions: document.section_versions,
            original_file_name: document.original_file_name,
            caption: document.caption,
            representation: document.representation,
            model_states: document.model_states,
            references: document.references,
            unparsed_tail: document.unparsed_tail,
        }),
        Err(CodecError::NotImplemented(detail)) => {
            let (schema, section_versions) = parse_schema_table(ctx, source)?;
            UfrxState::Unsupported {
                stream: stream.id(),
                schema,
                section_versions,
                source,
                detail,
            }
        }
        Err(error) => {
            let (schema, section_versions) = parse_schema_table(ctx, source)?;
            if schema == 15 {
                UfrxState::Unsupported {
                    stream: stream.id(),
                    schema,
                    section_versions,
                    source,
                    detail: format!(
                        "UFRxDoc schema 15 header/model-state branch is not implemented: {error}"
                    ),
                }
            } else {
                UfrxState::Malformed {
                    stream: stream.id(),
                    detail: error.to_string(),
                }
            }
        }
    })
}

struct ParsedUfrx<'a> {
    schema: u16,
    section_versions: Vec<u16>,
    original_file_name: String,
    caption: String,
    representation: Option<UfrxRepresentationState>,
    model_states: Vec<UfrxModelState<'a>>,
    references: Vec<InventorExternalReference>,
    unparsed_tail: View<'a>,
}

fn parse_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
) -> Result<ParsedUfrx<'a>, CodecError> {
    let mut cursor = Cursor::new(source.window());
    let schema = cursor.u16("schema")?;
    if !(11..=15).contains(&schema) {
        return Err(CodecError::NotImplemented(format!(
            "UFRxDoc schema {schema} is not implemented"
        )));
    }
    let section_count = cursor.count16("section-version count", 256)?;
    if section_count < 5 {
        return Err(CodecError::Malformed(
            "UFRxDoc section-version table is too short".into(),
        ));
    }
    ctx.charge_collection_items(
        section_count as u64,
        "admit UFRxDoc section-version entries",
    )?;
    let mut section_versions = Vec::with_capacity(section_count);
    for _ in 0..section_count {
        section_versions.push(cursor.u16("section version")?);
    }
    cursor.take(8, "save version")?;
    cursor.take(8, "save FILETIME")?;
    cursor.take(8, "secondary version")?;
    cursor.take(8, "secondary FILETIME")?;
    cursor.utf16(ctx, "comment", 65_536)?;
    cursor.take(8, "creation version")?;
    cursor.take(8, "creation FILETIME")?;
    cursor.take(8, "origin version")?;
    cursor.take(8, "origin FILETIME")?;
    cursor.take(16, "database revision id")?;
    cursor.u32("header padding")?;
    cursor.take(16, "internal document id")?;
    let original_file_name = cursor.utf16(ctx, "original file name", 65_536)?;
    cursor.u16("part flags")?;

    let lod_toc_count = cursor.count32("LOD table count", 65_536)?;
    ctx.charge_collection_items(lod_toc_count as u64, "admit UFRxDoc LOD table entries")?;
    for _ in 0..lod_toc_count {
        cursor.u16("LOD value")?;
        cursor.u16("LOD value")?;
        cursor.utf16(ctx, "LOD name", 65_536)?;
        cursor.take(2, "LOD state")?;
    }

    let pair_count = cursor.count32("header pair count", 65_536)?;
    ctx.charge_collection_items(pair_count as u64, "admit UFRxDoc header pairs")?;
    for _ in 0..pair_count {
        cursor.utf16(ctx, "header pair key", 65_536)?;
        cursor.utf16(ctx, "header pair value", 65_536)?;
    }
    cursor.take(4, "active LOD state")?;
    let representation = if schema == 15 {
        Some(UfrxRepresentationState {
            prefix: cursor.u16("schema 15 representation prefix")?,
            active_representation: cursor.utf16(ctx, "active representation", 65_536)?,
            active_representation_kind: cursor.utf16(ctx, "active representation kind", 65_536)?,
            secondary_active_lod_state: [
                cursor.u16("secondary active LOD state")?,
                cursor.u16("secondary active LOD state")?,
            ],
            active_model_state: cursor.utf16(ctx, "active model state", 65_536)?,
            active_model_state_state: [
                cursor.u16("active model-state state")?,
                cursor.u16("active model-state state")?,
            ],
        })
    } else {
        if section_versions[2] >= 12 {
            cursor.utf16(ctx, "active design view", 65_536)?;
        }
        if section_versions[2] >= 7 {
            cursor.take(4, "secondary active LOD state")?;
        }
        cursor.u16("header version flags")?;
        None
    };
    cursor.u32("highest occurrence id")?;
    cursor.u16("next LOD id")?;
    let invariant = cursor.u16("header invariant")?;
    if invariant != 1 {
        return Err(CodecError::Malformed(format!(
            "UFRxDoc header invariant is {invariant}, expected 1"
        )));
    }
    cursor.u32("highest file-reference id")?;
    cursor.u32("highest embedded-reference id")?;
    if section_versions[2] >= 13 {
        cursor.take(32, "document subtype ids")?;
    }

    let lod_count = cursor.count32("LOD count", 65_536)?;
    let model_states = if schema == 15 {
        parse_model_states(ctx, source, &mut cursor, lod_count)?
    } else if lod_count != 0 {
        return Err(CodecError::NotImplemented(format!(
            "UFRxDoc contains {lod_count} unframed LOD records"
        )));
    } else {
        Vec::new()
    };

    let reference_count = cursor.count32("external-reference count", 1_000_000)?;
    let caption = cursor.utf16(ctx, "external-reference caption", 65_536)?;
    cursor.u32("external-reference header state")?;
    ctx.charge_collection_items(reference_count as u64, "admit Inventor external references")?;
    let mut references = Vec::with_capacity(reference_count);
    for _ in 0..reference_count {
        let path = cursor.utf16(ctx, "external path", 65_536)?;
        let library_id = cursor.i32("library id")?;
        let library_name = cursor.utf16(ctx, "library name", 65_536)?;
        cursor.u16("reference state")?;
        let display_prefix = cursor.peek_u32("reference display-name prefix")?;
        if display_prefix & 0xffff_0000 != 0 && cursor.u16("reference display-name padding")? != 0 {
            return Err(CodecError::Malformed(
                "UFRxDoc reference display-name padding is nonzero".into(),
            ));
        }
        let display_name = cursor.utf16(ctx, "reference display name", 65_536)?;
        let state_count = cursor.count32("reference state-group count", 65_536)?;
        ctx.charge_collection_items(
            state_count as u64,
            "admit Inventor external-reference state groups",
        )?;
        let mut state_groups = Vec::with_capacity(state_count);
        for _ in 0..state_count {
            state_groups.push([
                cursor.u16("reference state group")?,
                cursor.u16("reference state group")?,
                cursor.u16("reference state group")?,
            ]);
        }
        let state = [
            cursor.u16("reference state")?,
            cursor.u16("reference state")?,
        ];
        let document_id = cursor.array("referenced document id")?;
        let database_id = cursor.array("referenced database id")?;
        references.push(InventorExternalReference {
            path,
            library_id,
            library_name,
            display_name,
            state_groups,
            state,
            document_id,
            database_id,
            reference_id: cursor.u32("reference id")?,
            occurrence_count: cursor.u32("reference occurrence count")?,
            version: cursor.u32("reference version")?,
            flags: cursor.u32("reference flags")?,
        });
    }
    if section_versions[4] >= 2 && cursor.u8("external-reference terminator")? != 0 {
        return Err(CodecError::Malformed(
            "UFRxDoc external-reference terminator is nonzero".into(),
        ));
    }
    let unparsed_tail = source
        .child(source.start() + cursor.position, source.end())
        .ok_or_else(|| CodecError::Malformed("UFRxDoc tail range is invalid".into()))?;
    Ok(ParsedUfrx {
        schema,
        section_versions,
        original_file_name,
        caption,
        representation,
        model_states,
        references,
        unparsed_tail,
    })
}

fn parse_model_states<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    cursor: &mut Cursor<'_>,
    count: usize,
) -> Result<Vec<UfrxModelState<'a>>, CodecError> {
    ctx.charge_collection_items(count as u64, "admit UFRxDoc model states")?;
    let mut states = Vec::with_capacity(count);
    for _ in 0..count {
        let prefix = cursor.u8("model-state prefix")?;
        let name = cursor.utf16(ctx, "model-state name", 65_536)?;
        let state = [
            cursor.u16("model-state state")?,
            cursor.u16("model-state state")?,
        ];
        let prefix_count = cursor.u32("model-state prefix count")?;
        let parameter_count = cursor.count32("model-state parameter count", 1_000_000)?;
        ctx.charge_collection_items(
            parameter_count as u64,
            "admit UFRxDoc model-state parameters",
        )?;
        let mut parameters = Vec::with_capacity(parameter_count);
        for _ in 0..parameter_count {
            parameters.push(UfrxModelStateParameter {
                name: cursor.utf16(ctx, "model-state parameter name", 65_536)?,
                tag: cursor.u8("model-state parameter tag")?,
                kind: cursor.u16("model-state parameter kind")?,
                state: cursor.u16("model-state parameter state")?,
                value: cursor.utf16(ctx, "model-state parameter value", 65_536)?,
                trailer: cursor.u16("model-state parameter trailer")?,
            });
        }
        let suffix_start = cursor.position;
        cursor.take(77, "model-state suffix")?;
        let suffix = source
            .child(
                source.start() + suffix_start,
                source.start() + cursor.position,
            )
            .ok_or_else(|| {
                CodecError::Malformed("UFRxDoc model-state suffix range is invalid".into())
            })?;
        states.push(UfrxModelState {
            prefix,
            name,
            state,
            prefix_count,
            parameters,
            suffix,
        });
    }
    Ok(states)
}

fn parse_schema_table(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
) -> Result<(u16, Vec<u16>), CodecError> {
    let mut cursor = Cursor::new(source.window());
    let schema = cursor.u16("schema")?;
    let section_count = cursor.count16("section-version count", 256)?;
    ctx.charge_collection_items(
        section_count as u64,
        "admit UFRxDoc section-version entries",
    )?;
    let mut section_versions = Vec::with_capacity(section_count);
    for _ in 0..section_count {
        section_versions.push(cursor.u16("section version")?);
    }
    Ok((schema, section_versions))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, len: usize, field: &str) -> Result<&'a [u8], CodecError> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(|| CodecError::Malformed(format!("UFRxDoc {field} range overflows")))?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| CodecError::Malformed(format!("truncated UFRxDoc {field}")))?;
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self, field: &str) -> Result<u8, CodecError> {
        Ok(self.take(1, field)?[0])
    }

    fn u16(&mut self, field: &str) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(self.array(field)?))
    }

    fn u32(&mut self, field: &str) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(self.array(field)?))
    }

    fn i32(&mut self, field: &str) -> Result<i32, CodecError> {
        Ok(i32::from_le_bytes(self.array(field)?))
    }

    fn array<const N: usize>(&mut self, field: &str) -> Result<[u8; N], CodecError> {
        Ok(self
            .take(N, field)?
            .try_into()
            .expect("cursor returned requested fixed length"))
    }

    fn peek_u32(&self, field: &str) -> Result<u32, CodecError> {
        let bytes = self
            .bytes
            .get(self.position..self.position.saturating_add(4))
            .ok_or_else(|| CodecError::Malformed(format!("truncated UFRxDoc {field}")))?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("four-byte peek"),
        ))
    }

    fn count16(&mut self, field: &str, maximum: usize) -> Result<usize, CodecError> {
        let value = self.u16(field)? as usize;
        if value > maximum {
            return Err(CodecError::Malformed(format!(
                "UFRxDoc {field} exceeds {maximum}"
            )));
        }
        Ok(value)
    }

    fn count32(&mut self, field: &str, maximum: usize) -> Result<usize, CodecError> {
        let offset = self.position;
        let value = usize::try_from(self.u32(field)?)
            .map_err(|_| CodecError::Malformed(format!("UFRxDoc {field} is too large")))?;
        if value > maximum {
            return Err(CodecError::Malformed(format!(
                "UFRxDoc {field} value {value} at offset {offset} exceeds {maximum}"
            )));
        }
        Ok(value)
    }

    fn utf16(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &str,
        maximum: usize,
    ) -> Result<String, CodecError> {
        let count = self.count32(field, maximum)?;
        let len = count
            .checked_mul(2)
            .ok_or_else(|| CodecError::Malformed(format!("UFRxDoc {field} length overflows")))?;
        ctx.charge_retained(len as u64, "retain UFRxDoc string", None)?;
        let units = self
            .take(len, field)?
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units)
            .map_err(|_| CodecError::Malformed(format!("UFRxDoc {field} is not UTF-16")))
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    use super::*;

    #[test]
    fn supported_schemas_frame_external_references_and_retain_the_tail() {
        for schema in 11..=15 {
            let (bytes, _) = fixture(schema);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic UFRxDoc fits policy");
            let document = parse_stream(&ctx, root)
                .unwrap_or_else(|error| panic!("synthetic UFRxDoc schema {schema}: {error}"));
            assert_eq!(document.schema, schema);
            assert_eq!(document.original_file_name, "synthetic.ipt");
            assert_eq!(document.references.len(), 1);
            assert_eq!(document.references[0].path, "relative/component.ipt");
            assert_eq!(document.references[0].occurrence_count, 2);
            if schema == 15 {
                let representation = document
                    .representation
                    .as_ref()
                    .expect("schema 15 representation state");
                assert_eq!(representation.active_model_state, "Master");
                assert_eq!(representation.active_representation, "Default");
                assert_eq!(representation.active_representation_kind, "DesignView");
                assert_eq!(document.model_states.len(), 1);
                assert_eq!(document.model_states[0].prefix, 0);
                assert_eq!(document.model_states[0].name, "Master");
                assert_eq!(document.model_states[0].parameters[0].name, "width");
                assert_eq!(document.model_states[0].parameters[0].value, "12.5");
                assert_eq!(document.model_states[0].suffix.window(), &[0x5a; 77]);
            } else {
                assert!(document.representation.is_none());
                assert!(document.model_states.is_empty());
            }
            assert_eq!(document.unparsed_tail.window(), b"tail");
        }
    }

    #[test]
    fn unsupported_schema_is_rejected() {
        let (bytes, _) = fixture(16);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic UFRxDoc fits policy");
        assert!(matches!(
            parse_stream(&ctx, root),
            Err(CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn schema_11_rejects_nonzero_header_invariant() {
        let (mut bytes, invariant_offset) = fixture(11);
        bytes[invariant_offset..invariant_offset + 2].copy_from_slice(&2_u16.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic UFRxDoc fits policy");
        assert!(parse_stream(&ctx, root).is_err());
    }

    fn fixture(schema: u16) -> (Vec<u8>, usize) {
        let mut bytes = Vec::new();
        push_u16(&mut bytes, schema);
        push_u16(&mut bytes, if schema == 15 { 27 } else { 23 });
        let header_section_version = if schema == 15 { 15 } else { 12 };
        for value in [
            31,
            19,
            header_section_version,
            18,
            1,
            2,
            4,
            2,
            1,
            3,
            1,
            2,
            6,
            2,
            2,
            5,
            0,
            1,
            2,
            0,
            0,
            0,
            0,
        ] {
            push_u16(&mut bytes, value);
        }
        if schema == 15 {
            for _ in 0..4 {
                push_u16(&mut bytes, 0);
            }
        }
        bytes.extend_from_slice(&[0; 32]);
        push_utf16(&mut bytes, "comment");
        bytes.extend_from_slice(&[0; 32]);
        bytes.extend_from_slice(&[0x10; 16]);
        push_u32(&mut bytes, 0);
        bytes.extend_from_slice(&[0x20; 16]);
        push_utf16(&mut bytes, "synthetic.ipt");
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        bytes.extend_from_slice(&[0; 4]);
        if schema == 15 {
            push_u16(&mut bytes, 0);
            push_utf16(&mut bytes, "Default");
            push_utf16(&mut bytes, "DesignView");
            bytes.extend_from_slice(&[0; 4]);
            push_utf16(&mut bytes, "Master");
            bytes.extend_from_slice(&[0; 4]);
        } else {
            push_utf16(&mut bytes, "Default");
            bytes.extend_from_slice(&[0; 4]);
            push_u16(&mut bytes, 0);
        }
        push_u32(&mut bytes, 3);
        push_u16(&mut bytes, 0);
        let invariant_offset = bytes.len();
        push_u16(&mut bytes, 1);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        if header_section_version >= 13 {
            bytes.extend_from_slice(&[0; 32]);
        }
        if schema == 15 {
            push_u32(&mut bytes, 1);
            bytes.push(0);
            push_utf16(&mut bytes, "Master");
            push_u16(&mut bytes, 2);
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 1);
            push_utf16(&mut bytes, "width");
            bytes.push(2);
            push_u16(&mut bytes, 0x48);
            push_u16(&mut bytes, 1);
            push_utf16(&mut bytes, "12.5");
            push_u16(&mut bytes, 0);
            bytes.extend_from_slice(&[0x5a; 77]);
        } else {
            push_u32(&mut bytes, 0);
        }
        push_u32(&mut bytes, 1);
        push_utf16(&mut bytes, "References");
        push_u32(&mut bytes, 0);
        push_utf16(&mut bytes, "relative/component.ipt");
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        push_utf16(&mut bytes, "library");
        push_u16(&mut bytes, 0);
        push_utf16(&mut bytes, "component");
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 2);
        bytes.extend_from_slice(&[0x31; 16]);
        bytes.extend_from_slice(&[0x32; 16]);
        push_u32(&mut bytes, 7);
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 12);
        push_u32(&mut bytes, 4);
        bytes.extend_from_slice(b"tail");
        (bytes, invariant_offset)
    }

    fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        push_u32(bytes, units.len() as u32);
        for unit in units {
            push_u16(bytes, unit);
        }
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
