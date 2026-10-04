// SPDX-License-Identifier: Apache-2.0
//! Identity framing for the pre-`7C05` design stream.

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::container;

const CATALOG_OPEN: &[u8] = b"\xde\x04\xfe\xfe\x12CATCatalogManager";
const SCHEMA_PROGRAM_PREFIX: &[u8] = b"\xfe\xfe\xfe";
const SCHEMA_PROGRAM_FOOTER: &[u8] = b"\x4e\x11\x00\x00\x00DASSAULT-SYSTEMES\x05\x00\x00\x00CATIA";
#[cfg(test)]
pub(crate) const SCHEMA_PROGRAM_OFFSET_FROM_CATALOG: usize =
    CATALOG_OPEN.len() + SCHEMA_PROGRAM_PREFIX.len();
const TEXT_OPEN: &[u8] = b"\xe8\x00\x12\x01";
const SCALAR_OPEN: &[u8] = b"\xfe\x85\x88\x82\xfe";
const NAMED_SCALAR_OPEN: &[u8] = b"\xfe\x84\x88\x82\xfe";
const STRING_OPEN: &[u8] = b"\xfe\x85\x93\x82\xfe";
const INTEGER_OPEN: &[u8] = b"\xfe\x85\x9d\x82\xfe";
const TYPE_OPEN: &[u8] = b"\xfe\x84\x92\x82";

/// Length production used by one legacy schema text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyTextEncoding {
    /// Nonzero one-byte inclusive length followed by the text and `FE`.
    U8InclusiveLength,
    /// Zero selector, little-endian `u32` byte length, text, and `FE`.
    ZeroU32Length,
    /// Nonzero one-byte inclusive length followed by text and an `E3` paged-role tail.
    U8InclusiveLengthE3RoleTail,
}

/// Framing production used by one legacy role selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyRoleSelectorEncoding {
    /// `80` followed by a nonzero little-endian `u32`.
    FixedU32,
    /// Page byte `D1..E4` followed by one low byte.
    Paged,
}

/// Stored representation of one legacy schema role name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum LegacyRoleName {
    /// Inclusive-length UTF-8 role name.
    Literal(String),
    /// Unresolved one-byte schema selector.
    Selector(u8),
}

impl LegacyRoleName {
    pub(crate) fn literal(&self) -> Option<&str> {
        match self {
            Self::Literal(value) => Some(value),
            Self::Selector(_) => None,
        }
    }

    pub(crate) fn byte_len(&self) -> usize {
        match self {
            Self::Literal(value) => 1 + value.len(),
            Self::Selector(_) => 1,
        }
    }
}

/// One length-framed schema role and its selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyRoleSelector {
    /// Offset of the literal length or schema-selector byte.
    pub(crate) offset: usize,
    /// Stored identity whose interval contains the role.
    pub(crate) entity_id: u32,
    /// Stored literal or unresolved role name.
    pub(crate) name: LegacyRoleName,
    /// Selector framing production.
    pub(crate) encoding: LegacyRoleSelectorEncoding,
    /// Stored selector following the role name.
    pub(crate) selector: u32,
    /// Field code when an `E8 <field-code:u16le> 01` opener follows immediately.
    pub(crate) field_code: Option<u16>,
}

/// One complete UTF-8 text field in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyTextField {
    /// Offset of the `E8 00 12 01` field opener.
    pub(crate) offset: usize,
    /// Stored identity whose interval contains the field.
    pub(crate) entity_id: u32,
    /// Text framing production.
    pub(crate) encoding: LegacyTextEncoding,
    /// Immediately preceding length-framed role and selector.
    pub(crate) role: Option<LegacyRoleSelector>,
    /// Decoded UTF-8 value.
    pub(crate) value: String,
}

/// One schema field bounded by consecutive role selectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacySchemaField {
    /// Offset of the `E8 <field-code:u16le> 01` opener.
    pub(crate) offset: usize,
    /// Stored identity whose interval contains the field.
    pub(crate) entity_id: u32,
    /// Role selector that binds this field.
    pub(crate) role_offset: usize,
    /// Following role selector that closes the payload.
    pub(crate) boundary_role_offset: usize,
    /// Stored schema field code.
    pub(crate) field_code: u16,
    /// Exact bytes after the opener and before the boundary role.
    pub(crate) payload: Vec<u8>,
}

/// One typed parameter clause in a legacy relation signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyRelationParameter {
    /// Expression-local parameter.
    pub(crate) parameter: String,
    /// Source value type.
    pub(crate) value_type: String,
}

/// Result of a complete legacy relation signature.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LegacyRelationResult {
    /// `VoidType` result with a named output parameter.
    Void {
        /// Output parameter for the void relation.
        output: LegacyRelationParameter,
    },
    /// Non-void result type with no output parameter.
    Typed(String),
}

/// Parsed roles in a complete legacy relation signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyRelationSignature {
    /// Ordered input parameters.
    pub(crate) inputs: Vec<LegacyRelationParameter>,
    /// Result type and optional void output.
    result: LegacyRelationResult,
}

impl LegacyRelationSignature {
    pub(crate) fn into_parts(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<
        (
            Vec<LegacyRelationParameter>,
            Option<LegacyRelationParameter>,
            String,
        ),
        cadmpeg_core::CodecError,
    > {
        let (output, result_type) = match self.result {
            LegacyRelationResult::Void { output } => (
                Some(output),
                ctx.copy_retained_text("VoidType", "catia_legacy_void_result_type")?,
            ),
            LegacyRelationResult::Typed(result_type) => (None, result_type),
        };
        Ok((self.inputs, output, result_type))
    }

    #[cfg(test)]
    pub(crate) fn output(&self) -> Option<&LegacyRelationParameter> {
        match &self.result {
            LegacyRelationResult::Void { output } => Some(output),
            LegacyRelationResult::Typed(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn result_type(&self) -> &str {
        match &self.result {
            LegacyRelationResult::Void { .. } => "VoidType",
            LegacyRelationResult::Typed(result_type) => result_type,
        }
    }
}

/// Paired expression and type-signature fields owned by one identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyRelation {
    /// Stored owner identity.
    pub(crate) entity_id: u32,
    /// Selector carried by the expression field's `body` role.
    pub(crate) body_selector: Option<u32>,
    /// Selector carried by the type-signature field's `param` role.
    pub(crate) parameter_selector: Option<u32>,
    /// Parameter identity selected by exact self-`body` and target-`param` roles.
    pub(crate) parameter_entity_id: Option<u32>,
    /// Expression-field opener offset.
    pub(crate) expression_offset: usize,
    /// Exact expression or rule program.
    pub(crate) expression: String,
    /// Signature-field opener offset.
    pub(crate) signature_offset: usize,
    /// Exact stored type signature.
    pub(crate) type_signature: String,
    /// Parsed input, output, and result roles.
    pub(crate) signature: LegacyRelationSignature,
}

/// One complete `synchrone` relation-update field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyRelationSynchronousState {
    /// Offset of the `synchrone` role-name length byte.
    pub(crate) role_offset: usize,
    /// Stored identity whose interval contains the field.
    pub(crate) entity_id: u32,
    /// Selector carried by the `synchrone` role.
    pub(crate) selector: u32,
    /// Whether the relation updates synchronously.
    pub(crate) synchronous: bool,
}

/// Value selected by one legacy type descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LegacyTypeValue {
    /// Inclusive-length UTF-8 type name.
    Name(String),
    /// Compact selector identity.
    Selector(u32),
}

/// One complete legacy type descriptor in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyTypeDescriptor {
    /// Offset of the fixed descriptor prefix.
    pub(crate) offset: usize,
    /// Stored identity whose interval contains the descriptor.
    pub(crate) entity_id: u32,
    /// Stored literal name or unresolved selector.
    pub(crate) value: LegacyTypeValue,
}

