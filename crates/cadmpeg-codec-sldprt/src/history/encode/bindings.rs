// SPDX-License-Identifier: Apache-2.0
//! Native parameter roles refer to canonical bindings, never copied values.

use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::ParameterId;

impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn bind_operation_dimensions(
        &self,
        encoding: &mut NeutralFeatureEncoding,
    ) -> Result<(), CodecError> {
        let operation = self.feature.evaluation.definition().operation();
        let value = serde_json::to_value(operation).map_err(|error| {
            CodecError::malformed(format!(
                "serialize canonical SLDPRT dimensional roles: {error}"
            ))
        })?;
        let bind = |roles: &[&str], paths: &[&str]| -> Result<(), CodecError> {
            let Some(role) = roles
                .iter()
                .find(|role| encoding.parameters.contains_key(**role))
                .copied()
            else {
                return Ok(());
            };
            let Some(dimension) = paths.iter().find_map(|path| value.pointer(path)) else {
                return Ok(());
            };
            if dimension.get("kind").and_then(serde_json::Value::as_str) == Some("literal") {
                if self.canonical_parameters.iter().any(|parameter| {
                    parameter.owner.as_ref() == Some(&self.feature.id) && parameter.name == role
                }) {
                    return Err(CodecError::UnsupportedContent(format!(
                        "SLDPRT native dimensional role {role} cannot also hold an independent literal control on feature {}",
                        self.feature.id
                    )));
                }
                return Ok(());
            }
            let Some(binding) = dimension
                .get("value")
                .and_then(serde_json::Value::as_str)
                .filter(|_| {
                    dimension.get("kind").and_then(serde_json::Value::as_str) == Some("parameter")
                })
            else {
                return Ok(());
            };
            let id = ParameterId::mint(binding.to_owned()).map_err(CodecError::malformed)?;
            let parameter = self
                .canonical_parameters
                .iter()
                .find(|parameter| parameter.id == id)
                .ok_or_else(|| {
                    CodecError::malformed(
                        "SLDPRT dimensional role references a missing canonical parameter",
                    )
                })?;
            let text = if parameter.name == role
                && parameter.owner.as_ref() == Some(&self.feature.id)
            {
                self.base_parameters().get(role).cloned().ok_or_else(|| {
                    CodecError::malformed("SLDPRT canonical parameter expression is absent")
                })?
            } else {
                if self.canonical_parameters.iter().any(|candidate| {
                    candidate.owner.as_ref() == Some(&self.feature.id) && candidate.name == role
                }) {
                    return Err(CodecError::UnsupportedContent(format!(
                        "SLDPRT native dimensional role {role} conflicts with an independently authored parameter on feature {}",
                        self.feature.id
                    )));
                }
                let mut name = parameter.name.clone();
                if parameter.owner.as_ref() != Some(&self.feature.id) {
                    if let Some(owner) = parameter
                        .owner
                        .as_ref()
                        .and_then(|id| {
                            self.canonical_features
                                .iter()
                                .find(|feature| feature.id == *id)
                        })
                        .and_then(|feature| feature.name.as_deref())
                    {
                        name = format!("{name}@{owner}");
                    }
                }
                crate::history::parameters::render_identifier(&name)
            };
            if let Some(target) = encoding.parameters.get_mut(role) {
                *target = text;
            }
            Ok(())
        };
        visit_operation_roles(operation, bind)
    }
}

