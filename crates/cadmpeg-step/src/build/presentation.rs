// SPDX-License-Identifier: Apache-2.0
//! STEP presentation styling: appearances, colors, and layers.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::report::{LossCategory, LossCode, Severity};

use crate::writer::{real, refs, string, Ref};

use super::Builder;

#[derive(Clone, Copy)]
struct ColorSpec<'a> {
    color: cadmpeg_ir::topology::Color,
    appearance: Option<&'a Appearance>,
    binding_id: Option<&'a str>,
}

impl Builder<'_> {
    pub(super) fn emit_presentation(&mut self, context: Ref) {
        use cadmpeg_ir::appearance::AppearanceTarget;

        let ir = self.ir;
        let appearances: HashMap<&str, &Appearance> = ir
            .model
            .appearances
            .iter()
            .map(|appearance| (appearance.id.as_str(), appearance))
            .collect();
        let mut body_colors: HashMap<&str, ColorSpec<'_>> = HashMap::new();
        let mut face_colors: HashMap<&str, ColorSpec<'_>> = HashMap::new();
        for binding in &ir.model.appearance_bindings {
            let Some(appearance) = appearances.get(binding.appearance.as_str()).copied() else {
                continue;
            };
            let Some(color) = appearance.base_color else {
                continue;
            };
            let spec = ColorSpec {
                color,
                appearance: Some(appearance),
                binding_id: Some(&binding.id),
            };
            match &binding.target {
                AppearanceTarget::Body(id) => {
                    body_colors.entry(id.as_str()).or_insert(spec);
                }
                AppearanceTarget::Face(id) => {
                    face_colors.entry(id.as_str()).or_insert(spec);
                }
                AppearanceTarget::Surface(_)
                | AppearanceTarget::Curve(_)
                | AppearanceTarget::Point(_)
                | AppearanceTarget::Edge(_)
                | AppearanceTarget::Vertex(_)
                | AppearanceTarget::Tessellation(_)
                | AppearanceTarget::Source { .. } => {}
            }
        }
        for body in &ir.model.bodies {
            if let Some(color) = body.color {
                body_colors.entry(body.id.as_str()).or_insert(ColorSpec {
                    color,
                    appearance: None,
                    binding_id: None,
                });
            }
        }
        for face in &ir.model.faces {
            if let Some(color) = face.color {
                face_colors.entry(face.id.as_str()).or_insert(ColorSpec {
                    color,
                    appearance: None,
                    binding_id: None,
                });
            }
        }

        let mut face_body: HashMap<&str, &str> = HashMap::new();
        for region in &ir.model.regions {
            let body = region.body.0.as_str();
            for shell_id in &region.shells {
                let Some(shell) = self.shells.get(shell_id.as_str()).copied() else {
                    continue;
                };
                for face in &shell.faces {
                    face_body.insert(face.0.as_str(), body);
                }
            }
        }

        // Every colored face carries its own face-level STYLED_ITEM. A face's
        // color is its own override when present, otherwise the color of the
        // body that owns it. Whole-solid styling is intentionally not emitted:
        // common OCCT/VTK-based viewers (f3d, CAD Assistant) read STEP surface
        // colors only from ADVANCED_FACE and ignore MANIFOLD_SOLID_BREP, so a
        // body color left at the solid level renders as the viewer default.
        let mut style_refs: HashMap<String, Ref> = HashMap::new();
        let mut styled = Vec::new();
        let mut faces: Vec<(String, Ref)> = self
            .face_step_refs
            .iter()
            .map(|(id, r)| (id.clone(), *r))
            .collect();
        faces.sort_by(|a, b| a.0.cmp(&b.0));
        let mut styled_bodies: BTreeSet<&str> = BTreeSet::new();
        for (face_id, face) in &faces {
            let own = face_colors.get(face_id.as_str()).copied();
            let body = face_body.get(face_id.as_str()).copied();
            let inherited = body.and_then(|b| body_colors.get(b).copied());
            let Some(spec) = own.or(inherited) else {
                continue;
            };
            // The body color is only counted as represented when a face without
            // its own override receives it.
            if own.is_none() {
                if let Some(b) = body {
                    styled_bodies.insert(b);
                }
            }
            if let Some(binding_id) = spec.binding_id {
                self.written_appearance_bindings
                    .insert(binding_id.to_string());
            }
            let name = spec
                .appearance
                .and_then(|appearance| appearance.name.as_deref())
                .unwrap_or("");
            let style = self.surface_style(spec.color, name, &mut style_refs);
            styled.push(
                self.emitter
                    .emit("STYLED_ITEM", &format!("'color',({style}),{face}")),
            );
        }
        let mut direct_unstyled = BTreeSet::new();
        for binding in &ir.model.appearance_bindings {
            if self.written_appearance_bindings.contains(&binding.id) {
                continue;
            }
            let Some(appearance) = appearances.get(binding.appearance.as_str()).copied() else {
                continue;
            };
            let Some(color) = appearance.base_color else {
                continue;
            };
            let (target, style_kind) = match &binding.target {
                AppearanceTarget::Face(id) => {
                    (self.face_step_refs.get(id.as_str()).copied(), "surface")
                }
                AppearanceTarget::Surface(id) => {
                    (self.surface_refs.get(id.as_str()).copied(), "surface")
                }
                AppearanceTarget::Curve(id) => (self.curve_refs.get(id.as_str()).copied(), "curve"),
                AppearanceTarget::Edge(id) => (self.edge_refs.get(id.as_str()).copied(), "curve"),
                AppearanceTarget::Point(id) => (self.point_refs.get(id.as_str()).copied(), "point"),
                AppearanceTarget::Tessellation(id) => {
                    (self.tessellation_step_refs.get(id).copied(), "surface")
                }
                AppearanceTarget::Body(_)
                | AppearanceTarget::Vertex(_)
                | AppearanceTarget::Source { .. } => continue,
            };
            let Some(target) = target else {
                let target_id = match &binding.target {
                    AppearanceTarget::Face(id) => id.0.clone(),
                    AppearanceTarget::Surface(id) => id.0.clone(),
                    AppearanceTarget::Curve(id) => id.0.clone(),
                    AppearanceTarget::Edge(id) => id.0.clone(),
                    AppearanceTarget::Point(id) => id.0.clone(),
                    AppearanceTarget::Tessellation(id) => id.clone(),
                    AppearanceTarget::Body(_)
                    | AppearanceTarget::Vertex(_)
                    | AppearanceTarget::Source { .. } => continue,
                };
                direct_unstyled.insert(target_id);
                continue;
            };
            let name = appearance.name.as_deref().unwrap_or("");
            let style = match style_kind {
                "surface" => self.surface_style(color, name, &mut style_refs),
                "curve" => self.curve_style(color, name, &mut style_refs),
                "point" => self.point_style(color, name, &mut style_refs),
                _ => unreachable!(),
            };
            self.written_appearance_bindings.insert(binding.id.clone());
            styled.push(
                self.emitter
                    .emit("STYLED_ITEM", &format!("'color',({style}),{target}")),
            );
        }
        // A color is unrepresented when no emitted ADVANCED_FACE could carry it:
        // a face override whose face was skipped, or a body whose faces were all
        // skipped (hidden bodies or faces without an explicit STEP surface).
        let emitted: BTreeSet<&str> = self.face_step_refs.keys().map(String::as_str).collect();
        let mut unstyled_targets = face_colors
            .keys()
            .filter(|id| !emitted.contains(**id as &str))
            .map(|id| (*id).to_string())
            .collect::<BTreeSet<_>>();
        unstyled_targets.extend(
            body_colors
                .keys()
                .filter(|id| !styled_bodies.contains(**id as &str))
                .map(|id| (*id).to_string()),
        );
        unstyled_targets.extend(direct_unstyled);
        self.unstyled_colors = unstyled_targets.len();
        if styled.is_empty() {
            return;
        }
        self.emitter.emit(
            "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION",
            &format!("'',{},{context}", refs(&styled)),
        );
    }

    fn surface_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        name: &str,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            real(f64::from(color.r)),
            real(f64::from(color.g)),
            real(f64::from(color.b))
        );
        let key = format!("surface:{name}:{rgb}");
        if let Some(style) = cache.get(&key) {
            return *style;
        }
        let colour = self
            .emitter
            .emit("COLOUR_RGB", &format!("{},{rgb}", string(name)));
        let fill_colour = self
            .emitter
            .emit("FILL_AREA_STYLE_COLOUR", &format!("'',{colour}"));
        let fill = self
            .emitter
            .emit("FILL_AREA_STYLE", &format!("'',({fill_colour})"));
        let style_fill = self
            .emitter
            .emit("SURFACE_STYLE_FILL_AREA", &fill.to_string());
        let side = self
            .emitter
            .emit("SURFACE_SIDE_STYLE", &format!("'',({style_fill})"));
        let usage = self
            .emitter
            .emit("SURFACE_STYLE_USAGE", &format!(".BOTH.,{side}"));
        let assignment = self
            .emitter
            .emit("PRESENTATION_STYLE_ASSIGNMENT", &format!("({usage})"));
        cache.insert(key, assignment);
        assignment
    }

    fn curve_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        name: &str,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            real(f64::from(color.r)),
            real(f64::from(color.g)),
            real(f64::from(color.b))
        );
        let key = format!("curve:{name}:{rgb}");
        if let Some(style) = cache.get(&key) {
            return *style;
        }
        let colour = self
            .emitter
            .emit("COLOUR_RGB", &format!("{},{rgb}", string(name)));
        let font = self
            .emitter
            .emit("DRAUGHTING_PRE_DEFINED_CURVE_FONT", &string("continuous"));
        let curve = self.emitter.emit(
            "CURVE_STYLE",
            &format!("'',{font},POSITIVE_LENGTH_MEASURE(0.1),{colour}"),
        );
        let assignment = self
            .emitter
            .emit("PRESENTATION_STYLE_ASSIGNMENT", &format!("({curve})"));
        cache.insert(key, assignment);
        assignment
    }

    fn point_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        name: &str,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            real(f64::from(color.r)),
            real(f64::from(color.g)),
            real(f64::from(color.b))
        );
        let key = format!("point:{name}:{rgb}");
        if let Some(style) = cache.get(&key) {
            return *style;
        }
        let colour = self
            .emitter
            .emit("COLOUR_RGB", &format!("{},{rgb}", string(name)));
        let point = self.emitter.emit(
            "POINT_STYLE",
            &format!("'',.DOT.,POSITIVE_LENGTH_MEASURE(1.),{colour}"),
        );
        let assignment = self
            .emitter
            .emit("PRESENTATION_STYLE_ASSIGNMENT", &format!("({point})"));
        cache.insert(key, assignment);
        assignment
    }

    pub(super) fn emit_layers(&mut self) {
        use cadmpeg_ir::presentation::PresentationItem;

        for layer in self.ir.model.presentation_layers.clone() {
            let mut assigned = Vec::new();
            let mut unsupported = 0usize;
            for item in layer.items {
                let reference = match item {
                    PresentationItem::Body { body } => {
                        self.body_shape_refs.get(body.as_str()).copied()
                    }
                    PresentationItem::Face { face } => {
                        self.face_step_refs.get(face.as_str()).copied()
                    }
                    PresentationItem::Edge { edge } => self.edge_refs.get(edge.as_str()).copied(),
                    PresentationItem::Vertex { vertex } => {
                        self.vertex_refs.get(vertex.as_str()).copied()
                    }
                    PresentationItem::Curve { curve } => {
                        self.curve_refs.get(curve.as_str()).copied()
                    }
                    PresentationItem::Surface { surface } => {
                        self.surface_refs.get(surface.as_str()).copied()
                    }
                    PresentationItem::Point { .. }
                    | PresentationItem::Product { .. }
                    | PresentationItem::Occurrence { .. }
                    | PresentationItem::Pmi { .. }
                    | PresentationItem::Tessellation { .. }
                    | PresentationItem::Source { .. } => None,
                };
                if let Some(reference) = reference {
                    assigned.push(reference);
                } else {
                    unsupported += 1;
                }
            }
            if unsupported > 0 {
                self.loss(
                    LossCode::AttributesNotTransferred,
                    LossCategory::Attribute,
                    Severity::Warning,
                    format!(
                        "layer '{}' has {unsupported} item(s) without a writable STEP carrier",
                        layer.name
                    ),
                );
            }
            if !assigned.is_empty() {
                self.emitter.emit(
                    "PRESENTATION_LAYER_ASSIGNMENT",
                    &format!(
                        "{},{},{}",
                        string(&layer.name),
                        string(layer.description.as_deref().unwrap_or("")),
                        refs(&assigned)
                    ),
                );
            }
        }
    }
}
