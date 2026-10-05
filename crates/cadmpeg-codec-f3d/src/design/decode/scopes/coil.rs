// SPDX-License-Identifier: Apache-2.0
//! Exact coil placements, discriminators and coil extents.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::bytes::f64s_at;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::operands::parse_entity_selection_frame;
use crate::design::decode::operands::parse_entity_selection_prefix;
use crate::design::decode::operands::parse_face_operand;
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::retain_class_tag;
use crate::layout::coil_compact_placement_identity_frame as coil_identity;
use crate::layout::coil_compact_placement_matrix_frame as coil_matrix;
use crate::layout::coil_compact_placement_owner_identity_frame as coil_owner_identity;
use crate::layout::coil_compact_scope_discriminators as coil_compact;
use crate::layout::coil_legacy_placement_identity_frame as coil_legacy_identity;
use crate::layout::coil_long_scope_fixed_prologue as coil_long;
use crate::layout::coil_modern_placement_matrix_frame as coil_modern_matrix;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::coil;
use crate::records::feature::coil::DesignCoilExtent;
use crate::records::feature::coil::DesignCoilPlacement;
use crate::records::feature::coil::DesignCoilSection;
use crate::records::feature::coil::DesignCoilSectionPlacement;
use crate::records::feature::coil::DesignCoilSelection;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::identity::Located;
use crate::records::parameters::DesignParameter;
use crate::records::recipes::ConstructionRecipe;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

const EPS_SCOPES_VALID_RIGHT_HANDED_COIL_TRANSFORM_E10: f64 = 1.0e-10;

/// Decode the fixed discriminator block of the closed Coil scope forms.
///
/// A scope whose envelope is valid but whose Coil dialect is not recognized
/// still belongs in the native arena. Returning `None` here leaves its
/// family-local fields unset without discarding the ordered references and
/// byte span that preserve the unsupported form.
pub(super) struct CoilDiscriminators {
    pub(super) operation: DesignExtrudeOperation,
    pub(super) operation_offset: u64,
    pub(super) extent: Option<crate::records::identity::MaybeRecordedValue<DesignCoilExtent>>,
    pub(super) section: DesignCoilSection,
    pub(super) section_offset: Option<u64>,
    pub(super) section_placement: DesignCoilSectionPlacement,
    pub(super) section_placement_offset: Option<u64>,
    pub(super) clockwise: bool,
    pub(super) clockwise_offset: Option<u64>,
}

/// Decode the two ordered placement carriers of a compact Coil form.
///
/// The first carrier is a nested support-selection frame. It may carry either
/// a persistent support identity or a face recipe. The second is a rigid frame
/// whose direct identity form omits the matrix and therefore has a 213-byte
/// span. The 442-byte scope form also has an owner-referenced identity carrier:
/// it appends a marked reference to the owning scope and has a 233-byte span.
/// The explicit form has the same fixed envelope plus 128 matrix bytes and a
/// 341-byte span. The legacy 427-byte scope form has a class-395 identity
/// carrier with a 186-byte span. The modern 427-byte scope form has a
/// class-450 matrix carrier with a 315-byte span. A malformed or ambiguous
/// carrier leaves the complete placement native.
pub(super) fn exact_coil_placement(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    recipes: &[ConstructionRecipe],
) -> Result<Option<DesignCoilPlacement>, CodecError> {
    let Some(carriers) = coil_placement_carriers(bytes, records, scope) else {
        return Ok(None);
    };
    let CoilPlacementCarriers {
        selection_record_index,
        selection_start,
        selection_class_tag,
        transform_record_index,
        transform_start,
        transform_class_tag,
        explicit_transform,
    } = carriers;
    let Ok(selection_class_text) = std::str::from_utf8(selection_class_tag) else {
        return Ok(None);
    };
    let persistent_frame = parse_entity_selection_frame(
        ctx,
        bytes,
        selection_record_index,
        u64_from_index(selection_start),
        selection_class_text,
    )?;
    let persistent_selection = persistent_frame.and_then(|selection| {
        Some(DesignCoilSelection::Persistent {
            asset_id: selection.asset_id.try_into().ok()?,
            context_id: selection.context_id.try_into().ok()?,
            identity_record_index: selection.identity_record_index,
            primary_identity: selection.primary_identity,
            secondary: selection.secondary.map(|identity| {
                crate::records::identity::DesignSecondaryIdentity {
                    identity: identity.identity.value,
                    curve_identity: identity.curve_identity.map(|identity| identity.value),
                }
            }),
        })
    });
    let selection = match persistent_selection {
        Some(selection) => selection,
        None => {
            let Some(selection) = exact_coil_face_selection(
                ctx,
                bytes,
                records,
                scope,
                (
                    selection_record_index,
                    selection_start,
                    selection_class_text,
                ),
                transform_start,
                recipes,
            )?
            else {
                return Ok(None);
            };
            selection
        }
    };
    Ok(Some(DesignCoilPlacement {
        selection_record_index,
        selection_record_byte_offset: u64_from_index(selection_start),
        selection_class_tag: retain_class_tag(ctx, *selection_class_tag, "copy F3D class tag")?,
        selection,
        transform_record_index,
        transform_record_byte_offset: u64_from_index(transform_start),
        transform_class_tag: retain_class_tag(ctx, *transform_class_tag, "copy F3D class tag")?,
        explicit_transform,
    }))
}

