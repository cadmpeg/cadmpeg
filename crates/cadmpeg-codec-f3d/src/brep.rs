// SPDX-License-Identifier: Apache-2.0
//! Wrap the format-independent ASM B-rep graph with Fusion-derived links,
//! blob-scoped id qualification, and Design body-map selector resolution.

use crate::records::{
    recipes::CreationTimestamp,
    sketch_links::{PersistentDesignLink, PersistentSubentityTag, SketchCurveLink},
};
use cadmpeg_asm::brep::attributes::attribute_key;
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_asm::brep::{decode_with_header, decode_with_purpose, AsmBrep, DecodePurpose};
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_asm::sab::Record;
use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::BodyId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod graph_ops;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum BodySelectorKey {
    Native(u64),
    Ordinal(u32),
}

impl cadmpeg_core::decode::cost::DecodeCost for BodySelectorKey {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));

    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

fn merge_brep_counts(
    ctx: &DecodeContext<'_>,
    target: &mut BTreeMap<String, usize>,
    source: BTreeMap<String, usize>,
) -> Result<(), CodecError> {
    for (kind, count) in ctx.admit_iter(source, "scan F3D BREP statistic kinds")? {
        if let Some(total) =
            ctx.get_mut_btree_map(target, kind.as_str(), "merge F3D BREP statistic kinds")?
        {
            *total = total.checked_add(count).ok_or_else(|| {
                ctx.refuse_codec_limit("merge F3D BREP statistic counts", 0, u64::MAX)
            })?;
        } else {
            ctx.insert_btree_map(target, kind, count, "merge F3D BREP statistic kinds")?;
        }
    }
    Ok(())
}

fn copy_attribute_target(
    ctx: &DecodeContext<'_>,
    target: &AttributeTarget,
) -> Result<AttributeTarget, CodecError> {
    Ok(match target {
        AttributeTarget::Document => AttributeTarget::Document,
        AttributeTarget::Body(id) => {
            AttributeTarget::Body(id.try_clone_for_decode(ctx, "copy F3D BREP attribute target")?)
        }
        AttributeTarget::Face(id) => {
            AttributeTarget::Face(id.try_clone_for_decode(ctx, "copy F3D BREP attribute target")?)
        }
        AttributeTarget::Shell(id) => {
            AttributeTarget::Shell(id.try_clone_for_decode(ctx, "copy F3D BREP attribute target")?)
        }
        AttributeTarget::Loop(id) => {
            AttributeTarget::Loop(id.try_clone_for_decode(ctx, "copy F3D BREP attribute target")?)
        }
        AttributeTarget::Coedge(id) => {
            AttributeTarget::Coedge(id.try_clone_for_decode(ctx, "copy F3D BREP attribute target")?)
        }
        AttributeTarget::Edge(id) => {
            AttributeTarget::Edge(id.try_clone_for_decode(ctx, "copy F3D BREP attribute target")?)
        }
        AttributeTarget::Vertex(id) => {
            AttributeTarget::Vertex(id.try_clone_for_decode(ctx, "copy F3D BREP attribute target")?)
        }
    })
}

/// The ASM B-rep graph plus links derived from Fusion attribute records.
///
/// Kernel arenas form the top-level structural projection.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Brep {
    /// The format-independent ASM graph.
    #[serde(flatten)]
    pub(crate) asm: AsmBrep,
    /// Typed sketch-curve provenance links.
    pub(crate) sketch_curve_links: Vec<SketchCurveLink>,
    /// Persistent design identifiers attached to solved entities.
    pub(crate) persistent_design_links: Vec<PersistentDesignLink>,
    /// Variable-width persistent tag groups attached to solved faces and edges.
    pub(crate) persistent_subentity_tags: Vec<PersistentSubentityTag>,
    /// Original authoring times attached to solved entities.
    pub(crate) creation_timestamps: Vec<CreationTimestamp>,
}

