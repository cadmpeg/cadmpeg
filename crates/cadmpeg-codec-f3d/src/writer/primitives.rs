// SPDX-License-Identifier: Apache-2.0
//! Byte and predicate helpers shared by the source-less generator and the
//! edit-and-patch engine.

use crate::history_records::AsmEntityChangeKind;
use crate::native::F3dNative;
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;

pub(crate) fn f3d_native(ir: &CadIr) -> Result<Option<F3dNative>, CodecError> {
    if let Some(namespace) = ir.native.namespace("f3d") {
        if namespace.version != crate::native::F3D_NATIVE_VERSION {
            let version = namespace.version;
            return Err(CodecError::Malformed(format!(
                "unsupported F3D native namespace version {version}"
            )));
        }
    }
    ir.native
        .namespace("f3d")
        .map(F3dNative::load)
        .transpose()
        .map_err(Into::into)
}

pub(crate) fn validate_configuration_projection(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    for configuration in &native.design_configurations {
        crate::design::configurations::validate_configuration_payload(
            &configuration.entry_name,
            configuration.kind,
            &configuration.payload,
        )?;
    }
    let mut projected =
        crate::design::configurations::project_configurations(&native.design_configurations);
    crate::design::configurations::bind_configuration_parameter_overrides(
        &mut projected,
        &target.model.parameters,
    );
    crate::design::configurations::bind_configuration_suppressed_features(
        &mut projected,
        &target.model.features,
    );
    if target.model.configurations != projected {
        return Err(CodecError::Malformed(
            "neutral F3D configurations must equal the projection of native configuration tables"
                .into(),
        ));
    }
    Ok(())
}

pub(crate) fn normalized_face_sense_to_native(
    desired: Sense,
    native_at_decode: Sense,
    normalized_at_decode: Sense,
) -> Sense {
    if native_at_decode == normalized_at_decode {
        desired
    } else {
        match desired {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        }
    }
}

pub(crate) fn native_bool(value: bool) -> u8 {
    if value {
        0x0a
    } else {
        0x0b
    }
}

pub(crate) fn history_change_kind(
    old_ref: Option<i64>,
    new_ref: Option<i64>,
) -> Result<AsmEntityChangeKind, CodecError> {
    match (old_ref, new_ref) {
        (None, Some(_)) => Ok(AsmEntityChangeKind::Insert),
        (Some(_), None) => Ok(AsmEntityChangeKind::Delete),
        (Some(_), Some(_)) => Ok(AsmEntityChangeKind::Update),
        (None, None) => Err(CodecError::Malformed(
            "ASM entity change cannot have two null references".into(),
        )),
    }
}

pub(crate) fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

pub(crate) fn unique_knot_count(knots: &[f64]) -> usize {
    knots
        .iter()
        .enumerate()
        .filter(|(index, value)| *index == 0 || knots[*index - 1] != **value)
        .count()
}

pub(crate) fn finite_vector(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}