/// The selection and transform carriers named by the first two references of
/// a compact Coil scope, each the only frame of its record.
struct CoilPlacementCarriers<'bytes> {
    selection_record_index: u32,
    selection_start: usize,
    selection_class_tag: &'bytes [u8; 3],
    transform_record_index: u32,
    transform_start: usize,
    transform_class_tag: &'bytes [u8; 3],
    explicit_transform: Option<Located<SketchPlacementMatrix>>,
}

fn coil_placement_carriers<'bytes>(
    bytes: &'bytes [u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<CoilPlacementCarriers<'bytes>> {
    if !matches!(scope.payload(), DesignScopePayload::CoilPrimitive(_)) {
        return None;
    }
    match (
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length(),
        scope.reference_members().len(),
    ) {
        ("393", "258", 427, 8) => {}
        ("353", "259", 427, 8) => {}
        (_, _, 411, 7) if matches!(scope.coil_extent(), Some(DesignCoilExtent::Spiral)) => {}
        (_, _, 432 | 442, 8) => {}
        _ => return None,
    }
    let mut references = scope.reference_members().values();
    let selection_record_index = *references.next()?;
    let transform_record_index = *references.next()?;
    let (selection_start, _) = records.only_frame(selection_record_index)?;
    let selection_class_tag =
        exact_indexed_header_at(bytes, selection_start, selection_record_index)?;
    let (transform_start, transform_paired) = records.only_frame(transform_record_index)?;
    let transform_class_tag =
        exact_indexed_header_at(bytes, transform_start, transform_record_index)?;
    let transform_paired_class_tag =
        exact_indexed_header_at(bytes, transform_paired, transform_record_index)?;
    let frame_length = transform_paired.checked_sub(transform_start)?;
    let explicit_transform = match frame_length {
        coil_legacy_identity::LEN
            if scope.class_tag.as_str() == "393"
                && scope.paired_class_tag.as_str() == "258"
                && transform_class_tag == b"395"
                && transform_paired_class_tag == b"258"
                && exact_coil_legacy_identity_frame(
                    bytes,
                    transform_start,
                    transform_paired,
                    selection_record_index,
                    transform_record_index,
                    scope.record_index,
                ) =>
        {
            None
        }
        coil_modern_matrix::LEN
            if transform_class_tag == b"450"
                && transform_paired_class_tag == b"259"
                && exact_coil_modern_placement_matrix_frame(
                    bytes,
                    transform_start,
                    transform_paired,
                    selection_record_index,
                    transform_record_index,
                    scope.record_index,
                ) =>
        {
            Some(located_transform(
                bytes,
                transform_start + coil_modern_matrix::MATRIX,
            )?)
        }
        coil_identity::LEN
            if bytes.get(transform_start + coil_identity::PLACEMENT_MARKER) == Some(&1)
                && zeros_at::<9>(bytes, transform_start + coil_identity::IDENTITY_ZERO_RUN)
                && bytes.get(transform_start + coil_identity::IDENTITY_MARKER) == Some(&1) =>
        {
            None
        }
        coil_owner_identity::LEN
            if bytes.get(transform_start + coil_identity::PLACEMENT_MARKER) == Some(&1)
                && zeros_at::<9>(bytes, transform_start + coil_identity::IDENTITY_ZERO_RUN)
                && bytes.get(transform_start + coil_identity::IDENTITY_MARKER) == Some(&1)
                && zeros_at::<9>(bytes, transform_start + coil_identity::LEN)
                && bytes.get(transform_start + coil_owner_identity::OWNER_REFERENCE_MARKER)
                    == Some(&1)
                && View::u32_le_at(
                    bytes,
                    transform_start + coil_owner_identity::OWNER_SCOPE_RECORD_INDEX,
                ) == Some(scope.record_index)
                && zeros_at::<6>(
                    bytes,
                    transform_start + coil_owner_identity::OWNER_REFERENCE_TAIL,
                ) =>
        {
            None
        }
        coil_matrix::LEN
            if bytes.get(transform_start + coil_matrix::PLACEMENT_MARKER) == Some(&1)
                && zeros_at::<9>(bytes, transform_start + coil_matrix::EXPLICIT_ZERO_RUN)
                && bytes.get(transform_start + coil_matrix::EXPLICIT_FORM_MARKER) == Some(&0) =>
        {
            Some(located_transform(
                bytes,
                transform_start + coil_matrix::MATRIX,
            )?)
        }
        _ => return None,
    };
    if explicit_transform
        .as_ref()
        .is_some_and(|matrix| !valid_right_handed_coil_transform(&matrix.value))
    {
        return None;
    }
    Some(CoilPlacementCarriers {
        selection_record_index,
        selection_start,
        selection_class_tag,
        transform_record_index,
        transform_start,
        transform_class_tag,
        explicit_transform,
    })
}