impl Brep {
    /// Wrap a decoded ASM graph and derive the Fusion attribute links.
    fn from_asm(ctx: &DecodeContext<'_>, asm: AsmBrep) -> Result<Self, CodecError> {
        let mut sketch_curve_links = Vec::new();
        let mut persistent_design_links = Vec::new();
        let mut persistent_subentity_tags = Vec::new();
        let mut creation_timestamps = Vec::new();
        for attribute in ctx.admit_iter(&asm.attributes, "scan F3D BREP attributes")? {
            if let Some(link) = sketch_curve_link(ctx, attribute)? {
                ctx.push_vec(
                    &mut sketch_curve_links,
                    link,
                    "collect F3D sketch curve links",
                )?;
            }
            ctx.extend_vec(
                &mut persistent_design_links,
                self::persistent_design_links(ctx, attribute)?,
                "collect F3D persistent design links",
            )?;
            ctx.extend_vec(
                &mut persistent_subentity_tags,
                self::persistent_subentity_tags(ctx, attribute)?,
                "collect F3D persistent subentity tags",
            )?;
            if let Some(timestamp) = creation_timestamp(ctx, attribute)? {
                ctx.push_vec(
                    &mut creation_timestamps,
                    timestamp,
                    "collect F3D creation timestamps",
                )?;
            }
        }
        Ok(Self {
            asm,
            sketch_curve_links,
            persistent_design_links,
            persistent_subentity_tags,
            creation_timestamps,
        })
    }