/// Evaluation stored by a complete legacy scalar packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyScalarEvaluation {
    /// `E6` followed by finite binary64 bits.
    Value(u64),
    /// `E7` without a scalar payload.
    Unset,
}

/// Fixed prefix selecting one legacy scalar production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyScalarEncoding {
    /// `FE 84 88 82 FE`.
    Named84,
    /// `FE 85 88 82 FE`.
    Standalone85,
}

/// One complete typed scalar packet in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyScalarValue {
    /// Offset of the fixed packet prefix.
    pub(crate) offset: usize,
    /// Stored identity whose interval contains the packet.
    pub(crate) entity_id: u32,
    /// Fixed scalar-prefix production.
    pub(crate) encoding: LegacyScalarEncoding,
    /// Unique co-owned `name` text-field opener.
    pub(crate) name_offset: Option<usize>,
    /// Unique co-owned stored name.
    pub(crate) name: Option<String>,
    /// Stored evaluation.
    pub(crate) evaluation: LegacyScalarEvaluation,
}

/// One complete UTF-8 string-value packet in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyStringValue {
    /// Offset of the fixed packet prefix.
    pub(crate) offset: usize,
    /// Stored identity whose interval contains the packet.
    pub(crate) entity_id: u32,
    /// Unique co-owned `name` text-field opener.
    pub(crate) name_offset: Option<usize>,
    /// Unique co-owned stored name.
    pub(crate) name: Option<String>,
    /// Stored UTF-8 value.
    pub(crate) value: String,
}

/// Stored encoding of one legacy signed integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyIntegerEncoding {
    /// One byte stores values zero through 126 as `value + 0x81`.
    Inline,
    /// `80` introduces one signed little-endian 32-bit value.
    WideI32,
}

/// One complete signed-integer packet in an identity interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyIntegerValue {
    /// Offset of the fixed packet prefix.
    pub(crate) offset: usize,
    /// Stored identity whose interval contains the packet.
    pub(crate) entity_id: u32,
    /// Stored integer encoding.
    pub(crate) encoding: LegacyIntegerEncoding,
    /// Unique co-owned `name` text-field opener.
    pub(crate) name_offset: Option<usize>,
    /// Unique co-owned stored name.
    pub(crate) name: Option<String>,
    /// Stored signed value.
    pub(crate) value: i32,
}

/// Stored legacy identity record lead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum CatiaLegacyIdentityLead {
    /// Lead `0x81`.
    Lead81,
    /// Lead `0x82`.
    Lead82,
    /// Lead `0xe5`.
    LeadE5,
    /// Lead `0xfd`.
    LeadFd,
}

impl TryFrom<u8> for CatiaLegacyIdentityLead {
    type Error = String;

    fn try_from(lead: u8) -> Result<Self, Self::Error> {
        match lead {
            0x81 => Ok(Self::Lead81),
            0x82 => Ok(Self::Lead82),
            0xe5 => Ok(Self::LeadE5),
            0xfd => Ok(Self::LeadFd),
            _ => Err(format!("lead {lead:#x} is not 0x81, 0x82, 0xe5, or 0xfd")),
        }
    }
}

impl From<CatiaLegacyIdentityLead> for u8 {
    fn from(lead: CatiaLegacyIdentityLead) -> Self {
        match lead {
            CatiaLegacyIdentityLead::Lead81 => 0x81,
            CatiaLegacyIdentityLead::Lead82 => 0x82,
            CatiaLegacyIdentityLead::LeadE5 => 0xe5,
            CatiaLegacyIdentityLead::LeadFd => 0xfd,
        }
    }
}

/// One stored entity identity in a legacy identity run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LegacyEntityIdentity {
    /// Offset of the `EA` identity delimiter.
    pub(crate) offset: usize,
    /// Little-endian identity following the delimiter.
    pub(crate) entity_id: u32,
    /// Stored record lead following the identity.
    pub(crate) lead: CatiaLegacyIdentityLead,
}

/// One complete compact schema program following a legacy catalog opener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacySchemaProgram {
    /// Offset of the first program byte after the fixed prefix.
    pub(crate) offset: usize,
    /// Offset of the production following the program.
    pub(crate) boundary_offset: usize,
    /// Production that closes the program.
    pub(crate) boundary: LegacySchemaProgramBoundary,
    /// Exact program bytes, including the terminal `FE`.
    pub(crate) bytes: Vec<u8>,
    /// Complete inclusive-length identifier packets in source order.
    pub(crate) identifiers: Vec<LegacySchemaIdentifier>,
}

/// Production that closes a compact legacy schema program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacySchemaProgramBoundary {
    /// Fixed vendor footer preceded by the terminal `FE`.
    VendorFooter,
    /// Validated outer stream directory preceded by the terminal `FE`.
    StreamDirectory,
}

/// One complete inclusive-length identifier packet in a compact schema program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacySchemaIdentifier {
    /// Offset of the inclusive-length byte.
    pub(crate) offset: usize,
    /// Stored identifier.
    pub(crate) value: String,
}

/// A monotonically identified legacy run terminated by its schema catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyEntityRun {
    /// Offset of the fixed catalog opening production.
    pub(crate) catalog_offset: usize,
    /// Complete compact schema program following the catalog opener.
    pub(crate) schema_program: Option<LegacySchemaProgram>,
    /// First stored identity in the run.
    pub(crate) first_identity: LegacyEntityIdentity,
    /// Remaining identities in source order.
    following_identities: Vec<LegacyEntityIdentity>,
    /// Complete length-framed role selectors in identity-interval order.
    pub(crate) role_selectors: Vec<LegacyRoleSelector>,
    /// Complete schema text fields contained by the identity intervals.
    pub(crate) text_fields: Vec<LegacyTextField>,
    /// Complete role-bounded schema fields.
    pub(crate) schema_fields: Vec<LegacySchemaField>,
    /// Complete expression/signature pairs.
    pub(crate) relations: Vec<LegacyRelation>,
    /// Complete `synchrone` relation-update fields.
    pub(crate) synchronous_states: Vec<LegacyRelationSynchronousState>,
    /// Complete literal or selector type descriptors.
    pub(crate) type_descriptors: Vec<LegacyTypeDescriptor>,
    /// Complete typed scalar packets.
    pub(crate) scalar_values: Vec<LegacyScalarValue>,
    /// Complete UTF-8 string-value packets.
    pub(crate) string_values: Vec<LegacyStringValue>,
    /// Complete signed-integer packets.
    pub(crate) integer_values: Vec<LegacyIntegerValue>,
}

