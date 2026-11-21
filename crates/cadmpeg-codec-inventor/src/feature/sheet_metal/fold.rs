// SPDX-License-Identifier: Apache-2.0
//! Unfold and Refold: the eight-property records that flatten and restore
//! sheet-metal bends.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::{DistinctMembers, FaceSelection};
use cadmpeg_ir::features::sheet_metal::{
    SheetMetalRefoldConstruction, SheetMetalUnfoldConstruction,
};
use cadmpeg_ir::features::{Feature, FeatureContent, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::native::bytes::NativeBytes;

use super::operands::{boolean, enumeration};
use super::refusal::{require, Refuse};
use super::{REFOLD_MODE_TYPE, UNFOLD_MODE_TYPE};
use crate::feature::{
    ClassId, PmDcFeature, PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily,
};
use crate::pmdc::{type_id_string, PmDcReference};

/// Feature-label class of an Unfold.
pub(crate) const UNFOLD_CLASS: ClassId = ClassId(NativeBytes::new([
    0x1a, 0x40, 0x66, 0x11, 0x5a, 0x47, 0x44, 0xb3, 0x15, 0xa6, 0x2a, 0xb0, 0x03, 0xa9, 0x88, 0xa7,
]));
/// Feature-label class of a Refold.
pub(crate) const REFOLD_CLASS: ClassId = ClassId(NativeBytes::new([
    0xe9, 0xa3, 0xcc, 0x35, 0xa8, 0x4f, 0xd1, 0x5b, 0x17, 0x9f, 0xa4, 0x99, 0xab, 0x57, 0x9f, 0xa5,
]));

/// The property count of an Unfold or Refold record.
const PROPERTY_COUNT: usize = 8;
/// Slots that an Unfold or Refold requires null.
const RESERVED_SLOTS: [usize; 2] = [5, 6];
/// The slot-7 enumeration `(type_value, value)`.
const MODE: (i16, u16) = (5, 1);

/// Whether a record unfolds or refolds bends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FoldKind {
    Unfold,
    Refold,
}

impl FoldKind {
    /// The fold kind of a feature-label class.
    pub(crate) fn of(class: ClassId) -> Option<Self> {
        if class == UNFOLD_CLASS {
            Some(Self::Unfold)
        } else if class == REFOLD_CLASS {
            Some(Self::Refold)
        } else {
            None
        }
    }

    /// The record type of the slot-7 enumeration.
    const fn mode_type(self) -> [u8; 16] {
        match self {
            Self::Unfold => UNFOLD_MODE_TYPE,
            Self::Refold => REFOLD_MODE_TYPE,
        }
    }

    const fn source_tag(self) -> &'static str {
        match self {
            Self::Unfold => "sheet_metal_unfold",
            Self::Refold => "sheet_metal_refold",
        }
    }
}

/// The first admission check that an Unfold or Refold fails.
///
/// The variants appear in the order the projection evaluates them. The kebab
/// string of each variant is the pinned `detail` of the loss note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum FoldRefusal {
    /// The record does not have the eight-property form.
    PropertyCount,
    /// Slot 0 is not a body selection of one body.
    Body,
    /// Slot 1 is not a face item that selects one face.
    Stationary,
    /// Slot 2 is not a nonempty face collection of face items that each
    /// select at least one face.
    Bends,
    /// Slot 3 is not Boolean true.
    Control,
    /// Slot 4 is not an empty fold list.
    List,
    /// Slot 5 or 6 is not null.
    ReservedSlot,
    /// Slot 7 is not the fold enumeration `(5, 1)` of the record kind.
    Mode,
}

impl FoldRefusal {
    /// The pinned kebab string of this refusal.
    pub(crate) const fn detail(self) -> &'static str {
        match self {
            Self::PropertyCount => "property-count",
            Self::Body => "body",
            Self::Stationary => "stationary",
            Self::Bends => "bends",
            Self::Control => "control",
            Self::List => "list",
            Self::ReservedSlot => "reserved-slot",
            Self::Mode => "mode",
        }
    }
}

/// The operands of an admitted Unfold or Refold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoldOperands {
    /// The body that slot 0 selects.
    pub(crate) body: PmDcReference,
    /// The slot-1 face item: the stationary face.
    pub(crate) stationary: String,
    /// The slot-2 face collection: the bend faces.
    pub(crate) bends: String,
}