    /// Map solved bodies to the selector used by this blob's Design body map.
    pub(crate) fn body_selectors<'ctx>(
        &self,
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<
        (
            BTreeMap<BodyId, u64>,
            cadmpeg_core::decode::ScopedReservation<'ctx>,
        ),
        CodecError,
    > {
        let mut storage = ctx.reserve_scoped(0, "stage F3D BREP body selectors")?;
        let ordinal_mode = ctx.all_by(
            &self.asm.body_native_keys,
            |body| Ok(body.asm_body_key.is_none()),
            "scan F3D ordinal body key mode",
        )?;
        let mut selectors = BTreeMap::new();
        for body in ctx.admit_iter(&self.asm.body_native_keys, "scan F3D body keys")? {
            let selector = if ordinal_mode {
                Some(u64::from(body.body_ordinal))
            } else {
                body.asm_body_key
            };
            let Some(selector) = selector else {
                continue;
            };
            let id = storage
                .with_storage(|| body.body.try_clone_for_decode(ctx, "copy F3D BREP body ID"))?;

            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut selectors,
                    id,
                    selector,
                    "index F3D BREP body selectors",
                )
            })?;
        }
        Ok((selectors, storage))
    }

    /// Resolve the Design selectors present for this blob. An exact native
    /// body key has precedence. A selector absent from the native-key domain
    /// selects the body with the same zero-based ordinal.
    pub(crate) fn body_selectors_for<'ctx>(
        &self,
        ctx: &'ctx DecodeContext<'_>,
        selectors: &BTreeSet<u64>,
    ) -> Result<
        (
            BTreeMap<BodyId, u64>,
            cadmpeg_core::decode::ScopedReservation<'ctx>,
        ),
        cadmpeg_core::CodecError,
    > {
        let mut storage = ctx.reserve_scoped(0, "stage F3D selected BREP bodies")?;
        let mut resolved = BTreeMap::new();
        ctx.charge_work(0, "scan F3D body selectors")?;
        if selectors.is_empty() {
            return Ok((resolved, storage));
        }
        let (body_keys, _body_keys_storage) = ctx.unique_index(
            ctx.admit_iter(&self.asm.body_native_keys, "scan F3D indexed body keys")?
                .flat_map(|body| {
                    [
                        body.asm_body_key
                            .map(|key| (BodySelectorKey::Native(key), &body.body)),
                        Some((BodySelectorKey::Ordinal(body.body_ordinal), &body.body)),
                    ]
                    .into_iter()
                    .flatten()
                }),
            "index F3D native body selectors",
        )?;
        for selector in ctx.admit_iter(selectors, "scan F3D body selectors")? {
            let native = ctx.get_hash_map(
                &body_keys,
                &BodySelectorKey::Native(*selector),
                "match F3D native body selector",
            )?;
            let (body, domain) = match native {
                Some(body) => (Some(body), "native body keys"),
                None => {
                    let Ok(ordinal) = u32::try_from(*selector) else {
                        continue;
                    };
                    (
                        ctx.get_hash_map(
                            &body_keys,
                            &BodySelectorKey::Ordinal(ordinal),
                            "match F3D ordinal body selector",
                        )?,
                        "body ordinals",
                    )
                }
            };
            let Some(body) = body else {
                continue;
            };
            let Some(body) = *body else {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("F3D body selector {selector} matches multiple {domain}"),
                    "describe ambiguous F3D body selector",
                )?));
            };
            let id =
                storage.with_storage(|| body.try_clone_for_decode(ctx, "copy F3D BREP body ID"))?;

            if let Some(previous) = storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut resolved,
                    id,
                    *selector,
                    "index F3D selected BREP bodies",
                )
            })? {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!(
                        "F3D body {} is selected by both {previous} and {selector}",
                        body.as_str()
                    ),
                    "describe conflicting F3D body selectors",
                )?));
            }
        }
        Ok((resolved, storage))
    }

    /// Append a disjoint, already-qualified BREP graph.
    pub(crate) fn append(
        &mut self,
        ctx: &DecodeContext<'_>,
        other: Self,
    ) -> Result<(), CodecError> {
        let mut asm = other.asm;
        macro_rules! append_vecs {
            ($($field:ident),+ $(,)?) => {
                $(ctx.append_vec(&mut self.asm.$field, &mut asm.$field, concat!("merge F3D BREP ", stringify!($field)))?;)+
            };
        }
        append_vecs!(
            bodies,
            regions,
            shells,
            faces,
            loops,
            coedges,
            edges,
            vertices,
            points,
            surfaces,
            curves,
            pcurves,
            procedural_surfaces,
            procedural_curves,
            edge_continuities,
            edge_ownerships,
            vertex_ownerships,
            face_sidedness,
            face_native_keys,
            tolerant_coedge_parameters,
            tolerant_edge_tails,
            tolerant_vertex_tails,
            mesh_surface_sentinels,
            transform_hints,
            body_native_keys,
            wire_topologies,
            attributes,
            unknowns,
            annotation_records,
        );
        self.asm.stats.mesh_surface_faces += asm.stats.mesh_surface_faces;
        self.asm.stats.nurbs_surfaces += asm.stats.nurbs_surfaces;
        self.asm.stats.nurbs_curves += asm.stats.nurbs_curves;
        self.asm.stats.partial_procedural_supports += asm.stats.partial_procedural_supports;
        macro_rules! merge_count_maps {
            ($($field:ident),+ $(,)?) => {
                $(merge_brep_counts(ctx, &mut self.asm.stats.$field, std::mem::take(&mut asm.stats.$field))?;)+
            };
        }
        merge_count_maps!(
            missing_face_surface_kinds,
            unknown_surface_kinds,
            procedural_curve_kinds,
            undecoded_pcurve_kinds,
            other_record_kinds,
        );
        let mut other = Self { asm, ..other };
        macro_rules! append_derived {
            ($($field:ident),+ $(,)?) => {
                $(ctx.append_vec(&mut self.$field, &mut other.$field, concat!("merge F3D BREP ", stringify!($field)))?;)+
            };
        }
        append_derived!(
            sketch_curve_links,
            persistent_design_links,
            persistent_subentity_tags,
            creation_timestamps,
        );
        Ok(())
    }
}

/// Decode a framed active slice into the IR B-rep graph.
///
/// `stream` names the source ZIP entry for provenance. Ids are minted as
/// `<format>:brep:entity#<record-index>`, unique across the `RecordTable`.
pub(crate) fn decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    bytes: &[u8],
    stream: &str,
    format: IdFormat,
) -> Result<Brep, cadmpeg_core::CodecError> {
    Brep::from_asm(
        ctx,
        decode_with_purpose(ctx, records, bytes, stream, format, DecodePurpose::Model)?,
    )
}