impl LegacyEntityRun {
    /// Stored identities in source order.
    pub(crate) fn identities(&self) -> impl Iterator<Item = &LegacyEntityIdentity> {
        std::iter::once(&self.first_identity).chain(&self.following_identities)
    }
}

/// Parse complete legacy identity runs terminated by the fixed schema-catalog opener.
pub(crate) fn parse_runs(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<LegacyEntityRun>, CodecError> {
    let directory_offset =
        container::outer_stream_directory_range(ctx, data)?.map(|range| range.start);
    parse_runs_with_directory_offset(ctx, data, directory_offset)
}

fn charge_scan(
    ctx: &DecodeContext<'_>,
    length: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let work = u64_from_index(length);
    ctx.charge_work(work, operation)
}

fn parse_runs_with_directory_offset(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    directory_offset: Option<usize>,
) -> Result<Vec<LegacyEntityRun>, CodecError> {
    charge_scan(ctx, data.len(), "catia_legacy_catalog_scan")?;
    let mut runs = Vec::new();
    for catalog_offset in memchr::memmem::find_iter(data, CATALOG_OPEN) {
        if let Some(run) = parse_run_before(ctx, data, catalog_offset, directory_offset)? {
            ctx.push_vec(&mut runs, run, "catia_legacy_runs")?;
        }
    }
    Ok(runs)
}

fn parse_run_before(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    catalog_offset: usize,
    directory_offset: Option<usize>,
) -> Result<Option<LegacyEntityRun>, CodecError> {
    charge_scan(ctx, catalog_offset, "catia_legacy_identity_scan")?;
    let mut identities = ctx.collect_vec(
        data[..catalog_offset]
            .windows(6)
            .enumerate()
            .filter_map(|(offset, bytes)| {
                if bytes[0] != 0xea {
                    return None;
                }
                let entity_id = View::u32_le_at(bytes, 1)?;
                let lead = match bytes[5] {
                    0x81 => CatiaLegacyIdentityLead::Lead81,
                    0x82 => CatiaLegacyIdentityLead::Lead82,
                    0xe5 => CatiaLegacyIdentityLead::LeadE5,
                    0xfd => CatiaLegacyIdentityLead::LeadFd,
                    _ => return None,
                };
                (entity_id != 0).then_some(LegacyEntityIdentity {
                    offset,
                    entity_id,
                    lead,
                })
            }),
        "catia_legacy_candidate_identities",
    )?;
    let suffix_start = identities
        .windows(2)
        .rposition(|pair| pair[0].entity_id >= pair[1].entity_id)
        .map_or(0, |index| index + 1);
    identities.drain(..suffix_start);
    let Some(&first_identity) = identities.first() else {
        return Ok(None);
    };
    if first_identity.entity_id != 1 {
        return Ok(None);
    }
    let mut role_selectors = Vec::new();
    let mut text_fields = Vec::new();
    for (index, identity) in identities.iter().enumerate() {
        let start = identity.offset + 6;
        let end = identities
            .get(index + 1)
            .map_or(catalog_offset, |next| next.offset);
        let mut interval_roles = parse_role_selectors(ctx, data, start, end, identity.entity_id)?;
        let mut interval_fields =
            parse_text_fields(ctx, data, start, end, identity.entity_id, &interval_roles)?;
        ctx.reserve_vec(
            &mut text_fields,
            interval_fields.len(),
            "catia_legacy_run_text_fields",
        )?;
        text_fields.append(&mut interval_fields);
        ctx.reserve_vec(
            &mut role_selectors,
            interval_roles.len(),
            "catia_legacy_run_roles",
        )?;
        role_selectors.append(&mut interval_roles);
    }
    let relations = parse_relations(ctx, &text_fields, &identities)?;
    let schema_fields = parse_schema_fields(ctx, data, &role_selectors, &text_fields)?;
    let synchronous_states =
        parse_synchronous_states(ctx, data, &role_selectors, &identities, catalog_offset)?;
    let mut type_descriptors = Vec::new();
    let mut scalar_values = Vec::new();
    let mut string_values = Vec::new();
    let mut integer_values = Vec::new();
    for (index, identity) in identities.iter().enumerate() {
        let start = identity.offset + 6;
        let end = identities
            .get(index + 1)
            .map_or(catalog_offset, |next| next.offset);
        let mut interval_types = parse_type_descriptors(ctx, data, start, end, identity.entity_id)?;
        ctx.reserve_vec(
            &mut type_descriptors,
            interval_types.len(),
            "catia_legacy_run_types",
        )?;
        type_descriptors.append(&mut interval_types);
        let mut interval_scalars = parse_scalar_values(ctx, data, start, end, identity.entity_id)?;
        ctx.reserve_vec(
            &mut scalar_values,
            interval_scalars.len(),
            "catia_legacy_run_scalars",
        )?;
        scalar_values.append(&mut interval_scalars);
        let mut interval_strings = parse_string_values(ctx, data, start, end, identity.entity_id)?;
        ctx.reserve_vec(
            &mut string_values,
            interval_strings.len(),
            "catia_legacy_run_strings",
        )?;
        string_values.append(&mut interval_strings);
        let mut interval_integers =
            parse_integer_values(ctx, data, start, end, identity.entity_id)?;
        ctx.reserve_vec(
            &mut integer_values,
            interval_integers.len(),
            "catia_legacy_run_integers",
        )?;
        integer_values.append(&mut interval_integers);
    }
    bind_value_names(ctx, data, &role_selectors, &text_fields, &mut scalar_values)?;
    bind_value_names(ctx, data, &role_selectors, &text_fields, &mut string_values)?;
    bind_value_names(
        ctx,
        data,
        &role_selectors,
        &text_fields,
        &mut integer_values,
    )?;
    let schema_program = parse_schema_program(ctx, data, catalog_offset, directory_offset)?;
    Ok(Some(LegacyEntityRun {
        catalog_offset,
        schema_program,
        first_identity,
        following_identities: ctx.collect_vec(
            identities.into_iter().skip(1),
            "catia_legacy_following_identities",
        )?,
        role_selectors,
        text_fields,
        schema_fields,
        relations,
        synchronous_states,
        type_descriptors,
        scalar_values,
        string_values,
        integer_values,
    }))
}

fn parse_schema_program(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    catalog_offset: usize,
    directory_offset: Option<usize>,
) -> Result<Option<LegacySchemaProgram>, CodecError> {
    let scan_len = data
        .len()
        .checked_sub(catalog_offset)
        .ok_or_else(|| ctx.refuse_codec_limit("catia_legacy_schema_scan", u64::MAX, u64::MAX))?;
    charge_scan(ctx, scan_len, "catia_legacy_schema_scan")?;
    let Some((offset, boundary_offset, boundary, source)) = (|| {
        let prefix_offset = catalog_offset.checked_add(CATALOG_OPEN.len())?;
        let offset = prefix_offset.checked_add(SCHEMA_PROGRAM_PREFIX.len())?;
        if data.get(prefix_offset..offset)? != SCHEMA_PROGRAM_PREFIX {
            return None;
        }
        let search_end = memchr::memmem::find(&data[offset..], CATALOG_OPEN)
            .and_then(|relative| offset.checked_add(relative))
            .unwrap_or(data.len());
        let footer_offset =
            memchr::memmem::find_iter(&data[offset..search_end], SCHEMA_PROGRAM_FOOTER).find_map(
                |relative| {
                    let footer_offset = offset.checked_add(relative)?;
                    (footer_offset > offset && data.get(footer_offset - 1) == Some(&0xfe))
                        .then_some(footer_offset)
                },
            );
        let (boundary_offset, boundary) = if let Some(footer_offset) = footer_offset {
            (footer_offset, LegacySchemaProgramBoundary::VendorFooter)
        } else {
            let directory_offset = directory_offset?;
            if directory_offset <= offset
                || directory_offset > search_end
                || data.get(directory_offset - 1) != Some(&0xfe)
            {
                return None;
            }
            (
                directory_offset,
                LegacySchemaProgramBoundary::StreamDirectory,
            )
        };
        Some((
            offset,
            boundary_offset,
            boundary,
            data.get(offset..boundary_offset)?,
        ))
    })() else {
        return Ok(None);
    };
    let bytes = ctx.copy_slice(source, "catia_legacy_schema_program_bytes")?;
    Ok(Some(LegacySchemaProgram {
        offset,
        boundary_offset,
        boundary,
        identifiers: parse_schema_identifiers(ctx, &bytes, offset)?,
        bytes,
    }))
}

pub(crate) fn parse_schema_identifiers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    program_offset: usize,
) -> Result<Vec<LegacySchemaIdentifier>, CodecError> {
    charge_scan(ctx, bytes.len(), "catia_legacy_identifier_scan")?;
    let mut identifiers = Vec::new();
    for (relative, first) in bytes.iter().enumerate() {
        let Some((offset, value)) = (|| {
            let value_len = usize::from(*first).checked_sub(1)?;
            if value_len == 0 {
                return None;
            }
            let value_offset = relative.checked_add(1)?;
            let end = value_offset.checked_add(value_len)?;
            let value = std::str::from_utf8(bytes.get(value_offset..end)?).ok()?;
            if !valid_identifier(value) || bytes.get(end).is_some_and(|following| *following < 0x81)
            {
                return None;
            }
            Some((program_offset.checked_add(relative)?, value))
        })() else {
            continue;
        };
        let identifier = LegacySchemaIdentifier {
            offset,
            value: ctx.copy_retained_text(value, "catia_legacy_schema_identifier_value")?,
        };
        ctx.push_vec(
            &mut identifiers,
            identifier,
            "catia_legacy_schema_identifiers",
        )?;
    }
    Ok(identifiers)
}