/// The placement matrix at `at` and its offset.
fn located_transform(bytes: &[u8], at: usize) -> Option<Located<SketchPlacementMatrix>> {
    Some(Located {
        value: rigid_transform_at(bytes, at)?,
        offset: u64_from_index(at),
    })
}

fn exact_coil_modern_placement_matrix_frame(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    selection_record_index: u32,
    transform_record_index: u32,
    scope_record_index: u32,
) -> bool {
    paired_at.checked_sub(start) == Some(coil_modern_matrix::LEN)
        && zeros_at::<39>(bytes, start + 11)
        && zeros_at::<26>(bytes, start + coil_modern_matrix::MATRIX + 16 * 8)
        && View::u32_le_at(bytes, start + coil_modern_matrix::CONSTANT_512) == Some(512)
        && zeros_at::<4>(bytes, start + coil_modern_matrix::CONSTANT_512 + 4)
        && View::u32_le_at(bytes, start + coil_modern_matrix::CONSTANT_256) == Some(256)
        && zeros_at::<1>(bytes, start + coil_modern_matrix::CONSTANT_256 + 4)
        && marked_record_reference(bytes, start + coil_modern_matrix::SELECTION_REFERENCE)
            == Some(selection_record_index)
        && zeros_at::<2>(bytes, start + coil_modern_matrix::SELECTION_REFERENCE + 11)
        && View::u32_le_at(bytes, start + coil_modern_matrix::SELECTION_FLAG) == Some(1)
        && marked_record_reference(bytes, start + coil_modern_matrix::AUXILIARY_REFERENCE)
            == transform_record_index.checked_add(25)
        && zeros_at::<3>(bytes, start + coil_modern_matrix::AUXILIARY_REFERENCE + 11)
        && View::u64_le_at(bytes, start + coil_modern_matrix::CONSTANT_1024) == Some(1024)
        && View::u64_le_at(bytes, start + coil_modern_matrix::IDENTITY_LANE_PREFIX)
            == Some(0x7000_0000_0000_0000)
        && zeros_at::<4>(bytes, start + coil_modern_matrix::IDENTITY_LANE_PREFIX + 8)
        && View::u64_le_at(bytes, start + coil_modern_matrix::IDENTITY_LANE)
            .is_some_and(|value| value >> 56 == 0x70)
        && zeros_at::<3>(bytes, start + coil_modern_matrix::IDENTITY_LANE + 8)
        && marked_record_reference(bytes, start + coil_modern_matrix::SUCCESSOR_REFERENCE)
            == transform_record_index.checked_add(2)
        && zeros_at::<2>(bytes, start + coil_modern_matrix::SUCCESSOR_REFERENCE + 11)
        && marked_record_reference(bytes, start + coil_modern_matrix::PREDECESSOR_REFERENCE)
            == transform_record_index.checked_add(1)
        && zeros_at::<1>(
            bytes,
            start + coil_modern_matrix::PREDECESSOR_REFERENCE + 11,
        )
        && marked_record_reference(bytes, start + coil_modern_matrix::OWNER_REFERENCE)
            == Some(scope_record_index)
}