/// Decode a parsed text stream ([`cadmpeg_asm::sat`]) into the IR B-rep graph.
///
/// The text parser already typed the records and converted lengths into the
/// binary centimetre convention, so the shared decode path runs unchanged.
/// The header comes from the stream's ASCII header lines rather than a binary
/// header parse of `bytes`.
pub(crate) fn decode_text(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    stream: &cadmpeg_asm::sat::TextStream,
    bytes: &[u8],
    entry: &str,
    format: IdFormat,
) -> Result<Brep, cadmpeg_core::CodecError> {
    Brep::from_asm(
        ctx,
        decode_with_header(
            ctx,
            &stream.records,
            bytes,
            Some(&stream.header.as_kernel_header(ctx)?),
            entry,
            format,
            DecodePurpose::Model,
        )?,
    )
}

/// Decode only the topology and analytic measurements used to bind ASM
/// history. Free-form carrier shapes are not materialized because historical
/// binding consumes their stable record identities, not their control data.
pub(crate) fn decode_history_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    bytes: &[u8],
    format: IdFormat,
) -> Result<Brep, cadmpeg_core::CodecError> {
    Brep::from_asm(
        ctx,
        decode_with_purpose(
            ctx,
            records,
            bytes,
            "history",
            format,
            DecodePurpose::History,
        )?,
    )
}

/// Resolve one Design body selector within one BREP blob. Exact native keys
/// take precedence; an absent key falls back to the zero-based body ordinal.
pub(crate) fn resolve_body_selector<'a>(
    ctx: &DecodeContext<'_>,
    body_keys: impl Iterator<Item = &'a BodyNativeKey> + Clone,
    selector: u64,
) -> Result<Option<&'a BodyId>, cadmpeg_core::CodecError> {
    let mut direct = None;
    for body in body_keys.clone() {
        ctx.charge_work(1, "match F3D native body selector")?;
        if body.asm_body_key == Some(selector) {
            if direct.is_some() {
                return Err(CodecError::malformed(format_args!(
                    "F3D body selector {selector} matches multiple native body keys"
                )));
            }
            direct = Some(&body.body);
        }
    }
    if direct.is_some() {
        return Ok(direct);
    }
    let Ok(ordinal) = u32::try_from(selector) else {
        return Ok(None);
    };
    let mut matched = None;
    for body in body_keys {
        ctx.charge_work(1, "match F3D ordinal body selector")?;
        if body.body_ordinal == ordinal {
            if matched.is_some() {
                return Err(CodecError::malformed(format_args!(
                    "F3D body selector {selector} matches multiple body ordinals"
                )));
            }
            matched = Some(&body.body);
        }
    }
    Ok(matched)
}

/// The five members every `sketch_attrib_def` payload form writes.
struct SketchLinkPayload {
    sketch_curve_id: i64,
    ref_b: u64,
    sense: i64,
    role: i64,
    closure: i64,
}