fn parse_synchronous_states(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    roles: &[LegacyRoleSelector],
    identities: &[LegacyEntityIdentity],
    catalog_offset: usize,
) -> Result<Vec<LegacyRelationSynchronousState>, CodecError> {
    ctx.collect_vec(
        roles.iter().filter_map(|role| {
            let at = role.end_offset()?;
            let interval_end = identities
                .iter()
                .find(|identity| identity.offset > role.offset)
                .map_or(catalog_offset, |identity| identity.offset);
            let (state, end) = match &role.name {
                LegacyRoleName::Literal(name) if name == "synchrone" => {
                    let end = at.checked_add(6)?;
                    let [0xe8, 0x00, 0x1c, 0x01, state, 0xfe] = *data.get(at..end)? else {
                        return None;
                    };
                    (state, end)
                }
                LegacyRoleName::Selector(_) => {
                    let end = at.checked_add(5)?;
                    let [0xe8, 0x00, 0x1c, 0x01, state] = *data.get(at..end)? else {
                        return None;
                    };
                    if !roles
                        .iter()
                        .any(|next| next.entity_id == role.entity_id && next.offset == end)
                    {
                        return None;
                    }
                    (state, end)
                }
                LegacyRoleName::Literal(_) => return None,
            };
            if end > interval_end {
                return None;
            }
            let synchronous = match state {
                0x81 => false,
                0x82 => true,
                _ => return None,
            };
            Some(LegacyRelationSynchronousState {
                role_offset: role.offset,
                entity_id: role.entity_id,
                selector: role.selector,
                synchronous,
            })
        }),
        "catia_legacy_synchronous_states",
    )
}

fn parse_type_descriptors(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Result<Vec<LegacyTypeDescriptor>, CodecError> {
    charge_scan(ctx, end - start, "catia_legacy_type_scan")?;
    let mut descriptors = Vec::new();
    for relative in memchr::memmem::find_iter(&data[start..end], TYPE_OPEN) {
        let Some((offset, name, selector)) = (|| {
            let offset = start + relative;
            let payload = offset.checked_add(TYPE_OPEN.len())?;
            let first = *data.get(payload)?;
            if (2..=0x7f).contains(&first) {
                let name_end = payload.checked_add(usize::from(first))?;
                if name_end >= end || data.get(name_end) != Some(&0x83) {
                    return None;
                }
                let name = text_value(data.get(payload + 1..name_end)?)?;
                valid_identifier(name).then_some((offset, Some(name), 0))
            } else if (0x81..=0xd0).contains(&first)
                && payload.checked_add(1)? < end
                && data.get(payload + 1) == Some(&0x83)
            {
                Some((offset, None, u32::from(first - 0x80)))
            } else {
                None
            }
        })() else {
            continue;
        };
        let value = match name {
            Some(name) => {
                LegacyTypeValue::Name(ctx.copy_retained_text(name, "catia_legacy_type_name")?)
            }
            None => LegacyTypeValue::Selector(selector),
        };
        ctx.push_vec(
            &mut descriptors,
            LegacyTypeDescriptor {
                offset,
                entity_id,
                value,
            },
            "catia_legacy_type_descriptors",
        )?;
    }
    Ok(descriptors)
}

fn parse_scalar_values(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Result<Vec<LegacyScalarValue>, CodecError> {
    charge_scan(
        ctx,
        (end - start).checked_mul(2).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_legacy_scalar_scan", u64::MAX, u64::MAX)
        })?,
        "catia_legacy_scalar_scan",
    )?;
    let mut values = ctx.collect_vec(
        [
            (NAMED_SCALAR_OPEN, LegacyScalarEncoding::Named84),
            (SCALAR_OPEN, LegacyScalarEncoding::Standalone85),
        ]
        .into_iter()
        .flat_map(|(opener, encoding)| {
            memchr::memmem::find_iter(&data[start..end], opener).filter_map(move |relative| {
                let offset = start + relative;
                if offset.checked_add(6)? > end {
                    return None;
                }
                let opcode = *data.get(offset + opener.len())?;
                let evaluation = match opcode {
                    0xe6 => {
                        if offset.checked_add(14)? > end {
                            return None;
                        }
                        let bits = View::u64_le_at(data, offset + 6)?;
                        f64::from_bits(bits)
                            .is_finite()
                            .then_some(LegacyScalarEvaluation::Value(bits))?
                    }
                    0xe7 => LegacyScalarEvaluation::Unset,
                    _ => return None,
                };
                Some(LegacyScalarValue {
                    offset,
                    entity_id,
                    encoding,
                    name_offset: None,
                    name: None,
                    evaluation,
                })
            })
        }),
        "catia_legacy_scalar_values",
    )?;
    for index in 1..values.len() {
        let mut at = index;
        while at > 0 && values[at - 1].offset > values[at].offset {
            ctx.charge_work(1, "catia_legacy_scalar_sort")?;
            values.swap(at - 1, at);
            at -= 1;
        }
    }
    Ok(values)
}

