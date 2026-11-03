// SPDX-License-Identifier: Apache-2.0
//! Browser work planes as datum-plane history features.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDatumPlaneFrame, FeatureDefinition,
    FeatureEvaluation, FeatureOperation, NativeFeatureKind,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;

use super::{history_feature_id, PmDcFeatureLabel, PmDcWorkPlane};

/// Unit-length and orthogonality bound of the stored in-plane axes.
const EPS_WORK_PLANE_FRAME: f64 = 1.0e-10;

/// Internal centimetres to model millimetres.
const CENTIMETRE_TO_MILLIMETRE: f64 = 10.0;

/// Native family of a work plane whose frame does not admit.
pub(crate) const NATIVE_KIND: &str = "WorkPlane";

/// The model-space plane of a work-plane record.
///
/// The stored `u` and `v` axes are orthonormal; the normal is `u × v` and the
/// origin converts from centimetres to millimetres.
pub(crate) fn frame(record: &PmDcWorkPlane) -> Option<FeatureDatumPlaneFrame> {
    let u = Vector3::from(record.u_axis.map(cadmpeg_ir::scalar::FiniteReal::get));
    let v = Vector3::from(record.v_axis.map(cadmpeg_ir::scalar::FiniteReal::get));
    if (u.norm() - 1.0).abs() > EPS_WORK_PLANE_FRAME
        || (v.norm() - 1.0).abs() > EPS_WORK_PLANE_FRAME
        || u.dot(v).abs() > EPS_WORK_PLANE_FRAME
    {
        return None;
    }
    let origin = record
        .origin
        .map(|value| value.get() * CENTIMETRE_TO_MILLIMETRE);
    FeatureDatumPlaneFrame::new(Point3::new(origin[0], origin[1], origin[2]), u.cross(v), u)
}

/// The history feature of a browser work plane: a datum plane when its
/// frame admits, otherwise a native work-plane feature. A work plane is not
/// a typed feature record, so it never counts toward the open operation
/// graphs.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    record: &PmDcWorkPlane,
    label: Option<&PmDcFeatureLabel>,
) -> Result<Feature, CodecError> {
    ctx.charge_entities(1, "project Inventor history feature")?;
    let (operation, tag) = match frame(record) {
        Some(frame) => (FeatureOperation::DatumPlane { frame }, "work_plane"),
        None => (
            FeatureOperation::Native {
                kind: NativeFeatureKind::Other(NATIVE_KIND.into()),
                parameters: BTreeMap::new(),
            },
            "native",
        ),
    };
    Ok(Feature {
        product_definition: None,
        id: history_feature_id(&record.identity)?,
        ordinal: label.map_or(0, |label| u64::from(label.index)),
        name: label.and_then(|label| label.name()).map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some(tag.into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: Some(record.id()),
    })
}

/// One loss per history work plane that stays native.
pub(crate) fn native_losses(planes: &[PmDcWorkPlane], features: &[Feature]) -> Vec<LossNote> {
    let ids = planes.iter().map(PmDcWorkPlane::id).collect::<HashSet<_>>();
    features
        .iter()
        .filter(|feature| {
            feature
                .native_ref
                .as_ref()
                .is_some_and(|native| ids.contains(native))
                && matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Native { .. })
                )
        })
        .map(|_| {
            crate::loss::InventorLossCode::FeatureWorkPlaneFrameNative.note(
                "A work plane whose stored in-plane axes are not orthonormal remains a native history feature.",
            )
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests;