/// Read the payload following a `sketch_attrib_def` family name.
///
/// The three header integers are `1`, `1`, and a form selector. Form `3` writes
/// the members as one tagged ASCII field with a `0` between the sense and the
/// role, form `2` as six integers with a trailing `0`, and form `0` as the five
/// members alone. All three write the same five members in the same order, so
/// each yields one link.
fn sketch_link_payload(
    ctx: &DecodeContext<'_>,
    values: &[AttributeValue],
) -> Result<Option<SketchLinkPayload>, CodecError> {
    let [AttributeValue::Integer(1), AttributeValue::Integer(1), AttributeValue::Integer(form), payload @ ..] =
        values
    else {
        return Ok(None);
    };
    Ok(match (*form, payload) {
        (3, [AttributeValue::String(field)]) => {
            let mut bytes = field.as_bytes().iter().enumerate();
            let mut fields = [None; 7];
            for member in &mut fields {
                let Some((start, _)) = ctx.find_by(
                    &mut bytes,
                    |(_, byte)| Ok(!byte.is_ascii_whitespace()),
                    "scan F3D sketch link fields",
                )?
                else {
                    continue;
                };
                let end = ctx
                    .find_by(
                        &mut bytes,
                        |(_, byte)| Ok(byte.is_ascii_whitespace()),
                        "scan F3D sketch link fields",
                    )?
                    .map_or(field.len(), |(end, _)| end);
                *member = Some(&field[start..end]);
            }
            let [Some(sketch_curve_id), Some(ref_b), Some(sense), Some("0"), Some(role), Some(closure), None] =
                fields
            else {
                return Ok(None);
            };
            // `ref_b` reaches the full unsigned 64-bit range, so it is read
            // unsigned; every other member is signed.
            Some(SketchLinkPayload {
                sketch_curve_id: match ctx
                    .parse_text(sketch_curve_id, "parse F3D sketch link sketch_curve_id")?
                {
                    Ok(value) => value,
                    Err(_) => return Ok(None),
                },
                ref_b: match ctx.parse_text(ref_b, "parse F3D sketch link ref_b")? {
                    Ok(value) => value,
                    Err(_) => return Ok(None),
                },
                sense: match ctx.parse_text(sense, "parse F3D sketch link sense")? {
                    Ok(value) => value,
                    Err(_) => return Ok(None),
                },
                role: match ctx.parse_text(role, "parse F3D sketch link role")? {
                    Ok(value) => value,
                    Err(_) => return Ok(None),
                },
                closure: match ctx.parse_text(closure, "parse F3D sketch link closure")? {
                    Ok(value) => value,
                    Err(_) => return Ok(None),
                },
            })
        }
        (
            2,
            [AttributeValue::Integer(sketch_curve_id), AttributeValue::Integer(ref_b), AttributeValue::Integer(sense), AttributeValue::Integer(role), AttributeValue::Integer(closure), AttributeValue::Integer(0)],
        )
        | (
            0,
            [AttributeValue::Integer(sketch_curve_id), AttributeValue::Integer(ref_b), AttributeValue::Integer(sense), AttributeValue::Integer(role), AttributeValue::Integer(closure)],
        ) => Some(SketchLinkPayload {
            sketch_curve_id: *sketch_curve_id,
            ref_b: match u64::try_from(*ref_b) {
                Ok(value) => value,
                Err(_) => return Ok(None),
            },
            sense: *sense,
            role: *role,
            closure: *closure,
        }),
        _ => None,
    })
}

/// Locate a Fusion family marker under the caller's scan and comparison allowance.
fn attribute_family(
    ctx: &DecodeContext<'_>,
    attribute: &SourceAttribute,
    family: &str,
) -> Result<Option<usize>, CodecError> {
    ctx.position_by(
        &attribute.values,
        |value| match value {
            AttributeValue::String(name) => {
                ctx.equal(name.as_str(), family, "compare Fusion attribute family")
            }
            _ => Ok(false),
        },
        "scan Fusion attribute family",
    )
}

fn sketch_curve_link(
    ctx: &DecodeContext<'_>,
    attribute: &SourceAttribute,
) -> Result<Option<SketchCurveLink>, CodecError> {
    let Some(family) = attribute_family(ctx, attribute, "sketch_attrib_def")? else {
        return Ok(None);
    };
    let Some(payload) = sketch_link_payload(ctx, &attribute.values[family + 1..])? else {
        return Ok(None);
    };
    Ok(Some(SketchCurveLink {
        id: ctx.format_retained(
            format_args!("f3d:design:sketch-curve-link#{}", attribute_key(attribute)),
            "retain F3D sketch curve link ID",
        )?,
        target: copy_attribute_target(ctx, &attribute.target)?,
        sketch_curve_id: payload.sketch_curve_id,
        ref_b: payload.ref_b,
        sense: crate::records::sketch_links::SketchLinkSense::try_from(payload.sense).ok(),
        role: payload.role,
        closure: payload.closure,
    }))
}