/// A stored value packet that can carry a unique co-owned `name` text field.
trait LegacyNamedValue {
    /// Stored identity whose interval contains the packet.
    fn entity_id(&self) -> u32;
    /// Offset of the fixed packet prefix.
    fn offset(&self) -> usize;
    /// Record the unique co-owned name opener and text.
    fn bind_name(&mut self, name_offset: usize, name: String);
}

impl LegacyNamedValue for LegacyScalarValue {
    fn entity_id(&self) -> u32 {
        self.entity_id
    }

    fn offset(&self) -> usize {
        self.offset
    }

    fn bind_name(&mut self, name_offset: usize, name: String) {
        self.name_offset = Some(name_offset);
        self.name = Some(name);
    }
}

impl LegacyNamedValue for LegacyStringValue {
    fn entity_id(&self) -> u32 {
        self.entity_id
    }

    fn offset(&self) -> usize {
        self.offset
    }

    fn bind_name(&mut self, name_offset: usize, name: String) {
        self.name_offset = Some(name_offset);
        self.name = Some(name);
    }
}

impl LegacyNamedValue for LegacyIntegerValue {
    fn entity_id(&self) -> u32 {
        self.entity_id
    }

    fn offset(&self) -> usize {
        self.offset
    }

    fn bind_name(&mut self, name_offset: usize, name: String) {
        self.name_offset = Some(name_offset);
        self.name = Some(name);
    }
}

/// Bind the unique co-owned `name` text field onto every value packet that is
/// the sole packet of its stored identity.
fn bind_value_names<Value: LegacyNamedValue>(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    roles: &[LegacyRoleSelector],
    fields: &[LegacyTextField],
    values: &mut [Value],
) -> Result<(), CodecError> {
    let mut counts = std::collections::HashMap::<u32, usize>::new();
    for value in values.iter() {
        if let Some(count) = counts.get_mut(&value.entity_id()) {
            *count = count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_legacy_name_counts", u64::MAX, u64::MAX)
            })?;
        } else {
            ctx.insert_hash_map(
                &mut counts,
                value.entity_id(),
                1usize,
                "catia_legacy_name_counts",
            )?;
        }
    }
    for value in values {
        if counts.get(&value.entity_id()) != Some(&1) {
            continue;
        }
        if let Some(name) =
            unique_value_name(data, roles, fields, value.entity_id(), value.offset())
        {
            value.bind_name(
                name.offset,
                ctx.copy_retained_text(&name.value, "catia_legacy_bound_name")?,
            );
        }
    }
    Ok(())
}

fn parse_string_values(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Result<Vec<LegacyStringValue>, CodecError> {
    charge_scan(ctx, end - start, "catia_legacy_string_scan")?;
    let mut values = Vec::new();
    for relative in memchr::memmem::find_iter(&data[start..end], STRING_OPEN) {
        let Some((offset, value)) = (|| {
            let offset = start + relative;
            let payload = offset.checked_add(STRING_OPEN.len())?;
            let inclusive_length = usize::from(*data.get(payload)?);
            if inclusive_length == 0 {
                return None;
            }
            let value_end = payload.checked_add(inclusive_length)?;
            if value_end > end {
                return None;
            }
            let value = text_value_allow_empty(data.get(payload + 1..value_end)?)?;
            Some((offset, value))
        })() else {
            continue;
        };
        ctx.push_vec(
            &mut values,
            LegacyStringValue {
                offset,
                entity_id,
                name_offset: None,
                name: None,
                value: ctx.copy_retained_text(value, "catia_legacy_string_value")?,
            },
            "catia_legacy_string_values",
        )?;
    }
    Ok(values)
}

fn parse_integer_values(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Result<Vec<LegacyIntegerValue>, CodecError> {
    charge_scan(ctx, end - start, "catia_legacy_integer_scan")?;
    ctx.collect_vec(
        memchr::memmem::find_iter(&data[start..end], INTEGER_OPEN).filter_map(|relative| {
            let offset = start + relative;
            let payload = offset.checked_add(INTEGER_OPEN.len())?;
            let lead = *data.get(payload)?;
            let (encoding, value, value_end) = if lead == 0x80 {
                let value_end = payload.checked_add(5)?;
                if value_end > end {
                    return None;
                }
                (
                    LegacyIntegerEncoding::WideI32,
                    View::i32_le_at(data, payload + 1)?,
                    value_end,
                )
            } else {
                (
                    LegacyIntegerEncoding::Inline,
                    i32::from(lead.checked_sub(0x81)?),
                    payload + 1,
                )
            };
            (value_end <= end).then_some(LegacyIntegerValue {
                offset,
                entity_id,
                encoding,
                name_offset: None,
                name: None,
                value,
            })
        }),
        "catia_legacy_integer_values",
    )
}

fn unique_value_name<'a>(
    data: &[u8],
    roles: &[LegacyRoleSelector],
    fields: &'a [LegacyTextField],
    entity_id: u32,
    value_offset: usize,
) -> Option<&'a LegacyTextField> {
    let mut names = fields.iter().filter(|field| {
        field.entity_id == entity_id
            && field
                .role
                .as_ref()
                .is_some_and(|role| role.name.literal() == Some("name"))
    });
    if let Some(name) = names.next() {
        return names.next().is_none().then_some(name);
    }
    unique_evaluated_value_name(data, roles, fields, entity_id, value_offset)
}

