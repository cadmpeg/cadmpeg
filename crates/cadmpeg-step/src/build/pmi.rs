// SPDX-License-Identifier: Apache-2.0
//! STEP semantic and presentation PMI emission.

use std::collections::{BTreeMap, HashMap};

use crate::geometry;
use crate::writer::{real, refs, string, Ref};

use super::{is_rigid_transform, Builder};

impl Builder<'_> {
    pub(super) fn emit_pmi(&mut self, context: Ref) {
        use cadmpeg_ir::pmi::{DimensionKind, GeometricToleranceKind, PmiDefinition, PmiTarget};

        if self.ir.model.pmi.is_empty() || !self.schema.supports_semantic_pmi() {
            return;
        }
        let annotations = self.ir.model.pmi.clone();
        let Some(pds) = self.default_product_definition_shape else {
            return;
        };
        let mut annotation_refs = HashMap::new();
        let mut aspects = HashMap::<String, Ref>::new();
        for annotation in &annotations {
            for target in &annotation.targets {
                let PmiTarget::ShapeAspect { source_id } = target else {
                    continue;
                };
                aspects.entry(source_id.clone()).or_insert_with(|| {
                    self.emitter.emit(
                        "SHAPE_ASPECT",
                        &format!("{},'',{pds},.T.", string(source_id)),
                    )
                });
            }
        }
        let fallback_aspect = self
            .emitter
            .emit("SHAPE_ASPECT", &format!("'PMI target','',{pds},.T."));
        let target_ref = |annotation: &cadmpeg_ir::PmiAnnotation| {
            annotation.targets.iter().find_map(|target| {
                if let PmiTarget::ShapeAspect { source_id } = target {
                    aspects.get(source_id).copied()
                } else {
                    None
                }
            })
        };
        let targets_exact = |annotation: &cadmpeg_ir::PmiAnnotation| {
            annotation
                .targets
                .iter()
                .all(|target| matches!(target, PmiTarget::ShapeAspect { .. }))
        };

        for annotation in &annotations {
            if let PmiDefinition::Datum { identification } = &annotation.definition {
                let datum = self.emitter.emit(
                    "DATUM",
                    &format!(
                        "{},$,{pds},.F.,{}",
                        string(annotation.name.as_deref().unwrap_or("")),
                        string(identification)
                    ),
                );
                annotation_refs.insert(annotation.id.clone(), datum);
                self.written_pmi += usize::from(targets_exact(annotation));
            }
        }
        for annotation in &annotations {
            if let PmiDefinition::DatumSystem { references } = &annotation.definition {
                let mut groups = BTreeMap::<(u32, Option<u32>), Vec<_>>::new();
                for reference in references {
                    groups
                        .entry((reference.precedence, reference.common_group))
                        .or_default()
                        .push(reference);
                }
                let compartments = groups
                    .values()
                    .filter_map(|group| {
                        let datum_refs = group
                            .iter()
                            .map(|reference| annotation_refs.get(&reference.datum).copied())
                            .collect::<Option<Vec<_>>>()?;
                        if group[0].common_group.is_none() && group.len() != 1 {
                            return None;
                        }
                        let (datum, modifiers) = if group[0].common_group.is_some() {
                            let elements = group
                                .iter()
                                .zip(datum_refs)
                                .map(|(reference, datum)| {
                                    let modifiers =
                                        self.emit_datum_modifiers(&reference.modifiers)?;
                                    Some(self.emitter.emit(
                                        "DATUM_REFERENCE_ELEMENT",
                                        &format!("'',$,{pds},.F.,{datum},({modifiers})"),
                                    ))
                                })
                                .collect::<Option<Vec<_>>>()?;
                            (
                                format!("COMMON_DATUM_LIST({})", refs(&elements)),
                                String::new(),
                            )
                        } else {
                            (
                                datum_refs[0].to_string(),
                                self.emit_datum_modifiers(&group[0].modifiers)?,
                            )
                        };
                        Some(self.emitter.emit(
                            "DATUM_REFERENCE_COMPARTMENT",
                            &format!("'',$,{pds},.F.,{datum},({modifiers})"),
                        ))
                    })
                    .collect::<Vec<_>>();
                let complete = compartments.len() == groups.len();
                if compartments.is_empty() {
                    continue;
                }
                let system = self.emitter.emit(
                    "DATUM_SYSTEM",
                    &format!(
                        "{},'',{pds},.F.,{}",
                        string(annotation.name.as_deref().unwrap_or("")),
                        refs(&compartments)
                    ),
                );
                annotation_refs.insert(annotation.id.clone(), system);
                self.written_pmi += usize::from(targets_exact(annotation) && complete);
            }
        }
        for annotation in &annotations {
            match &annotation.definition {
                PmiDefinition::Dimension {
                    dimension,
                    nominal,
                    lower_deviation,
                    upper_deviation,
                    limits_and_fits,
                } => {
                    let aspect = target_ref(annotation).unwrap_or(fallback_aspect);
                    let name = annotation.name.as_deref().unwrap_or("");
                    let (entity, kind_exact) = match dimension {
                        DimensionKind::Size => ("DIMENSIONAL_SIZE", true),
                        DimensionKind::Location => ("DIMENSIONAL_LOCATION", true),
                        DimensionKind::Angular => ("ANGULAR_SIZE", true),
                        // AP242 represents diameter and radius as a
                        // DIMENSIONAL_SIZE whose name identifies the size
                        // category; DIAMETER_SIZE and RADIUS_SIZE are not
                        // entity types.
                        DimensionKind::Diameter | DimensionKind::Radius => {
                            ("DIMENSIONAL_SIZE", true)
                        }
                        DimensionKind::Other(_) => ("DIMENSIONAL_SIZE", false),
                    };
                    let characteristic_name = match dimension {
                        DimensionKind::Diameter => "diameter",
                        DimensionKind::Radius => "radius",
                        _ => name,
                    };
                    let parameters = match dimension {
                        DimensionKind::Location => {
                            format!("{},$,{aspect},{aspect}", string(characteristic_name))
                        }
                        DimensionKind::Angular => {
                            format!("{aspect},{},.SMALL.", string(characteristic_name))
                        }
                        _ => format!("{aspect},{}", string(characteristic_name)),
                    };
                    let characteristic = self.emitter.emit(entity, &parameters);
                    if let Some(value) = nominal {
                        let measure = self.emit_pmi_measure_representation_item(*value, name);
                        let representation = self.emitter.emit(
                            "SHAPE_DIMENSION_REPRESENTATION",
                            &format!("'',({measure}),{context}"),
                        );
                        self.emitter.emit(
                            "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION",
                            &format!("{characteristic},{representation}"),
                        );
                    }
                    if let (Some(lower), Some(upper)) = (lower_deviation, upper_deviation) {
                        let lower = self.emit_pmi_measure(*lower);
                        let upper = self.emit_pmi_measure(*upper);
                        let tolerance = self
                            .emitter
                            .emit("TOLERANCE_VALUE", &format!("{lower},{upper}"));
                        self.emitter.emit(
                            "PLUS_MINUS_TOLERANCE",
                            &format!("{tolerance},{characteristic}"),
                        );
                    }
                    if let Some(fit) = limits_and_fits {
                        let fit = self.emitter.emit(
                            "LIMITS_AND_FITS",
                            &format!(
                                "{},{},{},{}",
                                string(&fit.form_variance),
                                string(&fit.zone_variance),
                                string(&fit.grade),
                                string(&fit.source)
                            ),
                        );
                        self.emitter
                            .emit("PLUS_MINUS_TOLERANCE", &format!("{fit},{characteristic}"));
                    }
                    annotation_refs.insert(annotation.id.clone(), characteristic);
                    let deviations_exact = lower_deviation.is_some() == upper_deviation.is_some();
                    self.written_pmi +=
                        usize::from(targets_exact(annotation) && deviations_exact && kind_exact);
                }
                PmiDefinition::GeometricTolerance {
                    tolerance,
                    magnitude,
                    datum_system,
                } => {
                    let kind_exact = !matches!(tolerance, GeometricToleranceKind::Other(value) if value != "geometric_tolerance");
                    let entity = match tolerance {
                        GeometricToleranceKind::Straightness => "STRAIGHTNESS_TOLERANCE",
                        GeometricToleranceKind::Flatness => "FLATNESS_TOLERANCE",
                        GeometricToleranceKind::Roundness => "ROUNDNESS_TOLERANCE",
                        GeometricToleranceKind::Cylindricity => "CYLINDRICITY_TOLERANCE",
                        GeometricToleranceKind::LineProfile => "LINE_PROFILE_TOLERANCE",
                        GeometricToleranceKind::SurfaceProfile => "SURFACE_PROFILE_TOLERANCE",
                        GeometricToleranceKind::Angularity => "ANGULARITY_TOLERANCE",
                        GeometricToleranceKind::Perpendicularity => "PERPENDICULARITY_TOLERANCE",
                        GeometricToleranceKind::Parallelism => "PARALLELISM_TOLERANCE",
                        GeometricToleranceKind::Position => "POSITION_TOLERANCE",
                        GeometricToleranceKind::Concentricity => "CONCENTRICITY_TOLERANCE",
                        GeometricToleranceKind::Symmetry => "SYMMETRY_TOLERANCE",
                        GeometricToleranceKind::CircularRunout => "CIRCULAR_RUNOUT_TOLERANCE",
                        GeometricToleranceKind::TotalRunout => "TOTAL_RUNOUT_TOLERANCE",
                        GeometricToleranceKind::Other(_) => continue,
                    };
                    let measure = self.emit_pmi_measure(*magnitude);
                    let aspect = target_ref(annotation).unwrap_or(fallback_aspect);
                    // Datum references are carried by the complex
                    // GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE subtype. Until
                    // that complex entity is modeled, refuse it through the
                    // unwritten-PMI accounting instead of emitting an invalid
                    // fifth parameter on the simple tolerance entity.
                    if datum_system.is_some() {
                        continue;
                    }
                    let tolerance_ref = self.emitter.emit(
                        entity,
                        &format!(
                            "{},'',{measure},{aspect}",
                            string(annotation.name.as_deref().unwrap_or(""))
                        ),
                    );
                    annotation_refs.insert(annotation.id.clone(), tolerance_ref);
                    self.written_pmi += usize::from(targets_exact(annotation) && kind_exact);
                }
                PmiDefinition::Datum { .. }
                | PmiDefinition::DatumSystem { .. }
                | PmiDefinition::Presentation { .. } => {}
            }
        }
        let mut presentation_items = Vec::new();
        let mut presentation_semantics = Vec::new();
        for annotation in &annotations {
            let PmiDefinition::Presentation {
                text,
                placement,
                semantics,
            } = &annotation.definition
            else {
                continue;
            };
            let (Some(text), Some(placement)) = (text.as_deref(), placement.as_ref()) else {
                continue;
            };
            if !annotation.targets.is_empty() || !is_rigid_transform(&placement.rows) {
                continue;
            }
            let rows = placement.rows;
            let placement = geometry::placement(
                &mut self.emitter,
                cadmpeg_ir::math::Point3::new(rows[0][3], rows[1][3], rows[2][3]),
                cadmpeg_ir::math::Vector3::new(rows[0][2], rows[1][2], rows[2][2]),
                cadmpeg_ir::math::Vector3::new(rows[0][0], rows[1][0], rows[2][0]),
            );
            let font_source = self.emitter.emit("EXTERNAL_SOURCE", "'ISO 3098'");
            let font = self.emitter.emit(
                "EXTERNALLY_DEFINED_TEXT_FONT",
                &format!("IDENTIFIER('ISO 3098'),{font_source}"),
            );
            let literal = self.emitter.emit(
                "TEXT_LITERAL",
                &format!("{},{placement},'left',.RIGHT.,{font}", string(text)),
            );
            let semantic_refs = semantics
                .iter()
                .filter_map(|semantic| annotation_refs.get(semantic).copied())
                .collect::<Vec<_>>();
            if semantic_refs.len() != semantics.len() {
                continue;
            }
            let style = self
                .emitter
                .emit("PRESENTATION_STYLE_ASSIGNMENT", "(.NULL.)");
            let occurrence = self.emitter.emit(
                "ANNOTATION_TEXT_OCCURRENCE",
                &format!(
                    "{},{},{literal}",
                    string(annotation.name.as_deref().unwrap_or("")),
                    refs(&[style])
                ),
            );
            presentation_items.push(occurrence);
            presentation_semantics.push((occurrence, semantic_refs));
            annotation_refs.insert(annotation.id.clone(), occurrence);
            self.written_pmi += 1;
        }
        if !presentation_items.is_empty() {
            let model = self.emitter.emit(
                "DRAUGHTING_MODEL",
                &format!(
                    "'PMI presentation',{}, {context}",
                    refs(&presentation_items)
                ),
            );
            for (occurrence, semantics) in presentation_semantics {
                for semantic in semantics {
                    self.emitter.emit(
                        "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
                        &format!("'','',{semantic},{model},{occurrence}"),
                    );
                }
            }
        }
    }

    fn emit_datum_modifiers(&mut self, source: &[String]) -> Option<String> {
        let mut modifiers = Vec::with_capacity(source.len());
        for modifier in source {
            if let Some((kind, value)) = modifier.split_once(':') {
                let value = value.parse::<f64>().ok()?;
                let measure = self.emit_pmi_measure(cadmpeg_ir::PmiValue {
                    value,
                    quantity: cadmpeg_ir::PmiQuantity::Length,
                });
                modifiers.push(
                    self.emitter
                        .emit(
                            "DATUM_REFERENCE_MODIFIER_WITH_VALUE",
                            &format!(".{}.,{measure}", kind.to_ascii_uppercase()),
                        )
                        .to_string(),
                );
            } else {
                modifiers.push(format!(".{}.", modifier.to_ascii_uppercase()));
            }
        }
        Some(modifiers.join(","))
    }

    fn emit_pmi_measure(&mut self, value: cadmpeg_ir::PmiValue) -> Ref {
        use cadmpeg_ir::pmi::PmiQuantity;
        let (entity, typed, unit) = match value.quantity {
            PmiQuantity::Length => (
                "LENGTH_MEASURE_WITH_UNIT",
                "LENGTH_MEASURE",
                self.emit_length_unit(),
            ),
            PmiQuantity::Angle => (
                "PLANE_ANGLE_MEASURE_WITH_UNIT",
                "PLANE_ANGLE_MEASURE",
                self.emit_angle_unit(),
            ),
            PmiQuantity::Ratio => ("MEASURE_WITH_UNIT", "RATIO_MEASURE", self.emit_ratio_unit()),
        };
        self.emitter
            .emit(entity, &format!("{typed}({}),{unit}", real(value.value)))
    }

    fn emit_pmi_measure_representation_item(
        &mut self,
        value: cadmpeg_ir::PmiValue,
        name: &str,
    ) -> Ref {
        use cadmpeg_ir::pmi::PmiQuantity;
        let (typed, unit) = match value.quantity {
            PmiQuantity::Length => ("LENGTH_MEASURE", self.emit_length_unit()),
            PmiQuantity::Angle => ("PLANE_ANGLE_MEASURE", self.emit_angle_unit()),
            PmiQuantity::Ratio => ("RATIO_MEASURE", self.emit_ratio_unit()),
        };
        self.emitter.emit(
            "MEASURE_REPRESENTATION_ITEM",
            &format!("{},{typed}({}),{unit}", string(name), real(value.value)),
        )
    }
}