fn persistent_design_links(
    ctx: &DecodeContext<'_>,
    attribute: &SourceAttribute,
) -> Result<Vec<PersistentDesignLink>, CodecError> {
    let AttributeTarget::Body(_) = &attribute.target else {
        return Ok(Vec::new());
    };
    let Some(GenericTagPayload {
        version,
        group_count,
        rest,
    }) = generic_tag_payload(ctx, attribute)?
    else {
        return Ok(Vec::new());
    };
    let group_width = match version {
        GenericTagVersion::V2 => 4,
        GenericTagVersion::V3 => 5,
    };
    if group_count.checked_mul(group_width) != Some(rest.len()) {
        return Ok(Vec::new());
    }
    let mut links = Vec::new();
    let mut groups = rest.chunks_exact(group_width);
    while let Some(values) = ctx.next_charged(&mut groups, "scan F3D persistent design groups")? {
        let (entity_kind, design_id, design_reference) = match values {
            [AttributeValue::Integer(entity_kind), AttributeValue::String(design_id), AttributeValue::Integer(design_reference), AttributeValue::Integer(0)]
            | [AttributeValue::Integer(entity_kind), AttributeValue::String(design_id), AttributeValue::Integer(design_reference), AttributeValue::Integer(0), AttributeValue::Integer(0)] => {
                (*entity_kind, design_id, *design_reference)
            }
            _ => return Ok(Vec::new()),
        };
        let design_id_text = ctx.copy_retained_text(design_id, "copy F3D persistent design ID")?;
        let Ok(design_id) =
            crate::records::sketch_links::DesignPersistentIdText::try_from(design_id_text)
        else {
            return Ok(Vec::new());
        };
        if entity_kind != 3 {
            continue;
        }
        let ordinal = u32::try_from(links.len()).map_err(|_| {
            CodecError::Malformed("F3D persistent design link ordinal overflows".into())
        })?;
        let link = PersistentDesignLink {
            id: ctx.format_retained(
                format_args!(
                    "f3d:design:persistent-design-link#{}:{ordinal}",
                    attribute_key(attribute)
                ),
                "retain F3D persistent design link ID",
            )?,
            target: copy_attribute_target(ctx, &attribute.target)?,
            design_id,
            design_reference,
            ordinal,
        };
        ctx.push_vec(&mut links, link, "collect F3D attribute design links")?;
    }
    Ok(links)
}

fn persistent_subentity_tags(
    ctx: &DecodeContext<'_>,
    attribute: &SourceAttribute,
) -> Result<Vec<PersistentSubentityTag>, CodecError> {
    if !matches!(
        attribute.target,
        AttributeTarget::Face(_) | AttributeTarget::Edge(_)
    ) {
        return Ok(Vec::new());
    }
    let Some(GenericTagPayload {
        version,
        group_count,
        rest,
    }) = generic_tag_payload(ctx, attribute)?
    else {
        return Ok(Vec::new());
    };
    // Each group consumes at least four leading attribute values from `rest`.
    let Ok(group_count) = u64::try_from(group_count) else {
        return Ok(Vec::new());
    };
    let Some(group_count) = bounded_len(group_count, 4, rest.len()) else {
        return Ok(Vec::new());
    };
    let mut position: usize = 0;
    let mut groups = Vec::new();
    for ordinal in ctx.admit_iter(&(0..group_count), "visit F3D persistent subentity groups")? {
        let Some(
            [AttributeValue::Integer(selector), AttributeValue::String(token), AttributeValue::Integer(0), AttributeValue::Integer(reference_count)],
        ) = rest.get(position..position + 4)
        else {
            return Ok(Vec::new());
        };
        let Some(token) = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            ctx.copy_retained_text(token, "copy F3D persistent subentity token")?,
            "validate nonblank text",
        )?
        else {
            return Ok(Vec::new());
        };
        if *reference_count < 0 {
            return Ok(Vec::new());
        }
        let Ok(reference_count) = usize::try_from(*reference_count) else {
            return Ok(Vec::new());
        };
        let reference_start = position + 4;
        let Some(reference_end) = reference_start.checked_add(reference_count) else {
            return Ok(Vec::new());
        };
        let Some(reference_values) = rest.get(reference_start..reference_end) else {
            return Ok(Vec::new());
        };
        let mut design_references = Vec::new();
        let mut references = reference_values.iter();
        while let Some(value) =
            ctx.next_charged(&mut references, "scan F3D persistent subentity references")?
        {
            let AttributeValue::Integer(value) = value else {
                return Ok(Vec::new());
            };
            ctx.push_vec(
                &mut design_references,
                *value,
                "collect F3D subentity references",
            )?;
        }
        if matches!(version, GenericTagVersion::V3) {
            if !matches!(rest.get(reference_end), Some(AttributeValue::Integer(0))) {
                return Ok(Vec::new());
            }
            position = reference_end + 1;
        } else {
            position = reference_end;
        }
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            CodecError::Malformed("F3D persistent subentity ordinal overflows".into())
        })?;
        let group = PersistentSubentityTag {
            id: ctx.format_retained(
                format_args!(
                    "f3d:design:persistent-subentity-tag#{}:{ordinal}",
                    attribute_key(attribute)
                ),
                "retain F3D persistent subentity tag ID",
            )?,
            target: copy_attribute_target(ctx, &attribute.target)?,
            selector: *selector,
            token,
            design_references,
            ordinal,
        };
        ctx.push_vec(&mut groups, group, "collect F3D subentity tag groups")?;
    }
    if position != rest.len() {
        return Ok(Vec::new());
    }
    Ok(groups)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GenericTagVersion {
    V2,
    V3,
}