/// Enumerate the target's understood scalar roles once, for both text and cache emission.
pub(crate) fn visit_operation_roles(
    operation: &cadmpeg_ir::features::FeatureOperation,
    mut bind: impl FnMut(&[&str], &[&str]) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::FeatureOperation;
    match operation {
        FeatureOperation::Extrude { .. } => {
            bind(
                &["Depth", "D1"],
                &[
                    "/extent/side/termination/length",
                    "/extent/first/termination/length",
                    "/extent/side/termination/offset",
                ],
            )?;
            bind(
                &["Depth2", "D2"],
                &[
                    "/extent/second/termination/length",
                    "/extent/second/termination/offset",
                ],
            )?;
            bind(
                &["Draft", "D3"],
                &["/extent/side/draft/angle", "/extent/first/draft/angle"],
            )?;
        }
        FeatureOperation::Revolve { .. } => {
            bind(
                &["Angle", "D1"],
                &[
                    "/construction/extent/termination/angle",
                    "/construction/extent/first/angle",
                ],
            )?;
            bind(&["Angle2", "D2"], &["/construction/extent/second/angle"])?;
        }
        FeatureOperation::Fillet { .. } => bind(&["Radius", "D1"], &["/groups/0/radius/radius"])?,
        FeatureOperation::Chamfer { .. } => {
            bind(
                &["Distance", "Distance1", "D1"],
                &["/groups/0/spec/distance", "/groups/0/spec/first"],
            )?;
            bind(&["Distance2", "D2"], &["/groups/0/spec/second"])?;
            bind(&["Angle", "D2"], &["/groups/0/spec/angle"])?;
        }
        FeatureOperation::Hole { .. } => {
            bind(&["Diameter"], &["/shape/diameter"])?;
            bind(&["Depth"], &["/extent/length"])?;
            bind(
                &["CounterboreDiameter", "CountersinkDiameter"],
                &["/shape/construction/kind/diameter"],
            )?;
            bind(&["CounterboreDepth"], &["/shape/construction/kind/depth"])?;
            bind(&["CountersinkAngle"], &["/shape/construction/kind/angle"])?;
            bind(
                &["ThreadMajorDiameter"],
                &["/shape/construction/major_diameter"],
            )?;
            bind(&["ThreadDepth"], &["/shape/construction/thread_depth"])?;
            bind(&["ThreadPitch"], &["/shape/construction/pitch"])?;
            bind(
                &["DrillPointAngle"],
                &[
                    "/shape/construction/kind/drill_point_angle",
                    "/shape/construction/drill_point_angle",
                    "/bottom/angle",
                ],
            )?;
        }
        FeatureOperation::Shell { .. } | FeatureOperation::Thicken { .. } => {
            bind(&["Thickness", "D1"], &["/thickness"])?
        }
        FeatureOperation::Draft { .. } => bind(&["Angle", "D1"], &["/angle"])?,
        FeatureOperation::Rib { .. } => {
            bind(&["Thickness", "D1"], &["/construction/thickness"])?;
            bind(&["Draft"], &["/construction/draft/angle"])?;
        }
        FeatureOperation::DatumOffsetPlane { .. } => bind(&["Distance", "D1"], &["/distance"])?,
        FeatureOperation::MoveFace { .. } => {
            bind(&["Distance"], &["/motion/distance"])?;
            bind(&["Angle"], &["/motion/angle"])?;
        }
        FeatureOperation::OffsetSurface { .. } => bind(&["Distance", "D1"], &["/distance"])?,
        FeatureOperation::ExtendSurface { .. } => bind(&["Distance", "D1"], &["/distance"])?,
        FeatureOperation::KnitSurface { .. } => bind(&["GapTolerance"], &["/gap_tolerance"])?,
        FeatureOperation::CosmeticThread { .. } => {
            bind(&["Diameter", "D2"], &["/diameter"])?;
            bind(&["Depth", "D1"], &["/extent/length"])?;
        }
        FeatureOperation::Helix { .. } => {
            bind(&["Radius"], &["/radius"])?;
            bind(&["Pitch"], &["/shape/law/pitch"])?;
            bind(&["Revolutions"], &["/shape/law/revolutions"])?;
            bind(&["StartAngle"], &["/start_angle"])?;
        }
        FeatureOperation::Wrap { .. } => bind(&["Depth"], &["/mode/depth"])?,
        FeatureOperation::RuledSurface { .. } => {
            bind(&["Distance", "D1"], &["/mode/distance"])?;
            bind(&["Angle"], &["/angle"])?;
        }
        FeatureOperation::Flex { .. } => {
            bind(&["Angle"], &["/mode/angle"])?;
            bind(&["Factor"], &["/mode/factor"])?;
            bind(&["Distance"], &["/mode/distance"])?;
        }
        FeatureOperation::MoveBody { .. } => {
            bind(&["TranslationX", "DX"], &["/translation/0"])?;
            bind(&["TranslationY", "DY"], &["/translation/1"])?;
            bind(&["TranslationZ", "DZ"], &["/translation/2"])?;
            bind(&["Rotation"], &["/rotation/angle"])?;
            bind(&["Copies"], &["/copies"])?;
        }
        FeatureOperation::Scale { .. } => {
            bind(&["Factor"], &["/factors/factor"])?;
            bind(&["ScaleX"], &["/factors/factors/0"])?;
            bind(&["ScaleY"], &["/factors/factors/1"])?;
            bind(&["ScaleZ"], &["/factors/factors/2"])?;
        }
        FeatureOperation::Sweep { .. } => {
            bind(&["Twist"], &["/twist"])?;
            bind(&["Scale"], &["/scale"])?;
        }
        FeatureOperation::Pattern { .. } => {
            bind(
                &["Spacing", "D3"],
                &["/pattern/distance/distance", "/pattern/spacing"],
            )?;
            bind(&["Count", "D1"], &["/pattern/count"])?;
            bind(&["D4"], &["/pattern/second/distance/distance"])?;
            bind(&["D2"], &["/pattern/second/count"])?;
            bind(&["Angle"], &["/pattern/angle/angle"])?;
        }
        FeatureOperation::EquationCurve { .. } => {
            bind(&["Start"], &["/curve/start"])?;
            bind(&["End"], &["/curve/end"])?;
        }
        FeatureOperation::Dome { .. } => bind(&["Height"], &["/height"])?,
        _ => {}
    }
    Ok(())
}

/// The current canonical parameter that supplies one native role, if bound.
pub(crate) fn role_parameter(
    feature: &cadmpeg_ir::features::Feature,
    native_roles: &std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    role: &str,
) -> Result<Option<ParameterId>, CodecError> {
    let operation = feature.definition().operation();
    let value = serde_json::to_value(operation)
        .map_err(|error| CodecError::malformed(format!("serialize canonical role: {error}")))?;
    let mut result = None;
    visit_operation_roles(operation, |roles, paths| {
        if roles
            .iter()
            .find(|candidate| native_roles.contains_key(**candidate))
            .copied()
            != Some(role)
        {
            return Ok(());
        }
        let Some(dimension) = paths.iter().find_map(|path| value.pointer(path)) else {
            return Ok(());
        };
        if dimension.get("kind").and_then(serde_json::Value::as_str) == Some("parameter") {
            result = dimension
                .get("value")
                .and_then(serde_json::Value::as_str)
                .map(|id| ParameterId::mint(id.to_owned()).map_err(CodecError::malformed))
                .transpose()?;
        }
        Ok(())
    })?;
    Ok(result)
}
