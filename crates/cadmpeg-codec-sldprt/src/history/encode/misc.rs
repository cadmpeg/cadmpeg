// SPDX-License-Identifier: Apache-2.0
//! Tree-node, cosmetic-thread, native, curve, helix, and unsupported write encoders.

use super::super::{
    face_selection_value, feature_tree_node_kind, format_angle_like, format_angle_rad,
    format_f64_literal, format_length_like, format_length_mm, format_point3_mm, format_vector3,
    is_helix, path_source, require_direction, require_same_family, valid_direction,
};
use super::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::classification::{classify, FeatureClass};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    CosmeticThreadExtent, CurveProjectionDirection, CurveProjectionDirectionState, FaceSelection,
    FeatureDefinition,
};

#[allow(
    clippy::unnecessary_wraps,
    reason = "Per-feature encoders use one fallible dispatch interface."
)]
impl NeutralFeatureEncoder<'_, '_, '_> {
    pub(super) fn encode_tree_node(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::TreeNode {
            role,
            children,
            active_child,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let retained_tree_node_roles = self.retained_tree_node_roles;
        Ok({
            if !children.is_empty() || active_child.is_some() {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses explicit tree membership",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| retained_tree_node_roles.get(&record.id) != Some(role))
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes feature-tree node role",
                    feature.id
                )));
            }
            (
                existing.map_or_else(
                    || feature_tree_node_kind(*role).into(),
                    |record| record.kind.clone(),
                ),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                feature.source_properties.clone(),
            )
        })
    }

    pub(super) fn encode_cosmetic_thread(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::CosmeticThread {
            face,
            diameter,
            extent,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            let Some(record) =
                existing.filter(|record| classify(record) == Some(FeatureClass::CosmeticThread))
            else {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} adds a cosmetic thread",
                    feature.id
                )));
            };
            let mut parameters = record.parameters.clone();
            if let Some(diameter) = diameter {
                let prefix = record
                    .parameters
                    .get("D2")
                    .filter(|value| value.trim().starts_with("&lt;MOD-DIAM&gt;"))
                    .map_or("<MOD-DIAM>", |_| "&lt;MOD-DIAM&gt;");
                parameters.insert(
                    "D2".into(),
                    format!("{prefix}{}", format_f64_literal(diameter.0)),
                );
            }
            match extent {
                Some(CosmeticThreadExtent::Blind { length }) => {
                    parameters.insert(
                        "D1".into(),
                        format_length_like(
                            length.0,
                            record.parameters.get("D1").map(String::as_str),
                        ),
                    );
                }
                Some(CosmeticThreadExtent::Through) => {
                    parameters.remove("D1");
                }
                None => {}
            }
            let mut properties = feature.source_properties.clone();
            if let Some(value) = face_selection_value(face) {
                properties.insert("Face".into(), value);
            } else if !matches!(face, FaceSelection::Unresolved) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes cosmetic-thread face selection",
                    feature.id
                )));
            }
            (record.kind.clone(), parameters, properties)
        })
    }

    pub(super) fn encode_native(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Native {
            kind,
            parameters,
            properties,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Ok({
            let mut merged = feature.source_properties.clone();
            merged.extend(properties.clone());
            (kind.clone(), parameters.clone(), merged)
        })
    }

    pub(super) fn encode_stored_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::StoredGeometry = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok((
            existing.map_or_else(|| "Feature".into(), |record| record.kind.clone()),
            existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default(),
            feature.source_properties.clone(),
        ))
    }

    pub(super) fn encode_derived_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::DerivedGeometry { .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported copied-geometry semantics",
            feature.id
        )))
    }

    pub(super) fn encode_imported_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::ImportedGeometry { .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported external-import semantics",
            feature.id
        )))
    }

    pub(super) fn encode_primitive(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Primitive { .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported analytic-primitive semantics",
            feature.id
        )))
    }

    pub(super) fn encode_equation_curve(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::EquationCurve {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            start,
            end,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if parameter.trim().is_empty()
                || x_expression.trim().is_empty()
                || y_expression.trim().is_empty()
                || z_expression.trim().is_empty()
                || !start.is_finite()
                || !end.is_finite()
                || start >= end
            {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has an invalid equation curve",
                    feature.id
                )));
            }
            require_same_family(
                existing,
                &feature.id,
                &["EquationDrivenCurve", "EquationCurve"],
            )?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Parameter".into(), parameter.clone());
            properties.insert("XEquation".into(), x_expression.clone());
            properties.insert("YEquation".into(), y_expression.clone());
            properties.insert("ZEquation".into(), z_expression.clone());
            properties.insert("Start".into(), start.to_string());
            properties.insert("End".into(), end.to_string());
            (
                existing.map_or_else(
                    || "EquationDrivenCurve".into(),
                    |record| record.kind.clone(),
                ),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }

    pub(super) fn encode_projected_curve(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::ProjectedCurve {
            source,
            target_faces,
            direction,
            bidirectional,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            let source = path_source(source, record_sources, sketch_sources).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} references a missing projection source",
                    feature.id
                ))
            })?;
            let target_faces = face_selection_value(target_faces).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT feature {} has no projection target faces",
                    feature.id
                ))
            })?;
            require_same_family(
                existing,
                &feature.id,
                &["ProjectedCurve", "ProjectionCurve"],
            )?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Source".into(), source);
            properties.insert("TargetFaces".into(), target_faces);
            if let Some(bidirectional) = bidirectional {
                properties.insert("Bidirectional".into(), bidirectional.to_string());
            }
            match direction {
                CurveProjectionDirection::Vector(direction) => {
                    require_direction(*direction, &feature.id, "projection direction")?;
                    properties.insert("Direction".into(), format_vector3(*direction));
                }
                CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal) => {
                    properties.remove("Direction");
                }
                CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved) => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT feature {} has unresolved projection direction",
                        feature.id
                    )));
                }
            }
            (
                existing.map_or_else(|| "ProjectedCurve".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }

    pub(super) fn encode_composite_curve(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::CompositeCurve { segments, closed } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        let record_sources = self.record_sources;
        let sketch_sources = self.sketch_sources;
        Ok({
            if segments.is_empty() {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has no composite-curve segments",
                    feature.id
                )));
            }
            require_same_family(existing, &feature.id, &["CompositeCurve"])?;
            let segments = segments
                .iter()
                .map(|segment| {
                    path_source(segment, record_sources, sketch_sources).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "SLDPRT feature {} references a missing composite segment",
                            feature.id
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut properties = feature.source_properties.clone();
            properties.insert("Segments".into(), segments.join(";"));
            properties.insert("Closed".into(), closed.to_string());
            (
                existing.map_or_else(|| "CompositeCurve".into(), |record| record.kind.clone()),
                existing
                    .map(|record| record.parameters.clone())
                    .unwrap_or_default(),
                properties,
            )
        })
    }

    pub(super) fn encode_helix(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Helix {
            axis_origin,
            axis_direction,
            radius,
            pitch,
            revolutions,
            start_angle,
            clockwise,
            radial_growth,
            cone_angle,
            segment_turns,
            construction_style,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if radial_growth.is_some()
                || cone_angle.is_some()
                || segment_turns.is_some()
                || construction_style.is_some()
            {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} uses unsupported helix construction controls",
                    feature.id
                )));
            }
            if ![axis_origin.x, axis_origin.y, axis_origin.z, pitch.0]
                .into_iter()
                .all(f64::is_finite)
                || !valid_direction(*axis_direction)
                || !radius.0.is_finite()
                || radius.0 <= 0.0
                || !revolutions.is_finite()
                || *revolutions <= 0.0
                || !start_angle.0.is_finite()
            {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has invalid helix geometry",
                    feature.id
                )));
            }
            if existing.is_some_and(|record| !is_helix(record)) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes operation family",
                    feature.id
                )));
            }
            let mut parameters = existing
                .map(|record| record.parameters.clone())
                .unwrap_or_default();
            parameters.insert("Radius".into(), format_length_mm(radius.0));
            parameters.insert("Pitch".into(), format_length_mm(pitch.0));
            parameters.insert("Revolutions".into(), revolutions.to_string());
            parameters.insert("StartAngle".into(), format_angle_rad(start_angle.0));
            let mut properties = feature.source_properties.clone();
            properties.insert("AxisOrigin".into(), format_point3_mm(*axis_origin));
            properties.insert("AxisDirection".into(), format_vector3(*axis_direction));
            properties.insert("Clockwise".into(), clockwise.to_string());
            (
                existing.map_or_else(|| "Helix".into(), |record| record.kind.clone()),
                parameters,
                properties,
            )
        })
    }

    pub(super) fn encode_helix_native_axis(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::HelixNativeAxis {
            axis_native_ref,
            axial_rise,
            pitch,
            revolutions,
            start_angle,
            clockwise,
        } = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        let existing = self.existing;
        Ok({
            if axis_native_ref.is_empty()
                || !axial_rise.0.is_finite()
                || !pitch.0.is_finite()
                || !revolutions.is_finite()
                || *revolutions <= 0.0
                || !start_angle.0.is_finite()
            {
                return Err(CodecError::Malformed(format!(
                    "SLDPRT feature {} has invalid native-axis helix geometry",
                    feature.id
                )));
            }
            let record = existing.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT feature {} requires a retained native helix axis",
                    feature.id
                ))
            })?;
            if !is_helix(record) || axis_native_ref != &record.id {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT feature {} changes its native helix axis",
                    feature.id
                )));
            }
            let mut parameters = record.parameters.clone();
            parameters.insert(
                "D3".into(),
                format_length_like(
                    axial_rise.0,
                    record.parameters.get("D3").map(String::as_str),
                ),
            );
            parameters.insert(
                "D4".into(),
                format_length_like(pitch.0, record.parameters.get("D4").map(String::as_str)),
            );
            parameters.insert("D5".into(), revolutions.to_string());
            parameters.insert(
                "D7".into(),
                format_angle_like(
                    start_angle.0,
                    record.parameters.get("D7").map(String::as_str),
                ),
            );
            let mut properties = feature.source_properties.clone();
            if properties.contains_key("Clockwise") || *clockwise {
                properties.insert("Clockwise".into(), clockwise.to_string());
            }
            (record.kind.clone(), parameters, properties)
        })
    }

    pub(super) fn encode_offset_shape(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::OffsetShape { .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported whole-shape offset semantics",
            feature.id
        )))
    }

    pub(super) fn encode_post_process(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::PostProcess { .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported topology post-processing semantics",
            feature.id
        )))
    }

    pub(super) fn encode_curve_geometry(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let (FeatureDefinition::PointGeometry { .. }
        | FeatureDefinition::LineSegment { .. }
        | FeatureDefinition::CircularArc { .. }
        | FeatureDefinition::EllipticArc { .. }
        | FeatureDefinition::Polyline { .. }
        | FeatureDefinition::RegularPolygonCurve { .. }
        | FeatureDefinition::PlanarPatch { .. }
        | FeatureDefinition::FaceFromShapes { .. }) = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported construction-geometry semantics",
            feature.id
        )))
    }

    pub(super) fn encode_shape_operation(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let (FeatureDefinition::Compound { .. }
        | FeatureDefinition::RefineShape { .. }
        | FeatureDefinition::ReverseShape { .. }
        | FeatureDefinition::RuledBetweenCurves { .. }
        | FeatureDefinition::SectionShape { .. }
        | FeatureDefinition::MirrorShape { .. }
        | FeatureDefinition::ProjectOnSurface { .. }) = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses unsupported derived-shape semantics",
            feature.id
        )))
    }

    pub(super) fn encode_binder(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let FeatureDefinition::Binder { .. } = &feature.definition else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses design-binder semantics that cannot be written",
            feature.id
        )))
    }

    pub(super) fn encode_explicitly_unsupported(
        &self,
    ) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        let (FeatureDefinition::DatumPointUnresolved
        | FeatureDefinition::DatumCoordinateSystemUnresolved
        | FeatureDefinition::Block { .. }
        | FeatureDefinition::ExtractBody { .. }
        | FeatureDefinition::LoftUnresolved
        | FeatureDefinition::FreeformSurfaceUnresolved
        | FeatureDefinition::DraftUnresolved
        | FeatureDefinition::FaceBlend { .. }
        | FeatureDefinition::SewBodies { .. }
        | FeatureDefinition::TrimBodies { .. }) = &feature.definition
        else {
            unreachable!("neutral feature encoder dispatched wrong variant")
        };
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses semantics that cannot be written",
            feature.id
        )))
    }

    pub(super) fn encode_unsupported(&self) -> Result<NeutralFeatureEncoding, CodecError> {
        let feature = self.feature;
        Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {} uses semantics that cannot be written",
            feature.id
        )))
    }
}