fn exact_coil_legacy_identity_frame(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    selection_record_index: u32,
    transform_record_index: u32,
    scope_record_index: u32,
) -> bool {
    let Some(auxiliary_record_index) = marked_record_reference(
        bytes,
        start + coil_legacy_identity::AUXILIARY_REFERENCE_MARKER,
    ) else {
        return false;
    };
    paired_at.checked_sub(start) == Some(coil_legacy_identity::LEN)
        && zeros_at::<37>(bytes, start + 11)
        && marked_record_reference(
            bytes,
            start + coil_legacy_identity::LEADING_REFERENCE_MARKER,
        ) == Some(0)
        && zeros_at::<17>(
            bytes,
            start + coil_legacy_identity::LEADING_REFERENCE_MARKER + 11,
        )
        && View::u32_le_at(bytes, start + coil_legacy_identity::PROLOGUE_VALUE) == Some(2)
        && zeros_at::<4>(bytes, start + coil_legacy_identity::PROLOGUE_VALUE + 4)
        && View::u32_le_at(bytes, start + coil_legacy_identity::PROLOGUE_FLAG) == Some(1)
        && marked_record_reference(
            bytes,
            start + coil_legacy_identity::SELECTION_REFERENCE_MARKER,
        ) == Some(selection_record_index)
        && zeros_at::<6>(
            bytes,
            start + coil_legacy_identity::SELECTION_RECORD_INDEX + 4,
        )
        && zeros_at::<2>(
            bytes,
            start + coil_legacy_identity::SELECTION_REFERENCE_MARKER + 11,
        )
        && View::u32_le_at(bytes, start + coil_legacy_identity::SELECTION_FLAG) == Some(1)
        && auxiliary_record_index != 0
        && auxiliary_record_index != selection_record_index
        && auxiliary_record_index != transform_record_index
        && auxiliary_record_index != scope_record_index
        && zeros_at::<6>(
            bytes,
            start + coil_legacy_identity::AUXILIARY_REFERENCE_MARKER + 5,
        )
        && zeros_at::<4>(
            bytes,
            start + coil_legacy_identity::AUXILIARY_REFERENCE_MARKER + 11,
        )
        && View::u32_le_at(bytes, start + coil_legacy_identity::TAIL_VALUE) == Some(4)
        && zeros_at::<10>(bytes, start + coil_legacy_identity::TAIL_VALUE + 4)
        && View::u32_le_at(bytes, start + coil_legacy_identity::INTERMEDIATE_SELECTOR) == Some(109)
        && View::f64_le_at(bytes, start + coil_legacy_identity::CARRIER_SCALAR)
            .is_some_and(|value| value.is_finite() && value > 0.0)
        && View::u32_le_at(bytes, start + coil_legacy_identity::TAIL_SELECTOR) == Some(109)
        && marked_record_reference(
            bytes,
            start + coil_legacy_identity::SUCCESSOR_REFERENCE_MARKER,
        ) == transform_record_index.checked_add(2)
        && zeros_at::<2>(
            bytes,
            start + coil_legacy_identity::SUCCESSOR_REFERENCE_MARKER + 11,
        )
        && marked_record_reference(
            bytes,
            start + coil_legacy_identity::PREDECESSOR_REFERENCE_MARKER,
        ) == transform_record_index.checked_add(1)
        && zeros_at::<6>(
            bytes,
            start + coil_legacy_identity::PREDECESSOR_REFERENCE_MARKER + 5,
        )
        && bytes.get(start + coil_legacy_identity::OWNER_REFERENCE_MARKER - 1) == Some(&0)
        && marked_record_reference(bytes, start + coil_legacy_identity::OWNER_REFERENCE_MARKER)
            == Some(scope_record_index)
}