fn unique_evaluated_value_name<'a>(
    data: &[u8],
    roles: &[LegacyRoleSelector],
    fields: &'a [LegacyTextField],
    entity_id: u32,
    value_offset: usize,
) -> Option<&'a LegacyTextField> {
    const EVALUATION_FIELD: &[u8] = b"\xe8\xc4\x17\x01\xfe\xfe";

    let mut evaluation_roles = roles.iter().filter(|role| {
        role.entity_id == entity_id
            && role.field_code == Some(0x17c4)
            && role
                .end_offset()
                .and_then(|offset| offset.checked_add(EVALUATION_FIELD.len()))
                == Some(value_offset)
            && role
                .end_offset()
                .and_then(|offset| data.get(offset..value_offset))
                == Some(EVALUATION_FIELD)
    });
    let evaluation_role = evaluation_roles.next()?;
    if evaluation_roles.next().is_some() {
        return None;
    }
    let mut names = fields.iter().filter(|field| {
        field.entity_id == entity_id
            && field.offset < evaluation_role.offset
            && valid_identifier(&field.value)
            && field
                .role
                .as_ref()
                .is_some_and(|role| role.field_code == Some(0x1200))
    });
    let name = names.next()?;
    names.next().is_none().then_some(name)
}

fn parse_relations(
    ctx: &DecodeContext<'_>,
    fields: &[LegacyTextField],
    identities: &[LegacyEntityIdentity],
) -> Result<Vec<LegacyRelation>, CodecError> {
    let mut relations = Vec::new();
    let mut start = 0;
    while start < fields.len() {
        ctx.charge_work(1, "catia_legacy_entity_iteration")?;
        let entity_id = fields[start].entity_id;
        let end = fields[start..]
            .iter()
            .position(|field| field.entity_id != entity_id)
            .map_or(fields.len(), |relative| start + relative);
        let entity_fields = &fields[start..end];
        let mut expressions = entity_fields.iter().filter(|field| {
            field
                .role
                .as_ref()
                .is_some_and(|role| role.name.literal() == Some("body"))
        });
        let expression = expressions.next();
        let duplicate_expression = expressions.next();
        let mut signatures = entity_fields.iter().filter(|field| {
            field
                .role
                .as_ref()
                .is_some_and(|role| role.name.literal() == Some("param"))
        });
        let signature = signatures.next();
        let duplicate_signature = signatures.next();
        let role_bound_pair = match (
            expression,
            duplicate_expression,
            signature,
            duplicate_signature,
        ) {
            (Some(expression), None, Some(signature), None)
                if expression.offset < signature.offset =>
            {
                Some((expression, signature))
            }
            _ => None,
        };
        let selected_role_pair = match entity_fields {
            [prelude, expression, signature]
                if prelude.value.is_empty()
                    && prelude
                        .role
                        .as_ref()
                        .is_none_or(|role| matches!(&role.name, LegacyRoleName::Selector(_)))
                    && prelude.encoding == LegacyTextEncoding::U8InclusiveLengthE3RoleTail
                    && expression.encoding == LegacyTextEncoding::U8InclusiveLengthE3RoleTail
                    && signature.encoding == LegacyTextEncoding::U8InclusiveLengthE3RoleTail
                    && expression
                        .role
                        .as_ref()
                        .is_some_and(|role| matches!(&role.name, LegacyRoleName::Selector(_)))
                    && signature
                        .role
                        .as_ref()
                        .is_some_and(|role| matches!(&role.name, LegacyRoleName::Selector(_))) =>
            {
                Some((expression, signature))
            }
            _ => None,
        };
        let pair = role_bound_pair.or(selected_role_pair).or_else(|| {
            let [expression, signature] = entity_fields else {
                return None;
            };
            Some((expression, signature))
        });
        if let Some((expression, type_signature)) = pair {
            if let Some(signature) = parse_relation_signature(ctx, &type_signature.value)? {
                let body_selector = relation_role_selector(expression, "body");
                let parameter_selector = relation_role_selector(type_signature, "param");
                let relation = LegacyRelation {
                    entity_id,
                    body_selector,
                    parameter_selector,
                    parameter_entity_id: relation_parameter_entity(
                        entity_id,
                        body_selector,
                        parameter_selector,
                        identities,
                    ),
                    expression_offset: expression.offset,
                    expression: ctx.copy_retained_text(
                        &expression.value,
                        "catia_legacy_relation_expression",
                    )?,
                    signature_offset: type_signature.offset,
                    type_signature: ctx.copy_retained_text(
                        &type_signature.value,
                        "catia_legacy_relation_signature",
                    )?,
                    signature,
                };
                ctx.push_vec(&mut relations, relation, "catia_legacy_relations")?;
            }
        }
        start = end;
    }
    Ok(relations)
}

fn relation_role_selector(field: &LegacyTextField, role_name: &str) -> Option<u32> {
    field
        .role
        .as_ref()
        .filter(|role| role.name.literal() == Some(role_name))
        .map(|role| role.selector)
}

fn relation_parameter_entity(
    entity_id: u32,
    body_selector: Option<u32>,
    parameter_selector: Option<u32>,
    identities: &[LegacyEntityIdentity],
) -> Option<u32> {
    let parameter_selector = parameter_selector?;
    (body_selector == Some(entity_id)
        && identities
            .iter()
            .any(|identity| identity.entity_id == parameter_selector))
    .then_some(parameter_selector)
}

/// Parse a complete legacy relation type signature.
pub(crate) fn parse_relation_signature(
    ctx: &DecodeContext<'_>,
    source: &str,
) -> Result<Option<LegacyRelationSignature>, CodecError> {
    let source = source.strip_suffix('\n').unwrap_or(source);
    let Some((clauses, result_type)) = source.rsplit_once(") : ") else {
        return Ok(None);
    };
    let Some(clauses) = clauses.strip_prefix('(') else {
        return Ok(None);
    };
    let result_type = result_type.trim();
    if result_type.is_empty() {
        return Ok(None);
    }
    let mut inputs = Vec::new();
    let mut output = None;
    let mut names = std::collections::HashSet::new();
    if !clauses.trim().is_empty() {
        for clause in clauses.split(',') {
            let Some((parameter, role_type)) = clause.split_once(':') else {
                return Ok(None);
            };
            let parameter = parameter.trim();
            let role_type = role_type.trim();
            let (output_role, value_type) = if let Some(value_type) = role_type.strip_prefix("#In")
            {
                (false, value_type.trim())
            } else {
                let Some(value_type) = role_type.strip_prefix("#Out") else {
                    return Ok(None);
                };
                (true, value_type.trim())
            };
            if parameter.is_empty() || value_type.is_empty() || names.contains(parameter) {
                return Ok(None);
            }
            ctx.insert_hash_set(&mut names, parameter, "catia_legacy_relation_names")?;
            let parameter = LegacyRelationParameter {
                parameter: ctx.copy_retained_text(parameter, "catia_legacy_relation_parameter")?,
                value_type: ctx
                    .copy_retained_text(value_type, "catia_legacy_relation_value_type")?,
            };
            if output_role {
                if output.replace(parameter).is_some() {
                    return Ok(None);
                }
            } else {
                ctx.push_vec(&mut inputs, parameter, "catia_legacy_relation_inputs")?;
            }
        }
    }
    let result = if result_type == "VoidType" {
        let Some(output) = output else {
            return Ok(None);
        };
        LegacyRelationResult::Void { output }
    } else if output.is_none() {
        LegacyRelationResult::Typed(
            ctx.copy_retained_text(result_type, "catia_legacy_relation_result_type")?,
        )
    } else {
        return Ok(None);
    };
    Ok(Some(LegacyRelationSignature { inputs, result }))
}