/// The operands of `feature` after every admission check.
pub(crate) fn operands<'a>(
    feature: &PmDcFeature,
    kind: FoldKind,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> Result<FoldOperands, FoldRefusal> {
    let slots = feature.properties.references();
    require(slots.len() == PROPERTY_COUNT, FoldRefusal::PropertyCount)?;
    let body = match property(slots[0]).map(|p| &p.kind) {
        Some(PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BodySelection,
            items,
        }) => match items.references() {
            [body] if body.index() != 0 => Some(*body),
            _ => None,
        },
        _ => None,
    }
    .refuse(FoldRefusal::Body)?;
    let stationary = property(slots[1])
        .filter(|item| face_count(item) == Some(1))
        .refuse(FoldRefusal::Stationary)?;
    let bends = property(slots[2])
        .filter(|collection| match &collection.kind {
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items,
            } => {
                !items.references().is_empty()
                    && items.references().iter().all(|item| {
                        property(*item)
                            .and_then(face_count)
                            .is_some_and(|count| count > 0)
                    })
            }
            _ => false,
        })
        .refuse(FoldRefusal::Bends)?;
    require(
        property(slots[3]).and_then(boolean) == Some(true),
        FoldRefusal::Control,
    )?;
    require(
        matches!(
            property(slots[4]).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::SheetMetalFoldList,
                items,
            }) if items.references().is_empty()
        ),
        FoldRefusal::List,
    )?;
    require(
        RESERVED_SLOTS.iter().all(|&slot| slots[slot].index() == 0),
        FoldRefusal::ReservedSlot,
    )?;
    require(
        property(slots[7]).is_some_and(|mode| {
            mode.identity.type_id.as_str() == type_id_string(kind.mode_type())
                && enumeration(mode) == Some(MODE)
        }),
        FoldRefusal::Mode,
    )?;
    Ok(FoldOperands {
        body,
        stationary: stationary.id(),
        bends: bends.id(),
    })
}

/// The number of faces that a face item selects.
fn face_count(item: &PmDcFeatureProperty) -> Option<usize> {
    match &item.kind {
        PmDcFeaturePropertyKind::FaceItem { index_references } => {
            Some(index_references.values().len())
        }
        _ => None,
    }
}

/// The operation of an admitted record. The Refold record stores no
/// reference to an Unfold, so the Refold source stays unresolved.
pub(crate) fn operation(kind: FoldKind, operands: &FoldOperands) -> FeatureOperation {
    match kind {
        FoldKind::Unfold => FeatureOperation::SheetMetalUnfold {
            construction: SheetMetalUnfoldConstruction {
                stationary: Some(FaceSelection::Native(operands.stationary.clone())),
                bends: Some(FaceSelection::Native(operands.bends.clone())),
                all_bends: None,
                neutral_factor: None,
            },
        },
        FoldKind::Refold => FeatureOperation::SheetMetalRefold {
            construction: SheetMetalRefoldConstruction::default(),
        },
    }
}

/// Project one Unfold or Refold record, or name its first refused check.
pub(in crate::feature) fn project<'a>(
    ctx: &DecodeContext<'_>,
    feature: &PmDcFeature,
    label: &PmDcFeatureLabel,
    kind: FoldKind,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> Result<Result<Feature, FoldRefusal>, CodecError> {
    let operands = match operands(feature, kind, property) {
        Ok(operands) => operands,
        Err(refusal) => return Ok(Err(refusal)),
    };
    ctx.charge_entities(1, "project Inventor unfold or refold")?;
    Ok(Ok(Feature {
        product_definition: None,
        id: crate::feature::history_feature_id(&feature.identity)?,
        ordinal: u64::from(label.index),
        name: label.name().map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some(kind.source_tag().into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation(
            kind, &operands,
        ))),
        native_ref: Some(feature.id()),
    }))
}

/// The first refused check of each Unfold or Refold that stays native.
#[derive(Debug, Default)]
pub(in crate::feature) struct Refusals(BTreeMap<String, FoldRefusal>);

impl Refusals {
    /// Record the refusal of the fold record `source`.
    pub(in crate::feature) fn insert(&mut self, source: String, refusal: FoldRefusal) {
        self.0.entry(source).or_insert(refusal);
    }

    /// One loss note for each fold record that stays native in the final
    /// features.
    pub(in crate::feature) fn losses(
        &self,
        features: &[Feature],
    ) -> Vec<cadmpeg_ir::report::loss::LossNote> {
        features
            .iter()
            .filter(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Native { .. })
                )
            })
            .filter_map(|feature| {
                let refusal = self.0.get(feature.native_ref.as_deref()?)?;
                Some(
                    crate::loss::InventorLossCode::FeatureSheetMetalFoldRefused.note(format!(
                        "Retained {} as native: the first refused unfold or refold check is {}.",
                        feature.id,
                        refusal.detail()
                    )),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