/// The face-recipe support selection of a compact Coil placement: a face
/// operand whose recipe is among `recipes`, ending where the transform
/// carrier at `transform_start` begins.
fn exact_coil_face_selection(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    (selection_record_index, selection_start, selection_class_tag): (u32, usize, &str),
    transform_start: usize,
    recipes: &[ConstructionRecipe],
) -> Result<Option<DesignCoilSelection>, CodecError> {
    let Some(prefix) =
        parse_entity_selection_prefix(ctx, bytes, selection_start, selection_record_index)?
    else {
        return Ok(None);
    };
    // The face-operand parser reads the selection header; its copies of the
    // scope ID and class tag, and the parsed operand, are dropped when this
    // search returns.
    let mut header_storage = ctx.reserve_scoped(0, "f3d Coil selection header")?;
    let id = ctx.copy_scoped_text(
        &scope.id,
        &mut header_storage,
        "f3d Coil selection header ID",
    )?;
    let Ok(class_tag) = ctx
        .copy_scoped_text(
            selection_class_tag,
            &mut header_storage,
            "copy F3D Coil face-selection class tag",
        )?
        .try_into()
    else {
        return Ok(None);
    };
    let header = DesignRecordHeader {
        id,
        byte_offset: u64_from_index(selection_start),
        class_tag,
        record_index: selection_record_index,
    };
    let Some(face) = header_storage.with_storage(|| {
        parse_face_operand(
            ctx,
            bytes,
            records,
            crate::design::decode::operands::FaceOperandFrame {
                scope,
                scope_reference_ordinal: 0,
                group_ownership: None,
                next_byte_offset: Some(u64_from_index(transform_start)),
                header: &header,
            },
            recipes,
        )
    })?
    else {
        return Ok(None);
    };
    if face.next_byte_offset() != u64_from_index(transform_start) {
        return Ok(None);
    }
    let operation = "find F3D Coil face construction recipe";
    let Some(recipe) = ctx.find_by(
        recipes,
        |recipe| ctx.equal_bytes(recipe.id.as_bytes(), face.recipe_id.as_bytes(), operation),
        operation,
    )?
    else {
        return Ok(None);
    };
    let (Ok(asset_id), Ok(context_id), Ok(recipe_kind)) = (
        prefix.asset_id.try_into(),
        prefix.context_id.try_into(),
        scope::DesignFaceRecipeKind::try_from(recipe.kind),
    ) else {
        return Ok(None);
    };
    let recipe_id = ctx.copy_retained_text(&recipe.id, "f3d Coil face recipe ID")?;
    let design = match recipe.design.as_ref() {
        Some(design) => Some(crate::records::recipes::ConstructionRecipeDesign {
            id: ctx.copy_retained_text(&design.id.value, "copy F3D Coil recipe design ID")?,
            selector: design.selector,
        }),
        None => None,
    };
    Ok(Some(DesignCoilSelection::FaceRecipe {
        asset_id,
        context_id,
        recipe_record_index: face.recipe_record_index(),
        recipe_record_byte_offset: face.recipe_record_byte_offset(),
        recipe_id,
        recipe_kind,
        design,
    }))
}

