// SPDX-License-Identifier: Apache-2.0
//! Tree-node, cosmetic-thread, native, curve, helix, and unsupported write encoders.

use super::super::literals::{
    format_angle_rad, format_length_mm, format_length_number, valid_direction,
};
use super::format::{format_length_like, format_point3_mm, format_vector3};
use super::support::{
    face_selection_value, feature_tree_node_kind, is_helix, path_source, require_same_family,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::{classify, FeatureClass};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{
    features::{
        CosmeticThreadExtent, CurveProjectionDirection, CurveProjectionDirectionState,
        FaceSelection, FeatureId, FeatureTreeNodeRole, HelixConstructionStyle, PathRef,
    },
    scalar::Angle,
};
use std::collections::BTreeMap;

/// The decoded fields of one `Helix` operation, borrowed from the feature definition.
#[derive(Clone, Copy)]
pub(super) struct HelixDefinition<'a> {
    pub(super) axis_origin: &'a Point3,
    pub(super) axis_direction: &'a Vector3,
    pub(super) radius: &'a cadmpeg_ir::features::Dimension<cadmpeg_ir::scalar::PositiveLength>,
    pub(super) shape: &'a cadmpeg_ir::features::HelixShape,
    pub(super) start_angle: &'a cadmpeg_ir::features::Dimension<Angle>,
    pub(super) clockwise: bool,
    pub(super) segment_turns:
        Option<&'a cadmpeg_ir::features::Dimension<cadmpeg_ir::scalar::PositiveReal>>,
    pub(super) construction_style: Option<&'a HelixConstructionStyle>,
}

impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_tree_node(
        &self,
        role: FeatureTreeNodeRole,
        _children: &[FeatureId],
        active_child: Option<&FeatureId>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let retained_tree_node_roles = self.retained_tree_node_roles;
        Ok({
            if existing.is_none() && role != FeatureTreeNodeRole::Equations {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} has no source-less keyword form for tree role {role:?}",
                    feature.id
                )));
            }
            if active_child.is_some() {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} selects an active tree child",
                    feature.id
                )));
            }
            if existing
                .is_some_and(|record| retained_tree_node_roles.get(&record.id) != Some(&role))
            {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} changes feature-tree node role",
                    feature.id
                )));
            }
            NeutralFeatureEncoding {
                kind: existing.map_or_else(
                    || feature_tree_node_kind(role).into(),
                    |record| record.kind.clone(),
                ),
                parameters: self.base_parameters(),
                properties: self.base_properties(),
            }
        })
    }

    pub(super) fn encode_cosmetic_thread(
        &self,
        face: &FaceSelection,
        diameter: Option<&cadmpeg_ir::features::Dimension<cadmpeg_ir::scalar::PositiveLength>>,
        extent: Option<&CosmeticThreadExtent>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let Some(record) =
                existing.filter(|record| classify(record) == Some(FeatureClass::CosmeticThread))
            else {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} adds a cosmetic thread",
                    feature.id
                )));
            };
            let mut parameters = self.base_parameters();
            if let Some(diameter) = diameter {
                let prefix = record
                    .parameters
                    .get("D2")
                    .filter(|value| value.trim().starts_with("&lt;MOD-DIAM&gt;"))
                    .map_or("<MOD-DIAM>", |_| "&lt;MOD-DIAM&gt;");
                parameters.insert(
                    cadmpeg_core::nonblank_literal!("D2"),
                    format!(
                        "{prefix}{}",
                        format_length_number(self.dimension(diameter)?.into())
                    ),
                );
            }
            match extent {
                Some(CosmeticThreadExtent::Blind { length }) => {
                    parameters.insert(
                        cadmpeg_core::nonblank_literal!("D1"),
                        format_length_like(
                            self.dimension(length)?.into(),
                            record.parameters.get("D1").map(String::as_str),
                        ),
                    );
                }
                Some(CosmeticThreadExtent::Through {}) => {
                    parameters.remove("D1");
                }
                None => {}
            }
            let mut properties = self.base_properties();
            if let Some(value) = face_selection_value(face) {
                properties.insert(cadmpeg_core::nonblank_literal!("Face"), value);
            } else if !matches!(face, FaceSelection::Unresolved) {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} changes cosmetic-thread face selection",
                    feature.id
                )));
            }
            NeutralFeatureEncoding {
                kind: record.kind.clone(),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_native(
        &self,
        kind: &cadmpeg_ir::features::NativeFeatureKind,
        parameters: &BTreeMap<
            cadmpeg_core::text::NonBlankString,
            cadmpeg_ir::features::Dimension<cadmpeg_ir::features::Expression>,
        >,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let mut rendered = BTreeMap::new();
        for (name, dimension) in parameters {
            let expression = self.dimension(dimension)?;
            let expression = expression
                .render_with(|id| {
                    let parameter = self
                        .canonical_parameters
                        .iter()
                        .find(|parameter| parameter.id == *id)?;
                    let mut name = parameter.name.to_string();
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
                    Some(crate::history::parameters::render_identifier(&name))
                })
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                "SLDPRT native feature {} references a missing canonical expression operand",
                self.feature.id
            ))
                })?;
            rendered.insert(name.clone(), expression);
        }
        Ok(NeutralFeatureEncoding {
            kind: kind.to_string(),
            parameters: rendered,
            properties: self.base_properties(),
        })
    }

    pub(super) fn encode_stored_geometry(&self) -> NeutralFeatureEncoding {
        let existing = self.existing;
        NeutralFeatureEncoding {
            kind: existing.map_or_else(|| "Feature".into(), |record| record.kind.clone()),
            parameters: self.base_parameters(),
            properties: self.base_properties(),
        }
    }

    pub(super) fn encode_derived_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} uses unsupported copied-geometry semantics",
            feature.id
        )))
    }

    pub(super) fn encode_imported_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} uses unsupported external-import semantics",
            feature.id
        )))
    }

    pub(super) fn encode_primitive(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} uses unsupported analytic-primitive semantics",
            feature.id
        )))
    }

    pub(super) fn encode_equation_curve(
        &self,
        curve: &cadmpeg_ir::features::FeatureEquationCurve,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            require_same_family(
                existing,
                &feature.id,
                &["EquationDrivenCurve", "EquationCurve"],
            )?;
            let mut properties = self.base_properties();
            properties.insert(
                cadmpeg_core::nonblank_literal!("Parameter"),
                curve.parameter().to_owned(),
            );
            properties.insert(
                cadmpeg_core::nonblank_literal!("XEquation"),
                curve.x_expression().to_owned(),
            );
            properties.insert(
                cadmpeg_core::nonblank_literal!("YEquation"),
                curve.y_expression().to_owned(),
            );
            properties.insert(
                cadmpeg_core::nonblank_literal!("ZEquation"),
                curve.z_expression().to_owned(),
            );
            let [start, end] = curve.domain().endpoints();
            let start = self.dimension(start)?.get().to_string();
            let end = self.dimension(end)?.get().to_string();
            properties.insert(cadmpeg_core::nonblank_literal!("Start"), start.clone());
            properties.insert(cadmpeg_core::nonblank_literal!("End"), end.clone());
            let mut parameters = self.base_parameters();
            parameters.insert(cadmpeg_core::nonblank_literal!("Start"), start);
            parameters.insert(cadmpeg_core::nonblank_literal!("End"), end);
            NeutralFeatureEncoding {
                kind: existing.map_or_else(
                    || "EquationDrivenCurve".into(),
                    |record| record.kind.clone(),
                ),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_projected_curve(
        &self,
        source: &PathRef,
        target_faces: &FaceSelection,
        direction: &CurveProjectionDirection,
        bidirectional: Option<bool>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            let source = path_source(source, record_sources, sketch_sources).ok_or_else(|| {
                CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} references a missing projection source",
                    feature.id
                ))
            })?;
            let target_faces = face_selection_value(target_faces).ok_or_else(|| {
                CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} has no projection target faces",
                    feature.id
                ))
            })?;
            require_same_family(
                existing,
                &feature.id,
                &["ProjectedCurve", "ProjectionCurve"],
            )?;
            let mut properties = self.base_properties();
            properties.insert(cadmpeg_core::nonblank_literal!("Source"), source);
            properties.insert(cadmpeg_core::nonblank_literal!("TargetFaces"), target_faces);
            if let Some(bidirectional) = bidirectional {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("Bidirectional"),
                    bidirectional.to_string(),
                );
            }
            match direction {
                CurveProjectionDirection::Vector(direction) => {
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("Direction"),
                        format_vector3((*direction).into()),
                    );
                }
                CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal) => {
                    properties.remove("Direction");
                }
                CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved) => {
                    return Err(CodecError::UnsupportedContent(format!(
                        "SLDPRT feature {} has unresolved projection direction",
                        feature.id
                    )));
                }
            }
            NeutralFeatureEncoding {
                kind: existing
                    .map_or_else(|| "ProjectedCurve".into(), |record| record.kind.clone()),
                parameters: self.base_parameters(),
                properties,
            }
        })
    }

    pub(super) fn encode_composite_curve(
        &self,
        segments: &[PathRef],
        closed: bool,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            if segments.is_empty() {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} has no composite-curve segments",
                    feature.id
                )));
            }
            require_same_family(existing, &feature.id, &["CompositeCurve"])?;
            let segments = segments
                .iter()
                .map(|segment| {
                    path_source(segment, record_sources, sketch_sources).ok_or_else(|| {
                        CodecError::UnsupportedContent(format!(
                            "SLDPRT feature {} references a missing composite segment",
                            feature.id
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut properties = self.base_properties();
            properties.insert(
                cadmpeg_core::nonblank_literal!("Segments"),
                segments.join(";"),
            );
            properties.insert(
                cadmpeg_core::nonblank_literal!("Closed"),
                closed.to_string(),
            );
            NeutralFeatureEncoding {
                kind: existing
                    .map_or_else(|| "CompositeCurve".into(), |record| record.kind.clone()),
                parameters: self.base_parameters(),
                properties,
            }
        })
    }

    pub(super) fn encode_helix(
        &self,
        definition: HelixDefinition<'_>,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let HelixDefinition {
            axis_origin,
            axis_direction,
            radius,
            shape,
            start_angle,
            clockwise,
            segment_turns,
            construction_style,
        } = definition;
        let feature = self.feature;
        let existing = self.existing;
        Ok({
            let cadmpeg_ir::features::HelixShape::Cylindrical {
                law: cadmpeg_ir::features::HelixLaw::PitchRevolutions { pitch, revolutions },
            } = shape
            else {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} uses unsupported helix construction controls",
                    feature.id
                )));
            };
            if segment_turns.is_some() || construction_style.is_some() {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} uses unsupported helix construction controls",
                    feature.id
                )));
            }
            let (Some(axis_origin), Some(axis_direction)) = (
                cadmpeg_ir::features::FinitePoint3::new(*axis_origin),
                cadmpeg_ir::features::FiniteVector3::new(*axis_direction)
                    .filter(|direction| valid_direction(direction.get())),
            ) else {
                return Err(CodecError::malformed(format_args!(
                    "SLDPRT feature {} has invalid helix geometry",
                    feature.id
                )));
            };
            if existing.is_some_and(|record| !is_helix(record)) {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT feature {} changes operation family",
                    feature.id
                )));
            }
            let mut parameters = self.base_parameters();
            parameters.insert(
                cadmpeg_core::nonblank_literal!("Radius"),
                format_length_mm(self.dimension(radius)?.into()),
            );
            parameters.insert(
                cadmpeg_core::nonblank_literal!("Pitch"),
                format_length_mm(self.dimension(pitch)?.into()),
            );
            parameters.insert(
                cadmpeg_core::nonblank_literal!("Revolutions"),
                self.dimension(revolutions)?.get().to_string(),
            );
            parameters.insert(
                cadmpeg_core::nonblank_literal!("StartAngle"),
                format_angle_rad(self.dimension(start_angle)?),
            );
            let mut properties = self.base_properties();
            properties.insert(
                cadmpeg_core::nonblank_literal!("AxisOrigin"),
                format_point3_mm(axis_origin),
            );
            properties.insert(
                cadmpeg_core::nonblank_literal!("AxisDirection"),
                format_vector3(axis_direction),
            );
            properties.insert(
                cadmpeg_core::nonblank_literal!("Clockwise"),
                clockwise.to_string(),
            );
            NeutralFeatureEncoding {
                kind: existing.map_or_else(|| "Helix".into(), |record| record.kind.clone()),
                parameters,
                properties,
            }
        })
    }

    pub(super) fn encode_helix_native_axis(
        &self,
        _axis_native_ref: &str,
        _law: &cadmpeg_ir::features::HelixLaw,
        _start_angle: Angle,
        _clockwise: bool,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        // (P/S-entity): this native axis envelope has no decoded serializer for
        // its authoring-law discriminator. Synthesis omits this operation with
        // a named loss while preserving independently exportable geometry.
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} cannot encode the native-axis helix authoring law",
            self.feature.id
        )))
    }

    pub(super) fn encode_offset_shape(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} uses unsupported whole-shape offset semantics",
            feature.id
        )))
    }

    pub(super) fn encode_post_process(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} uses unsupported topology post-processing semantics",
            feature.id
        )))
    }

    pub(super) fn encode_curve_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} uses unsupported construction-geometry semantics",
            feature.id
        )))
    }

    pub(super) fn encode_shape_operation(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} uses unsupported derived-shape semantics",
            feature.id
        )))
    }

    pub(super) fn encode_binder(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} uses design-binder semantics that cannot be written",
            feature.id
        )))
    }

    pub(super) fn encode_unsupported(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::UnsupportedContent(format!(
            "SLDPRT feature {} uses semantics that cannot be written",
            feature.id
        )))
    }
}