/// Generic-tag fields after the equal supported envelope versions are checked.
struct GenericTagPayload<'a> {
    version: GenericTagVersion,
    group_count: usize,
    rest: &'a [AttributeValue],
}

/// Return the common generic-tag version, group count, and payload.
///
/// The two leading integers are an equal envelope version. Versions two and
/// three select distinct, bounded group envelopes; a mixed or unsupported
/// pair is not a generic-tag envelope.
fn generic_tag_payload<'a>(
    ctx: &DecodeContext<'_>,
    attribute: &'a SourceAttribute,
) -> Result<Option<GenericTagPayload<'a>>, CodecError> {
    let Some(family) = attribute_family(ctx, attribute, "generic_tag_attrib_def")? else {
        return Ok(None);
    };
    let Some(values) = attribute.values.get(family + 1..) else {
        return Ok(None);
    };
    let [AttributeValue::Integer(left_version), AttributeValue::Integer(right_version), AttributeValue::Integer(-1), AttributeValue::String(marker), AttributeValue::Integer(group_count), rest @ ..] =
        values
    else {
        return Ok(None);
    };
    if left_version != right_version
        || !ctx.equal(
            marker.as_str(),
            "generic_tag_attrib_def ",
            "compare Fusion generic-tag marker",
        )?
        || *group_count < 0
    {
        return Ok(None);
    }
    let version = match *left_version {
        2 => GenericTagVersion::V2,
        3 => GenericTagVersion::V3,
        _ => return Ok(None),
    };
    let Ok(group_count) = usize::try_from(*group_count) else {
        return Ok(None);
    };
    Ok(Some(GenericTagPayload {
        version,
        group_count,
        rest,
    }))
}

fn creation_timestamp(
    ctx: &DecodeContext<'_>,
    attribute: &SourceAttribute,
) -> Result<Option<CreationTimestamp>, CodecError> {
    let Some(family) = attribute_family(ctx, attribute, "Timestamp_attrib_def")? else {
        return Ok(None);
    };
    let Some(marker) = attribute.values.get(family + 1) else {
        return Ok(None);
    };
    if !matches!(marker, AttributeValue::Integer(1)) {
        return Ok(None);
    }
    let Some(AttributeValue::Float(unix_microseconds)) = attribute.values.get(family + 2) else {
        return Ok(None);
    };
    let Some(record_index) = ctx
        .parse_text(attribute_key(attribute), "parse F3D timestamp record key")?
        .ok()
    else {
        return Ok(None);
    };
    Ok(Some(CreationTimestamp {
        id: ctx.format_retained(
            format_args!("f3d:design:creation-timestamp#{}", attribute_key(attribute)),
            "retain F3D creation timestamp ID",
        )?,
        target: copy_attribute_target(ctx, &attribute.target)?,
        record_index,
        unix_microseconds: *unix_microseconds,
    }))
}

#[cfg(test)]
mod tests;