fn valid_right_handed_coil_transform(
    transform: &crate::records::sketch_placement::SketchPlacementMatrix,
) -> bool {
    let radial = [transform[0][0], transform[1][0], transform[2][0]];
    let tangent = [transform[0][1], transform[1][1], transform[2][1]];
    let axis = [transform[0][2], transform[1][2], transform[2][2]];
    let cross = [
        radial[1] * tangent[2] - radial[2] * tangent[1],
        radial[2] * tangent[0] - radial[0] * tangent[2],
        radial[0] * tangent[1] - radial[1] * tangent[0],
    ];
    cross
        .into_iter()
        .zip(axis)
        .map(|(left, right)| left * right)
        .sum::<f64>()
        > 1.0 - EPS_SCOPES_VALID_RIGHT_HANDED_COIL_TRANSFORM_E10
}

pub(super) fn exact_coil_discriminators(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    kind: &scope::DesignFeatureKind,
    reference_members: &[u32],
) -> Option<CoilDiscriminators> {
    if let Some(fields) =
        exact_long_coil_discriminators(bytes, start, paired_at, kind, reference_members)
    {
        return Some(fields);
    }
    let operation_offset = start.checked_add(coil_compact::OPERATION)?;
    let operation = match (kind, View::u32_le_at(bytes, operation_offset)?) {
        (&scope::DesignFeatureKind::SpirePrimitive, 1) => DesignExtrudeOperation::Join,
        (&scope::DesignFeatureKind::SpirePrimitive, 2) => DesignExtrudeOperation::Cut,
        (&scope::DesignFeatureKind::SpirePrimitive, 3) => DesignExtrudeOperation::Intersect,
        (&scope::DesignFeatureKind::SpirePrimitive, 4)
        | (&scope::DesignFeatureKind::CoilPrimitive, 1) => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let clockwise_offset = start.checked_add(coil_compact::CLOCKWISE)?;
    let clockwise = match bytes.get(clockwise_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let structural_constant = match kind {
        scope::DesignFeatureKind::SpirePrimitive => 2,
        scope::DesignFeatureKind::CoilPrimitive => 4,
        _ => return None,
    };
    if View::u32_le_at(bytes, start.checked_add(coil_compact::STRUCTURAL_CONSTANT)?)?
        != structural_constant
    {
        return None;
    }
    let extent_offset = start.checked_add(coil_compact::EXTENT)?;
    let extent = match View::u32_le_at(bytes, extent_offset)? {
        1 => DesignCoilExtent::RevolutionsHeight,
        2 => DesignCoilExtent::RevolutionsPitch,
        3 => DesignCoilExtent::HeightPitch,
        4 => DesignCoilExtent::Spiral,
        _ => return None,
    };
    let section_offset = start.checked_add(coil_compact::SECTION_PLACEMENT)?;
    let section_placement_offset = start.checked_add(coil_compact::SECTION_SHAPE)?;
    let (section, section_placement) = match kind {
        scope::DesignFeatureKind::SpirePrimitive => (
            match View::u32_le_at(bytes, section_offset)? {
                0 => DesignCoilSection::Circular,
                1 => DesignCoilSection::Square,
                2 => DesignCoilSection::ExternalTriangle,
                3 => DesignCoilSection::InternalTriangle,
                _ => return None,
            },
            match View::u32_le_at(bytes, section_placement_offset)? {
                4 => DesignCoilSectionPlacement::Inside,
                _ => return None,
            },
        ),
        // The compact Coil dialect stores the two discriminators in the
        // opposite lanes from SpirePrimitive: position at offset 92 and
        // section shape at offset 107.
        scope::DesignFeatureKind::CoilPrimitive => (
            match View::u32_le_at(bytes, section_placement_offset)? {
                1 => DesignCoilSection::Circular,
                2 => DesignCoilSection::Square,
                3 => DesignCoilSection::ExternalTriangle,
                4 => DesignCoilSection::InternalTriangle,
                _ => return None,
            },
            match View::u32_le_at(bytes, section_offset)? {
                1 => DesignCoilSectionPlacement::Inside,
                2 => DesignCoilSectionPlacement::Center,
                3 => DesignCoilSectionPlacement::Outside,
                _ => return None,
            },
        ),
        _ => return None,
    };
    Some(CoilDiscriminators {
        operation,
        operation_offset: u64_from_index(operation_offset),
        extent: Some(crate::records::identity::MaybeRecordedValue::Located(
            crate::records::identity::RecordedValue {
                value: extent,
                offset: u64_from_index(extent_offset),
            },
        )),
        section,
        section_offset: Some(u64_from_index(section_offset)),
        section_placement,
        section_placement_offset: Some(u64_from_index(section_placement_offset)),
        clockwise,
        clockwise_offset: Some(u64_from_index(clockwise_offset)),
    })
}

fn exact_long_coil_discriminators(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    kind: &scope::DesignFeatureKind,
    reference_members: &[u32],
) -> Option<CoilDiscriminators> {
    if !matches!(kind, scope::DesignFeatureKind::CoilPrimitive) || reference_members.len() != 10 {
        return None;
    }
    let frame_length = paired_at.checked_sub(start)?;
    if !matches!(frame_length, 450 | 572 | 578)
        || !zeros_at::<11>(bytes, start.checked_add(coil_long::ZERO_RUN_11)?)
        || View::u32_le_at(bytes, start.checked_add(coil_long::STRUCTURAL_CONSTANT)?)? != 1
        || marked_record_reference(bytes, start.checked_add(coil_long::FIFTH_REFERENCE)?)?
            != *reference_members.get(4)?
        || marked_record_reference(bytes, start.checked_add(coil_long::NINTH_REFERENCE)?)?
            != *reference_members.get(8)?
    {
        return None;
    }
    let matrix_form = matches!(frame_length, 572 | 578) && exact_long_coil_matrix(bytes, start);
    let operation_value = View::u32_le_at(bytes, start.checked_add(coil_long::OPERATION)?)?;
    let operation = match (frame_length, operation_value) {
        (450, 1) => DesignExtrudeOperation::Join,
        (450, 2) => DesignExtrudeOperation::Cut,
        (450, 3) => DesignExtrudeOperation::Intersect,
        (572, 1) if matrix_form => DesignExtrudeOperation::Join,
        (572, 2) if matrix_form => DesignExtrudeOperation::Cut,
        (572, 3) if matrix_form => DesignExtrudeOperation::Intersect,
        (578, 2) if matrix_form => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    Some(CoilDiscriminators {
        operation,
        operation_offset: u64::try_from(start.checked_add(coil_long::OPERATION)?).ok()?,
        // The long form has no extent selector. Its exact owned parameter set
        // supplies the mode after the scope is parsed.
        extent: None,
        // The long form fixes these settings in its dialect envelope.
        section: DesignCoilSection::Circular,
        section_offset: None,
        section_placement: DesignCoilSectionPlacement::Inside,
        section_placement_offset: None,
        clockwise: false,
        clockwise_offset: None,
    })
}

fn exact_long_coil_matrix(bytes: &[u8], start: usize) -> bool {
    let Some(values) = start
        .checked_add(77)
        .and_then(|at| f64s_at::<16>(bytes, at))
    else {
        return false;
    };
    values.iter().all(|value| value.is_finite())
        && values[12..15].iter().all(|value| *value == 0.0)
        && values[15] == 1.0
}

pub(super) fn exact_long_coil_transform(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    kind: &scope::DesignFeatureKind,
    reference_members: &[u32],
) -> Option<coil::DesignCoilTransform> {
    if !matches!(kind, scope::DesignFeatureKind::CoilPrimitive)
        || reference_members.len() != 10
        || !matches!(paired_at.checked_sub(start)?, 572 | 578)
    {
        return None;
    }
    let transform = exact_long_coil_transform_values(bytes, start)?;
    Some(coil::DesignCoilTransform {
        transform,
        transform_offset: u64::try_from(start.checked_add(77)?).ok()?,
    })
}

fn exact_long_coil_transform_values(
    bytes: &[u8],
    start: usize,
) -> Option<crate::records::sketch_placement::SketchPlacementMatrix> {
    let values = f64s_at::<16>(bytes, start.checked_add(77)?)?;
    if !exact_long_coil_matrix(bytes, start) {
        return None;
    }
    let mut transform = [[0.0; 4]; 4];
    for (ordinal, value) in values.into_iter().enumerate() {
        transform[ordinal / 4][ordinal % 4] = value;
    }
    let transform =
        crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform).ok()?;
    valid_right_handed_coil_transform(&transform).then_some(transform)
}

/// Bind the extent mode of a long-form `CoilPrimitive` scope from the source
/// kinds of its owned parameters, in local-ordinal order.
pub(super) fn bind_coil_extent_from_parameters(
    ctx: &DecodeContext<'_>,
    scope: &mut DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
) -> Result<(), CodecError> {
    if !matches!(scope.payload(), DesignScopePayload::CoilPrimitive(_))
        || scope.coil_extent().is_some()
    {
        return Ok(());
    }
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(());
    };
    // At most five owned parameters name an extent; empty slots sort last.
    let mut owned: [Option<(u32, &str)>; 5] = [None; 5];
    let mut count = 0usize;
    // Each owner is admitted as the scan reaches it; a sixth owned parameter
    // stops the scan.
    for owner in parameter_owners {
        ctx.charge_work(1, "scan F3D Coil parameter owners")?;
        if owner.scope_record_index() != scope.record_index || !in_stream(ctx, owner.id(), stream)?
        {
            continue;
        }
        let operation = "find F3D Coil owner parameter";
        let Some(parameter) = ctx.find_by(
            parameters,
            |parameter| {
                Ok(parameter.record_index == owner.parameter_record_index()
                    && in_stream(ctx, &parameter.id, stream)?)
            },
            operation,
        )?
        else {
            continue;
        };
        let Some(slot) = owned.get_mut(count) else {
            return Ok(());
        };
        *slot = Some((owner.local_ordinal(), parameter.source_kind()));
        count += 1;
    }
    ctx.stable_sort_by_key(
        &mut owned[..],
        |slot| slot.map_or((true, 0), |(ordinal, _)| (false, ordinal)),
        Ord::cmp,
        "sort F3D Coil owned parameters",
    )?;
    let kinds = owned.map(|slot| slot.map_or("", |(_, source_kind)| source_kind));
    let extent = match &kinds[..count] {
        ["Diameter", "SectionSize", "TaperAngle", "Revolutions", "Height"]
        | ["Diameter", "SectionSize", "TaperAngle", "Height", "Revolutions"] => {
            Some(DesignCoilExtent::RevolutionsHeight)
        }
        ["Diameter", "SectionSize", "TaperAngle", "Revolutions", "Pitch"]
        | ["Diameter", "SectionSize", "TaperAngle", "Pitch", "Revolutions"] => {
            Some(DesignCoilExtent::RevolutionsPitch)
        }
        ["Diameter", "SectionSize", "TaperAngle", "Height", "Pitch"]
        | ["Diameter", "SectionSize", "TaperAngle", "Pitch", "Height"] => {
            Some(DesignCoilExtent::HeightPitch)
        }
        ["Diameter", "SectionSize", "Revolutions", "Pitch"]
        | ["Diameter", "SectionSize", "Pitch", "Revolutions"] => Some(DesignCoilExtent::Spiral),
        _ => None,
    };
    if let Some(extent) = extent {
        if let scope::DesignScopePayloadMut::SpirePrimitive(slot)
        | scope::DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).extent = Some(
                crate::records::identity::MaybeRecordedValue::Unlocated(extent),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