fn parse_text_fields(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
    role_selectors: &[LegacyRoleSelector],
) -> Result<Vec<LegacyTextField>, CodecError> {
    charge_scan(ctx, end - start, "catia_legacy_text_scan")?;
    let mut fields = Vec::new();
    for relative in memchr::memmem::find_iter(&data[start..end], TEXT_OPEN) {
        let Some((offset, encoding, value)) = (|| {
            let offset = start + relative;
            let payload = offset.checked_add(TEXT_OPEN.len())?;
            let (encoding, value) = parse_text_field(data, payload, end)?;
            Some((offset, encoding, value))
        })() else {
            continue;
        };
        let role = role_selectors
            .iter()
            .find(|role| role.end_offset() == Some(offset))
            .map(|role| role.copy_charged(ctx))
            .transpose()?;
        let field = LegacyTextField {
            offset,
            entity_id,
            encoding,
            role,
            value: ctx.copy_retained_text(value, "catia_legacy_text_value")?,
        };
        ctx.push_vec(&mut fields, field, "catia_legacy_text_fields")?;
    }
    Ok(fields)
}

impl LegacyRoleSelector {
    fn copy_charged(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let name = match &self.name {
            LegacyRoleName::Literal(name) => LegacyRoleName::Literal(
                ctx.copy_retained_text(name, "catia_legacy_copied_role_name")?,
            ),
            LegacyRoleName::Selector(selector) => LegacyRoleName::Selector(*selector),
        };
        Ok(Self {
            offset: self.offset,
            entity_id: self.entity_id,
            name,
            encoding: self.encoding,
            selector: self.selector,
            field_code: self.field_code,
        })
    }

    fn end_offset(&self) -> Option<usize> {
        self.offset
            .checked_add(self.name.byte_len())?
            .checked_add(match self.encoding {
                LegacyRoleSelectorEncoding::FixedU32 => 5,
                LegacyRoleSelectorEncoding::Paged => 2,
            })
    }
}

fn parse_schema_fields(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    roles: &[LegacyRoleSelector],
    text_fields: &[LegacyTextField],
) -> Result<Vec<LegacySchemaField>, CodecError> {
    let mut fields = Vec::new();
    for pair in roles.windows(2) {
        let Some((offset, role, boundary, field_code, payload)) = (|| {
            let [role, boundary] = pair else {
                return None;
            };
            if role.entity_id != boundary.entity_id {
                return None;
            }
            let offset = role.end_offset()?;
            let payload_offset = offset.checked_add(4)?;
            if payload_offset > boundary.offset
                || data.get(offset) != Some(&0xe8)
                || data.get(offset + 3) != Some(&0x01)
            {
                return None;
            }
            let boundary_binds_field = boundary.end_offset().is_some_and(|next_offset| {
                data.get(next_offset) == Some(&0xe8)
                    && next_offset.checked_add(3).and_then(|at| data.get(at)) == Some(&0x01)
            });
            let boundary_closes_text = text_fields.iter().any(|field| {
                field.offset == offset
                    && field.entity_id == role.entity_id
                    && field.encoding == LegacyTextEncoding::U8InclusiveLengthE3RoleTail
                    && field.role.as_ref().is_some_and(|bound| bound == role)
                    && payload_offset
                        .checked_add(1)
                        .and_then(|value_offset| value_offset.checked_add(field.value.len()))
                        == Some(boundary.offset)
            });
            if !boundary_binds_field && !boundary_closes_text {
                return None;
            }
            Some((
                offset,
                role,
                boundary,
                View::u16_le_at(data, offset + 1)?,
                data.get(payload_offset..boundary.offset)?,
            ))
        })() else {
            continue;
        };
        let field = LegacySchemaField {
            offset,
            entity_id: role.entity_id,
            role_offset: role.offset,
            boundary_role_offset: boundary.offset,
            field_code,
            payload: ctx.copy_slice(payload, "catia_legacy_schema_field_payload")?,
        };
        ctx.push_vec(&mut fields, field, "catia_legacy_schema_fields")?;
    }
    Ok(fields)
}

fn parse_role_selectors(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    entity_id: u32,
) -> Result<Vec<LegacyRoleSelector>, CodecError> {
    charge_scan(
        ctx,
        (end - start)
            .checked_mul(3)
            .ok_or_else(|| ctx.refuse_codec_limit("catia_legacy_role_scan", u64::MAX, u64::MAX))?,
        "catia_legacy_role_scan",
    )?;
    let mut roles = Vec::new();
    for offset in start..end {
        let Some((name, encoding, selector)) = (|| {
            let inclusive_length = usize::from(*data.get(offset)?);
            if inclusive_length < 2 {
                return None;
            }
            let selector_offset = offset.checked_add(inclusive_length)?;
            if selector_offset >= end {
                return None;
            }
            let name = text_value(data.get(offset + 1..selector_offset)?)?;
            if !valid_identifier(name) {
                return None;
            }
            let first = *data.get(selector_offset)?;
            let (encoding, selector) = if first == 0x80 {
                let selector_end = selector_offset.checked_add(5)?;
                if selector_end > end {
                    return None;
                }
                (
                    LegacyRoleSelectorEncoding::FixedU32,
                    View::u32_le_at(data, selector_offset + 1)?,
                )
            } else if (0xd1..=0xe4).contains(&first) {
                if selector_offset.checked_add(2)? > end {
                    return None;
                }
                (
                    LegacyRoleSelectorEncoding::Paged,
                    u32::from(first - 0xd1)
                        .checked_mul(256)?
                        .checked_add(u32::from(*data.get(selector_offset + 1)?))?
                        .checked_add(1)?,
                )
            } else {
                return None;
            };
            (selector != 0).then_some((name, encoding, selector))
        })() else {
            continue;
        };
        let role = LegacyRoleSelector {
            offset,
            entity_id,
            name: LegacyRoleName::Literal(ctx.copy_retained_text(name, "catia_legacy_role_name")?),
            encoding,
            selector,
            field_code: None,
        };
        ctx.push_vec(&mut roles, role, "catia_legacy_roles")?;
    }
    for relative in memchr::memmem::find_iter(&data[start..end], TEXT_OPEN) {
        let Some(role) = (|| {
            let payload = start.checked_add(relative)?.checked_add(TEXT_OPEN.len())?;
            let value_length = usize::from(*data.get(payload)?).checked_sub(1)?;
            let role_offset = payload.checked_add(1)?.checked_add(value_length)?;
            let name_selector = *data.get(role_offset)?;
            let page_offset = role_offset.checked_add(1)?;
            let low_offset = role_offset.checked_add(2)?;
            if name_selector == 0 || data.get(page_offset) != Some(&0xe3) {
                return None;
            }
            // A declared one-byte text field owns its following FE terminator.
            // Do not reinterpret that same byte as an unresolved role selector
            // merely because E3 follows it (DI-25).
            if length_closed_text(data, payload + 1, value_length, end).is_some() {
                return None;
            }
            let selector_low = *data.get(low_offset)?;
            if role_offset.checked_add(3)? > end
                || text_value_allow_empty(data.get(payload.checked_add(1)?..role_offset)?).is_none()
            {
                return None;
            }
            Some(LegacyRoleSelector {
                offset: role_offset,
                entity_id,
                name: LegacyRoleName::Selector(name_selector),
                encoding: LegacyRoleSelectorEncoding::Paged,
                selector: u32::from(0xe3_u8 - 0xd1)
                    .checked_mul(256)?
                    .checked_add(u32::from(selector_low))?
                    .checked_add(1)?,
                field_code: None,
            })
        })() else {
            continue;
        };
        ctx.push_vec(&mut roles, role, "catia_legacy_roles")?;
    }
    let field_bound_roles = ctx.collect_vec(
        memchr::memchr_iter(0xe8, &data[start..end]).filter_map(|relative| {
            let field_offset = start.checked_add(relative)?;
            let field_header_end = field_offset.checked_add(4)?;
            if field_header_end > end || data.get(field_offset + 3) != Some(&0x01) {
                return None;
            }
            if roles
                .iter()
                .any(|role| role.end_offset() == Some(field_offset))
            {
                return None;
            }
            let fixed = field_offset
                .checked_sub(6)
                .filter(|offset| *offset >= start)
                .and_then(|role_offset| {
                    let name_selector = *data.get(role_offset)?;
                    (name_selector != 0 && data.get(role_offset + 1) == Some(&0x80))
                        .then_some(())?;
                    let selector = View::u32_le_at(data, role_offset + 2)?;
                    (selector != 0).then_some(LegacyRoleSelector {
                        offset: role_offset,
                        entity_id,
                        name: LegacyRoleName::Selector(name_selector),
                        encoding: LegacyRoleSelectorEncoding::FixedU32,
                        selector,
                        field_code: None,
                    })
                });
            let paged = field_offset
                .checked_sub(3)
                .filter(|offset| *offset >= start)
                .and_then(|role_offset| {
                    let name_selector = *data.get(role_offset)?;
                    let page = *data.get(role_offset + 1)?;
                    if name_selector == 0 || !(0xd1..=0xe4).contains(&page) {
                        return None;
                    }
                    let low = *data.get(role_offset + 2)?;
                    Some(LegacyRoleSelector {
                        offset: role_offset,
                        entity_id,
                        name: LegacyRoleName::Selector(name_selector),
                        encoding: LegacyRoleSelectorEncoding::Paged,
                        selector: u32::from(page - 0xd1)
                            .checked_mul(256)?
                            .checked_add(u32::from(low))?
                            .checked_add(1)?,
                        field_code: None,
                    })
                });
            // DI-24: fixed-width and paged selector layouts have no
            // precedence when both fit the same field boundary.
            match (fixed, paged) {
                (Some(_), Some(_)) => None,
                (Some(role), None) | (None, Some(role)) => Some(role),
                (None, None) => None,
            }
        }),
        "catia_legacy_bound_roles",
    )?;
    ctx.reserve_vec(&mut roles, field_bound_roles.len(), "catia_legacy_roles")?;
    roles.extend(field_bound_roles);
    for index in 1..roles.len() {
        let mut at = index;
        while at > 0 && roles[at - 1].offset > roles[at].offset {
            ctx.charge_work(1, "catia_legacy_role_sort")?;
            roles.swap(at - 1, at);
            at -= 1;
        }
    }
    roles.dedup_by_key(|role| role.offset);
    for role in &mut roles {
        role.field_code = role.end_offset().and_then(|offset| {
            (offset.checked_add(4)? <= end
                && data.get(offset) == Some(&0xe8)
                && data.get(offset + 3) == Some(&0x01))
            .then(|| View::u16_le_at(data, offset + 1))?
        });
    }
    Ok(roles)
}

pub(crate) fn valid_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

fn parse_text_field(data: &[u8], payload: usize, end: usize) -> Option<(LegacyTextEncoding, &str)> {
    let first = *data.get(payload)?;
    if first == 0 {
        if let Some(length) =
            View::u32_le_at(data, payload + 1).and_then(|n| usize::try_from(n).ok())
        {
            if let Some(value) = length_closed_text(data, payload + 5, length, end) {
                return Some((LegacyTextEncoding::ZeroU32Length, value));
            }
        }
    } else if let Some(length) = usize::from(first).checked_sub(1) {
        if let Some(value) = length_closed_text(data, payload + 1, length, end) {
            return Some((LegacyTextEncoding::U8InclusiveLength, value));
        }
        if let Some(value) = role_tailed_text(data, payload + 1, length, end) {
            return Some((LegacyTextEncoding::U8InclusiveLengthE3RoleTail, value));
        }
    }
    None
}

fn role_tailed_text(data: &[u8], start: usize, length: usize, end: usize) -> Option<&str> {
    let value_end = start.checked_add(length)?;
    if value_end >= end {
        return None;
    }
    if value_end.checked_add(3)? <= end
        && data.get(value_end).is_some_and(|selector| *selector != 0)
        && data.get(value_end + 1) == Some(&0xe3)
    {
        return text_value_allow_empty(data.get(start..value_end)?);
    }
    let role_length = usize::from(*data.get(value_end)?);
    if role_length < 2 {
        return None;
    }
    let separator = value_end.checked_add(role_length)?;
    let tail_end = separator.checked_add(2)?;
    if tail_end > end
        || data.get(separator) != Some(&0xe3)
        || !text_value(data.get(value_end + 1..separator)?).is_some_and(valid_identifier)
    {
        return None;
    }
    text_value_allow_empty(data.get(start..value_end)?)
}

fn length_closed_text(data: &[u8], start: usize, length: usize, end: usize) -> Option<&str> {
    let value_end = start.checked_add(length)?;
    if length == 0 || value_end >= end || data.get(value_end) != Some(&0xfe) {
        return None;
    }
    text_value(data.get(start..value_end)?)
}

fn text_value(bytes: &[u8]) -> Option<&str> {
    (!bytes.is_empty()).then_some(())?;
    text_value_allow_empty(bytes)
}

fn text_value_allow_empty(bytes: &[u8]) -> Option<&str> {
    let value = std::str::from_utf8(bytes).ok()?;
    value
        .chars()
        .all(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
        .then_some(value)
}

#[cfg(test)]
mod tests;
